//! Integration tests for the ad-hoc `driller run -` stdin target.
//!
//! Exercise the stdin wiring end to end: the empty and read-error paths need no
//! socket, and the happy path aims a single request at a refused loopback port
//! so it completes near-instantly while still proving the piped URL drove the
//! run.

use std::io::Write;
use std::process::{Command, Stdio};

/// Path to the driller binary that cargo built for this test.
fn driller_bin() -> &'static str {
  env!("CARGO_BIN_EXE_driller")
}

/// Spawns `driller <args>` with piped stdio and writes `stdin_bytes` to its
/// stdin (closing the write end to signal EOF), returning the finished output.
fn run_with_stdin(args: &[&str], stdin_bytes: &[u8]) -> std::process::Output {
  let mut child = Command::new(driller_bin()).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("failed to spawn driller binary");
  child.stdin.take().expect("child stdin").write_all(stdin_bytes).expect("failed to write child stdin");
  child.wait_with_output().expect("failed to wait for driller")
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

/// A URL piped on stdin must actually drive the run: it becomes the run's base
/// URL, which driller echoes in the preamble. Aimed at a refused loopback port
/// so the single request returns immediately.
#[test]
fn run_dash_pipes_url_into_the_run() {
  let output = run_with_stdin(&["run", "-", "-i", "1"], b"http://127.0.0.1:9/health\n");
  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(stdout.contains("http://127.0.0.1:9"), "the piped URL should become the run's base URL, stdout={stdout}");
  assert!(!stdout.contains("panicked") && !String::from_utf8_lossy(&output.stderr).contains("panicked"), "should not panic");
}

/// `run -` is an ad-hoc source and cannot be combined with `--benchmark`. The
/// rejection must fire before any stdin read (so a plan-only run never blocks).
#[test]
fn run_dash_with_benchmark_is_rejected() {
  let output = run_with_stdin(&["run", "-", "--benchmark", "plan.yml"], b"http://127.0.0.1:9/\n");
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(1), "expected exit code 1, stderr={stderr}");
  assert!(stderr.contains("cannot be combined with --benchmark"), "should reject the -/--benchmark combo, got: {stderr}");
}

/// Non-UTF-8 bytes on stdin must surface as a distinct read error, not the
/// generic missing-URL message.
#[test]
fn run_dash_invalid_utf8_stdin_errors_clearly() {
  let output = run_with_stdin(&["run", "-"], &[0xff, 0xff, b'\n']);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(1), "expected exit code 1, stderr={stderr}");
  assert!(stderr.contains("couldn't read URL from stdin"), "should surface the stdin read error, got: {stderr}");
}

/// A scheme-less URL piped on stdin is rejected up front rather than building a
/// base URL that never connects.
#[test]
fn run_dash_scheme_less_url_is_rejected() {
  let output = run_with_stdin(&["run", "-"], b"example.com/health\n");
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(1), "expected exit code 1, stderr={stderr}");
  assert!(stderr.contains("must include a scheme"), "should reject a scheme-less URL, got: {stderr}");
}

/// The same scheme requirement applies to a positional ad-hoc URL.
#[test]
fn run_positional_scheme_less_url_is_rejected() {
  let output = Command::new(driller_bin()).args(["run", "example.com"]).output().expect("failed to invoke driller binary");
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert_eq!(output.status.code(), Some(1), "expected exit code 1, stderr={stderr}");
  assert!(stderr.contains("must include a scheme"), "should reject a scheme-less URL, got: {stderr}");
}
