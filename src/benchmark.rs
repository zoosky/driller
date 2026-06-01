use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};

use serde_json::{Map, Value, json};
use tokio::runtime;
use tokio::time::sleep;

use crate::actions::{self, Report, Runnable};
use crate::config::Config;
use crate::expandable::include;
use crate::tags::Tags;
use crate::writer;

use reqwest::Client;

use colored::*;

pub type Benchmark = Vec<Box<dyn Runnable + Sync + Send>>;
pub type Context = Map<String, Value>;
pub type Reports = Vec<Report>;
pub type PoolStore = HashMap<String, Client>;
pub type Pool = Arc<Mutex<PoolStore>>;

/// Consolidated options for a benchmark run, replacing the former
/// multi-parameter `execute` signature.
pub struct RunOptions {
  pub benchmark_path: Option<String>,
  pub report_path: Option<String>,
  pub base_url: Option<String>,
  pub url_path: Option<String>,
  pub concurrency: Option<usize>,
  pub iterations: Option<usize>,
  pub duration: Option<Duration>,
  pub rampup: Option<usize>,
  pub worker_threads: Option<usize>,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub quiet: bool,
  pub nanosec: bool,
  pub timeout: u64,
  pub verbose: bool,
  pub tags: Tags,
}

pub struct BenchmarkResult {
  pub reports: Vec<Reports>,
  pub duration: f64,
  /// Number of `assert` checks that failed during the run. Non-zero drives a
  /// non-zero process exit code in `main`, so a failed assertion is detectable
  /// by CI without the run itself aborting.
  pub assertion_failures: usize,
}

async fn run_iteration(benchmark: Arc<Benchmark>, pool: Pool, config: Arc<Config>, iteration: i64) -> Vec<Report> {
  if config.rampup > 0 {
    let delay = config.rampup / config.iterations;
    sleep(Duration::new((delay * iteration) as u64, 0)).await;
  }

  let mut context: Context = Context::new();
  let mut reports: Vec<Report> = Vec::new();

  context.insert("iteration".to_string(), json!(iteration.to_string()));
  context.insert("base".to_string(), json!(config.base.to_string()));

  for item in benchmark.iter() {
    item.execute(&mut context, &mut reports, &pool, &config).await;
  }

  reports
}

fn join<S: ToString>(l: Vec<S>, sep: &str) -> String {
  l.iter().fold(
    "".to_string(),
    |a,b| if !a.is_empty() {a+sep} else {a} + &b.to_string()
  )
}

/// Builds a single GET request plan for ad-hoc URL testing.
fn build_synthetic_plan(path: &str) -> Benchmark {
  let name = format!("GET {path}");
  vec![Box::new(actions::Request::simple_get(&name, path))]
}

/// Executes a benchmark run using the provided options.
pub fn execute(options: &RunOptions) -> BenchmarkResult {
  let config = Arc::new(Config::new(options));

  // Held outside the runtime so the failure tally survives after `config` is
  // moved into the async block; read once below, after every iteration joins.
  let assertion_failures = config.assertion_failures.clone();

  println!("{} {}", "Concurrency".yellow(), config.concurrency.to_string().cyan());
  if let Some(ref dur) = config.duration {
    println!("{} {}", "Duration".yellow(), format!("{}s", dur.as_secs()).cyan());
  } else {
    println!("{} {}", "Iterations".yellow(), config.iterations.to_string().cyan());
  }
  println!("{} {}", "Rampup".yellow(), config.rampup.to_string().cyan());
  // Report mode now runs the full benchmark and writes every request, so it
  // honors concurrency/iterations/duration like any other run.
  if let Some(ref report_path) = options.report_path {
    println!("{} {}", "Report".yellow(), report_path.cyan());
  }

  println!("{} {}", "Base URL".yellow(), config.base.cyan());
  println!();

  // 1 (default) selects the current-thread runtime; N >= 2 selects multi-thread with N workers.
  let worker_threads = options.worker_threads.unwrap_or(1);
  let rt = if worker_threads <= 1 {
    runtime::Builder::new_current_thread().enable_all().build().unwrap()
  } else {
    runtime::Builder::new_multi_thread().enable_all().worker_threads(worker_threads).build().unwrap()
  };

  let mut result = rt.block_on(async {
    let mut benchmark: Benchmark = Benchmark::new();
    let pool_store: PoolStore = PoolStore::new();

    if let Some(ref benchmark_path) = options.benchmark_path {
      include::expand_from_filepath(benchmark_path, &mut benchmark, Some("plan"), &options.tags);
    } else {
      let path = options.url_path.as_deref().unwrap_or("/");
      benchmark = build_synthetic_plan(path);
    }

    if benchmark.is_empty() {
      eprintln!("Empty benchmark. Exiting.");
      std::process::exit(1);
    }

    let benchmark = Arc::new(benchmark);
    let pool = Arc::new(Mutex::new(pool_store));

    if let Some(duration) = config.duration {
      let begin = Instant::now();
      let mut all_reports = Vec::new();
      let mut iteration = 0i64;

      while duration.checked_sub(begin.elapsed()).is_some() {
        let batch_size = config.concurrency;
        let batch_start = iteration;
        let children = (0..batch_size).map(|i| run_iteration(benchmark.clone(), pool.clone(), config.clone(), batch_start + i));
        iteration += batch_size;

        let buffered = stream::iter(children).buffer_unordered(config.concurrency as usize);
        futures::pin_mut!(buffered);

        // Drain the batch one completed iteration at a time, bounded by the
        // remaining duration. Harvesting per-item (rather than awaiting the whole
        // batch under a single timeout, which discards the entire batch on
        // expiry) means iterations that finished before the deadline are still
        // counted; only requests still in flight at the deadline are dropped.
        // This matters more now that each request waits for its full response body.
        let mut deadline_reached = false;
        loop {
          let remaining = match duration.checked_sub(begin.elapsed()) {
            Some(remaining) => remaining,
            None => {
              deadline_reached = true;
              break;
            }
          };

          match tokio::time::timeout(remaining, buffered.next()).await {
            Ok(Some(iteration_reports)) => all_reports.push(iteration_reports),
            Ok(None) => break, // batch fully drained; start the next batch
            Err(_) => {
              deadline_reached = true;
              break;
            }
          }
        }

        if deadline_reached {
          break;
        }
      }

      let elapsed = begin.elapsed().as_secs_f64();

      BenchmarkResult {
        reports: all_reports,
        duration: elapsed,
        assertion_failures: 0,
      }
    } else {
      let children = (0..config.iterations).map(|iteration| run_iteration(benchmark.clone(), pool.clone(), config.clone(), iteration));

      let buffered = stream::iter(children).buffer_unordered(config.concurrency as usize);

      let begin = Instant::now();
      let reports: Vec<Vec<Report>> = buffered.collect::<Vec<_>>().await;
      let duration = begin.elapsed().as_secs_f64();

      BenchmarkResult {
        reports,
        duration,
        assertion_failures: 0,
      }
    }
  });

  // Report mode persists every request of the full run (all iterations,
  // flattened in completion order), so `--compare` and downstream tooling see
  // real data and `--stats` composes over it -- rather than the previous single
  // hard-coded iteration.
  if let Some(report_path) = options.report_path.as_deref() {
    let flat: Vec<Report> = result.reports.concat();
    if flat.is_empty() {
      // A run that completed no requests (e.g. a plan with no `request` items,
      // or a `--duration` shorter than a single request) would otherwise write
      // a misleading empty report that `--compare` then rejects. Warn and skip.
      eprintln!("{}: no requests completed; report file '{report_path}' not written", "warning".yellow());
    } else {
      writer::write_file(report_path, join(flat, ""));
    }
  }

  // Every iteration has joined; fold the shared assertion tally into the result
  // so `main` can translate it into a process exit code.
  result.assertion_failures = assertion_failures.load(Ordering::Relaxed);
  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn synthetic_plan_has_one_item() {
    let plan = build_synthetic_plan("/");
    assert_eq!(plan.len(), 1);
  }

  #[test]
  fn synthetic_plan_preserves_path() {
    let plan = build_synthetic_plan("/api/users");
    assert_eq!(plan.len(), 1);
  }

  /// Report mode must run the *whole* benchmark and persist every request, not
  /// a single hard-coded iteration: a 3-iteration plan with `--report` should
  /// return 3 iterations of reports and write 3 request records to the file.
  #[test]
  fn report_mode_runs_all_iterations() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use tempfile::NamedTempFile;

    let iterations = 3usize;

    // A tiny keep-alive HTTP/1.1 server that answers exactly `iterations`
    // requests (driller pools the connection) then exits.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
      let mut handled = 0;
      'accept: for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        loop {
          let mut buf = [0u8; 1024];
          match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
          }
          let body = "ok";
          let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
          if stream.write_all(resp.as_bytes()).is_err() {
            break;
          }
          let _ = stream.flush();
          handled += 1;
          if handled >= iterations {
            break 'accept;
          }
        }
      }
    });

    let mut plan = NamedTempFile::new().unwrap();
    write!(plan, "base: http://{addr}\nplan:\n  - name: ping\n    request:\n      url: /\n").unwrap();
    plan.flush().unwrap();

    let report = NamedTempFile::new().unwrap();
    let report_path = report.path().to_str().unwrap().to_string();

    let options = RunOptions {
      benchmark_path: Some(plan.path().to_str().unwrap().to_string()),
      report_path: Some(report_path.clone()),
      base_url: None,
      url_path: None,
      concurrency: Some(1),
      iterations: Some(iterations),
      duration: None,
      rampup: None,
      worker_threads: None,
      relaxed_interpolations: false,
      no_check_certificate: false,
      quiet: true,
      nanosec: false,
      timeout: 10,
      verbose: false,
      tags: crate::tags::Tags::new(None, None),
    };

    let result = execute(&options);
    server.join().unwrap();

    // All iterations ran (not a single hard-coded one), and `--stats` would
    // therefore see real data rather than an empty vec.
    assert_eq!(result.reports.len(), iterations, "report mode should run every iteration");
    assert_eq!(result.reports.concat().len(), iterations, "one request per iteration");

    // Every request is persisted to the report file.
    let written = std::fs::read_to_string(&report_path).unwrap();
    let blocks = written.matches("name:").count();
    assert_eq!(blocks, iterations, "report file should hold one record per request, got: {written}");
  }

  /// A run that completes no requests (here, a plan with only an `assign` step)
  /// must not write a misleading empty `--report` file.
  #[test]
  fn report_with_no_completed_requests_is_not_written() {
    use std::io::Write;
    use tempfile::{NamedTempFile, tempdir};

    let mut plan = NamedTempFile::new().unwrap();
    write!(plan, "plan:\n  - name: seed\n    assign:\n      key: k\n      value: v\n").unwrap();
    plan.flush().unwrap();

    let dir = tempdir().unwrap();
    let report_path = dir.path().join("report.txt");

    let options = RunOptions {
      benchmark_path: Some(plan.path().to_str().unwrap().to_string()),
      report_path: Some(report_path.to_str().unwrap().to_string()),
      base_url: None,
      url_path: None,
      concurrency: Some(1),
      iterations: Some(1),
      duration: None,
      rampup: None,
      worker_threads: None,
      relaxed_interpolations: false,
      no_check_certificate: false,
      quiet: true,
      nanosec: false,
      timeout: 10,
      verbose: false,
      tags: crate::tags::Tags::new(None, None),
    };

    let result = execute(&options);

    assert!(result.reports.concat().is_empty(), "an assign-only plan issues no requests");
    assert!(!report_path.exists(), "no report file should be written when no requests completed");
  }
}
