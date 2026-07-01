use serde_yaml::Value;
use std::path::Path;

use crate::interpolator::INTERPOLATION_REGEX;

use crate::actions;
use crate::benchmark::Benchmark;
use crate::error::Error;
use crate::expandable::{include, multi_csv_request, multi_file_request, multi_iter_request, multi_request};
use crate::tags::Tags;

use crate::reader;

pub fn is_that_you(item: &Value) -> bool {
  item.get("include").and_then(|v| v.as_str()).is_some()
}

/// Expands an `include:` item by loading and expanding the referenced file.
///
/// # Errors
///
/// Propagates [`Error`] from reading or parsing the included file.
pub fn expand(parent_path: &str, item: &Value, benchmark: &mut Benchmark, tags: &Tags) -> Result<(), Error> {
  let include_path = item.get("include").and_then(|v| v.as_str()).unwrap();

  if INTERPOLATION_REGEX.is_match(include_path) {
    panic!("Interpolations not supported in 'include' property!");
  }

  let include_filepath = Path::new(parent_path).with_file_name(include_path);
  let final_path = include_filepath.to_str().unwrap();

  expand_from_filepath(final_path, benchmark, None, tags)
}

/// Loads a plan file and expands its items into `benchmark`.
///
/// # Errors
///
/// Propagates [`Error`] from reading or parsing the file (including the
/// `with_items_from_csv` / `with_items_from_file` sources reached during
/// expansion).
pub fn expand_from_filepath(parent_path: &str, benchmark: &mut Benchmark, accessor: Option<&str>, tags: &Tags) -> Result<(), Error> {
  let docs = reader::read_file_as_yml(parent_path)?;
  let items = reader::read_yaml_doc_accessor(&docs[0], accessor)?;

  for item in items {
    if include::is_that_you(item) {
      include::expand(parent_path, item, benchmark, tags)?;

      continue;
    }

    if tags.should_skip_item(item) {
      continue;
    }

    if multi_request::is_that_you(item) {
      multi_request::expand(item, benchmark);
    } else if multi_iter_request::is_that_you(item) {
      multi_iter_request::expand(item, benchmark);
    } else if multi_csv_request::is_that_you(item) {
      multi_csv_request::expand(parent_path, item, benchmark)?;
    } else if multi_file_request::is_that_you(item) {
      multi_file_request::expand(parent_path, item, benchmark)?;
    } else if actions::Delay::is_that_you(item) {
      benchmark.push(Box::new(actions::Delay::new(item, None)));
    } else if actions::Exec::is_that_you(item) {
      benchmark.push(Box::new(actions::Exec::new(item, None)));
    } else if actions::Assign::is_that_you(item) {
      benchmark.push(Box::new(actions::Assign::new(item, None)));
    } else if actions::Assert::is_that_you(item) {
      benchmark.push(Box::new(actions::Assert::new(item, None)));
    } else if actions::Request::is_that_you(item) {
      benchmark.push(Box::new(actions::Request::new(item, None, None)));
    } else {
      let out_str = serde_yaml::to_string(item).unwrap();
      panic!("Unknown node:\n\n{out_str}\n\n");
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use crate::benchmark::Benchmark;
  use crate::expandable::include::{expand, is_that_you};
  use crate::tags::Tags;

  #[test]
  fn expand_include() {
    let text = "---\nname: Include comment\ninclude: comments.yml";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    expand("example/benchmark.yml", doc, &mut benchmark, &Tags::new(None, None)).unwrap();

    assert!(is_that_you(doc));
    assert_eq!(benchmark.len(), 2);
  }

  #[test]
  #[should_panic(expected = "Interpolations not supported in 'include' property")]
  fn invalid_expand() {
    // Quoted so the YAML parser accepts the value; the bare `{{` form
    // used to rely on a YAML parse panic, which is now a returned `Error`.
    // The intent of this test is to verify the interpolation guard inside
    // `expand`, not the YAML parser.
    let text = "---\nname: Include comment\ninclude: \"{{ memory }}.yml\"";
    let docs = crate::reader::read_file_as_yml_from_str(text);
    let doc = &docs[0];
    let mut benchmark: Benchmark = Benchmark::new();

    let _ = expand("example/benchmark.yml", doc, &mut benchmark, &Tags::new(None, None));
  }
}
