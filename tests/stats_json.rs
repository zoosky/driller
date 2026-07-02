//! End-to-end tests for the `--stats-format json` machine-readable output.
//!
//! These run the real binary against a throwaway in-process HTTP server and
//! assert the stdout/stderr contract the feature is built on: stdout carries
//! only the JSON document, the run banner is routed to stderr, and `--nanosec`
//! never leaks into the JSON numbers (which stay in milliseconds).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::thread;

/// Path to the driller binary that cargo built for this test.
fn driller_bin() -> &'static str {
  env!("CARGO_BIN_EXE_driller")
}

/// Spawns a throwaway HTTP/1.1 server on an ephemeral port that answers every
/// request with `200 OK`. Returns the bound `http://host:port` base URL. The
/// listener thread is detached and dies with the test process; each request
/// gets `Connection: close`, so the client opens a fresh connection per
/// iteration and the accept loop handles them one at a time.
fn spawn_ok_server() -> String {
  let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
  let addr = listener.local_addr().expect("local addr");
  thread::spawn(move || {
    for stream in listener.incoming() {
      let Ok(mut stream) = stream else {
        break;
      };
      thread::spawn(move || {
        // Drain the request (a small localhost GET arrives in one read) so the
        // client's write completes; the content is irrelevant, every request
        // gets 200.
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = b"ok";
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
        let _ = stream.flush();
      });
    }
  });
  format!("http://{addr}")
}

#[test]
fn json_run_emits_pure_json_on_stdout_and_banner_on_stderr() {
  let url = spawn_ok_server();
  let output = Command::new(driller_bin()).args(["run", &url, "--stats-format", "json", "-i", "5"]).output().expect("failed to invoke driller binary");

  let stdout = String::from_utf8_lossy(&output.stdout);
  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(output.status.success(), "run should exit 0. stderr={stderr}");

  // stdout is exactly one JSON document and nothing else -- it parses whole.
  let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("stdout should be a single JSON document, parse error: {e}. stdout={stdout}"));
  assert_eq!(parsed["schema"], 1);
  assert_eq!(parsed["global"]["total_requests"], 5);
  assert_eq!(parsed["global"]["successful_requests"], 5);
  assert_eq!(parsed["global"]["status_counts"]["200"], 5);

  // The run banner is on stderr, and must not pollute the JSON on stdout.
  assert!(stderr.contains("Concurrency"), "banner (Concurrency) should be on stderr, got: {stderr}");
  assert!(stderr.contains("Base URL"), "banner (Base URL) should be on stderr, got: {stderr}");
  assert!(!stdout.contains("Concurrency"), "banner must not leak onto stdout, got: {stdout}");
  assert!(!stdout.contains("Base URL"), "banner must not leak onto stdout, got: {stdout}");
}

#[test]
fn json_run_ignores_nanosec_flag() {
  let url = spawn_ok_server();
  // `--nanosec` is a text-display concern: the JSON must stay in milliseconds.
  let output = Command::new(driller_bin()).args(["run", &url, "--stats-format", "json", "--nanosec", "-i", "5"]).output().expect("failed to invoke driller binary");

  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(output.status.success(), "run should exit 0. stderr={}", String::from_utf8_lossy(&output.stderr));

  let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("stdout should be JSON even with --nanosec, parse error: {e}. stdout={stdout}"));

  // A localhost request is a few milliseconds at most; if `--nanosec` leaked
  // into the JSON the values would be ~1e6x larger and blow past this generous
  // millisecond ceiling.
  let mean = parsed["global"]["latency_ms"]["mean"].as_f64().expect("latency_ms.mean should be a number");
  assert!((0.0..60_000.0).contains(&mean), "latency_ms.mean should be milliseconds, got {mean}");
}
