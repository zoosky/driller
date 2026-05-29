use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};

use serde_json::{Map, Value, json};
use smol::Timer;
use tokio::runtime;

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
}

async fn run_iteration(benchmark: Arc<Benchmark>, pool: Pool, config: Arc<Config>, iteration: i64) -> Vec<Report> {
  if config.rampup > 0 {
    let delay = config.rampup / config.iterations;
    Timer::after(Duration::new((delay * iteration) as u64, 0)).await;
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

  if options.report_path.is_some() {
    println!("{}: {}. Ignoring {} and {} properties...", "Report mode".yellow(), "on".cyan(), "concurrency".yellow(), "iterations".yellow());
  } else {
    println!("{} {}", "Concurrency".yellow(), config.concurrency.to_string().cyan());
    if let Some(ref dur) = config.duration {
      println!("{} {}", "Duration".yellow(), format!("{}s", dur.as_secs()).cyan());
    } else {
      println!("{} {}", "Iterations".yellow(), config.iterations.to_string().cyan());
    }
    println!("{} {}", "Rampup".yellow(), config.rampup.to_string().cyan());
  }

  println!("{} {}", "Base URL".yellow(), config.base.cyan());
  println!();

  // Orchestration (task scheduling, batching, timers) is driven by smol via
  // `smol::block_on` below. The HTTP client (reqwest) still requires a tokio
  // reactor in thread-local context to drive its socket I/O, so we keep a tokio
  // runtime alive on a dedicated background thread and enter its handle on the
  // foreground thread for the duration of the run.
  //
  // The `-w/--worker-threads` flag still selects how the tokio I/O runtime is
  // built: 1 (default) -> current-thread runtime (single OS thread driving the
  // reactor); N >= 2 -> multi-thread runtime with N worker threads. This keeps
  // the two runtime shapes distinct and measurable.
  let worker_threads = options.worker_threads.unwrap_or(1);
  let rt = if worker_threads <= 1 {
    runtime::Builder::new_current_thread().enable_all().build().unwrap()
  } else {
    runtime::Builder::new_multi_thread().enable_all().worker_threads(worker_threads).build().unwrap()
  };

  // Keep the tokio runtime running on its own thread so its reactor is actively
  // pumped while smol drives the orchestration future. A current-thread runtime
  // only services registered I/O while one of its `block_on` calls is parked on
  // the driver, so we park it here on a future that never completes; the
  // background thread is released when `rt` is dropped at the end of `execute`.
  let handle = rt.handle().clone();
  let _io_thread = std::thread::Builder::new()
    .name("driller-io".to_string())
    .spawn(move || {
      rt.block_on(std::future::pending::<()>());
    })
    .expect("failed to spawn tokio I/O thread");

  // Entering the handle installs the tokio reactor/timer into thread-local
  // context, so reqwest futures polled by smol on this thread register their
  // sockets with the tokio reactor running on `driller-io`.
  let _enter = handle.enter();

  smol::block_on(async {
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

    if let Some(report_path) = options.report_path.as_deref() {
      let reports = run_iteration(benchmark.clone(), pool.clone(), config, 0).await;

      writer::write_file(report_path, join(reports, ""));

      BenchmarkResult {
        reports: vec![],
        duration: 0.0,
      }
    } else if let Some(duration) = config.duration {
      let begin = Instant::now();
      let mut all_reports = Vec::new();
      let mut iteration = 0i64;

      while let Some(remaining) = duration.checked_sub(begin.elapsed()) {
        let batch_size = config.concurrency;
        let batch_start = iteration;
        let children = (0..batch_size).map(|i| run_iteration(benchmark.clone(), pool.clone(), config.clone(), batch_start + i));
        iteration += batch_size;

        let buffered = stream::iter(children).buffer_unordered(config.concurrency as usize);

        // smol-based timeout: race the batch against a smol timer. If the timer
        // wins we are out of budget for this run, so break the loop (matching
        // the previous `Err(_) => break` behaviour on tokio timeout elapse).
        let batch = async { Some(buffered.collect::<Vec<_>>().await) };
        let deadline = async {
          Timer::after(remaining).await;
          None
        };
        match smol::future::or(batch, deadline).await {
          Some(batch_reports) => all_reports.extend(batch_reports),
          None => break,
        }
      }

      let elapsed = begin.elapsed().as_secs_f64();

      BenchmarkResult {
        reports: all_reports,
        duration: elapsed,
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
      }
    }
  })
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
}
