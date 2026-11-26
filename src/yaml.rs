//! The YAML types and functions driller parses benchmark plans with.
//!
//! This module is the single boundary between driller and its YAML crate.
//! Every other module imports [`Value`], [`Mapping`], [`Number`], [`Error`],
//! [`from_str`] and [`to_string`] from here rather than naming the crate, so
//! replacing the parser touches this file (and the characterization tests in
//! `tests/yaml_moat.rs`), not the whole engine. Library users can name the same
//! types, including the ones inside [`Value`]'s variants ([`Sequence`],
//! [`TaggedValue`], [`Tag`]) and [`Location`], as `driller::yaml::*` without
//! depending on the YAML crate themselves.
//!
//! The implementation is [`noyalib`](https://docs.rs/noyalib)'s
//! `serde_yaml`-compatible API: pure Rust, with no `unsafe` code. A few
//! properties differ from the `serde_yaml` crate driller used before and are
//! worth knowing when writing plan-handling code:
//!
//! - Mapping keys are always strings, written in a standard form: `1:` is
//!   `"1"`, `0x10:` is `"16"`, `1e3:` is `"1000.0"`, `true:` is `"true"`, and
//!   `~:`, `null:` or an empty key (`: value`) is `"null"`.
//! - Two keys that become the same string (`1:` and `"1":`) are a parse error,
//!   and so is a sequence or mapping used as a key.
//! - [`to_string`] output has no trailing newline and indents sequence items
//!   under their key.
//!
//! # Limits
//!
//! [`from_str`] keeps the two limits `serde_yaml` enforced, so hostile input
//! still fails fast: nesting deeper than 128 levels, and alias expansion beyond
//! 100 jumps per parser event (which stops "billion laughs" documents). It
//! lifts noyalib's other default budgets (node, sequence, mapping, size and
//! alias-count caps), which `serde_yaml` did not have and which real plans and
//! large `--report` files exceed.
//!
//! # Examples
//!
//! ```
//! use driller::yaml::{self, Value};
//!
//! let plan: Value = yaml::from_str("name: Fetch users\nrequest:\n  url: /users\n").unwrap();
//! assert_eq!(plan["request"]["url"].as_str(), Some("/users"));
//!
//! // Mapping keys are strings, even when the YAML key looks like a number.
//! let doc: Value = yaml::from_str("1: one").unwrap();
//! let key = doc.as_mapping().unwrap().keys().next().unwrap();
//! assert_eq!(key, "1");
//! ```
//!
//! # Errors
//!
//! [`from_str`] returns an [`Error`] for malformed YAML, for keys that collide
//! or are not scalars, and when a limit above is exceeded. Driller wraps it in
//! [`crate::Error::Yaml`] together with the parse context.

pub use noyalib::compat::serde_yaml::{Error, Location, Mapping, Number, Sequence, Tag, TaggedValue, Value, to_string};
use noyalib::{DuplicateKeyPolicy, ParserConfig};

/// Parses a YAML document into `T` (driller always uses [`Value`]).
///
/// This is `noyalib`'s `serde_yaml`-compatible parse, with the limits described
/// in the [module docs](self): `serde_yaml`'s depth and alias-expansion limits
/// stay, noyalib's extra size budgets are lifted.
///
/// # Examples
///
/// ```
/// use driller::yaml::{self, Value};
///
/// // One anchor reused many times, as plans do for shared headers.
/// let mut src = String::from("h: &h {Accept: application/json}\nplan:\n");
/// for i in 0..50 {
///   src.push_str(&format!("  - {{name: r{i}, headers: *h}}\n"));
/// }
/// let doc: Value = yaml::from_str(&src).unwrap();
/// assert_eq!(doc["plan"][49]["headers"]["Accept"].as_str(), Some("application/json"));
/// ```
///
/// # Errors
///
/// Returns an [`Error`] for malformed YAML, duplicate or colliding keys,
/// non-scalar keys, nesting deeper than 128 levels, or runaway alias expansion.
pub fn from_str<T>(s: &str) -> Result<T, Error>
where
  T: serde::de::DeserializeOwned + 'static,
{
  noyalib::from_str_with_config(s, &parser_config()).map_err(|e| Error::from_noyalib_with_input(e, s))
}

/// The parser configuration behind [`from_str`].
///
/// Starts from noyalib's `serde_yaml` compatibility profile (scalar resolution,
/// error shape, depth 128 and the 100-jumps-per-event alias rule) and removes
/// the budgets `serde_yaml` never had. Duplicate keys stay an error, as they
/// were for `serde_yaml::Value`.
fn parser_config() -> ParserConfig {
  let base = ParserConfig::serde_yaml_compat();
  let mut limits = base.limits();
  limits.max_document_length = usize::MAX;
  limits.max_stream_bytes = usize::MAX;
  limits.max_alias_expansions = usize::MAX;
  limits.max_mapping_keys = usize::MAX;
  limits.max_sequence_length = usize::MAX;
  limits.max_events = usize::MAX;
  limits.max_nodes = usize::MAX;
  limits.max_total_scalar_bytes = usize::MAX;
  limits.max_documents = usize::MAX;
  limits.max_merge_keys = usize::MAX;
  limits.alias_anchor_ratio = None;
  let mut config = base.with_limits(limits);
  config.duplicate_key_policy = DuplicateKeyPolicy::Error;
  config
}
