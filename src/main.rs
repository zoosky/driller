mod actions;
mod benchmark;
mod checker;
mod config;
mod expandable;
mod interpolator;
mod reader;
mod tags;
mod writer;

use crate::actions::Report;
use crate::benchmark::RunOptions;
use clap::{Args, Parser, Subcommand};
use colored::*;
use hdrhistogram::Histogram;
use linked_hash_map::LinkedHashMap;
use std::collections::HashMap;
use std::process;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "driller", version, about = "HTTP load testing application written in Rust inspired by Ansible syntax")]
struct Cli {
  #[command(subcommand)]
  command: Option<Commands>,

  /// Sets the benchmark file
  #[arg(short, long, global = true)]
  benchmark: Option<String>,

  /// Shows request statistics
  #[arg(short, long, global = true, conflicts_with = "compare")]
  stats: bool,

  /// Sets a report file
  #[arg(short, long, global = true, conflicts_with = "compare")]
  report: Option<String>,

  /// Sets a compare file
  #[arg(short, long, global = true, conflicts_with = "report")]
  compare: Option<String>,

  /// Sets a threshold value in ms amongst the compared file
  #[arg(short, long, global = true, conflicts_with = "report")]
  threshold: Option<String>,

  /// Do not panic if an interpolation is not present. (Not recommended)
  #[arg(long, global = true)]
  relaxed_interpolations: bool,

  /// Disables SSL certification check. (Not recommended)
  #[arg(long, global = true)]
  no_check_certificate: bool,

  /// Tags to include
  #[arg(long, global = true)]
  tags: Option<String>,

  /// Tags to exclude
  #[arg(long, global = true)]
  skip_tags: Option<String>,

  /// List all benchmark tags
  #[arg(long, global = true, conflicts_with_all = ["tags", "skip_tags"])]
  list_tags: bool,

  /// List benchmark tasks (executes --tags/--skip-tags filter)
  #[arg(long, global = true)]
  list_tasks: bool,

  /// Disables output
  #[arg(short, long, global = true)]
  quiet: bool,

  /// Set timeout in seconds for all requests
  #[arg(short = 'o', long, global = true)]
  timeout: Option<String>,

  /// Shows statistics in nanoseconds
  #[arg(short, long, global = true)]
  nanosec: bool,

  /// Toggle verbose output
  #[arg(short, long, global = true)]
  verbose: bool,
}

// Feature f0001
#[derive(Subcommand)]
enum Commands {
  /// Execute a benchmark or ad-hoc HTTP request
  Run(RunArgs),
}

/// CLI flags specific to the `run` subcommand.
// Feature f0001
#[derive(Args)]
struct RunArgs {
  /// Target URL for ad-hoc testing (creates a synthetic GET request)
  url: Option<String>,

  /// Override the base URL from the benchmark file
  #[arg(short = 'u', long)]
  base_url: Option<String>,

  /// Number of concurrent requests
  #[arg(short = 'p', long)]
  concurrency: Option<usize>,

  /// Number of iterations to run
  #[arg(short = 'i', long, conflicts_with = "duration")]
  iterations: Option<usize>,

  /// Run for a fixed wall-clock duration (e.g. "30s", "5m", "1h")
  #[arg(short = 'd', long, conflicts_with = "iterations")]
  duration: Option<String>,

  /// Ramp-up time in seconds
  #[arg(short = 'e', long)]
  rampup: Option<usize>,
}

/// Parses a human-readable duration string into a `Duration`.
///
/// Accepts suffixes: `s` (seconds), `m` (minutes), `h` (hours).
/// Plain numbers are treated as seconds.
// Feature f0001
fn parse_duration(s: &str) -> Duration {
  let s = s.trim();
  let (num_part, multiplier) = if let Some(n) = s.strip_suffix('s') {
    (n, 1u64)
  } else if let Some(n) = s.strip_suffix('m') {
    (n, 60)
  } else if let Some(n) = s.strip_suffix('h') {
    (n, 3600)
  } else {
    (s, 1)
  };

  let value: u64 = num_part.parse().unwrap_or_else(|_| {
    eprintln!("error: invalid duration '{s}' (expected e.g. '30s', '5m', '1h')");
    process::exit(1);
  });

  Duration::from_secs(value * multiplier)
}

fn main() {
  let cli = Cli::parse();

  #[cfg(windows)]
  let _ = control::set_virtual_terminal(true);

  if cli.list_tags {
    let benchmark = cli.benchmark.as_deref().unwrap_or_else(|| {
      eprintln!("error: --list-tags requires --benchmark");
      process::exit(1);
    });
    tags::list_benchmark_file_tags(benchmark);
    process::exit(0);
  };

  let tags = tags::Tags::new(cli.tags.as_deref(), cli.skip_tags.as_deref());

  if cli.list_tasks {
    let benchmark = cli.benchmark.as_deref().unwrap_or_else(|| {
      eprintln!("error: --list-tasks requires --benchmark");
      process::exit(1);
    });
    tags::list_benchmark_file_tasks(benchmark, &tags);
    process::exit(0);
  };

  let timeout = cli.timeout.as_deref().map_or(10, |t| t.parse().unwrap_or(10));

  // Feature f0001 — build RunOptions from either `run` subcommand or legacy flat-flags
  let options = match cli.command {
    Some(Commands::Run(ref run_args)) => {
      let base_url = run_args.base_url.clone().or_else(|| run_args.url.clone());

      if cli.benchmark.is_none() && run_args.url.is_none() {
        eprintln!("error: either a URL or --benchmark is required");
        eprintln!("usage: driller run <URL>");
        eprintln!("       driller run --benchmark <FILE>");
        process::exit(1);
      }

      RunOptions {
        benchmark_path: cli.benchmark.clone(),
        report_path: cli.report.clone(),
        base_url,
        concurrency: run_args.concurrency,
        iterations: run_args.iterations,
        duration: run_args.duration.as_deref().map(parse_duration),
        rampup: run_args.rampup,
        relaxed_interpolations: cli.relaxed_interpolations,
        no_check_certificate: cli.no_check_certificate,
        quiet: cli.quiet,
        nanosec: cli.nanosec,
        timeout,
        verbose: cli.verbose,
      }
    }
    None => {
      if cli.benchmark.is_none() {
        eprintln!("error: --benchmark is required (or use `driller run <URL>`)");
        process::exit(1);
      }

      RunOptions {
        benchmark_path: cli.benchmark.clone(),
        report_path: cli.report.clone(),
        base_url: None,
        concurrency: None,
        iterations: None,
        duration: None,
        rampup: None,
        relaxed_interpolations: cli.relaxed_interpolations,
        no_check_certificate: cli.no_check_certificate,
        quiet: cli.quiet,
        nanosec: cli.nanosec,
        timeout,
        verbose: cli.verbose,
      }
    }
  };

  let benchmark_result = benchmark::execute(&options, &tags);
  let list_reports = benchmark_result.reports;
  let duration = benchmark_result.duration;

  show_stats(&list_reports, cli.stats, cli.nanosec, duration);

  // Feature f0001 — threshold parsing moved to CLI boundary
  let threshold = cli.threshold.as_deref().map(|t| {
    t.parse::<f64>().unwrap_or_else(|_| {
      eprintln!("error: --threshold must be a number in ms");
      process::exit(1);
    })
  });
  compare_benchmark(&list_reports, cli.compare.as_deref(), threshold);

  process::exit(0)
}

struct DrillStats {
  total_requests: usize,
  successful_requests: usize,
  failed_requests: usize,
  hist: Histogram<u64>,
}

impl DrillStats {
  fn mean_duration(&self) -> f64 {
    self.hist.mean() / 1_000.0
  }
  fn median_duration(&self) -> f64 {
    self.hist.value_at_quantile(0.5) as f64 / 1_000.0
  }
  fn stdev_duration(&self) -> f64 {
    self.hist.stdev() / 1_000.0
  }
  fn value_at_quantile(&self, quantile: f64) -> f64 {
    self.hist.value_at_quantile(quantile) as f64 / 1_000.0
  }
}

fn compute_stats(sub_reports: &[Report]) -> DrillStats {
  // Values are recorded in microseconds (duration_ms * 1000), so the upper
  // bound must also be in microseconds. 1 hour = 3_600_000_000 us.
  let mut hist = Histogram::<u64>::new_with_bounds(1, 60 * 60 * 1_000_000, 2).unwrap();
  let mut group_by_status = HashMap::new();

  for req in sub_reports {
    group_by_status.entry(req.status / 100).or_insert_with(Vec::new).push(req);
  }

  for r in sub_reports.iter() {
    let duration_us = (r.duration * 1_000.0) as u64;
    if let Err(e) = hist.record(duration_us) {
      eprintln!("warning: request '{}' duration {:.0}ms exceeds histogram range, skipped: {}", r.name, r.duration, e);
    }
  }

  let total_requests = sub_reports.len();
  let successful_requests = group_by_status.entry(2).or_insert_with(Vec::new).len();
  let failed_requests = total_requests - successful_requests;

  DrillStats {
    total_requests,
    successful_requests,
    failed_requests,
    hist,
  }
}

fn format_time(tdiff: f64, nanosec: bool) -> String {
  if nanosec {
    (1_000_000.0 * tdiff).round().to_string() + "ns"
  } else {
    tdiff.round().to_string() + "ms"
  }
}

fn show_stats(list_reports: &[Vec<Report>], stats_option: bool, nanosec: bool, duration: f64) {
  if !stats_option {
    return;
  }

  let mut group_by_name = LinkedHashMap::new();

  for req in list_reports.concat() {
    group_by_name.entry(req.name.clone()).or_insert_with(Vec::new).push(req);
  }

  // compute stats per name
  for (name, reports) in group_by_name {
    let substats = compute_stats(&reports);
    println!();
    println!("{:width$} {:width2$} {}", name.green(), "Total requests".yellow(), substats.total_requests.to_string().purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Successful requests".yellow(), substats.successful_requests.to_string().purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Failed requests".yellow(), substats.failed_requests.to_string().purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Median time per request".yellow(), format_time(substats.median_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Average time per request".yellow(), format_time(substats.mean_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Sample standard deviation".yellow(), format_time(substats.stdev_duration(), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.0'th percentile".yellow(), format_time(substats.value_at_quantile(0.99), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.5'th percentile".yellow(), format_time(substats.value_at_quantile(0.995), nanosec).purple(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.9'th percentile".yellow(), format_time(substats.value_at_quantile(0.999), nanosec).purple(), width = 25, width2 = 25);
  }

  // compute global stats
  let allreports = list_reports.concat();
  let global_stats = compute_stats(&allreports);
  let requests_per_second = global_stats.total_requests as f64 / duration;

  println!();
  println!("{:width2$} {} {}", "Time taken for tests".yellow(), format!("{duration:.1}").purple(), "seconds".purple(), width2 = 25);
  println!("{:width2$} {}", "Total requests".yellow(), global_stats.total_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Successful requests".yellow(), global_stats.successful_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {}", "Failed requests".yellow(), global_stats.failed_requests.to_string().purple(), width2 = 25);
  println!("{:width2$} {} {}", "Requests per second".yellow(), format!("{requests_per_second:.2}").purple(), "[#/sec]".purple(), width2 = 25);
  println!("{:width2$} {}", "Median time per request".yellow(), format_time(global_stats.median_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Average time per request".yellow(), format_time(global_stats.mean_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "Sample standard deviation".yellow(), format_time(global_stats.stdev_duration(), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.0'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.99), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.5'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.995), nanosec).purple(), width2 = 25);
  println!("{:width2$} {}", "99.9'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.999), nanosec).purple(), width2 = 25);
}

// Feature f0001 — threshold parsed at CLI boundary, passed as f64
fn compare_benchmark(list_reports: &[Vec<Report>], compare_path_option: Option<&str>, threshold_option: Option<f64>) {
  if let Some(compare_path) = compare_path_option {
    if let Some(threshold) = threshold_option {
      let compare_result = checker::compare(list_reports, compare_path, threshold);

      match compare_result {
        Ok(_) => process::exit(0),
        Err(_) => process::exit(1),
      }
    } else {
      eprintln!("error: --threshold is required when using --compare");
      process::exit(1);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn report(name: &str, duration_ms: f64, status: u16) -> Report {
    Report {
      name: name.to_string(),
      duration: duration_ms,
      status,
    }
  }

  // Regression: upstream #151, #174, #201, #216
  // Durations above 3.6 s caused a panic because the histogram upper bound
  // was 3_600_000 (microseconds) while the code records values in
  // microseconds (duration_ms * 1000). A 5 s request = 5_000_000 us
  // exceeded the bound.
  #[test]
  fn histogram_accepts_durations_above_5s() {
    let reports = vec![
      report("fast", 100.0, 200),
      report("slow", 5_000.0, 200),       // 5 seconds
      report("very_slow", 30_000.0, 200), // 30 seconds
    ];
    let stats = compute_stats(&reports);
    assert_eq!(stats.total_requests, 3);
    assert_eq!(stats.successful_requests, 3);
    assert!(stats.mean_duration() > 1_000.0, "mean should reflect long durations");
  }

  #[test]
  fn histogram_accepts_duration_near_one_hour() {
    let reports = vec![
      report("marathon", 3_500_000.0, 200), // ~58 minutes
    ];
    let stats = compute_stats(&reports);
    assert_eq!(stats.total_requests, 1);
  }

  #[test]
  fn stats_counts_failures() {
    let reports = vec![report("ok", 50.0, 200), report("redirect", 60.0, 301), report("err", 70.0, 500)];
    let stats = compute_stats(&reports);
    assert_eq!(stats.total_requests, 3);
    assert_eq!(stats.successful_requests, 1);
    assert_eq!(stats.failed_requests, 2);
  }

  #[test]
  fn parse_duration_seconds() {
    assert_eq!(parse_duration("30s"), Duration::from_secs(30));
  }

  #[test]
  fn parse_duration_minutes() {
    assert_eq!(parse_duration("5m"), Duration::from_secs(300));
  }

  #[test]
  fn parse_duration_hours() {
    assert_eq!(parse_duration("1h"), Duration::from_secs(3600));
  }

  #[test]
  fn parse_duration_plain_number() {
    assert_eq!(parse_duration("60"), Duration::from_secs(60));
  }

  #[test]
  fn parse_duration_whitespace_trimmed() {
    assert_eq!(parse_duration("  30s  "), Duration::from_secs(30));
  }

  // -- CLI argument parsing ---------------------------------------------------

  #[test]
  fn cli_legacy_benchmark_flag() {
    let cli = Cli::try_parse_from(["driller", "--benchmark", "bench.yml"]).unwrap();
    assert_eq!(cli.benchmark.as_deref(), Some("bench.yml"));
    assert!(cli.command.is_none());
  }

  #[test]
  fn cli_run_with_url() {
    let cli = Cli::try_parse_from(["driller", "run", "http://example.com"]).unwrap();
    match cli.command {
      Some(Commands::Run(ref args)) => {
        assert_eq!(args.url.as_deref(), Some("http://example.com"));
      }
      _ => panic!("expected Run command"),
    }
  }

  #[test]
  fn cli_run_benchmark_with_overrides() {
    let cli = Cli::try_parse_from(["driller", "run", "--benchmark", "bench.yml", "--concurrency", "20", "--iterations", "100"]).unwrap();
    assert_eq!(cli.benchmark.as_deref(), Some("bench.yml"));
    match cli.command {
      Some(Commands::Run(ref args)) => {
        assert_eq!(args.concurrency, Some(20));
        assert_eq!(args.iterations, Some(100));
      }
      _ => panic!("expected Run command"),
    }
  }

  #[test]
  fn cli_run_duration_and_concurrency() {
    let cli = Cli::try_parse_from(["driller", "run", "http://example.com", "--duration", "30s", "--concurrency", "10"]).unwrap();
    match cli.command {
      Some(Commands::Run(ref args)) => {
        assert_eq!(args.duration.as_deref(), Some("30s"));
        assert_eq!(args.concurrency, Some(10));
      }
      _ => panic!("expected Run command"),
    }
  }

  #[test]
  fn cli_run_duration_iterations_conflict() {
    let result = Cli::try_parse_from(["driller", "run", "http://example.com", "--duration", "30s", "--iterations", "10"]);
    assert!(result.is_err());
  }

  #[test]
  fn cli_run_global_flags_after_subcommand() {
    let cli = Cli::try_parse_from(["driller", "run", "http://example.com", "--stats", "--quiet"]).unwrap();
    assert!(cli.stats);
    assert!(cli.quiet);
  }

  #[test]
  fn cli_run_base_url_override() {
    let cli = Cli::try_parse_from(["driller", "run", "--benchmark", "bench.yml", "--base-url", "http://staging:3000"]).unwrap();
    match cli.command {
      Some(Commands::Run(ref args)) => {
        assert_eq!(args.base_url.as_deref(), Some("http://staging:3000"));
      }
      _ => panic!("expected Run command"),
    }
  }

  #[test]
  fn cli_run_rampup() {
    let cli = Cli::try_parse_from(["driller", "run", "http://example.com", "--rampup", "5", "--iterations", "10"]).unwrap();
    match cli.command {
      Some(Commands::Run(ref args)) => {
        assert_eq!(args.rampup, Some(5));
        assert_eq!(args.iterations, Some(10));
      }
      _ => panic!("expected Run command"),
    }
  }

  #[test]
  fn cli_no_args_is_valid_parse() {
    let cli = Cli::try_parse_from(["driller"]).unwrap();
    assert!(cli.command.is_none());
    assert!(cli.benchmark.is_none());
  }

  #[test]
  fn cli_stats_compare_conflict() {
    let result = Cli::try_parse_from(["driller", "--stats", "--compare", "report.yml"]);
    assert!(result.is_err());
  }
}
