//! Characterization ("moat") tests for every way driller uses YAML.
//!
//! driller reads plans through `driller::yaml`, its single YAML boundary, which
//! re-exports `noyalib`'s `serde_yaml`-compatible API. These tests pin the exact
//! behaviour that boundary gives, so swapping the YAML crate becomes a readable
//! list of concrete diffs instead of a silent behavioural migration. They
//! characterise *the library*, not driller's wrapper functions, so they survive
//! refactors of `src/` and isolate the dependency boundary.
//!
//! The suite was first captured against `serde_yaml` 0.9. Where `noyalib`
//! behaves differently, the test asserts the current behaviour and a comment
//! records what `serde_yaml` 0.9 did.
//!
//! Each test carries a `mirrors:` comment naming the driller call site whose
//! behaviour it protects. A failure here points directly at the code that
//! would break under a replacement.
//!
//! ## Running the moat against a replacement candidate
//!
//! 1. Point `src/yaml.rs` at the candidate's API and swap the dependency in
//!    `Cargo.toml`.
//! 2. `cargo test --test yaml_moat`.
//! 3. Each failure is a behavioural difference. Either absorb it in driller's
//!    code or reject the candidate.
//! 4. A green suite means the swap is behaviour-preserving for everything
//!    driller actually relies on.
//!
//! Assertions are golden (exact strings, exact ordering, exact `Option`/
//! `Result` shape) on purpose: a "compatible but different" crate must trip a
//! specific, legible failure rather than slip through a loose `is_some()`.

use driller::yaml::{self, Mapping, Number, Value};

// ---------------------------------------------------------------------------
// Scalar parsing + coercion accessors
//
// mirrors: src/config.rs (`as_str`, `as_i64`), src/expandable/* (`as_bool`,
// `as_i64`), and the pervasive `.get(k).and_then(|v| v.as_*())` pattern.
// ---------------------------------------------------------------------------
mod scalars {
  use super::*;

  #[test]
  fn parses_integer_as_i64_not_str() {
    // mirrors: read_i64_configuration — an int scalar must coerce via as_i64,
    // and must NOT masquerade as a string (the code branches on that).
    let v: Value = yaml::from_str("42").unwrap();
    assert_eq!(v.as_i64(), Some(42));
    assert_eq!(v.as_str(), None);
    assert_eq!(v.as_f64(), Some(42.0));
    assert_eq!(v.as_bool(), None);
  }

  #[test]
  fn parses_negative_integer() {
    // mirrors: read_i64_configuration — negative values are detected and
    // rejected by driller, so the sign must round-trip through as_i64.
    let v: Value = yaml::from_str("-7").unwrap();
    assert_eq!(v.as_i64(), Some(-7));
  }

  #[test]
  fn parses_float_as_f64_not_i64() {
    // mirrors: yaml_to_json (request.rs) — Number is probed as_i64 first, then
    // as_f64. A float must fail as_i64 and succeed as_f64.
    let v: Value = yaml::from_str("3.5").unwrap();
    assert_eq!(v.as_i64(), None);
    assert_eq!(v.as_f64(), Some(3.5));
  }

  #[test]
  fn parses_bool() {
    // mirrors: multi_request.rs / multi_iter_request.rs `shuffle` and `pick`.
    let t: Value = yaml::from_str("true").unwrap();
    let f: Value = yaml::from_str("false").unwrap();
    assert_eq!(t.as_bool(), Some(true));
    assert_eq!(f.as_bool(), Some(false));
    // A bare `yes`/`no` must NOT be a bool under YAML 1.2 (serde_yaml 0.9
    // semantics); driller relies on explicit true/false.
    let yes: Value = yaml::from_str("yes").unwrap();
    assert_eq!(yes.as_bool(), None);
    assert_eq!(yes.as_str(), Some("yes"));
  }

  #[test]
  fn parses_bare_and_quoted_string() {
    // mirrors: read_str_configuration (config.rs) and the url/body accessors.
    let bare: Value = yaml::from_str("hello").unwrap();
    let quoted: Value = yaml::from_str("\"hello\"").unwrap();
    assert_eq!(bare.as_str(), Some("hello"));
    assert_eq!(quoted.as_str(), Some("hello"));
  }

  #[test]
  fn numeric_string_when_quoted_stays_string() {
    // mirrors: read_i64_configuration — a quoted number is a string and is
    // routed through the interpolate-then-parse branch, not as_i64.
    let v: Value = yaml::from_str("\"42\"").unwrap();
    assert_eq!(v.as_i64(), None);
    assert_eq!(v.as_str(), Some("42"));
  }
}

// ---------------------------------------------------------------------------
// Null handling
//
// mirrors: src/reader.rs — `matches!(doc, Value::Null)` guards drop empty /
// comment-only documents, and an empty file yields a single Null document.
// ---------------------------------------------------------------------------
mod nulls {
  use super::*;

  #[test]
  fn tilde_and_null_keyword_parse_to_null() {
    assert!(matches!(yaml::from_str::<Value>("~").unwrap(), Value::Null));
    assert!(matches!(yaml::from_str::<Value>("null").unwrap(), Value::Null));
  }

  #[test]
  fn empty_input_parses_to_null() {
    // mirrors: parse_yaml_content — an empty file must parse to Null (Ok), not
    // an error, so the reader can fall back to a single Null document.
    let v: Value = yaml::from_str("").unwrap();
    assert!(matches!(v, Value::Null));
  }

  #[test]
  fn comment_only_input_parses_to_null() {
    // mirrors: parse_yaml_content — comment-only parts are skipped because
    // they parse to Null.
    let v: Value = yaml::from_str("# just a comment").unwrap();
    assert!(matches!(v, Value::Null));
  }
}

// ---------------------------------------------------------------------------
// Mapping parsing, accessors, and missing-key behaviour
//
// mirrors: every action's `is_that_you` (`item.get(k).and_then(|v|
// v.as_mapping())`), config.rs `.get(name)`, request.rs nested
// `body.file` / `body.hex` lookups.
// ---------------------------------------------------------------------------
mod mappings {
  use super::*;

  fn doc() -> Value {
    yaml::from_str("name: foo\nrequest:\n  url: /api\n  body:\n    file: payload.json\nassert:\n  key: status\n  value: 200\n").unwrap()
  }

  #[test]
  fn as_mapping_some_on_map_none_on_scalar() {
    // mirrors: Request/Assert/Exec/Delay/Assign::is_that_you.
    let d = doc();
    assert!(d.as_mapping().is_some());
    assert!(d.get("request").and_then(|v| v.as_mapping()).is_some());
    assert!(d.get("name").and_then(|v| v.as_mapping()).is_none());
  }

  #[test]
  fn get_string_value() {
    let d = doc();
    assert_eq!(d.get("name").and_then(|v| v.as_str()), Some("foo"));
  }

  #[test]
  fn missing_key_is_none() {
    // mirrors: read_*_configuration default fallbacks and is_that_you probes.
    let d = doc();
    assert!(d.get("does_not_exist").is_none());
  }

  #[test]
  fn nested_get_chain() {
    // mirrors: request.rs — request.get("body").and_then(|v| v.get("file")).
    let d = doc();
    let file = d.get("request").and_then(|v| v.get("body")).and_then(|v| v.get("file")).and_then(|v| v.as_str());
    assert_eq!(file, Some("payload.json"));
  }

  #[test]
  fn nested_value_keeps_yaml_typing() {
    // mirrors: assert.rs — `value: 200` is an integer, exercised via extract's
    // as_i64-or-as_str fallback (actions/mod.rs extract_optional_number).
    let d = doc();
    let value = d.get("assert").and_then(|v| v.get("value"));
    assert_eq!(value.and_then(|v| v.as_i64()), Some(200));
    assert_eq!(value.and_then(|v| v.as_str()), None);
  }
}

// ---------------------------------------------------------------------------
// Sequence parsing + iteration
//
// mirrors: src/tags.rs (`tags` sequence, filter_map(as_str)), reader.rs
// `read_yaml_doc_accessor` (the `plan` sequence), multi_request.rs `with_items`.
// ---------------------------------------------------------------------------
mod sequences {
  use super::*;

  #[test]
  fn as_sequence_and_filter_map_as_str() {
    // mirrors: tags.rs should_skip_item — item.get("tags").as_sequence() then
    // iter().filter_map(|t| t.as_str()).
    let d: Value = yaml::from_str("tags:\n  - tag1\n  - tag2\n  - 3\n").unwrap();
    let seq = d.get("tags").and_then(|v| v.as_sequence()).unwrap();
    let strs: Vec<&str> = seq.iter().filter_map(|t| t.as_str()).collect();
    // The integer `3` is filtered out by as_str returning None — exactly the
    // tags.rs behaviour.
    assert_eq!(strs, vec!["tag1", "tag2"]);
  }

  #[test]
  fn as_sequence_none_on_mapping() {
    // mirrors: read_yaml_doc_accessor — a non-sequence node must yield None so
    // the reader can `die` with a clear message.
    let d: Value = yaml::from_str("plan:\n  not: a-sequence\n").unwrap();
    assert!(d.get("plan").and_then(|v| v.as_sequence()).is_none());
  }

  #[test]
  fn top_level_sequence() {
    // mirrors: read_yaml_doc_accessor with accessor=None — doc.as_sequence().
    let d: Value = yaml::from_str("- a\n- b\n").unwrap();
    let seq = d.as_sequence().unwrap();
    assert_eq!(seq.len(), 2);
    assert_eq!(seq[0].as_str(), Some("a"));
  }
}

// ---------------------------------------------------------------------------
// Mapping construction + keyed lookup (programmatic build)
//
// mirrors: src/reader.rs `read_csv_file_as_yml` (Mapping::new + insert +
// Value::Mapping) and src/expandable/multi_iter_request.rs (get by a string
// key, Number::from). Mapping keys are strings; serde_yaml 0.9 keyed mappings
// by `Value`.
// ---------------------------------------------------------------------------
mod construction {
  use super::*;

  #[test]
  fn build_mapping_like_csv_reader() {
    // mirrors: read_csv_file_as_yml — headers + record become a Value::Mapping.
    let mut mapping = Mapping::new();
    mapping.insert("id", Value::String("1".to_string()));
    mapping.insert("name", Value::String("ada".to_string()));
    let row = Value::Mapping(mapping);

    assert_eq!(row.get("id").and_then(|v| v.as_str()), Some("1"));
    assert_eq!(row.get("name").and_then(|v| v.as_str()), Some("ada"));
  }

  #[test]
  fn keyed_get_by_str() {
    // mirrors: multi_iter_request.rs — with_iter_items.get("step").
    // serde_yaml 0.9 looked the key up as `&Value::String("step")`.
    let m: Value = yaml::from_str("start: 2\nstep: 2\nstop: 20\n").unwrap();
    let mapping = m.as_mapping().unwrap();
    assert_eq!(mapping.get("step").and_then(|v| v.as_i64()), Some(2));
    assert!(mapping.get("nope").is_none());
  }

  #[test]
  fn number_from_i64_round_trips() {
    // mirrors: multi_iter_request.rs — Value::Number(Number::from(*value)).
    let v = Value::Number(Number::from(123_i64));
    assert_eq!(v.as_i64(), Some(123));
    assert_eq!(v.as_f64(), Some(123.0));
  }
}

// ---------------------------------------------------------------------------
// Mapping key INSERTION ORDER preservation
//
// mirrors: request.rs yaml_to_json (iterate mapping into a serde_json::Map) and
// header emission. If a replacement re-orders keys, driller's request bodies and
// header order change silently. This is the single most likely silent divergence.
// ---------------------------------------------------------------------------
mod ordering {
  use super::*;

  #[test]
  fn mapping_preserves_insertion_order() {
    // Keys are deliberately NOT alphabetical. Document order must be kept
    // (as serde_yaml 0.9 did); a BTreeMap-backed replacement would sort them.
    let d: Value = yaml::from_str("zebra: 1\napple: 2\nmango: 3\n").unwrap();
    let keys: Vec<&str> = d.as_mapping().unwrap().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["zebra", "apple", "mango"]);
  }

  #[test]
  fn constructed_mapping_preserves_insertion_order() {
    // mirrors: read_csv_file_as_yml — columns must keep CSV header order.
    let mut mapping = Mapping::new();
    for k in ["gamma", "alpha", "beta"] {
      mapping.insert(k, Value::Null);
    }
    let keys: Vec<&str> = mapping.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["gamma", "alpha", "beta"]);
  }
}

// ---------------------------------------------------------------------------
// Serialization (`to_string`) — exact, user-visible output
//
// mirrors: src/tags.rs (prints each plan item) and src/expandable/include.rs
// (unknown-node panic message). The exact format — document marker, indentation,
// key quoting, number rendering, block-sequence style — is what users see.
// ---------------------------------------------------------------------------
mod serialization {
  use super::*;

  #[test]
  fn to_string_scalar_format() {
    // serde_yaml 0.9 ended each scalar with a newline ("foo\n", "7\n", ...).
    assert_eq!(yaml::to_string(&Value::String("foo".into())).unwrap(), "foo");
    assert_eq!(yaml::to_string(&Value::Number(Number::from(7_i64))).unwrap(), "7");
    assert_eq!(yaml::to_string(&Value::Bool(true)).unwrap(), "true");
    assert_eq!(yaml::to_string(&Value::Null).unwrap(), "null");
  }

  #[test]
  fn to_string_mapping_with_nested_sequence_golden() {
    // mirrors: tags.rs list_benchmark_file_tasks — a plan item is serialized
    // and printed. This golden string pins indentation and block-sequence style.
    // serde_yaml 0.9 gave "name: foo\nrequest:\n  url: /\ntags:\n- tag1\n- tag2\n":
    // sequence items at the key's indent and a trailing newline. tags.rs adds
    // the blank line between items itself, so only the item indent is visible.
    let item: Value = yaml::from_str("name: foo\nrequest:\n  url: /\ntags:\n  - tag1\n  - tag2\n").unwrap();
    let out = yaml::to_string(&item).unwrap();
    let expected = "name: foo\nrequest:\n  url: /\ntags:\n  - tag1\n  - tag2";
    assert_eq!(out, expected);
  }

  #[test]
  fn to_string_keeps_the_newline_of_a_closing_block_scalar() {
    // A value ending in a block scalar keeps its newline, so callers that add
    // their own separator (src/tags.rs) trim it first.
    let v: Value = yaml::from_str("x: |\n  line1\n  line2\n").unwrap();
    assert!(yaml::to_string(&v).unwrap().ends_with("line2\n"));
  }

  #[test]
  fn parse_serialize_round_trip_is_stable() {
    // A second parse+serialize must be a fixed point.
    let src = "a: 1\nb:\n- x\n- y\n";
    let v1: Value = yaml::from_str(src).unwrap();
    let s1 = yaml::to_string(&v1).unwrap();
    let v2: Value = yaml::from_str(&s1).unwrap();
    let s2 = yaml::to_string(&v2).unwrap();
    assert_eq!(s1, s2);
    assert_eq!(v1, v2);
  }
}

// ---------------------------------------------------------------------------
// Multi-document behaviour
//
// mirrors: src/reader.rs parse_yaml_content, which splits multi-document input
// by `---\n` itself. driller's manual split rests on `from_str` being
// SINGLE-document. Pin that assumption: a
// replacement that silently accepts multi-doc (or errors differently) changes
// how the reader must behave.
// ---------------------------------------------------------------------------
mod multi_document {
  use super::*;

  // mirrors: src/reader.rs parse_yaml_content, which reads every plan file,
  // include and `--compare` report through from_str_multi.

  #[test]
  fn from_str_rejects_multi_document_input() {
    // from_str::<Value> parses exactly one document; from_str_multi reads a
    // stream.
    let result: Result<Value, _> = yaml::from_str("a: 1\n---\nb: 2\n");
    assert!(result.is_err(), "from_str must reject multi-document input");
  }

  #[test]
  fn single_document_with_leading_marker_parses() {
    let v: Value = yaml::from_str("---\nname: foo\n").unwrap();
    assert_eq!(v.get("name").and_then(|v| v.as_str()), Some("foo"));
  }

  #[test]
  fn from_str_multi_splits_documents_at_markers() {
    let docs = yaml::from_str_multi("---\na: 1\n---\nb: 2\n...\n---\nc: 3\n").unwrap();
    assert_eq!(docs.len(), 3);
    assert_eq!(docs[2]["c"].as_i64(), Some(3));
  }

  #[test]
  fn from_str_multi_keeps_an_indented_marker_line_as_content() {
    // driller 0.14.2 and earlier split the text on every `---\n` substring,
    // so this body broke the plan.
    let docs = yaml::from_str_multi("---\nbody: |\n  intro\n  ---\n  end\n").unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["body"].as_str(), Some("intro\n---\nend\n"));
  }

  #[test]
  fn from_str_multi_keeps_the_final_line_break_of_a_trailing_block_scalar() {
    // driller 0.14.2 and earlier trimmed the file first, which dropped it.
    let docs = yaml::from_str_multi("body: |\n  line one\n").unwrap();
    assert_eq!(docs[0]["body"].as_str(), Some("line one\n"));
    let keep = yaml::from_str_multi("body: |+\n  line one\n\n").unwrap();
    assert_eq!(keep[0]["body"].as_str(), Some("line one\n\n"));
  }

  #[test]
  fn from_str_multi_reads_crlf_documents() {
    let docs = yaml::from_str_multi("---\r\na: 1\r\n---\r\nb: 2\r\n").unwrap();
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[1]["b"].as_i64(), Some(2));
  }

  #[test]
  fn from_str_multi_empty_and_comment_documents() {
    assert!(yaml::from_str_multi("").unwrap().is_empty());
    let docs = yaml::from_str_multi("# only a comment\n").unwrap();
    assert!(docs.iter().all(Value::is_null), "{docs:?}");
    let docs = yaml::from_str_multi("---\n# comment doc\n---\na: 1\n").unwrap();
    assert_eq!(docs.last().and_then(|d| d["a"].as_i64()), Some(1));
  }

  #[test]
  fn from_str_multi_uses_the_same_rules_as_from_str() {
    // Duplicate keys in any document, with a stream-relative position.
    let err = yaml::from_str_multi("---\na: 1\n---\nb: 2\nb: 3\n").unwrap_err();
    assert!(err.to_string().contains("duplicate entry with key \"b\""), "{err}");
    let location = err.location().expect("duplicate-key error has a location");
    assert_eq!((location.line(), location.column()), (5, 1));
    // Tags removed in every document.
    let docs = yaml::from_str_multi("---\nurl: !path /x\n---\nurl: !path /y\n").unwrap();
    assert_eq!(docs[1]["url"].as_str(), Some("/y"));
  }
}

// ---------------------------------------------------------------------------
// Error on malformed input
//
// mirrors: src/reader.rs — a parse error feeds `die(format!("failed to parse
// YAML ...: {e}"))`. The Result::Err shape drives the clean `error:` exit path.
// ---------------------------------------------------------------------------
mod errors {
  use super::*;

  #[test]
  fn malformed_yaml_is_err() {
    // Unbalanced flow mapping — must be Err, not a panic and not Ok.
    let result: Result<Value, _> = yaml::from_str("{ unclosed: ");
    assert!(result.is_err());
  }

  #[test]
  fn error_has_displayable_message() {
    // mirrors: die(format!("... {e}")) — the error must Display to a non-empty
    // message that gets shown to the user.
    let result: Result<Value, _> = yaml::from_str("\t- tab-indent-is-invalid");
    let err = result.unwrap_err();
    assert!(!err.to_string().is_empty());
  }
}

// ---------------------------------------------------------------------------
// yaml_to_json variant bridge
//
// mirrors: src/actions/request.rs `yaml_to_json` — matches on each Value variant
// (Bool, Number i64/f64, String, Mapping, Sequence, Null) to build a
// serde_json::Value for the interpolation context. This reproduces that match
// and asserts the structural outcome, so a variant or coercion change is caught.
// ---------------------------------------------------------------------------
mod yaml_to_json_bridge {
  use super::*;
  use serde_json::json;
  use serde_json::{Map, Value as JsonValue};

  /// Local copy of driller's conversion, kept in lockstep with request.rs so
  /// the moat exercises the exact variant matching driller depends on.
  fn yaml_to_json(data: Value) -> JsonValue {
    match data {
      Value::Bool(b) => json!(b),
      Value::Number(n) => {
        if let Some(i) = n.as_i64() {
          json!(i)
        } else if let Some(u) = n.as_u64() {
          json!(u)
        } else {
          json!(n.as_f64())
        }
      }
      Value::String(s) => json!(s),
      Value::Mapping(m) => {
        let mut map = Map::new();
        for (key, value) in m.iter() {
          map.insert(key.clone(), yaml_to_json(value.clone()));
        }
        json!(map)
      }
      Value::Sequence(v) => {
        let mut array = Vec::new();
        for value in v.iter() {
          array.push(yaml_to_json(value.clone()));
        }
        json!(array)
      }
      Value::Null => json!(null),
      _ => panic!("Unknown Yaml node"),
    }
  }

  #[test]
  fn converts_nested_structure_with_mixed_scalars() {
    let yaml: Value = yaml::from_str("id: 1\nratio: 1.5\nactive: true\nname: ada\nempty: null\ntags:\n  - x\n  - y\nmeta:\n  k: v\n").unwrap();
    let got = yaml_to_json(yaml);
    let expected = json!({
      "id": 1,
      "ratio": 1.5,
      "active": true,
      "name": "ada",
      "empty": null,
      "tags": ["x", "y"],
      "meta": { "k": "v" },
    });
    assert_eq!(got, expected);
  }

  #[test]
  fn integer_becomes_json_integer_not_float() {
    // The i64-first probe matters: `1` must serialize as `1`, not `1.0`.
    let yaml: Value = yaml::from_str("n: 1\n").unwrap();
    assert_eq!(yaml_to_json(yaml).to_string(), r#"{"n":1}"#);
  }

  #[test]
  fn non_string_mapping_keys_become_strings() {
    // mirrors: yaml_to_json keeps every key. Mapping keys are strings, so an
    // integer-looking key arrives as "1". serde_yaml 0.9 kept it as a number
    // and yaml_to_json dropped it, giving `{ "two": 2 }`.
    let yaml: Value = yaml::from_str("1: one\ntwo: 2\n").unwrap();
    let got = yaml_to_json(yaml);
    assert_eq!(got, json!({ "1": "one", "two": 2 }));
  }

  #[test]
  fn unsigned_integer_above_i64_max_stays_exact() {
    // mirrors: yaml_to_json's as_u64 branch. serde_yaml 0.9 (and driller before
    // 0.14.0) turned this into the float 1.8446744073709552e19.
    let yaml: Value = yaml::from_str("id: 18446744073709551615\n").unwrap();
    assert_eq!(yaml_to_json(yaml).to_string(), r#"{"id":18446744073709551615}"#);
  }
}

// ---------------------------------------------------------------------------
// Mapping keys: string-only, in a standard form
//
// mirrors: every `.get("key")` lookup, the request.rs header loop and
// yaml_to_json. serde_yaml 0.9 kept non-string keys as `Value`s; noyalib turns
// every scalar key into a string and rejects the cases that would collide.
// ---------------------------------------------------------------------------
mod key_handling {
  use super::*;

  fn first_key(src: &str) -> String {
    let v: Value = yaml::from_str(src).unwrap();
    v.as_mapping().unwrap().keys().next().unwrap().clone()
  }

  #[test]
  fn scalar_keys_become_strings_in_standard_form() {
    // serde_yaml 0.9 kept these as Number / Bool / Null keys.
    assert_eq!(first_key("1: a"), "1");
    assert_eq!(first_key("+1: a"), "1");
    assert_eq!(first_key("0x10: a"), "16");
    assert_eq!(first_key("1e3: a"), "1000.0");
    assert_eq!(first_key("true: a"), "true");
    // Strings stay as written.
    assert_eq!(first_key("01: a"), "01");
    assert_eq!(first_key("yes: a"), "yes");
  }

  #[test]
  fn empty_and_null_keys_become_null() {
    // mirrors: the request.rs header guard, which rejects a "null" header name.
    // serde_yaml 0.9 read `~:`/`NULL:` as a Null key and failed to parse `: a`.
    assert_eq!(first_key("~: a"), "null");
    assert_eq!(first_key("NULL: a"), "null");
    assert_eq!(first_key("\"null\": a"), "null");
    let v: Value = yaml::from_str("headers:\n  : application/json\n").unwrap();
    assert_eq!(v["headers"].as_mapping().unwrap().keys().next().map(String::as_str), Some("null"));
  }

  #[test]
  fn keys_that_collide_as_strings_are_a_parse_error() {
    // serde_yaml 0.9 accepted both as distinct keys.
    let err = yaml::from_str::<Value>("1: a\n\"1\": b\n").unwrap_err();
    assert!(err.to_string().contains("collide"), "{err}");
    assert!(yaml::from_str::<Value>("true: a\n\"true\": b\n").is_err());
    assert!(yaml::from_str::<Value>("1: a\n+1: b\n").is_err());
  }

  #[test]
  fn non_scalar_keys_are_a_parse_error() {
    // serde_yaml 0.9 accepted sequences and mappings as keys.
    assert!(yaml::from_str::<Value>("[x, y]: a\n").is_err());
    assert!(yaml::from_str::<Value>("{x: 1}: a\n").is_err());
    assert!(yaml::from_str::<Value>("? [a, b]\n: c\n").is_err());
  }

  #[test]
  fn duplicate_keys_are_a_parse_error() {
    // Same as serde_yaml 0.9 for Value targets. The position is kept in
    // `location()` (src/error.rs adds it to the message driller prints).
    let err = yaml::from_str::<Value>("a: 1\na: 2\n").unwrap_err();
    assert!(err.to_string().contains("duplicate"), "{err}");
    let location = err.location().expect("duplicate-key error has a location");
    assert_eq!((location.line(), location.column()), (2, 1));
  }

  #[test]
  fn duplicate_keys_are_a_parse_error_for_hashmap_targets() {
    // serde_yaml 0.9 kept the last entry for typed map targets. YAML 1.2
    // requires unique keys, and driller::yaml::from_str rejects the duplicate
    // for every target type.
    let err = yaml::from_str::<std::collections::HashMap<String, i32>>("a: 1\na: 2\n").unwrap_err();
    assert!(err.to_string().contains("duplicate entry with key \"a\""), "{err}");
    let location = err.location().expect("duplicate-key error has a location");
    assert_eq!((location.line(), location.column()), (2, 1));
  }

  #[test]
  fn duplicate_struct_field_is_an_error() {
    // serde_yaml 0.9: ``duplicate field `a` ``.
    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct S {
      a: i32,
    }
    let err = yaml::from_str::<S>("a: 1\na: 2\n").unwrap_err();
    assert!(err.to_string().contains("duplicate entry with key \"a\""), "{err}");
    let location = err.location().expect("duplicate-key error has a location");
    assert_eq!((location.line(), location.column()), (2, 1));
  }

  #[test]
  fn duplicate_keys_inside_a_typed_target_are_a_parse_error() {
    // A nested map deserialized into a HashMap field, as a library caller
    // reading headers into a struct would.
    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct Request {
      headers: std::collections::HashMap<String, String>,
    }
    let err = yaml::from_str::<Request>("headers:\n  Accept: a\n  Accept: b\n").unwrap_err();
    assert!(err.to_string().contains("duplicate entry with key \"Accept\""), "{err}");
  }

  #[test]
  fn mapping_target_rejects_duplicates_and_removes_tags() {
    // Mapping is handled like Value (serde_yaml 0.9 rejected duplicates for both).
    assert!(yaml::from_str::<Mapping>("a: 1\na: 2\n").is_err());
    let m: Mapping = yaml::from_str("url: !path /x\n").unwrap();
    assert_eq!(m.get("url").and_then(|v| v.as_str()), Some("/x"));
  }
}

// ---------------------------------------------------------------------------
// Tags
//
// mirrors: every accessor driller calls on plan values (`as_str`, `as_i64`,
// `as_mapping`, `as_sequence`, ...), extract() in src/actions/mod.rs and
// yaml_to_json. serde_yaml 0.9's accessors looked through tags and it read
// `!!binary` / `!!timestamp` as plain strings; driller::yaml::from_str removes
// tags so the same accessors keep working.
// ---------------------------------------------------------------------------
mod tags {
  use super::*;

  #[test]
  fn custom_tag_on_a_scalar_reads_as_the_scalar() {
    let v: Value = yaml::from_str("url: !path /x\n").unwrap();
    assert_eq!(v["url"].as_str(), Some("/x"));
  }

  #[test]
  fn binary_and_timestamp_read_as_their_text() {
    let v: Value = yaml::from_str("b: !!binary aGVsbG8=\nt: !!timestamp 2001-12-14\n").unwrap();
    assert_eq!(v["b"].as_str(), Some("aGVsbG8="));
    assert_eq!(v["t"].as_str(), Some("2001-12-14"));
  }

  #[test]
  fn tags_inside_other_targets_are_kept() {
    // Only Value and Mapping targets are untagged; a Vec<Value> keeps them.
    let v: Vec<Value> = yaml::from_str("- !path /x\n").unwrap();
    assert!(v[0].is_tagged());
    assert_eq!(v[0].as_str(), None);
  }

  #[test]
  fn tags_are_removed_inside_collections() {
    let v: Value = yaml::from_str("with_items:\n  - !item {id: 1}\n  - !!timestamp 2001-12-14\ntags: !list [a, b]\n").unwrap();
    assert_eq!(v["with_items"][0]["id"].as_i64(), Some(1));
    assert_eq!(v["with_items"][1].as_str(), Some("2001-12-14"));
    assert_eq!(v["tags"].as_sequence().map(Vec::len), Some(2));
  }
}

// ---------------------------------------------------------------------------
// Multi-line flow scalars and collections: continuation indentation
//
// mirrors: plan bodies written as multi-line quoted JSON, and inline
// `with_items: [...]` lists. serde_yaml 0.9 (libyaml) accepted a continuation
// line at the parent key's indent; noyalib rejects it and has no setting to
// relax this. These goldens pin both sides, so a noyalib change shows up here.
// ---------------------------------------------------------------------------
mod continuation_indentation {
  use super::*;

  #[test]
  fn quoted_continuation_at_the_parent_indent_is_rejected() {
    // serde_yaml 0.9 parsed this to `{ "a": 1 }`.
    let src = "request:\n  body: '{\n    \"a\": 1\n  }'\n";
    let err = yaml::from_str::<Value>(src).unwrap_err();
    assert!(err.to_string().contains("continuation must be indented more than the parent block"), "{err}");
    let double = "request:\n  body: \"{\n    a: 1\n  }\"\n";
    assert!(yaml::from_str::<Value>(double).is_err());
  }

  #[test]
  fn quoted_continuation_indented_deeper_parses() {
    let src = "request:\n  body: '{\n    \"a\": 1\n    }'\n";
    let v: Value = yaml::from_str(src).unwrap();
    assert_eq!(v["request"]["body"].as_str(), Some("{ \"a\": 1 }"));
  }

  #[test]
  fn block_scalar_body_parses() {
    let src = "request:\n  body: |\n    {\"a\": 1}\n";
    let v: Value = yaml::from_str(src).unwrap();
    assert_eq!(v["request"]["body"].as_str(), Some("{\"a\": 1}\n"));
  }

  #[test]
  fn flow_list_items_at_the_parent_indent_are_rejected() {
    // serde_yaml 0.9 parsed this to a two-item list.
    let src = "with_items: [\n{id: 1},\n{id: 2}\n]\n";
    assert!(yaml::from_str::<Value>(src).is_err());
    let indented = "with_items: [\n  {id: 1},\n  {id: 2}\n  ]\n";
    let v: Value = yaml::from_str(indented).unwrap();
    assert_eq!(v["with_items"].as_sequence().map(Vec::len), Some(2));
  }
}

// ---------------------------------------------------------------------------
// Parser limits
//
// mirrors: src/yaml.rs `parser_config`, used by every parse and every target
// type (plans, CSV-free with_items lists, src/checker.rs reading `--report`
// baselines for `--compare`, and library callers parsing into their own
// types). serde_yaml 0.9 enforced only nesting depth and the alias expansion
// factor; noyalib's other default budgets would reject real input.
// ---------------------------------------------------------------------------
mod limits {
  use super::*;

  #[test]
  fn anchor_reused_many_times_parses() {
    // noyalib's default alias/anchor ratio (10) rejected the 11th use.
    let mut src = String::from("h: &h {Accept: application/json}\nplan:\n");
    for i in 0..1_000 {
      src.push_str(&format!("  - name: r{i}\n    request: {{url: /x, headers: *h}}\n"));
    }
    let v: Value = yaml::from_str(&src).unwrap();
    assert_eq!(v["plan"].as_sequence().unwrap().len(), 1_000);
    assert_eq!(v["plan"][999]["request"]["headers"]["Accept"].as_str(), Some("application/json"));
  }

  #[test]
  fn large_report_file_parses() {
    // mirrors: --compare reading a --report baseline. noyalib's default
    // max_nodes (250,000) rejected reports from runs of about 35,700 requests.
    let mut src = String::new();
    for i in 0..100_000 {
      src.push_str(&format!("- name: r{i}\n  duration: 1.5\n  status: 200\n"));
    }
    let v: Value = yaml::from_str(&src).unwrap();
    assert_eq!(v.as_sequence().unwrap().len(), 100_000);
  }

  #[test]
  fn long_inline_list_parses() {
    // noyalib's default max_sequence_length (65,536) rejected this.
    let mut src = String::from("with_items:\n");
    for i in 0..70_000 {
      src.push_str(&format!("  - {i}\n"));
    }
    let v: Value = yaml::from_str(&src).unwrap();
    assert_eq!(v["with_items"].as_sequence().unwrap().len(), 70_000);
  }

  #[test]
  fn typed_targets_get_the_same_limits() {
    // noyalib's own compat from_str applies its default max_sequence_length
    // (65,536) and other budgets to typed targets; driller::yaml::from_str
    // lifts them for every target, and keeps the depth limit.
    let mut src = String::new();
    for i in 0..70_000 {
      src.push_str(&format!("- {i}\n"));
    }
    let v: Vec<i64> = yaml::from_str(&src).unwrap();
    assert_eq!(v.len(), 70_000);

    let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
    assert!(yaml::from_str::<Vec<Value>>(&deep).is_err());
  }

  #[test]
  fn billion_laughs_is_still_rejected() {
    // The alias expansion factor serde_yaml 0.9 used stays in place.
    let mut src = String::from("a: &a [lol, lol, lol, lol, lol, lol, lol, lol, lol]\n");
    let mut prev = "a";
    for name in ["b", "c", "d", "e", "f", "g", "h", "i"] {
      let refs = vec![format!("*{prev}"); 9].join(", ");
      src.push_str(&format!("{name}: &{name} [{refs}]\n"));
      prev = name;
    }
    let err = yaml::from_str::<Value>(&src).unwrap_err();
    assert!(err.to_string().contains("repetition limit exceeded"), "{err}");
  }

  #[test]
  fn nesting_deeper_than_128_is_rejected() {
    // Same depth limit as serde_yaml 0.9.
    let ok = format!("{}1{}", "[".repeat(100), "]".repeat(100));
    assert!(yaml::from_str::<Value>(&ok).is_ok());
    let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
    let err = yaml::from_str::<Value>(&deep).unwrap_err();
    assert!(err.to_string().contains("recursion limit exceeded"), "{err}");
  }
}
