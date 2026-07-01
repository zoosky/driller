//! Integration test for the ad-hoc `driller run -` stdin target.
//!
//! Exercises the stdin wiring end to end without a socket: empty stdin must
//! fall through to the standard "a URL or --benchmark is required" error, exit
//! 1, and not panic.

use std::process::{Command, Stdio};

/// Path to the driller binary that cargo built for this test.
fn driller_bin() -> &'static str {
  env!("CARGO_BIN_EXE_driller")
}

/// `driller run -` with an empty stdin (EOF, no bytes) resolves no URL, so the
/// invocation must print the standard requirement error and exit 1 -- not start
/// an empty run and not panic.
#[test]
fn run_dash_empty_stdin_errors_cleanly() {
  let mut child = Command::new(driller_bin()).args(["run", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("failed to spawn driller binary");

  // Close the write end immediately so the child reads EOF with no data.
  drop(child.stdin.take());

  let output = child.wait_with_output().expect("failed to wait for driller");
  let stderr = String::from_utf8_lossy(&output.stderr);

  assert_eq!(output.status.code(), Some(1), "expected exit code 1, got {:?}. stderr={stderr}", output.status.code());
  assert!(stderr.contains("either a URL or --benchmark is required"), "empty stdin should surface the standard requirement error, got: {stderr}");
  assert!(!stderr.contains("panicked"), "stderr should not mention 'panicked', got: {stderr}");
}
