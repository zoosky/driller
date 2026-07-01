use serde_yaml::{Mapping, Value};
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
/// serde_yaml does not expose multi-document parsing, so multi-document input
/// (separated by `---\n`) is split and each part parsed individually. An empty
/// or comment-only input yields a single `Null` document to keep callers that
/// index `[0]` working.
fn parse_yaml_content(content: &str) -> Result<Vec<Value>, Error> {
  let mut docs = Vec::new();
  let trimmed_content = content.trim();

  // Handle multi-document YAML (separated by "---\n")
  if trimmed_content.contains("\n---\n") || (trimmed_content.starts_with("---\n") && trimmed_content.matches("---\n").count() > 1) {
    let parts: Vec<&str> = trimmed_content.split("---\n").collect();
    for doc_str in parts {
      let trimmed = doc_str.trim();
      // Skip empty parts and parts that are only comments
      if !trimmed.is_empty() && !trimmed.chars().all(|c| c == '#' || c.is_whitespace() || c == '\n') {
        let doc = serde_yaml::from_str::<Value>(trimmed).map_err(|source| Error::Yaml {
          what: "document",
          source,
        })?;
        // Skip Null documents (which can result from comments-only content)
        if !matches!(doc, Value::Null) {
          docs.push(doc);
        }
      }
    }
  }

  // If no documents were found (empty file or no "---"), try parsing the whole content
  if docs.is_empty() {
    // Remove leading "---\n" if present for single-document files
    let content_to_parse = trimmed_content.strip_prefix("---\n").unwrap_or(trimmed_content);
    let doc = serde_yaml::from_str::<Value>(content_to_parse.trim()).map_err(|source| Error::Yaml {
      what: "content",
      source,
    })?;
    if !matches!(doc, Value::Null) {
      docs.push(doc);
    }
  }

  // If still empty, return a single Null document to maintain compatibility
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
    doc.as_sequence().ok_or_else(|| Error::NotASequence(format!("{doc:?}")))
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
          let item_key = Value::String(header.to_string());
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
