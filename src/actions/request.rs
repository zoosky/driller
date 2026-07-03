use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use colored::Colorize;
use encoding_rs::{Encoding, UTF_8};
use reqwest::{
  ClientBuilder, Method, StatusCode,
  header::{self, HeaderMap, HeaderName, HeaderValue},
};
use serde_yaml::Value as YamlValue;
use std::fmt::Write;
use std::fs::File;
use std::io::Read;
use url::Url;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::actions::{extract, extract_optional};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

use crate::actions::{Report, Runnable};

static USER_AGENT: &str = "driller";

#[derive(Clone)]
pub enum Body {
  Template(String),
  Binary(Vec<u8>),
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct Request {
  name: String,
  url: String,
  time: f64,
  method: String,
  headers: HashMap<String, String>,
  pub body: Option<Body>,
  pub with_item: Option<YamlValue>,
  pub index: Option<u32>,
  pub assign: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct AssignedRequest {
  status: u16,
  body: Value,
  headers: Map<String, Value>,
}

/// An owned snapshot of an HTTP response, captured before its body is consumed.
///
/// The latency timer now spans the full body download (time-to-last-byte), which
/// means the streaming `reqwest::Response` is consumed while the clock is still
/// running. Anything that borrows the response -- status, headers, cookies, and
/// the final URL -- is therefore cloned out into this struct *before* the body is
/// drained, so callers retain access to it afterwards.
struct ResponseData {
  /// Final request URL, used by verbose response logging.
  url: Url,
  /// HTTP status of the response.
  status: StatusCode,
  /// Response headers, surfaced to later plan steps through `assign`.
  headers: HeaderMap,
  /// `Set-Cookie` name/value pairs, materialized for the cookie jar.
  cookies: Vec<(String, String)>,
  /// Decoded response body, retained only when the request has an `assign`.
  body: Option<String>,
}

impl Request {
  /// Creates a minimal GET request without parsing YAML.
  pub fn simple_get(name: &str, url: &str) -> Request {
    Request {
      name: name.to_string(),
      url: url.to_string(),
      time: 0.0,
      method: "GET".to_string(),
      headers: HashMap::new(),
      body: None,
      with_item: None,
      index: None,
      assign: None,
    }
  }

  pub fn is_that_you(item: &YamlValue) -> bool {
    item.get("request").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &YamlValue, with_item: Option<YamlValue>, index: Option<u32>) -> Request {
    let name = extract(item, "name");
    let request_val = item.get("request").expect("request field is required");
    let url = extract(request_val, "url");
    let assign = extract_optional(item, "assign");

    let method = if let Some(v) = extract_optional(request_val, "method") {
      v.to_uppercase()
    } else {
      "GET".to_string()
    };

    let body_verbs = ["POST", "PATCH", "PUT"];
    let body = if body_verbs.contains(&method.as_str()) {
      if let Some(body) = request_val.get("body").and_then(|v| v.as_str()) {
        Some(Body::Template(body.to_string()))
      } else if let Some(file_path) = request_val.get("body").and_then(|v| v.get("file")).and_then(|v| v.as_str()) {
        let mut file = File::open(file_path).expect("Unable to open file");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).expect("Unable to read file");
        Some(Body::Binary(buffer))
      } else if let Some(hex_str) = request_val.get("body").and_then(|v| v.get("hex")).and_then(|v| v.as_str()) {
        Some(Body::Binary(hex::decode(hex_str).expect("Invalid hex string")))
      } else {
        panic!("{} Body must be string, file or hex!!", "WARNING!".yellow().bold());
      }
    } else {
      None
    };

    let mut headers = HashMap::new();

    if let Some(mapping) = request_val.get("headers").and_then(|v| v.as_mapping()) {
      for (key, val) in mapping.iter() {
        if let Some(vs) = val.as_str() {
          if let Some(key_str) = key.as_str() {
            headers.insert(key_str.to_string(), vs.to_string());
          } else {
            panic!("{} Header keys must be strings!!", "WARNING!".yellow().bold());
          }
        } else {
          panic!("{} Headers must be strings!!", "WARNING!".yellow().bold());
        }
      }
    }

    Request {
      name,
      url,
      time: 0.0,
      method,
      headers,
      body,
      with_item,
      index,
      assign,
    }
  }

  fn format_time(tdiff: f64, nanosec: bool) -> String {
    if nanosec {
      (1_000_000.0 * tdiff).round().to_string() + "ns"
    } else {
      tdiff.round().to_string() + "ms"
    }
  }

  async fn send_request(&self, context: &mut Context, pool: &Pool, config: &Config) -> (Option<ResponseData>, f64) {
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

    // Method
    let method = match self.method.to_uppercase().as_ref() {
      "GET" => Method::GET,
      "POST" => Method::POST,
      "PUT" => Method::PUT,
      "PATCH" => Method::PATCH,
      "DELETE" => Method::DELETE,
      "HEAD" => Method::HEAD,
      _ => panic!("Unknown method '{}'", self.method),
    };

    // Canonical method label, captured before `method` is moved into the request
    // builder so a failure line (which has no response status to show) can still
    // name the verb that was attempted.
    let method_label = method.as_str().to_string();

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
      let cookie = cookies.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(";");

      headers.insert(header::COOKIE, HeaderValue::from_str(&cookie).unwrap());
    }

    // Resolve headers
    for (key, val) in self.headers.iter() {
      let interpolated_header = uninterpolator.get_or_insert(interpolator::Interpolator::new(context)).resolve(val, !config.relaxed_interpolations);
      headers.insert(HeaderName::from_bytes(key.as_bytes()).unwrap(), HeaderValue::from_str(&interpolated_header).unwrap());
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
          log_request_failure(&interpolated_name, interpolated_base_url.as_str(), &method_label, duration_ms, &e, config);
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
        log_request_failure(&interpolated_name, interpolated_base_url.as_str(), &method_label, duration_ms, &e, config);
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

/// The classification-relevant facts pulled out of a `reqwest::Error`.
///
/// Fact extraction is deliberately separated from the decision below so the
/// branching logic is a pure function of plain data. `reqwest::Error` has no
/// public constructor, so this split is also what makes the classifier
/// unit-testable at all: a test builds an `ErrorFacts` by hand instead of trying
/// to synthesize a real network failure.
struct ErrorFacts {
  /// The request did not complete within the configured timeout.
  is_timeout: bool,
  /// The failure happened while establishing the connection.
  is_connect: bool,
  /// The client gave up after following too many redirects.
  is_redirect: bool,
  /// The failure was in reading or decoding the response body.
  is_body_or_decode: bool,
  /// `ErrorKind` of the first `std::io::Error` found in the source chain, if any.
  io_kind: Option<std::io::ErrorKind>,
  /// Lowercased concatenation of every `Display` in the `source()` chain, used to
  /// spot DNS- and TLS-level causes that reqwest does not expose as predicates.
  source_text: String,
}

impl ErrorFacts {
  /// Walks a `reqwest::Error` and its `source()` chain, recording the high-level
  /// predicates, the first io-error kind, and the accumulated (lowercased) chain
  /// text used for keyword matching.
  fn from_error(e: &reqwest::Error) -> ErrorFacts {
    use std::error::Error as _;

    let mut source_text = String::new();
    let mut io_kind = None;
    let mut source = e.source();
    while let Some(err) = source {
      source_text.push_str(&err.to_string());
      source_text.push(' ');
      if io_kind.is_none()
        && let Some(io_err) = err.downcast_ref::<std::io::Error>()
      {
        io_kind = Some(io_err.kind());
      }
      source = err.source();
    }
    source_text.make_ascii_lowercase();

    ErrorFacts {
      is_timeout: e.is_timeout(),
      is_connect: e.is_connect(),
      is_redirect: e.is_redirect(),
      is_body_or_decode: e.is_body() || e.is_decode(),
      io_kind,
      source_text,
    }
  }

  /// Heuristic: does the source chain read like a name-resolution failure?
  fn mentions_dns(&self) -> bool {
    const NEEDLES: [&str; 6] = ["dns", "failed to lookup address", "name or service not known", "nodename nor servname", "no such host", "name resolution"];
    NEEDLES.iter().any(|needle| self.source_text.contains(needle))
  }

  /// Heuristic: does the source chain read like a TLS/certificate failure?
  fn mentions_tls(&self) -> bool {
    const NEEDLES: [&str; 4] = ["tls", "ssl", "certificate", "handshake"];
    NEEDLES.iter().any(|needle| self.source_text.contains(needle))
  }
}

/// Maps the extracted [`ErrorFacts`] to a short, plain-language cause.
///
/// The order mirrors reqwest's own layering: the high-level predicates (timeout,
/// redirect) win first, then a connect-time failure is refined into refused /
/// DNS / TLS / generic using the io kind and source-chain keywords, and finally
/// body/decode failures fall through to a generic label. Every branch returns a
/// `&'static str` free of Rust type names.
fn classify_facts(facts: &ErrorFacts) -> &'static str {
  if facts.is_timeout {
    return "connection timed out";
  }
  if facts.is_redirect {
    return "too many redirects";
  }
  if facts.is_connect {
    if facts.io_kind == Some(std::io::ErrorKind::ConnectionRefused) {
      return "connection refused";
    }
    if facts.mentions_dns() {
      return "DNS resolution failed";
    }
    if facts.mentions_tls() {
      return "TLS error";
    }
    return "could not connect";
  }
  if facts.mentions_tls() {
    return "TLS error";
  }
  if facts.is_body_or_decode {
    return "response body error";
  }
  "request failed"
}

/// Classifies a failed request's `reqwest::Error` into a short, human-readable
/// cause -- e.g. `connection timed out`, `connection refused`,
/// `DNS resolution failed`, `TLS error` -- for the per-step failure line, so a
/// normal network outcome no longer surfaces as a raw `Debug` struct dump that
/// reads like a panic. Anything unclassifiable falls back to `request failed`.
fn classify_connection_error(e: &reqwest::Error) -> &'static str {
  classify_facts(&ErrorFacts::from_error(e))
}

/// Renders the full `source()` chain of an error as a single `": "`-joined line.
///
/// This is the `--verbose` `cause:` detail that replaces the old `{:?}` dump:
/// the terse classified label stays on the default line, and the underlying
/// chain (timeout kind, os error number, ...) is available here for debugging.
fn error_source_chain(e: &reqwest::Error) -> String {
  use std::error::Error as _;

  let mut chain = e.to_string();
  let mut source = e.source();
  while let Some(err) = source {
    write!(chain, ": {err}").unwrap();
    source = err.source();
  }
  chain
}

/// Prints the classified, colored failure line for a request that produced no
/// usable response, aligned with (and styled like) the success line in
/// `send_request`. The `ERR <cause>` marker is red; under `--verbose` the full
/// `source()` chain is appended on a dimmed `cause:` line so the debugging detail
/// from the old raw dump is still available on demand. No purple hues.
fn log_request_failure(name: &str, url: &str, method: &str, duration_ms: f64, error: &reqwest::Error, config: &Config) {
  let label = classify_connection_error(error);
  println!("{:width$} {} {} {} {}", name.green(), url.blue().bold(), method, format!("ERR {label}").red(), Request::format_time(duration_ms, config.nanosec).cyan(), width = 25);
  if config.verbose {
    println!("  {} {}", "cause:".dimmed(), error_source_chain(error));
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

fn yaml_to_json(data: YamlValue) -> Value {
  match data {
    YamlValue::Bool(b) => json!(b),
    YamlValue::Number(n) => {
      if let Some(i) = n.as_i64() {
        json!(i)
      } else if let Some(f) = n.as_f64() {
        json!(f)
      } else {
        // Fallback: convert to string representation
        json!(n.to_string())
      }
    }
    YamlValue::String(s) => json!(s),
    YamlValue::Mapping(m) => {
      let mut map = Map::new();
      for (key, value) in m.iter() {
        if let Some(key_str) = key.as_str() {
          map.insert(key_str.to_string(), yaml_to_json(value.clone()));
        }
      }
      json!(map)
    }
    YamlValue::Sequence(v) => {
      let mut array = Vec::new();
      for value in v.iter() {
        array.push(yaml_to_json(value.clone()));
      }
      json!(array)
    }
    YamlValue::Null => json!(null),
    _ => panic!("Unknown Yaml node"),
  }
}

#[async_trait]
impl Runnable for Request {
  async fn execute(&self, context: &mut Context, reports: &mut Reports, pool: &Pool, config: &Config) {
    if let Some(ref item) = self.with_item {
      context.insert("item".to_string(), yaml_to_json(item.clone()));
    }

    if let Some(index) = self.index {
      context.insert("index".to_string(), json!(index));
    }

    let (res, duration_ms) = self.send_request(context, pool, config).await;

    let log_message_response = if config.verbose {
      Some(log_message_response(&res, duration_ms))
    } else {
      None
    };

    match res {
      None => {
        reports.push(Report {
          name: self.name.to_owned(),
          duration: duration_ms,
          status: 520u16,
        });

        // In verbose mode still emit the response marker so connection and
        // body-read failures are visible in the request/response log (no body).
        if let Some(msg) = log_message_response {
          log_response(msg, &None);
        }
      }
      Some(response) => {
        let status = response.status.as_u16();

        reports.push(Report {
          name: self.name.to_owned(),
          duration: duration_ms,
          status,
        });

        for (name, value) in &response.cookies {
          let cookies = context.entry("cookies").or_insert_with(|| json!({})).as_object_mut().unwrap();
          cookies.insert(name.clone(), json!(value));
        }

        let data = if let Some(ref key) = self.assign {
          let mut headers = Map::new();

          response.headers.iter().for_each(|(header, value)| {
            headers.insert(header.to_string(), json!(value.to_str().unwrap()));
          });

          let data = response.body.clone().unwrap_or_default();

          let body: Value = serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);

          let assigned = AssignedRequest {
            status,
            body,
            headers,
          };

          let value = serde_json::to_value(assigned).unwrap();

          context.insert(key.to_owned(), value);

          Some(data)
        } else {
          None
        };

        if let Some(msg) = log_message_response {
          log_response(msg, &data)
        }
      }
    }
  }
}

fn log_request(request: &reqwest::Request) {
  let mut message = String::new();
  write!(message, "{}", ">>>".bold().green()).unwrap();
  write!(message, " {} {},", "URL:".bold(), request.url()).unwrap();
  write!(message, " {} {},", "METHOD:".bold(), request.method()).unwrap();
  write!(message, " {} {:?}", "HEADERS:".bold(), request.headers()).unwrap();
  println!("{message}");
}

fn log_message_response(response: &Option<ResponseData>, duration_ms: f64) -> String {
  let mut message = String::new();
  match response {
    Some(response) => {
      write!(message, " {} {},", "URL:".bold(), response.url).unwrap();
      write!(message, " {} {},", "STATUS:".bold(), response.status).unwrap();
      write!(message, " {} {:?}", "HEADERS:".bold(), response.headers).unwrap();
      write!(message, " {} {:.4} ms,", "DURATION:".bold(), duration_ms).unwrap();
    }
    None => {
      message = String::from("No response from server!");
    }
  }
  message
}

fn log_response(log_message_response: String, body: &Option<String>) {
  let mut message = String::new();
  write!(message, "{}{}", "<<<".bold().green(), log_message_response).unwrap();
  if let Some(body) = body.as_ref() {
    write!(message, " {} {:?}", "BODY:".bold(), body).unwrap()
  }
  println!("{message}");
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_yaml::Value as YamlValue;
  use std::io::Write;
  use tempfile::NamedTempFile;

  fn create_yaml_request_with_string_body(body_content: &str) -> YamlValue {
    let yaml_str = format!(
      r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body: "{}"
"#,
      body_content
    );
    serde_yaml::from_str(&yaml_str).unwrap()
  }

  fn create_yaml_request_with_hex_body(hex_content: &str) -> YamlValue {
    let yaml_str = format!(
      r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    hex: "{}"
"#,
      hex_content
    );
    serde_yaml::from_str(&yaml_str).unwrap()
  }

  fn create_yaml_request_with_file_body(file_path: &str) -> YamlValue {
    let yaml_str = format!(
      r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    file: "{}"
"#,
      file_path
    );
    serde_yaml::from_str(&yaml_str).unwrap()
  }

  #[test]
  fn test_body_template_string() {
    let yaml = create_yaml_request_with_string_body("Hello, World!");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Template(content)) => {
        assert_eq!(content, "Hello, World!");
      }
      _ => panic!("Expected Body::Template"),
    }
  }

  #[test]
  fn test_body_hex() {
    // "Hello" in hex is "48656c6c6f"
    let yaml = create_yaml_request_with_hex_body("48656c6c6f");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_hex_empty() {
    let yaml = create_yaml_request_with_hex_body("");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"");
      }
      _ => panic!("Expected Body::Binary with empty data"),
    }
  }

  #[test]
  fn test_body_hex_complex() {
    // "Hello, World!" in hex
    let yaml = create_yaml_request_with_hex_body("48656c6c6f2c20576f726c6421");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello, World!");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_file() {
    // Create a temporary file with test content
    let mut temp_file = NamedTempFile::new().unwrap();
    let test_content = b"Test file content";
    temp_file.write_all(test_content).unwrap();
    temp_file.flush().unwrap();

    let file_path = temp_file.path().to_str().unwrap();
    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, test_content);
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_file_empty() {
    // Create an empty temporary file
    let temp_file = NamedTempFile::new().unwrap();
    let file_path = temp_file.path().to_str().unwrap();

    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"");
      }
      _ => panic!("Expected Body::Binary with empty data"),
    }
  }

  #[test]
  fn test_body_file_binary_data() {
    // Create a file with binary data (not UTF-8)
    let mut temp_file = NamedTempFile::new().unwrap();
    let binary_content = vec![0x00, 0x01, 0x02, 0xFF, 0xFE, 0xFD];
    temp_file.write_all(&binary_content).unwrap();
    temp_file.flush().unwrap();

    let file_path = temp_file.path().to_str().unwrap();
    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, binary_content);
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_file_large_content() {
    // Create a file with larger content
    let mut temp_file = NamedTempFile::new().unwrap();
    let large_content: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    temp_file.write_all(&large_content).unwrap();
    temp_file.flush().unwrap();

    let file_path = temp_file.path().to_str().unwrap();
    let yaml = create_yaml_request_with_file_body(file_path);

    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data.len(), 10000);
        assert_eq!(data, large_content);
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_none_for_get() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: GET
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    assert!(request.body.is_none());
  }

  #[test]
  fn test_body_none_for_delete() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: DELETE
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    assert!(request.body.is_none());
  }

  #[test]
  fn test_body_hex_uppercase() {
    // Test that hex decoding works with uppercase letters
    let yaml = create_yaml_request_with_hex_body("48656C6C6F");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  fn test_body_hex_mixed_case() {
    // Test that hex decoding works with mixed case
    let yaml = create_yaml_request_with_hex_body("48656c6C6F");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Hello");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

  #[test]
  #[should_panic(expected = "Invalid hex string")]
  fn test_body_hex_invalid() {
    let yaml = create_yaml_request_with_hex_body("InvalidHexString!");
    Request::new(&yaml, None, None);
  }

  #[test]
  #[should_panic(expected = "Unable to open file")]
  fn test_body_file_not_found() {
    let yaml = create_yaml_request_with_file_body("/nonexistent/path/to/file.txt");
    Request::new(&yaml, None, None);
  }

  #[test]
  fn test_body_priority_string_over_hex() {
    // When body is a string, it should be treated as Template, not hex
    let yaml = create_yaml_request_with_string_body("48656c6c6f");
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Template(content)) => {
        assert_eq!(content, "48656c6c6f");
      }
      _ => panic!("Expected Body::Template when body is a string"),
    }
  }

  #[test]
  fn test_body_put_method() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: PUT
  body: "PUT body content"
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Template(content)) => {
        assert_eq!(content, "PUT body content");
      }
      _ => panic!("Expected Body::Template"),
    }
  }

  #[test]
  fn test_body_patch_method() {
    let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: PATCH
  body:
    hex: "5061746368"
"#;
    let yaml: YamlValue = serde_yaml::from_str(yaml_str).unwrap();
    let request = Request::new(&yaml, None, None);

    match request.body {
      Some(Body::Binary(data)) => {
        assert_eq!(data, b"Patch");
      }
      _ => panic!("Expected Body::Binary"),
    }
  }

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

  /// Builds an `ErrorFacts` directly (there is no public `reqwest::Error`
  /// constructor). `source_text` is lowercased to mirror `from_error`.
  fn facts(is_timeout: bool, is_connect: bool, is_redirect: bool, is_body_or_decode: bool, io_kind: Option<std::io::ErrorKind>, source_text: &str) -> ErrorFacts {
    ErrorFacts {
      is_timeout,
      is_connect,
      is_redirect,
      is_body_or_decode,
      io_kind,
      source_text: source_text.to_lowercase(),
    }
  }

  #[test]
  fn classify_facts_maps_representative_errors() {
    use std::io::ErrorKind;

    // Timeout wins over everything, including a refused io kind underneath it.
    assert_eq!(classify_facts(&facts(true, true, false, false, Some(ErrorKind::ConnectionRefused), "connection refused")), "connection timed out");
    assert_eq!(classify_facts(&facts(true, false, false, false, None, "")), "connection timed out");

    // Redirect loop.
    assert_eq!(classify_facts(&facts(false, false, true, false, None, "")), "too many redirects");

    // Connect-time failures, refined by io kind then source-chain keywords.
    assert_eq!(classify_facts(&facts(false, true, false, false, Some(ErrorKind::ConnectionRefused), "connection refused (os error 61)")), "connection refused");
    assert_eq!(classify_facts(&facts(false, true, false, false, None, "failed to lookup address information: nodename nor servname provided")), "DNS resolution failed");
    assert_eq!(classify_facts(&facts(false, true, false, false, None, "invalid peer certificate: UnknownIssuer")), "TLS error");
    assert_eq!(classify_facts(&facts(false, true, false, false, None, "network is unreachable (os error 51)")), "could not connect");

    // TLS handshake failure that is not reported as a connect error.
    assert_eq!(classify_facts(&facts(false, false, false, false, None, "unexpected eof during tls handshake")), "TLS error");

    // Body / decode failure and the generic fallback.
    assert_eq!(classify_facts(&facts(false, false, false, true, None, "")), "response body error");
    assert_eq!(classify_facts(&facts(false, false, false, false, None, "something unexpected")), "request failed");
  }

  /// End-to-end check that the classifier maps a *real* `reqwest::Error` (not a
  /// hand-built `ErrorFacts`) to the expected label: a connect to a just-closed
  /// localhost port is refused, exercising `is_connect()` + the io-kind walk.
  #[test]
  fn classifies_real_connection_refused() {
    use std::net::TcpListener;

    // Bind to grab a free port, then drop the listener so the port is closed and
    // connects are refused (RST) rather than left hanging.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://{addr}/");

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let error = runtime.block_on(async { reqwest::Client::new().get(&url).timeout(Duration::from_secs(5)).send().await.unwrap_err() });

    assert_eq!(classify_connection_error(&error), "connection refused");
  }
}
