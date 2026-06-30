//! The single error type returned by the engine.
//!
//! The library reports fatal input problems by returning [`Error`] rather than
//! calling [`std::process::exit`], so an embedding caller -- an integration
//! test, a benchmark, or a sibling crate -- decides how a bad plan or an
//! unreadable file is handled instead of having its whole process terminated.
//! The `driller` binary maps each variant to an `error: <message>` line on
//! stderr and a non-zero exit code; the exit decision stays at that boundary.

use std::fmt;

/// Everything that can go wrong while loading a plan, building configuration,
/// or comparing a run against a baseline.
///
/// Every variant carries enough context to render a self-contained, user-facing
/// message via [`Display`](fmt::Display); the wrapped I/O, YAML, and CSV errors
/// are also reachable through [`std::error::Error::source`] for callers that
/// want the underlying cause.
#[derive(Debug)]
pub enum Error {
  /// An I/O failure while opening or reading a user-supplied file. `action` is
  /// the verb shown to the user (`"open"` or `"read"`) and `path` names the
  /// file, so the message reads as a bad-input problem rather than a crash.
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
  Yaml {
    /// The parse context, used in the message.
    what: &'static str,
    /// The underlying deserialization error.
    source: serde_yaml::Error,
  },
  /// A CSV header could not be parsed. `path` names the source file.
  Csv {
    /// The CSV file path the user supplied.
    path: String,
    /// The underlying CSV error.
    source: csv::Error,
  },
  /// A required top-level node (for example `plan`) was missing from the
  /// document; carries the accessor that was looked up.
  MissingNode(String),
  /// A document expected to be a sequence of items was something else; carries
  /// a debug rendering of what was found.
  NotASequence(String),
  /// The expanded plan contained no runnable items.
  EmptyPlan,
  /// No plan items survived tag filtering on the `--list-tasks` /
  /// `--list-tags` surface.
  NoItems,
  /// A `--compare` baseline file was empty or was not a list of recorded
  /// requests; carries the file path.
  EmptyComparison(String),
  /// Configuration failed validation (for example concurrency exceeding the
  /// iteration count); carries the explanatory message.
  InvalidConfig(String),
  /// The `--compare` verdict: this many named requests regressed past the
  /// threshold. This is not an input error -- the per-request slowness lines
  /// are printed by the comparator; the binary turns this count into a
  /// non-zero exit code.
  Regressions(usize),
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Error::Io {
        action,
        path,
        source,
      } => write!(f, "couldn't {action} {path}: {source}"),
      Error::Yaml {
        what,
        source,
      } => write!(f, "failed to parse YAML {what}: {source}"),
      Error::Csv {
        path,
        source,
      } => write!(f, "couldn't parse CSV header in {path}: {source}"),
      Error::MissingNode(node) => write!(f, "node missing on config: {node}"),
      Error::NotASequence(found) => write!(f, "expected document to be a sequence, got: {found}"),
      Error::EmptyPlan => write!(f, "empty benchmark"),
      Error::NoItems => write!(f, "no items"),
      Error::EmptyComparison(path) => write!(f, "comparison file '{path}' is empty or not a list of recorded requests"),
      Error::InvalidConfig(msg) => write!(f, "{msg}"),
      Error::Regressions(count) => write!(f, "{count} request(s) slower than the baseline"),
    }
  }
}

impl std::error::Error for Error {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      Error::Io {
        source,
        ..
      } => Some(source),
      Error::Yaml {
        source,
        ..
      } => Some(source),
      Error::Csv {
        source,
        ..
      } => Some(source),
      _ => None,
    }
  }
}
