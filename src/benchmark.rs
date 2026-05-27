use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};

use serde_json::{Map, Value, json};
use tokio::{runtime, time::sleep};

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
// Feature f0001
pub struct RunOptions {
  pub benchmark_path: Option<String>,
  pub report_path: Option<String>,
  pub base_url: Option<String>,
  pub concurrency: Option<usize>,
  pub iterations: Option<usize>,
  pub duration: Option<Duration>,
  pub rampup: Option<usize>,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub quiet: bool,
  pub nanosec: bool,
  pub timeout: u64,
  pub verbose: bool,
}

pub struct BenchmarkResult {
  pub reports: Vec<Reports>,
  pub duration: f64,
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

/// Builds a single GET / request plan for ad-hoc URL testing.
// Feature f0001
fn build_synthetic_plan() -> Benchmark {
  let mut mapping = serde_yaml::Mapping::new();
  mapping.insert(serde_yaml::Value::String("name".into()), serde_yaml::Value::String("GET /".into()));
  let mut request = serde_yaml::Mapping::new();
  request.insert(serde_yaml::Value::String("url".into()), serde_yaml::Value::String("/".into()));
  mapping.insert(serde_yaml::Value::String("request".into()), serde_yaml::Value::Mapping(request));
  let item = serde_yaml::Value::Mapping(mapping);
  vec![Box::new(actions::Request::new(&item, None, None))]
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn synthetic_plan_has_one_item() {
    let plan = build_synthetic_plan();
    assert_eq!(plan.len(), 1);
  }

  #[test]
  fn synthetic_plan_is_valid_runnable() {
    let plan = build_synthetic_plan();
    assert!(!plan.is_empty());
  }
}

/// Executes a benchmark run using the provided options and tag filters.
// Feature f0001
pub fn execute(options: &RunOptions, tags: &Tags) -> BenchmarkResult {
  let config = Arc::new(Config::new(options));

  if options.report_path.is_some() {
    println!("{}: {}. Ignoring {} and {} properties...", "Report mode".yellow(), "on".purple(), "concurrency".yellow(), "iterations".yellow());
  } else {
    println!("{} {}", "Concurrency".yellow(), config.concurrency.to_string().purple());
    if let Some(ref dur) = config.duration {
      println!("{} {}", "Duration".yellow(), format!("{}s", dur.as_secs()).purple());
    } else {
      println!("{} {}", "Iterations".yellow(), config.iterations.to_string().purple());
    }
    println!("{} {}", "Rampup".yellow(), config.rampup.to_string().purple());
  }

  println!("{} {}", "Base URL".yellow(), config.base.purple());
  println!();

  let threads = std::cmp::min(num_cpus::get(), config.concurrency as usize);
  let rt = runtime::Builder::new_current_thread().enable_all().worker_threads(threads).build().unwrap();

  rt.block_on(async {
    let mut benchmark: Benchmark = Benchmark::new();
    let pool_store: PoolStore = PoolStore::new();

    // Feature f0001
    if let Some(ref benchmark_path) = options.benchmark_path {
      include::expand_from_filepath(benchmark_path, &mut benchmark, Some("plan"), tags);
    } else {
      benchmark = build_synthetic_plan();
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
      // Feature f0001 — duration-based run: loop plan until time expires
      let begin = Instant::now();
      let mut all_reports = Vec::new();
      let mut iteration = 0i64;

      while begin.elapsed() < duration {
        let batch_size = config.concurrency;
        let batch_start = iteration;
        let children = (0..batch_size).map(|i| run_iteration(benchmark.clone(), pool.clone(), config.clone(), batch_start + i));
        iteration += batch_size;

        let buffered = stream::iter(children).buffer_unordered(config.concurrency as usize);
        let batch_reports: Vec<Vec<Report>> = buffered.collect::<Vec<_>>().await;
        all_reports.extend(batch_reports);
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
