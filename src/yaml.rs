//! The YAML types and functions driller parses benchmark plans with.
//!
//! This module is the single boundary between driller and its YAML crate.
//! Every other module imports [`Value`], [`Mapping`], [`Number`], [`Error`],
//! [`from_str`] and [`to_string`] from here rather than naming the crate, so
//! replacing the parser touches this file (and the characterization tests in
//! `tests/yaml_moat.rs`), not the whole engine. Library users can name the same
//! types as `driller::yaml::Value` without depending on the YAML crate
//! themselves.
//!
//! The implementation is [`noyalib`](https://docs.rs/noyalib)'s
//! `serde_yaml`-compatible API: pure Rust, with no `unsafe` code. Two
//! properties differ from the `serde_yaml` crate driller used before and are
//! worth knowing when writing plan-handling code:
//!
//! - Mapping keys are always strings. A YAML key such as `1:` or `true:` is
//!   read as the string `"1"` or `"true"`.
//! - [`to_string`] output has no trailing newline and indents sequence items
//!   under their key.
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
//! [`from_str`] returns an [`Error`] for malformed YAML. Driller wraps it in
//! [`crate::Error::Yaml`] together with the parse context.

pub use noyalib::compat::serde_yaml::{Error, Mapping, Number, Value, from_str, to_string};
