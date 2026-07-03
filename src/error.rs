//! The single error type returned by the engine.
//!
//! The library reports fatal input problems by returning [`Error`] rather than
//! calling [`std::process::exit`], so an embedding caller -- an integration
//! test, a benchmark, or a sibling crate -- decides how a bad plan or an
//! unreadable file is handled instead of having its whole process terminated.
//! The `driller` binary maps each variant to an `error: <message>` line on
//! stderr and a non-zero exit code; the exit decision stays at that boundary.

/// Everything that can go wrong while loading a plan, building configuration,
/// or comparing a run against a baseline.
///
/// Every variant carries enough context to render a self-contained, user-facing
/// message via [`Display`](std::fmt::Display); the wrapped I/O, YAML, and CSV
/// errors are also reachable through [`std::error::Error::source`] for callers
/// that want the underlying cause.
///
/// The `Display` text and `source()` chain are derived with `thiserror`: each
/// variant's `#[error(...)]` template is its exact user-facing message, and a
/// field named `source` is wired as the underlying cause automatically.
/// Conversions stay explicit (each call site builds the variant with
/// `map_err`, attaching an `action`/`path`/`what`), so no blanket `#[from]` is
/// used -- that would discard the context those fields carry.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// An I/O failure while opening or reading a user-supplied file. `action` is
  /// the verb shown to the user (`"open"` or `"read"`) and `path` names the
  /// file, so the message reads as a bad-input problem rather than a crash.
  #[error("couldn't {action} {path}: {source}")]
  Io {
    /// The operation that failed, used as the verb in the message.
    action: &'static str,
    /// The file path the user supplied.
    path: String,
    /// The underlying I/O error.
    source: std::io::Error,
  },
  /// A YAML document failed to parse. `what` distinguishes the parse context
  /// (`"document"` for one record of a multi-document file, `"content"` for the
  /// whole file).
  #[error("failed to parse YAML {what}: {source}")]
  Yaml {
    /// The parse context, used in the message.
    what: &'static str,
    /// The underlying deserialization error.
    source: serde_yaml::Error,
  },
  /// A CSV header could not be parsed. `path` names the source file.
  #[error("couldn't parse CSV header in {path}: {source}")]
  Csv {
    /// The CSV file path the user supplied.
    path: String,
    /// The underlying CSV error.
    source: csv::Error,
  },
  /// A required top-level node (for example `plan`) was missing from the
  /// document; carries the accessor that was looked up.
  #[error("node missing on config: {0}")]
  MissingNode(String),
  /// A document expected to be a sequence of items was something else; carries
  /// a debug rendering of what was found.
  #[error("expected document to be a sequence, got: {0}")]
  NotASequence(String),
  /// The expanded plan contained no runnable items.
  #[error("empty benchmark")]
  EmptyPlan,
  /// No plan items survived tag filtering on the `--list-tasks` /
  /// `--list-tags` surface.
  #[error("no items")]
  NoItems,
  /// A `--compare` baseline file was empty or was not a list of recorded
  /// requests; carries the file path.
  #[error("comparison file '{0}' is empty or not a list of recorded requests")]
  EmptyComparison(String),
  /// Configuration failed validation (for example concurrency exceeding the
  /// iteration count); carries the explanatory message.
  #[error("{0}")]
  InvalidConfig(String),
  /// The `--compare` verdict: this many named requests regressed past the
  /// threshold. This is not an input error -- the per-request slowness lines
  /// are printed by the comparator; the binary turns this count into a
  /// non-zero exit code.
  #[error("{0} request(s) slower than the baseline")]
  Regressions(usize),
}

#[cfg(test)]
mod tests {
  use super::Error;
  use std::error::Error as _;

  /// The context-free variants render byte-for-byte the strings the CLI and
  /// tests depend on.
  #[test]
  fn display_messages_are_stable() {
    assert_eq!(Error::MissingNode("plan".into()).to_string(), "node missing on config: plan");
    assert_eq!(Error::NotASequence("Mapping".into()).to_string(), "expected document to be a sequence, got: Mapping");
    assert_eq!(Error::EmptyPlan.to_string(), "empty benchmark");
    assert_eq!(Error::NoItems.to_string(), "no items");
    assert_eq!(Error::EmptyComparison("base.jsonl".into()).to_string(), "comparison file 'base.jsonl' is empty or not a list of recorded requests");
    assert_eq!(Error::InvalidConfig("the concurrency can not be higher than the number of iterations".into()).to_string(), "the concurrency can not be higher than the number of iterations");
    assert_eq!(Error::Regressions(3).to_string(), "3 request(s) slower than the baseline");
  }

  /// `Io` interpolates action, path, and the underlying cause -- the
  /// `couldn't open <path>: ...` shape pinned by `tests/cli_errors.rs` -- and
  /// exposes the `io::Error` as its source.
  #[test]
  fn io_variant_interpolates_action_path_and_source() {
    let err = Error::Io {
      action: "open",
      path: "/no/such/plan.yml".into(),
      source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
    };
    assert_eq!(err.to_string(), "couldn't open /no/such/plan.yml: not found");
    assert!(err.source().is_some(), "Io must expose its io::Error as source");
  }

  /// The wrapped `Yaml`/`Csv` variants keep their exact template around the
  /// underlying error's own message, and forward `source()`.
  #[test]
  fn wrapped_variants_keep_template_and_source() {
    let yaml = serde_yaml::from_str::<i32>("not an integer").unwrap_err();
    let yaml_display = yaml.to_string();
    let err = Error::Yaml {
      what: "document",
      source: yaml,
    };
    assert_eq!(err.to_string(), format!("failed to parse YAML document: {yaml_display}"));
    assert!(err.source().is_some());

    let csv_err = {
      let mut rdr = csv::ReaderBuilder::new().from_reader("a,b\n1,2,3\n".as_bytes());
      rdr.records().next().expect("a record").unwrap_err()
    };
    let csv_display = csv_err.to_string();
    let err = Error::Csv {
      path: "data.csv".into(),
      source: csv_err,
    };
    assert_eq!(err.to_string(), format!("couldn't parse CSV header in data.csv: {csv_display}"));
    assert!(err.source().is_some());
  }

  /// The variants that wrap no lower-level error have no source.
  #[test]
  fn context_free_variants_have_no_source() {
    assert!(Error::EmptyPlan.source().is_none());
    assert!(Error::Regressions(1).source().is_none());
    assert!(Error::InvalidConfig("bad".into()).source().is_none());
  }
}
