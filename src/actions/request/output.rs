//! The lines driller prints for each request: the `ERR` line for a failed
//! request and the `--verbose` request and response log.

use colored::Colorize;
use reqwest::header::HeaderName;
use std::fmt::Write;

use crate::config::Config;

use super::classify::{classify_connection_error, error_source_chain};
use super::{Request, ResponseData};

/// Prints the classified, colored failure line for a request that produced no
/// usable response. The format matches the success line (`name | url | status |
/// time`), substituting a red `ERR <cause>` marker for the HTTP status. Under
/// `--verbose` the full `source()` chain is appended on a dimmed `cause:` line
/// so the debugging detail from the old raw dump is still available on demand.
/// No purple hues.
pub(super) fn log_request_failure(name: &str, url: &str, duration_ms: f64, error: &reqwest::Error, config: &Config) {
  let label = classify_connection_error(error);
  println!("{:width$} {} {} {}", name.green(), url.blue().bold(), format!("ERR {label}").red(), Request::format_time(duration_ms, config.nanosec).cyan(), width = 25);
  if config.verbose {
    println!("  {} {}", "cause:".dimmed(), error_source_chain(error));
  }
}

/// Prints the failure line for a request that was not sent because an
/// interpolated header value is not a valid HTTP header value.
///
/// Same layout as [`log_request_failure`], with `ERR invalid value for header
/// <name>` in place of the connection error; under `--verbose` a `cause:` line
/// says why.
pub(super) fn log_invalid_header_value(name: &str, url: &str, header: &HeaderName, config: &Config) {
  println!("{:width$} {} {} {}", name.green(), url.blue().bold(), format!("ERR invalid value for header {header}").red(), Request::format_time(0.0, config.nanosec).cyan(), width = 25);
  if config.verbose {
    println!("  {} the interpolated value contains a line break or another control character", "cause:".dimmed());
  }
}

/// Prints the `>>>` line `--verbose` shows before a request is sent: URL,
/// method and headers.
pub(super) fn log_request(request: &reqwest::Request) {
  let mut message = String::new();
  write!(message, "{}", ">>>".bold().green()).unwrap();
  write!(message, " {} {},", "URL:".bold(), request.url()).unwrap();
  write!(message, " {} {},", "METHOD:".bold(), request.method()).unwrap();
  write!(message, " {} {:?}", "HEADERS:".bold(), request.headers()).unwrap();
  println!("{message}");
}

/// Builds the response part of the `<<<` line `--verbose` shows: URL, status,
/// headers and duration, or `No response from server!` for a failed request.
pub(super) fn log_message_response(response: &Option<ResponseData>, duration_ms: f64) -> String {
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

/// Prints the `<<<` line `--verbose` shows after a request, with the response
/// body when the request has an `assign`.
pub(super) fn log_response(log_message_response: String, body: &Option<String>) {
  let mut message = String::new();
  write!(message, "{}{}", "<<<".bold().green(), log_message_response).unwrap();
  if let Some(body) = body.as_ref() {
    write!(message, " {} {:?}", "BODY:".bold(), body).unwrap()
  }
  println!("{message}");
}
