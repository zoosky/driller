use crate::yaml::{Mapping, Value};
use std::fs::File;
use std::io::{BufReader, prelude::*};
use std::path::Path;

use crate::error::Error;

/// Reads a whole file into a `String`.
///
/// Used at boundaries where a user-supplied path may legitimately be wrong, so
/// failures are returned as [`Error::Io`] (rendered as a clean `couldn't
/// open/read <path>: ...` line by the binary) rather than panicking with a
/// backtrace hint that reads as a crash.
pub fn read_file(filepath: &str) -> Result<String, Error> {
  let path = Path::new(filepath);
  let display = path.display().to_string();

  // Open the path in read-only mode.
  let mut file = File::open(path).map_err(|source| Error::Io {
    action: "open",
    path: display.clone(),
    source,
  })?;

  // Read the file contents into a string.
  let mut content = String::new();
  file.read_to_string(&mut content).map_err(|source| Error::Io {
    action: "read",
    path: display,
    source,
  })?;

  Ok(content)
}

/// Parses YAML text into one or more documents.
///
/// The text goes to the YAML parser unchanged ([`crate::yaml::from_str_multi`]),
/// so documents are split only at `---` / `...` markers, every value keeps its
/// exact text (a block scalar at the end of the file keeps its final line
/// break), and error positions count lines from the top of the file.
///
/// Empty and comment-only documents are dropped. When nothing is left (an
/// empty or comment-only file), a single `Null` document is returned, so
/// callers that index `[0]` keep working.
fn parse_yaml_content(content: &str) -> Result<Vec<Value>, Error> {
  let mut docs: Vec<Value> = crate::yaml::from_str_multi(content)
    .map_err(|source| Error::Yaml {
      what: "content",
      source,
    })?
    .into_iter()
    .filter(|doc| !matches!(doc, Value::Null))
    .collect();

  if docs.is_empty() {
    docs.push(Value::Null);
  }

  Ok(docs)
}

/// Reads a YAML file and parses it into one or more documents.
pub fn read_file_as_yml(filepath: &str) -> Result<Vec<Value>, Error> {
  let content = read_file(filepath)?;
  parse_yaml_content(&content)
}

#[cfg(test)]
pub fn read_file_as_yml_from_str(content: &str) -> Vec<Value> {
  parse_yaml_content(content).expect("test fixture YAML should parse")
}

/// Returns the sequence of items at `accessor` (e.g. `plan`), or the document
/// itself as a sequence when `accessor` is `None`.
///
/// A missing node or a non-sequence document is a user-supplied-config problem,
/// returned as [`Error::MissingNode`] / [`Error::NotASequence`].
pub fn read_yaml_doc_accessor<'a>(doc: &'a Value, accessor: Option<&str>) -> Result<&'a Vec<Value>, Error> {
  if let Some(accessor_id) = accessor {
    match doc.get(accessor_id).and_then(|v| v.as_sequence()) {
      Some(items) => Ok(items),
      None => Err(Error::MissingNode(accessor_id.to_string())),
    }
  } else {
    doc.as_sequence().ok_or_else(|| Error::NotASequence(kind_of(doc).to_string()))
  }
}

/// Names the kind of a YAML value for error messages ("a mapping", "a string").
///
/// Used instead of the value's `Debug` output, which dumps the whole document
/// in the YAML crate's internal representation.
fn kind_of(value: &Value) -> &'static str {
  match value {
    Value::Null => "null",
    Value::Bool(_) => "a boolean",
    Value::Number(_) => "a number",
    Value::String(_) => "a string",
    Value::Sequence(_) => "a sequence",
    Value::Mapping(_) => "a mapping",
    _ => "a tagged value",
  }
}

/// Reads a file as a list of strings, one per line, wrapped as YAML values.
pub fn read_file_as_yml_array(filepath: &str) -> Result<Vec<Value>, Error> {
  let path = Path::new(filepath);
  let display = path.display().to_string();

  let file = File::open(path).map_err(|source| Error::Io {
    action: "open",
    path: display,
    source,
  })?;

  let reader = BufReader::new(file);
  let mut items = Vec::new();
  for line in reader.lines() {
    match line {
      Ok(text) => {
        items.push(Value::String(text));
      }
      Err(e) => println!("error parsing line: {e:?}"),
    }
  }

  Ok(items)
}

// TODO: Try to split this fn into two
/// Reads a CSV file into a list of YAML mappings keyed by the header row.
pub fn read_csv_file_as_yml(filepath: &str, quote: u8) -> Result<Vec<Value>, Error> {
  let path = Path::new(filepath);
  let display = path.display().to_string();

  let file = File::open(path).map_err(|source| Error::Io {
    action: "open",
    path: display.clone(),
    source,
  })?;

  let mut rdr = csv::ReaderBuilder::new().has_headers(true).quote(quote).from_reader(file);

  let mut items = Vec::new();

  let headers = rdr.headers().cloned().map_err(|source| Error::Csv {
    path: display,
    source,
  })?;

  for result in rdr.records() {
    match result {
      Ok(record) => {
        let mut mapping = Mapping::new();

        for (i, header) in headers.iter().enumerate() {
          let item_key = header.to_string();
          let item_value = Value::String(record.get(i).unwrap().to_string());

          mapping.insert(item_key, item_value);
        }

        items.push(Value::Mapping(mapping));
      }
      Err(e) => println!("error parsing header: {e:?}"),
    }
  }

  Ok(items)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn error_positions_count_from_the_start_of_the_file() {
    // A leading `---` and a second document must not shift reported lines.
    let single = "---\nbase: http://localhost:9000\nplan:\n  - name: dup\n    request:\n      url: /a\n      url: /b\n";
    let err = parse_yaml_content(single).unwrap_err();
    assert!(err.to_string().ends_with("at line 7 column 7"), "{err}");

    let multi = "---\nfirst: 1\n---\nplan:\n  - name: dup\n    request:\n      url: /a\n      url: /b\n";
    let err = parse_yaml_content(multi).unwrap_err();
    assert!(err.to_string().ends_with("at line 8 column 7"), "{err}");
  }

  #[test]
  fn plan_text_reaches_the_parser_unchanged() {
    // A `---` line inside a body and a body at the end of the file: before
    // 0.14.3 the reader split the text at the first and trimmed away the
    // final line break of the second.
    let content = "---\nplan:\n  - name: a\n    request:\n      url: /a\n      method: POST\n      body: |\n        intro\n        ---\n        end\n  - name: b\n    request:\n      url: /b\n      method: POST\n      body: |\n        last\n";
    let docs = parse_yaml_content(content).unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["plan"][0]["request"]["body"].as_str(), Some("intro\n---\nend\n"));
    assert_eq!(docs[0]["plan"][1]["request"]["body"].as_str(), Some("last\n"));
  }

  #[test]
  fn empty_and_comment_only_files_read_as_one_null_document() {
    assert_eq!(parse_yaml_content("").unwrap(), vec![Value::Null]);
    assert_eq!(parse_yaml_content("# nothing here\n").unwrap(), vec![Value::Null]);
    let docs = parse_yaml_content("---\n# comment doc\n---\na: 1\n").unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["a"].as_i64(), Some(1));
  }

  #[test]
  fn non_sequence_document_error_names_the_kind() {
    let doc: Value = crate::yaml::from_str("base: http://localhost:9000\n").unwrap();
    let err = read_yaml_doc_accessor(&doc, None).unwrap_err();
    assert_eq!(err.to_string(), "expected document to be a sequence, got: a mapping");
  }
}
