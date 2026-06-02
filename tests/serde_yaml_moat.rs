//! Characterization ("moat") tests for every way driller uses `serde_yaml`.
//!
//! `serde_yaml` (`= "0.9"`) is archived upstream. These tests pin the exact
//! behaviour the current version gives so that a future swap to a replacement
//! crate becomes a readable list of concrete diffs instead of a silent
//! behavioural migration. They characterise *the library*, not driller's
//! wrapper functions, so they survive refactors of `src/` and isolate the
//! dependency boundary.
//!
//! Each test carries a `mirrors:` comment naming the driller call site whose
//! behaviour it protects. A failure here points directly at the code that
//! would break under a replacement.
//!
//! ## Running the moat against a replacement candidate
//!
//! 1. Re-point the `serde_yaml` name at the candidate in `Cargo.toml`, e.g.
//!    `serde_yaml = { package = "serde_yaml_ng", version = "0.10" }`.
//! 2. `cargo test --test serde_yaml_moat`.
//! 3. Each failure is a behavioural difference. Either absorb it in driller's
//!    code or reject the candidate.
//! 4. A green suite means the swap is behaviour-preserving for everything
//!    driller actually relies on.
//!
//! Assertions are golden (exact strings, exact ordering, exact `Option`/
//! `Result` shape) on purpose: a "compatible but different" crate must trip a
//! specific, legible failure rather than slip through a loose `is_some()`.

use serde_yaml::{Mapping, Number, Value};

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
    let v: Value = serde_yaml::from_str("42").unwrap();
    assert_eq!(v.as_i64(), Some(42));
    assert_eq!(v.as_str(), None);
    assert_eq!(v.as_f64(), Some(42.0));
    assert_eq!(v.as_bool(), None);
  }

  #[test]
  fn parses_negative_integer() {
    // mirrors: read_i64_configuration — negative values are detected and
    // rejected by driller, so the sign must round-trip through as_i64.
    let v: Value = serde_yaml::from_str("-7").unwrap();
    assert_eq!(v.as_i64(), Some(-7));
  }

  #[test]
  fn parses_float_as_f64_not_i64() {
    // mirrors: yaml_to_json (request.rs) — Number is probed as_i64 first, then
    // as_f64. A float must fail as_i64 and succeed as_f64.
    let v: Value = serde_yaml::from_str("3.5").unwrap();
    assert_eq!(v.as_i64(), None);
    assert_eq!(v.as_f64(), Some(3.5));
  }

  #[test]
  fn parses_bool() {
    // mirrors: multi_request.rs / multi_iter_request.rs `shuffle` and `pick`.
    let t: Value = serde_yaml::from_str("true").unwrap();
    let f: Value = serde_yaml::from_str("false").unwrap();
    assert_eq!(t.as_bool(), Some(true));
    assert_eq!(f.as_bool(), Some(false));
    // A bare `yes`/`no` must NOT be a bool under YAML 1.2 (serde_yaml 0.9
    // semantics); driller relies on explicit true/false.
    let yes: Value = serde_yaml::from_str("yes").unwrap();
    assert_eq!(yes.as_bool(), None);
    assert_eq!(yes.as_str(), Some("yes"));
  }

  #[test]
  fn parses_bare_and_quoted_string() {
    // mirrors: read_str_configuration (config.rs) and the url/body accessors.
    let bare: Value = serde_yaml::from_str("hello").unwrap();
    let quoted: Value = serde_yaml::from_str("\"hello\"").unwrap();
    assert_eq!(bare.as_str(), Some("hello"));
    assert_eq!(quoted.as_str(), Some("hello"));
  }

  #[test]
  fn numeric_string_when_quoted_stays_string() {
    // mirrors: read_i64_configuration — a quoted number is a string and is
    // routed through the interpolate-then-parse branch, not as_i64.
    let v: Value = serde_yaml::from_str("\"42\"").unwrap();
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
    assert!(matches!(serde_yaml::from_str::<Value>("~").unwrap(), Value::Null));
    assert!(matches!(serde_yaml::from_str::<Value>("null").unwrap(), Value::Null));
  }

  #[test]
  fn empty_input_parses_to_null() {
    // mirrors: parse_yaml_content — an empty file must parse to Null (Ok), not
    // an error, so the reader can fall back to a single Null document.
    let v: Value = serde_yaml::from_str("").unwrap();
    assert!(matches!(v, Value::Null));
  }

  #[test]
  fn comment_only_input_parses_to_null() {
    // mirrors: parse_yaml_content — comment-only parts are skipped because
    // they parse to Null.
    let v: Value = serde_yaml::from_str("# just a comment").unwrap();
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
    serde_yaml::from_str("name: foo\nrequest:\n  url: /api\n  body:\n    file: payload.json\nassert:\n  key: status\n  value: 200\n").unwrap()
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
    let d: Value = serde_yaml::from_str("tags:\n  - tag1\n  - tag2\n  - 3\n").unwrap();
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
    let d: Value = serde_yaml::from_str("plan:\n  not: a-sequence\n").unwrap();
    assert!(d.get("plan").and_then(|v| v.as_sequence()).is_none());
  }

  #[test]
  fn top_level_sequence() {
    // mirrors: read_yaml_doc_accessor with accessor=None — doc.as_sequence().
    let d: Value = serde_yaml::from_str("- a\n- b\n").unwrap();
    let seq = d.as_sequence().unwrap();
    assert_eq!(seq.len(), 2);
    assert_eq!(seq[0].as_str(), Some("a"));
  }
}

// ---------------------------------------------------------------------------
// Mapping construction + keyed lookup by Value (programmatic build)
//
// mirrors: src/reader.rs `read_csv_file_as_yml` (Mapping::new + insert +
// Value::Mapping) and src/expandable/multi_iter_request.rs (get by a
// Value::String key, Number::from).
// ---------------------------------------------------------------------------
mod construction {
  use super::*;

  #[test]
  fn build_mapping_like_csv_reader() {
    // mirrors: read_csv_file_as_yml — headers + record become a Value::Mapping.
    let mut mapping = Mapping::new();
    mapping.insert(Value::String("id".to_string()), Value::String("1".to_string()));
    mapping.insert(Value::String("name".to_string()), Value::String("ada".to_string()));
    let row = Value::Mapping(mapping);

    assert_eq!(row.get("id").and_then(|v| v.as_str()), Some("1"));
    assert_eq!(row.get("name").and_then(|v| v.as_str()), Some("ada"));
  }

  #[test]
  fn keyed_get_by_value_string() {
    // mirrors: multi_iter_request.rs — with_iter_items.get(&Value::String(..)).
    let m: Value = serde_yaml::from_str("start: 2\nstep: 2\nstop: 20\n").unwrap();
    let mapping = m.as_mapping().unwrap();
    let key = Value::String("step".into());
    assert_eq!(mapping.get(&key).and_then(|v| v.as_i64()), Some(2));
    let missing = Value::String("nope".into());
    assert!(mapping.get(&missing).is_none());
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
    // Keys are deliberately NOT alphabetical. serde_yaml 0.9 preserves document
    // order; a BTreeMap-backed replacement would sort them.
    let d: Value = serde_yaml::from_str("zebra: 1\napple: 2\nmango: 3\n").unwrap();
    let keys: Vec<&str> = d.as_mapping().unwrap().iter().filter_map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, vec!["zebra", "apple", "mango"]);
  }

  #[test]
  fn constructed_mapping_preserves_insertion_order() {
    // mirrors: read_csv_file_as_yml — columns must keep CSV header order.
    let mut mapping = Mapping::new();
    for k in ["gamma", "alpha", "beta"] {
      mapping.insert(Value::String(k.to_string()), Value::Null);
    }
    let keys: Vec<&str> = mapping.iter().filter_map(|(k, _)| k.as_str()).collect();
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
    assert_eq!(serde_yaml::to_string(&Value::String("foo".into())).unwrap(), "foo\n");
    assert_eq!(serde_yaml::to_string(&Value::Number(Number::from(7_i64))).unwrap(), "7\n");
    assert_eq!(serde_yaml::to_string(&Value::Bool(true)).unwrap(), "true\n");
    assert_eq!(serde_yaml::to_string(&Value::Null).unwrap(), "null\n");
  }

  #[test]
  fn to_string_mapping_with_nested_sequence_golden() {
    // mirrors: tags.rs list_benchmark_file_tasks — a plan item is serialized
    // and printed. This golden string pins indentation and block-sequence style.
    let item: Value = serde_yaml::from_str("name: foo\nrequest:\n  url: /\ntags:\n  - tag1\n  - tag2\n").unwrap();
    let out = serde_yaml::to_string(&item).unwrap();
    let expected = "name: foo\nrequest:\n  url: /\ntags:\n- tag1\n- tag2\n";
    assert_eq!(out, expected);
  }

  #[test]
  fn parse_serialize_round_trip_is_stable() {
    // A second parse+serialize must be a fixed point.
    let src = "a: 1\nb:\n- x\n- y\n";
    let v1: Value = serde_yaml::from_str(src).unwrap();
    let s1 = serde_yaml::to_string(&v1).unwrap();
    let v2: Value = serde_yaml::from_str(&s1).unwrap();
    let s2 = serde_yaml::to_string(&v2).unwrap();
    assert_eq!(s1, s2);
    assert_eq!(v1, v2);
  }
}

// ---------------------------------------------------------------------------
// Multi-document behaviour
//
// mirrors: src/reader.rs parse_yaml_content — the comment "serde_yaml doesn't
// support multiple documents natively, so we split by `---\n`". driller's manual
// split rests on `from_str` being SINGLE-document. Pin that assumption: a
// replacement that silently accepts multi-doc (or errors differently) changes
// how the reader must behave.
// ---------------------------------------------------------------------------
mod multi_document {
  use super::*;

  #[test]
  fn from_str_rejects_multi_document_input() {
    // Two documents separated by `---`. serde_yaml::from_str::<Value> parses a
    // single document and errors on a second one — which is exactly why driller
    // splits manually before calling from_str.
    let result: Result<Value, _> = serde_yaml::from_str("a: 1\n---\nb: 2\n");
    assert!(result.is_err(), "from_str must reject multi-document input so the manual split stays necessary");
  }

  #[test]
  fn single_document_with_leading_marker_parses() {
    // mirrors: parse_yaml_content strips a single leading `---\n`; a lone
    // document with a leading marker must still parse cleanly.
    let v: Value = serde_yaml::from_str("---\nname: foo\n").unwrap();
    assert_eq!(v.get("name").and_then(|v| v.as_str()), Some("foo"));
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
    let result: Result<Value, _> = serde_yaml::from_str("{ unclosed: ");
    assert!(result.is_err());
  }

  #[test]
  fn error_has_displayable_message() {
    // mirrors: die(format!("... {e}")) — the error must Display to a non-empty
    // message that gets shown to the user.
    let result: Result<Value, _> = serde_yaml::from_str("\t- tab-indent-is-invalid");
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
        } else if let Some(f) = n.as_f64() {
          json!(f)
        } else {
          json!(n.to_string())
        }
      }
      Value::String(s) => json!(s),
      Value::Mapping(m) => {
        let mut map = Map::new();
        for (key, value) in m.iter() {
          if let Some(key_str) = key.as_str() {
            map.insert(key_str.to_string(), yaml_to_json(value.clone()));
          }
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
    let yaml: Value = serde_yaml::from_str("id: 1\nratio: 1.5\nactive: true\nname: ada\nempty: null\ntags:\n  - x\n  - y\nmeta:\n  k: v\n").unwrap();
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
    let yaml: Value = serde_yaml::from_str("n: 1\n").unwrap();
    assert_eq!(yaml_to_json(yaml).to_string(), r#"{"n":1}"#);
  }

  #[test]
  fn non_string_mapping_keys_are_dropped() {
    // mirrors: yaml_to_json skips keys where key.as_str() is None. A mapping
    // keyed by an integer contributes nothing to the JSON object.
    let yaml: Value = serde_yaml::from_str("1: one\ntwo: 2\n").unwrap();
    let got = yaml_to_json(yaml);
    assert_eq!(got, json!({ "two": 2 }));
  }
}
