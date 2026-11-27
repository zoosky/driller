//! Sending one request: interpolation, the HTTP call, latency measurement and
//! the response snapshot.

use std::time::{Duration, Instant};

use colored::Colorize;
use encoding_rs::{Encoding, UTF_8};
use reqwest::{
  ClientBuilder,
  header::{self, HeaderMap, HeaderValue},
};
use url::Url;

use serde_json::{Map, Value};

use crate::benchmark::{Context, Pool};
use crate::config::Config;
use crate::interpolator;

use super::output::{log_invalid_header_value, log_request, log_request_failure};
use super::{Body, Request, ResponseData, USER_AGENT};

impl Request {
  /// Interpolates the name, URL, body and headers against `context`, sends the
  /// request and measures it until the last byte of the body has arrived.
  ///
  /// Returns the response snapshot and the latency in milliseconds, or `None`
  /// when the request failed: a connection or body-read error, or an
  /// interpolated header value that is not a valid HTTP header value (that
  /// request is never sent and reports `0.0` ms). Failures print an `ERR` line
  /// unless `--quiet`.
  ///
  /// # Panics
  ///
  /// Panics on plan errors that only show up after interpolation: a relative
  /// URL without a `base`, a URL that does not parse, or an unknown `{{ }}`
  /// variable in strict mode.
  pub(super) async fn send_request(&self, context: &mut Context, pool: &Pool, config: &Config) -> (Option<ResponseData>, f64) {
    let mut uninterpolator = None;

    // Resolve the name
    let interpolated_name = if self.name.contains('{') {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(&self.name, !config.relaxed_interpolations)
    } else {
      self.name.clone()
    };

    // Resolve the url
    let interpolated_url = if self.url.contains('{') {
      uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(&self.url, !config.relaxed_interpolations)
    } else {
      self.url.clone()
    };

    // Resolve relative urls
    let interpolated_base_url = if &interpolated_url[..1] == "/" {
      match context.get("base") {
        Some(value) => {
          if let Some(vs) = value.as_str() {
            format!("{vs}{interpolated_url}")
          } else {
            panic!("{} Wrong type 'base' variable!", "WARNING!".yellow().bold());
          }
        }
        _ => {
          panic!("{} Unknown 'base' variable!", "WARNING!".yellow().bold());
        }
      }
    } else {
      interpolated_url
    };

    let url = Url::parse(&interpolated_base_url).expect("Invalid url!");
    let domain = format!("{}://{}:{}", url.scheme(), url.host_str().unwrap(), url.port().unwrap_or(0)); // Unique domain key for keep-alive

    let interpolated_body;

    let method = self.method.clone();

    // Clone Client out of the Pool lock so RequestBuilder construction does not run under the Mutex.
    let client = {
      let mut pool2 = pool.lock().unwrap();
      pool2.entry(domain).or_insert_with(|| ClientBuilder::default().danger_accept_invalid_certs(config.no_check_certificate).build().unwrap()).clone()
    };

    let request = match self.body.as_ref() {
      Some(Body::Template(template_body)) => {
        interpolated_body = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(template_body, !config.relaxed_interpolations);
        client.request(method, interpolated_base_url.as_str()).body(interpolated_body)
      }
      Some(Body::Binary(binary_body)) => client.request(method, interpolated_base_url.as_str()).body(binary_body.clone()),
      None => client.request(method, interpolated_base_url.as_str()),
    };

    // Headers
    let mut headers = HeaderMap::new();
    headers.insert(header::USER_AGENT, HeaderValue::from_str(USER_AGENT).unwrap());

    if let Some(cookies) = context.get("cookies") {
      let cookies: Map<String, Value> = serde_json::from_value(cookies.clone()).unwrap();
      let cookie = cookie_header(&cookies);

      headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
    }

    // Resolve headers. Names and the literal parts of values were checked when
    // the plan loaded; an interpolated value can still bring in a line break
    // (from `with_items`, a CSV row or an `assign`ed response). That request
    // fails like a connection error instead of stopping the whole run.
    for (name, val) in self.headers.iter() {
      let interpolated_header = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(val, !config.relaxed_interpolations);
      match HeaderValue::from_str(&interpolated_header) {
        Ok(value) => {
          headers.insert(name.clone(), value);
        }
        Err(_) => {
          if !config.quiet || config.verbose {
            log_invalid_header_value(&interpolated_name, interpolated_base_url.as_str(), name, config);
          }
          return (None, 0.0);
        }
      }
    }

    let request_builder = request.headers(headers).timeout(Duration::from_secs(config.timeout));
    let request = request_builder.build().expect("Cannot create request");

    if config.verbose {
      log_request(&request);
    }

    let begin = Instant::now();
    let response_result = client.execute(request).await;

    let mut response = match response_result {
      Err(e) => {
        let duration_ms = begin.elapsed().as_secs_f64() * 1000.0;
        if !config.quiet || config.verbose {
          log_request_failure(&interpolated_name, interpolated_base_url.as_str(), duration_ms, &e, config);
        }
        return (None, duration_ms);
      }
      Ok(response) => response,
    };

    // Snapshot everything that borrows the response before the body stream is
    // consumed below.
    let url = response.url().clone();
    let status = response.status();
    let headers = response.headers().clone();
    let cookies: Vec<(String, String)> = response.cookies().map(|cookie| (cookie.name().to_string(), cookie.value().to_string())).collect();

    // Read the full response body so the measured latency reflects
    // time-to-last-byte, matching wrk, k6, vegeta and other load-testing tools.
    // The timer previously stopped at the response headers, which under-reported
    // any endpoint serving a non-trivial body (files, large JSON).
    //
    // The body is drained one chunk at a time. It is only buffered when an
    // `assign` needs to decode it; otherwise each chunk is dropped immediately,
    // so peak memory stays O(chunk) rather than O(body) even for large responses.
    let mut body_buf = self.assign.is_some().then(Vec::new);
    let drain_result = loop {
      match response.chunk().await {
        Ok(Some(chunk)) => {
          if let Some(buf) = body_buf.as_mut() {
            buf.extend_from_slice(&chunk);
          }
        }
        Ok(None) => break Ok(()),
        Err(e) => break Err(e),
      }
    };
    let duration_ms = begin.elapsed().as_secs_f64() * 1000.0;

    if let Err(e) = drain_result {
      if !config.quiet || config.verbose {
        log_request_failure(&interpolated_name, interpolated_base_url.as_str(), duration_ms, &e, config);
      }
      return (None, duration_ms);
    }

    if !config.quiet {
      let status_text = if status.is_server_error() {
        status.to_string().red()
      } else if status.is_client_error() {
        status.to_string().cyan()
      } else {
        status.to_string().yellow()
      };

      println!("{:width$} {} {} {}", interpolated_name.green(), interpolated_base_url.blue().bold(), status_text, Request::format_time(duration_ms, config.nanosec).cyan(), width = 25);
    }

    // Decode the buffered body (only present for `assign`) using the response
    // charset, mirroring reqwest's `Response::text`, so non-UTF-8 bodies are not
    // corrupted.
    let body = body_buf.map(|buf| decode_body(&headers, &buf));

    (
      Some(ResponseData {
        url,
        status,
        headers,
        cookies,
        body,
      }),
      duration_ms,
    )
  }
}

/// Decodes a response body using the charset declared in the `Content-Type`
/// header, defaulting to UTF-8 when none is present or the label is unknown.
///
/// This mirrors reqwest's `Response::text`, which driller can no longer call
/// directly: the body is drained as raw bytes inside the latency timer (so the
/// measured duration covers the full transfer), and only then decoded for the
/// `assign` path. Decoding the drained bytes here keeps charset-aware behaviour
/// for non-UTF-8 responses (e.g. `charset=iso-8859-1`).
fn decode_body(headers: &HeaderMap, bytes: &[u8]) -> String {
  let encoding = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(charset_from_content_type).and_then(|label| Encoding::for_label(label.as_bytes())).unwrap_or(UTF_8);

  encoding.decode(bytes).0.into_owned()
}

/// Extracts the `charset` parameter value from a `Content-Type` header value,
/// e.g. `text/html; charset=iso-8859-1` -> `iso-8859-1`. Surrounding quotes are
/// stripped. Returns `None` when no `charset` parameter is present.
fn charset_from_content_type(content_type: &str) -> Option<&str> {
  content_type.split(';').skip(1).find_map(|param| {
    let (key, value) = param.split_once('=')?;
    if key.trim().eq_ignore_ascii_case("charset") {
      Some(value.trim().trim_matches('"'))
    } else {
      None
    }
  })
}

/// Builds the `Cookie` request header from the cookie jar in the context.
///
/// The jar stores each value as the JSON string the server sent in
/// `Set-Cookie`. It is sent back unchanged, without the quotes JSON formatting
/// would add (`sid=s0`, not `sid="s0"`), and pairs are separated by `"; "` as
/// RFC 6265 specifies. A non-string value, which only appears if a plan
/// overwrites the jar, is sent in its JSON form.
fn cookie_header(cookies: &Map<String, Value>) -> String {
  cookies.iter().map(|(name, value)| format!("{name}={}", value.as_str().map_or_else(|| value.to_string(), str::to_owned))).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;
  use std::io::Write;

  fn test_config() -> Config {
    Config {
      base: String::new(),
      concurrency: 1,
      iterations: 1,
      relaxed_interpolations: false,
      no_check_certificate: false,
      rampup: 0,
      quiet: true,
      nanosec: false,
      timeout: 10,
      verbose: false,
      duration: None,
      assertion_failures: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }
  }

  #[test]
  fn invalid_interpolated_header_value_fails_the_request_without_panicking() {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    // Port 9 (discard) is never contacted: the request fails before it is sent.
    let item: crate::yaml::Value = crate::yaml::from_str("name: bad header\nrequest:\n  url: http://127.0.0.1:9/x\n  headers:\n    X-Trace: \"{{ trace }}\"\n").unwrap();
    let request = Request::new(&item, None, None);
    let mut context: Context = Context::new();
    context.insert("trace".to_string(), json!("bad\nvalue"));
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let config = test_config();

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (response, duration_ms) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    assert!(response.is_none(), "a request with an invalid header value must fail");
    assert_eq!(duration_ms, 0.0);
  }

  #[test]
  fn cookies_are_sent_back_as_the_server_set_them() {
    let mut cookies = Map::new();
    cookies.insert("sid".to_string(), json!("s0"));
    cookies.insert("theme".to_string(), json!("dark mode"));
    assert_eq!(cookie_header(&cookies), "sid=s0; theme=dark mode");
  }

  /// End to end: the `Cookie` header on the wire carries the stored value
  /// without added quotes.
  #[test]
  fn cookie_header_on_the_wire_has_no_added_quotes() {
    use std::collections::HashMap;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let mut buf = [0u8; 4096];
      let n = stream.read(&mut buf).unwrap();
      stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
      String::from_utf8_lossy(&buf[..n]).lines().find(|line| line.to_ascii_lowercase().starts_with("cookie:")).map(str::to_owned)
    });

    let request = Request::simple_get("cookie", &format!("http://{addr}/"));
    let mut context: Context = Context::new();
    context.insert("cookies".to_string(), json!({"sid": "s0"}));
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let config = test_config();

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (response, _) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    assert!(response.is_some(), "expected a response");
    assert_eq!(server.join().unwrap().as_deref(), Some("cookie: sid=s0"));
  }

  /// The latency timer must span the full body download (time-to-last-byte):
  /// a server that streams its body 300ms after the headers should measure at
  /// roughly 300ms, not ~0ms. Regression guard for the bug where the timer
  /// stopped at the response headers and the body was never read.
  #[test]
  fn measures_full_body_transfer_time() {
    use std::collections::HashMap;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body_delay = Duration::from_millis(300);

    // A bare HTTP/1.1 server that sends the response head immediately but delays
    // the body, so time-to-headers and time-to-last-byte differ measurably.
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let mut buf = [0u8; 1024];
      let _ = stream.read(&mut buf).unwrap();

      let body = "x".repeat(4096);
      let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
      stream.write_all(head.as_bytes()).unwrap();
      stream.flush().unwrap();
      thread::sleep(body_delay);
      stream.write_all(body.as_bytes()).unwrap();
      stream.flush().unwrap();
    });

    let url = format!("http://{addr}/");
    let request = Request::simple_get("delayed-body", &url);
    let mut context: Context = Context::new();
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let config = test_config();

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (response, duration_ms) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    server.join().unwrap();

    assert!(response.is_some(), "expected a successful response");
    assert!(duration_ms >= 250.0, "measured {duration_ms}ms; expected >= 250ms to include the 300ms body-transfer delay");
  }

  #[test]
  fn charset_parsed_from_content_type() {
    assert_eq!(charset_from_content_type("text/html; charset=iso-8859-1"), Some("iso-8859-1"));
    assert_eq!(charset_from_content_type("text/plain;charset=\"UTF-16\""), Some("UTF-16"));
    assert_eq!(charset_from_content_type("application/json"), None);
    assert_eq!(charset_from_content_type("text/html; boundary=x"), None);
  }

  #[test]
  fn decode_body_honors_declared_charset() {
    // 0xE9 is 'e-acute' in ISO-8859-1 but invalid as standalone UTF-8.
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=iso-8859-1"));
    assert_eq!(decode_body(&headers, &[0xE9]), "\u{e9}");
  }

  #[test]
  fn decode_body_defaults_to_utf8() {
    let headers = HeaderMap::new();
    assert_eq!(decode_body(&headers, "hello \u{e9}".as_bytes()), "hello \u{e9}");
  }

  /// A request without `assign` must still fully drain a large body (so the
  /// timer covers the transfer and the connection can be reused), but must not
  /// retain it -- the chunks are dropped as they arrive rather than buffered.
  #[test]
  fn large_body_without_assign_is_drained_not_retained() {
    use std::collections::HashMap;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body_len = 1024 * 1024; // 1 MiB

    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      let mut buf = [0u8; 1024];
      let _ = stream.read(&mut buf).unwrap();

      let body = "x".repeat(body_len);
      let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
      stream.write_all(head.as_bytes()).unwrap();
      stream.write_all(body.as_bytes()).unwrap();
      stream.flush().unwrap();
    });

    let url = format!("http://{addr}/");
    let request = Request::simple_get("large-body", &url); // no `assign`
    let mut context: Context = Context::new();
    let pool: Pool = Arc::new(Mutex::new(HashMap::new()));
    let config = test_config();

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (response, _duration_ms) = runtime.block_on(request.send_request(&mut context, &pool, &config));

    server.join().unwrap();

    let response = response.expect("expected a successful response after draining the body");
    assert_eq!(response.status.as_u16(), 200);
    assert!(response.body.is_none(), "a non-assign body must be drained and dropped, not retained");
  }
}
