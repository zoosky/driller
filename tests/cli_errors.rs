//! Integration tests for user-facing CLI error messages.
//!
//! Exercises end-to-end exit code and stderr for invocations whose
//! failure modes were previously surfaced as `panic!` (i.e. with a
//! Rust backtrace hint that reads as a crash). The fixed behaviour is
//! a clean `error: ...` line on stderr with exit code 1.

use std::process::Command;

/// Path to the driller binary that cargo built for this test.
fn driller_bin() -> &'static str {
  env!("CARGO_BIN_EXE_driller")
}

/// Asserts that the given driller invocation exits with code 1, a clean
/// `error: couldn't open <path>: ...` line, and no Rust panic backtrace hint.
fn assert_clean_missing_file(args: &[&str], expected_path: &str) {
  let output = Command::new(driller_bin()).args(args).output().expect("failed to invoke driller binary");

  let stderr = String::from_utf8_lossy(&output.stderr);

  assert_eq!(output.status.code(), Some(1), "expected exit code 1, got {:?}. stderr={stderr}", output.status.code());
  assert!(stderr.contains("error: couldn't open"), "stderr should contain clean 'error: couldn't open' line, got: {stderr}");
  assert!(stderr.contains(expected_path), "stderr should name the missing file '{expected_path}', got: {stderr}");
  assert!(!stderr.contains("panicked"), "stderr should not mention 'panicked' (regression), got: {stderr}");
  assert!(!stderr.contains("RUST_BACKTRACE"), "stderr should not advise RUST_BACKTRACE (regression), got: {stderr}");
}

#[test]
fn missing_benchmark_file_exits_cleanly() {
  // `read_file` is used to load the benchmark YAML before any network
  // traffic, so this exercises the same panic-to-clean-error fix that
  // a missing --compare baseline.yml would hit, without needing a
  // socket.
  let missing = "/tmp/driller-integration-test-missing-benchmark.yml";
  assert_clean_missing_file(&["--benchmark", missing], missing);
}
