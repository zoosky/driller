//! Tests for [`Request::new`](super::Request::new): body, method and header
//! handling when the plan loads.

use super::*;
use crate::yaml::Value as YamlValue;
use std::io::Write;
use tempfile::NamedTempFile;

fn create_yaml_request_with_string_body(body_content: &str) -> YamlValue {
  let yaml_str = format!(
    r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body: "{}"
"#,
    body_content
  );
  crate::yaml::from_str(&yaml_str).unwrap()
}

fn create_yaml_request_with_hex_body(hex_content: &str) -> YamlValue {
  let yaml_str = format!(
    r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    hex: "{}"
"#,
    hex_content
  );
  crate::yaml::from_str(&yaml_str).unwrap()
}

fn create_yaml_request_with_file_body(file_path: &str) -> YamlValue {
  let yaml_str = format!(
    r#"
name: test_request
request:
  url: http://example.com
  method: POST
  body:
    file: "{}"
"#,
    file_path
  );
  crate::yaml::from_str(&yaml_str).unwrap()
}

#[test]
#[should_panic(expected = "Header name is empty or null")]
fn empty_header_name_is_rejected() {
  let yaml: YamlValue = crate::yaml::from_str("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    : application/json\n").unwrap();
  Request::new(&yaml, None, None);
}

#[test]
#[should_panic(expected = "Header name is empty or null")]
fn null_header_name_is_rejected() {
  let yaml: YamlValue = crate::yaml::from_str("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    ~: x\n").unwrap();
  Request::new(&yaml, None, None);
}

#[test]
#[should_panic(expected = "Invalid header name \"\"")]
fn explicitly_empty_header_name_is_rejected() {
  let yaml: YamlValue = crate::yaml::from_str("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    \"\": application/json\n").unwrap();
  Request::new(&yaml, None, None);
}

#[test]
#[should_panic(expected = "Invalid header name \"X Trace\"")]
fn header_name_with_a_space_is_rejected() {
  let yaml: YamlValue = crate::yaml::from_str("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    X Trace: abc\n").unwrap();
  Request::new(&yaml, None, None);
}

#[test]
fn tagged_url_is_read_through_its_tag() {
  let yaml: YamlValue = crate::yaml::from_str("name: test_request\nrequest:\n  url: !path http://example.com/x\n").unwrap();
  let request = Request::new(&yaml, None, None);
  assert_eq!(request.url, "http://example.com/x");
}

#[test]
fn header_keys_that_look_like_numbers_are_kept_as_strings() {
  // YAML mapping keys are always strings, so `1:` arrives as "1" and is sent
  // as a header name instead of aborting the run.
  let yaml: YamlValue = crate::yaml::from_str("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    X-Trace: abc\n    1: one\n").unwrap();
  let request = Request::new(&yaml, None, None);

  let headers: Vec<(&str, &str)> = request.headers.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
  assert_eq!(headers, [("x-trace", "abc"), ("1", "one")]);
}

fn request_from(yaml: &str) -> Request {
  let item: YamlValue = crate::yaml::from_str(yaml).unwrap();
  Request::new(&item, None, None)
}

#[test]
#[should_panic(expected = "Unsupported method \"OPTIONS\"")]
fn unsupported_method_is_rejected_when_the_plan_loads() {
  request_from("name: test_request\nrequest:\n  url: http://example.com\n  method: OPTIONS\n");
}

#[test]
fn method_is_case_insensitive() {
  let request = request_from("name: test_request\nrequest:\n  url: http://example.com\n  method: delete\n");
  assert_eq!(request.method, Method::DELETE);
}

#[test]
#[should_panic(expected = "Invalid value for header \"Authorization\"")]
fn header_value_with_a_line_break_is_rejected_when_the_plan_loads() {
  // A `|` block scalar keeps its final line break.
  request_from("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    Authorization: |\n      Bearer abc\n    Accept: application/json\n");
}

#[test]
#[should_panic(expected = "Invalid value for header \"X-Trace\"")]
fn line_break_outside_an_interpolation_is_rejected_when_the_plan_loads() {
  request_from("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    X-Trace: \"{{ item }}\\n\"\n");
}

#[test]
fn block_scalar_without_its_line_break_is_accepted() {
  let request = request_from("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    Authorization: |-\n      Bearer abc\n    Accept: application/json\n");
  assert_eq!(request.headers[0].1, "Bearer abc");
}

#[test]
#[should_panic(expected = "Header \"accept\" is set twice (also as \"Accept\")")]
fn header_names_that_differ_only_in_case_are_rejected() {
  request_from("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    Accept: application/json\n    accept: text/html\n");
}

#[test]
fn headers_keep_plan_order() {
  let request = request_from("name: test_request\nrequest:\n  url: http://example.com\n  headers:\n    X-B: b\n    X-A: a\n    X-C: c\n");
  let names: Vec<&str> = request.headers.iter().map(|(name, _)| name.as_str()).collect();
  assert_eq!(names, ["x-b", "x-a", "x-c"]);
}

#[test]
fn test_body_template_string() {
  let yaml = create_yaml_request_with_string_body("Hello, World!");
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Template(content)) => {
      assert_eq!(content, "Hello, World!");
    }
    _ => panic!("Expected Body::Template"),
  }
}

#[test]
fn test_body_hex() {
  // "Hello" in hex is "48656c6c6f"
  let yaml = create_yaml_request_with_hex_body("48656c6c6f");
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, b"Hello");
    }
    _ => panic!("Expected Body::Binary"),
  }
}

#[test]
fn test_body_hex_empty() {
  let yaml = create_yaml_request_with_hex_body("");
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, b"");
    }
    _ => panic!("Expected Body::Binary with empty data"),
  }
}

#[test]
fn test_body_hex_complex() {
  // "Hello, World!" in hex
  let yaml = create_yaml_request_with_hex_body("48656c6c6f2c20576f726c6421");
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, b"Hello, World!");
    }
    _ => panic!("Expected Body::Binary"),
  }
}

#[test]
fn test_body_file() {
  // Create a temporary file with test content
  let mut temp_file = NamedTempFile::new().unwrap();
  let test_content = b"Test file content";
  temp_file.write_all(test_content).unwrap();
  temp_file.flush().unwrap();

  let file_path = temp_file.path().to_str().unwrap();
  let yaml = create_yaml_request_with_file_body(file_path);

  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, test_content);
    }
    _ => panic!("Expected Body::Binary"),
  }
}

#[test]
fn test_body_file_empty() {
  // Create an empty temporary file
  let temp_file = NamedTempFile::new().unwrap();
  let file_path = temp_file.path().to_str().unwrap();

  let yaml = create_yaml_request_with_file_body(file_path);

  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, b"");
    }
    _ => panic!("Expected Body::Binary with empty data"),
  }
}

#[test]
fn test_body_file_binary_data() {
  // Create a file with binary data (not UTF-8)
  let mut temp_file = NamedTempFile::new().unwrap();
  let binary_content = vec![0x00, 0x01, 0x02, 0xFF, 0xFE, 0xFD];
  temp_file.write_all(&binary_content).unwrap();
  temp_file.flush().unwrap();

  let file_path = temp_file.path().to_str().unwrap();
  let yaml = create_yaml_request_with_file_body(file_path);

  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, binary_content);
    }
    _ => panic!("Expected Body::Binary"),
  }
}

#[test]
fn test_body_file_large_content() {
  // Create a file with larger content
  let mut temp_file = NamedTempFile::new().unwrap();
  let large_content: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
  temp_file.write_all(&large_content).unwrap();
  temp_file.flush().unwrap();

  let file_path = temp_file.path().to_str().unwrap();
  let yaml = create_yaml_request_with_file_body(file_path);

  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data.len(), 10000);
      assert_eq!(data, large_content);
    }
    _ => panic!("Expected Body::Binary"),
  }
}

#[test]
fn test_body_none_for_get() {
  let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: GET
"#;
  let yaml: YamlValue = crate::yaml::from_str(yaml_str).unwrap();
  let request = Request::new(&yaml, None, None);

  assert!(request.body.is_none());
}

#[test]
fn test_body_none_for_delete() {
  let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: DELETE
"#;
  let yaml: YamlValue = crate::yaml::from_str(yaml_str).unwrap();
  let request = Request::new(&yaml, None, None);

  assert!(request.body.is_none());
}

#[test]
fn test_body_hex_uppercase() {
  // Test that hex decoding works with uppercase letters
  let yaml = create_yaml_request_with_hex_body("48656C6C6F");
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, b"Hello");
    }
    _ => panic!("Expected Body::Binary"),
  }
}

#[test]
fn test_body_hex_mixed_case() {
  // Test that hex decoding works with mixed case
  let yaml = create_yaml_request_with_hex_body("48656c6C6F");
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, b"Hello");
    }
    _ => panic!("Expected Body::Binary"),
  }
}

#[test]
#[should_panic(expected = "Invalid hex string")]
fn test_body_hex_invalid() {
  let yaml = create_yaml_request_with_hex_body("InvalidHexString!");
  Request::new(&yaml, None, None);
}

#[test]
#[should_panic(expected = "Unable to open file")]
fn test_body_file_not_found() {
  let yaml = create_yaml_request_with_file_body("/nonexistent/path/to/file.txt");
  Request::new(&yaml, None, None);
}

#[test]
fn test_body_priority_string_over_hex() {
  // When body is a string, it should be treated as Template, not hex
  let yaml = create_yaml_request_with_string_body("48656c6c6f");
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Template(content)) => {
      assert_eq!(content, "48656c6c6f");
    }
    _ => panic!("Expected Body::Template when body is a string"),
  }
}

#[test]
fn test_body_put_method() {
  let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: PUT
  body: "PUT body content"
"#;
  let yaml: YamlValue = crate::yaml::from_str(yaml_str).unwrap();
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Template(content)) => {
      assert_eq!(content, "PUT body content");
    }
    _ => panic!("Expected Body::Template"),
  }
}

#[test]
fn test_body_patch_method() {
  let yaml_str = r#"
name: test_request
request:
  url: http://example.com
  method: PATCH
  body:
    hex: "5061746368"
"#;
  let yaml: YamlValue = crate::yaml::from_str(yaml_str).unwrap();
  let request = Request::new(&yaml, None, None);

  match request.body {
    Some(Body::Binary(data)) => {
      assert_eq!(data, b"Patch");
    }
    _ => panic!("Expected Body::Binary"),
  }
}
