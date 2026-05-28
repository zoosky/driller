use serde_yaml::{Mapping, Value};
use std::fs::File;
use std::io::{BufReader, prelude::*};
use std::path::Path;
use std::process;

/// Prints `error: <msg>` to stderr and exits with status 1.
///
/// Used at boundaries where user-supplied paths or file contents
/// turn out to be invalid. Avoids the Rust panic + backtrace hint
/// that `panic!` would produce, which reads as a crash rather than
/// a user-input problem.
fn die(msg: impl std::fmt::Display) -> ! {
  eprintln!("error: {msg}");
  process::exit(1)
}

pub fn read_file(filepath: &str) -> String {
  // Create a path to the desired file
  let path = Path::new(filepath);
  let display = path.display();

  // Open the path in read-only mode, returns `io::Result<File>`
  let mut file = File::open(path).unwrap_or_else(|why| die(format!("couldn't open {display}: {why}")));

  // Read the file contents into a string, returns `io::Result<usize>`
  let mut content = String::new();
  if let Err(why) = file.read_to_string(&mut content) {
    die(format!("couldn't read {display}: {why}"));
  }

  content
}

fn parse_yaml_content(content: &str) -> Vec<Value> {
  // serde_yaml doesn't support multiple documents natively, so we split by "---\n" and parse each
  let mut docs = Vec::new();
  let trimmed_content = content.trim();

  // Handle multi-document YAML (separated by "---\n")
  if trimmed_content.contains("\n---\n") || (trimmed_content.starts_with("---\n") && trimmed_content.matches("---\n").count() > 1) {
    let parts: Vec<&str> = trimmed_content.split("---\n").collect();
    for doc_str in parts {
      let trimmed = doc_str.trim();
      // Skip empty parts and parts that are only comments
      if !trimmed.is_empty() && !trimmed.chars().all(|c| c == '#' || c.is_whitespace() || c == '\n') {
        match serde_yaml::from_str::<Value>(trimmed) {
          Ok(doc) => {
            // Skip Null documents (which can result from comments-only content)
            if !matches!(doc, Value::Null) {
              docs.push(doc);
            }
          }
          Err(e) => die(format!("failed to parse YAML document: {e}")),
        }
      }
    }
  }

  // If no documents were found (empty file or no "---"), try parsing the whole content
  if docs.is_empty() {
    // Remove leading "---\n" if present for single-document files
    let content_to_parse = trimmed_content.strip_prefix("---\n").unwrap_or(trimmed_content);
    match serde_yaml::from_str::<Value>(content_to_parse.trim()) {
      Ok(doc) => {
        if !matches!(doc, Value::Null) {
          docs.push(doc);
        }
      }
      Err(e) => die(format!("failed to parse YAML content: {e}")),
    }
  }

  // If still empty, return a single Null document to maintain compatibility
  if docs.is_empty() {
    docs.push(Value::Null);
  }

  docs
}

pub fn read_file_as_yml(filepath: &str) -> Vec<Value> {
  let content = read_file(filepath);
  parse_yaml_content(&content)
}

#[cfg(test)]
pub fn read_file_as_yml_from_str(content: &str) -> Vec<Value> {
  parse_yaml_content(content)
}

pub fn read_yaml_doc_accessor<'a>(doc: &'a Value, accessor: Option<&str>) -> &'a Vec<Value> {
  if let Some(accessor_id) = accessor {
    match doc.get(accessor_id).and_then(|v| v.as_sequence()) {
      Some(items) => items,
      None => die(format!("node missing on config: {accessor_id}")),
    }
  } else {
    doc.as_sequence().unwrap_or_else(|| die(format!("expected document to be a sequence, got: {doc:?}")))
  }
}

pub fn read_file_as_yml_array(filepath: &str) -> Vec<Value> {
  let path = Path::new(filepath);
  let display = path.display();

  let file = File::open(path).unwrap_or_else(|why| die(format!("couldn't open {display}: {why}")));

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

  items
}

// TODO: Try to split this fn into two
pub fn read_csv_file_as_yml(filepath: &str, quote: u8) -> Vec<Value> {
  // Create a path to the desired file
  let path = Path::new(filepath);
  let display = path.display();

  // Open the path in read-only mode, returns `io::Result<File>`
  let file = File::open(path).unwrap_or_else(|why| die(format!("couldn't open {display}: {why}")));

  let mut rdr = csv::ReaderBuilder::new().has_headers(true).quote(quote).from_reader(file);

  let mut items = Vec::new();

  let headers = rdr.headers().cloned().unwrap_or_else(|why| die(format!("couldn't parse CSV header in {display}: {why}")));

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

  items
}
