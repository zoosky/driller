//! Turning a failed request's `reqwest::Error` into a short, plain-language
//! label (`connection refused`, `request timed out`, ...) for the `ERR` line.

use std::fmt::Write;

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
    return "request timed out";
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
/// cause -- e.g. `request timed out`, `connection refused`,
/// `DNS resolution failed`, `TLS error` -- for the per-step failure line, so a
/// normal network outcome no longer surfaces as a raw `Debug` struct dump that
/// reads like a panic. Anything unclassifiable falls back to `request failed`.
pub(super) fn classify_connection_error(e: &reqwest::Error) -> &'static str {
  classify_facts(&ErrorFacts::from_error(e))
}

/// Renders the full `source()` chain of an error as a single `": "`-joined line.
///
/// This is the `--verbose` `cause:` detail that replaces the old `{:?}` dump:
/// the terse classified label stays on the default line, and the underlying
/// chain (timeout kind, os error number, ...) is available here for debugging.
pub(super) fn error_source_chain(e: &reqwest::Error) -> String {
  use std::error::Error as _;

  let mut chain = e.to_string();
  let mut source = e.source();
  while let Some(err) = source {
    write!(chain, ": {err}").unwrap();
    source = err.source();
  }
  chain
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

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
    assert_eq!(classify_facts(&facts(true, true, false, false, Some(ErrorKind::ConnectionRefused), "connection refused")), "request timed out");
    assert_eq!(classify_facts(&facts(true, false, false, false, None, "")), "request timed out");

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
