//! The `request` plan action: one HTTP request built from a plan item.
//!
//! - This file holds the [`Request`] type, [`Request::new`] (which reads a
//!   plan item and checks the method and headers before the benchmark starts)
//!   and the [`Runnable`] implementation that runs a request in the benchmark.
//! - `send` interpolates and sends one request and measures its latency.
//! - `classify` turns a failed request's `reqwest::Error` into a short label.
//! - `output` prints the per-request failure lines and the `--verbose` log.

mod classify;
mod output;
mod send;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use crate::yaml::Value as YamlValue;
use async_trait::async_trait;
use colored::Colorize;
use reqwest::{
  Method, StatusCode,
  header::{HeaderMap, HeaderName, HeaderValue},
};
use std::fs::File;
use std::io::Read;
use url::Url;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::actions::{extract, extract_optional};
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;

use crate::actions::{Report, Runnable};

use output::{log_message_response, log_response};

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
  method: Method,
  /// Custom headers in plan order: the validated name and the value template,
  /// which is interpolated per request.
  headers: Vec<(HeaderName, String)>,
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
      method: Method::GET,
      headers: Vec::new(),
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

    let method = match extract_optional(request_val, "method") {
      Some(value) => parse_method(&value),
      None => Method::GET,
    };

    let body_verbs = [Method::POST, Method::PATCH, Method::PUT];
    let body = if body_verbs.contains(&method) {
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

    let mut headers: Vec<(HeaderName, String)> = Vec::new();

    if let Some(mapping) = request_val.get("headers").and_then(|v| v.as_mapping()) {
      // The name each header was first written with, to report a second
      // spelling of the same (case-insensitive) name.
      let mut spellings: HashMap<HeaderName, &str> = HashMap::new();

      for (key, val) in mapping.iter() {
        // Mapping keys are always strings, so a key such as `1:` arrives as "1".
        // A missing or null key (`: value`, `~:`, `null:`) arrives as "null".
        // Header names are not interpolated, so validate each one here: a null,
        // empty or otherwise invalid name stops the run before any request is
        // sent, instead of panicking inside a worker mid-load.
        if key == "null" {
          panic!("{} Header name is empty or null!!", "WARNING!".yellow().bold());
        }
        let Ok(name) = HeaderName::from_bytes(key.as_bytes()) else {
          panic!("{} Invalid header name {key:?}: header names must be non-empty and use only token characters!!", "WARNING!".yellow().bold());
        };
        let Some(value) = val.as_str() else {
          panic!("{} Headers must be strings!!", "WARNING!".yellow().bold());
        };
        // HTTP header names are case-insensitive, so `Accept` and `accept` are
        // one header; only one of the two values could be sent.
        if let Some(first) = spellings.insert(name.clone(), key) {
          panic!("{} Header {key:?} is set twice (also as {first:?}): header names are case-insensitive, so set each header once!!", "WARNING!".yellow().bold());
        }
        // `{{` and `}}` are valid header characters, so checking the template
        // catches every invalid character outside the interpolations.
        if HeaderValue::from_str(value).is_err() {
          panic!("{} Invalid value for header {key:?}: header values cannot contain line breaks or other control characters (a `|` block scalar keeps its final line break; use `|-`)!!", "WARNING!".yellow().bold());
        }
        headers.push((name, value.to_string()));
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

  /// Formats a duration in milliseconds as `<n>ms`, or as `<n>ns` when
  /// `--nanosec` is set.
  fn format_time(tdiff: f64, nanosec: bool) -> String {
    if nanosec {
      (1_000_000.0 * tdiff).round().to_string() + "ns"
    } else {
      tdiff.round().to_string() + "ms"
    }
  }
}

/// Maps a plan's `method:` value to the HTTP method driller sends.
///
/// The value is case-insensitive (`post` works like `POST`). It is not
/// interpolated, so it is checked once, when the plan is loaded.
///
/// # Panics
///
/// Panics with a plan error that lists the supported methods when the value is
/// anything else, so the run stops before any request is sent.
fn parse_method(value: &str) -> Method {
  match value.to_uppercase().as_str() {
    "GET" => Method::GET,
    "POST" => Method::POST,
    "PUT" => Method::PUT,
    "PATCH" => Method::PATCH,
    "DELETE" => Method::DELETE,
    "HEAD" => Method::HEAD,
    _ => panic!("{} Unsupported method {value:?}: use GET, POST, PUT, PATCH, DELETE or HEAD!!", "WARNING!".yellow().bold()),
  }
}

/// Converts a plan value (a `with_items` entry) into JSON for the `{{ item }}`
/// interpolation context.
///
/// Integers stay integers (`i64`, then `u64` above `i64::MAX`), mapping keys are
/// kept as the strings the YAML parser produced, and tags are already removed by
/// `driller::yaml::from_str`.
///
/// # Panics
///
/// Panics on a node kind that has no JSON form.
fn yaml_to_json(data: YamlValue) -> Value {
  match data {
    YamlValue::Bool(b) => json!(b),
    YamlValue::Number(n) => {
      if let Some(i) = n.as_i64() {
        json!(i)
      } else if let Some(u) = n.as_u64() {
        // Unsigned integers above `i64::MAX` stay exact.
        json!(u)
      } else {
        json!(n.as_f64())
      }
    }
    YamlValue::String(s) => json!(s),
    YamlValue::Mapping(m) => {
      let mut map = Map::new();
      // Mapping keys are always strings, so every entry is kept; a YAML key
      // such as `1:` becomes the JSON key "1".
      for (key, value) in m.iter() {
        map.insert(key.clone(), yaml_to_json(value.clone()));
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
