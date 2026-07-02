mod stats;

use clap::{Args, Parser, Subcommand};
use colored::*;
use driller::actions::Report;
use driller::tags;
use driller::{Error, RunOptions, checker};
use stats::{StatsFormat, show_stats};
use std::io::{BufRead, IsTerminal};
use std::process;
use std::time::Duration;

/// Short version string: `<cargo-pkg-version> (<git-hash>)`. Bound to `-V`.
///
/// Compact enough to grep / paste into a comment. Sufficient to identify a
/// build by commit when the workbench is the source of truth.
const SHORT_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")");

/// Long version string: `<cargo-pkg-version> (<git-hash> <build-time> <target>)`.
/// Bound to `--version`.
///
/// The bracketed half comes from `build.rs`, so a `cargo install --path .`
/// burns the current commit hash, build timestamp, and target triple into
/// the binary. Useful when verifying which exact build is running -- in
/// particular during performance investigations where install metadata
/// alone is not enough.
const LONG_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), " ", env!("BUILD_TIME"), " ", env!("BUILD_TARGET"), ")");

#[derive(Parser)]
#[command(name = "driller", version = SHORT_VERSION, long_version = LONG_VERSION, about = "HTTP load testing application written in Rust inspired by Ansible syntax")]
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
  #[arg(short, long, global = true, conflicts_with = "report", value_parser = parse_threshold)]
  threshold: Option<f64>,

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

  /// Statistics output format (json implies --stats and keeps stdout pure)
  #[arg(long, value_enum, default_value_t = StatsFormat::Text, global = true)]
  stats_format: StatsFormat,

  /// Toggle verbose output
  #[arg(short, long, global = true)]
  verbose: bool,
}

/// Available subcommands for the driller CLI.
#[derive(Subcommand)]
enum Commands {
  /// Execute a benchmark or ad-hoc HTTP request
  Run(RunArgs),
}

/// CLI flags specific to the `run` subcommand.
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

  /// Worker threads for the multi-thread tokio runtime.
  ///
  /// 1 (default) selects the current-thread runtime -- single OS thread, no
  /// cross-worker coordination, lowest per-request overhead. N >= 2 selects
  /// the multi-thread runtime with N worker threads. Optimal N depends on
  /// payload size and target; see the user guide for the workload-vs-N table.
  #[arg(short = 'w', long, value_parser = parse_worker_threads)]
  worker_threads: Option<usize>,
}

/// Parses the `--worker-threads` value.
///
/// Rejects 0 at clap parse time -- `worker_threads(0)` would panic inside
/// tokio's runtime builder. Any positive integer is accepted; the runtime
/// builder uses 1 to select the current-thread scheduler and >= 2 to select
/// the multi-thread scheduler.
fn parse_worker_threads(s: &str) -> Result<usize, String> {
  let n: usize = s.parse().map_err(|_| format!("'{s}' is not a positive integer"))?;
  if n == 0 {
    return Err("--worker-threads must be at least 1".to_string());
  }
  Ok(n)
}

/// Parses the `--threshold` value as milliseconds.
///
/// Runs at clap parse time so an invalid value fails before any benchmark
/// executes. The error message also flags a common pitfall: a single-dash
/// long-style flag like `-stats` is parsed by clap as the bundled shorts
/// `-s -t ats`, which silently feeds `ats` into `--threshold`.
fn parse_threshold(s: &str) -> Result<f64, String> {
  s.parse::<f64>().map_err(|_| {
    format!("'{s}' is not a number in ms.\nHint: a single-dash long flag like '-stats' is parsed as bundled shorts ('-s -t ats'), which feeds the next characters into '--threshold'. Use '--stats' (two dashes) if that is what you meant.")
  })
}

/// Parses a human-readable duration string into a `Duration`.
///
/// Accepts suffixes: `s` (seconds), `m` (minutes), `h` (hours).
/// Plain numbers are treated as seconds.
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

/// Reads an ad-hoc target URL from `reader`, used when the positional URL is the
/// standard stdin sentinel `-`.
///
/// Returns the first non-empty line, trimmed of surrounding whitespace and a
/// leading UTF-8 byte-order mark, so `echo http://host/path | driller run -`
/// runs the same synthetic-GET test as `driller run http://host/path`.
///
/// Whitespace-only or empty input yields `Ok(None)`, letting the caller fall
/// through to the usual "a URL or --benchmark is required" message. A read or
/// non-UTF-8 decode failure is returned as `Err` so the caller can report the
/// real cause instead of a misleading missing-URL error.
fn read_url_from_reader(reader: impl BufRead) -> std::io::Result<Option<String>> {
  for line in reader.lines() {
    let line = line?;
    // `str::trim` does not strip a UTF-8 BOM (U+FEFF is not whitespace); left in
    // place it would survive into a malformed base URL that never connects.
    let trimmed = line.trim().trim_start_matches('\u{feff}').trim();
    if !trimmed.is_empty() {
      return Ok(Some(trimmed.to_string()));
    }
  }
  Ok(None)
}

/// Splits a URL into its base (scheme + authority) and path components.
fn split_url(url: &str) -> (String, String) {
  if let Some(scheme_end) = url.find("://") {
    let after_scheme = &url[scheme_end + 3..];
    if let Some(path_start) = after_scheme.find('/') {
      let base = &url[..scheme_end + 3 + path_start];
      let path = &after_scheme[path_start..];
      return (base.to_string(), path.to_string());
    }
  }
  (url.to_string(), "/".to_string())
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
    // `NoItems` already printed its own friendly line; other errors are
    // reported here so the library never has to.
    match tags::list_benchmark_file_tags(benchmark) {
      Ok(()) => process::exit(0),
      Err(Error::NoItems) => process::exit(1),
      Err(e) => {
        eprintln!("error: {e}");
        process::exit(1);
      }
    }
  };

  let tags = tags::Tags::new(cli.tags.as_deref(), cli.skip_tags.as_deref());

  if cli.list_tasks {
    let benchmark = cli.benchmark.as_deref().unwrap_or_else(|| {
      eprintln!("error: --list-tasks requires --benchmark");
      process::exit(1);
    });
    match tags::list_benchmark_file_tasks(benchmark, &tags) {
      Ok(()) => process::exit(0),
      Err(Error::NoItems) => process::exit(1),
      Err(e) => {
        eprintln!("error: {e}");
        process::exit(1);
      }
    }
  };

  let timeout = cli.timeout.as_deref().map_or(10, |t| t.parse().unwrap_or(10));

  // In JSON mode stdout is reserved for the single stats document, so the engine
  // must stay silent there: its banner is routed to stderr (`machine_readable`)
  // and per-request progress/verbose tracing is suppressed. The JSON summary
  // carries the same information machine-readably.
  let json_mode = cli.stats_format == StatsFormat::Json;

  let options = match cli.command {
    Some(Commands::Run(ref run_args)) => {
      // `-` reads the ad-hoc target URL from stdin, so a single-endpoint test
      // composes in a pipeline: `echo http://host/path | driller run - --stats`.
      // It is an ad-hoc source, so reject it up front when a benchmark file is
      // also given -- otherwise the read would block on a plan-only run that
      // never needs a URL, and a piped host would silently override the plan's
      // own base.
      let resolved_url = match run_args.url.as_deref() {
        Some("-") => {
          if cli.benchmark.is_some() {
            eprintln!("error: `run -` reads an ad-hoc target URL from stdin and cannot be combined with --benchmark");
            process::exit(1);
          }
          let stdin = std::io::stdin();
          // On an interactive terminal the blocking read would otherwise look
          // frozen; a hint tells the user it is waiting for a URL.
          if stdin.is_terminal() {
            eprintln!("reading target URL from stdin (type one and press Ctrl-D, or pipe it in)...");
          }
          match read_url_from_reader(stdin.lock()) {
            Ok(url) => url,
            Err(e) => {
              eprintln!("error: couldn't read URL from stdin: {e}");
              process::exit(1);
            }
          }
        }
        other => other.map(str::to_string),
      };

      let (base_url, url_path) = if let Some(ref url) = resolved_url {
        let (base, path) = split_url(url);
        (run_args.base_url.clone().or(Some(base)), Some(path))
      } else {
        (run_args.base_url.clone(), None)
      };

      if cli.benchmark.is_none() && resolved_url.is_none() {
        eprintln!("error: either a URL or --benchmark is required");
        eprintln!("usage: driller run <URL>");
        eprintln!("       driller run --benchmark <FILE>");
        process::exit(1);
      }

      // driller drives HTTP(S) only: an effective base URL without a scheme --
      // an ad-hoc URL (positional or piped), or a --base-url override -- would
      // build a malformed base that fails every request, so reject it up front
      // with a clear message. A benchmark file supplies its own base and is
      // unaffected (base_url is None there).
      if let Some(base) = base_url.as_deref()
        && !base.contains("://")
      {
        eprintln!("error: URL must include a scheme, e.g. http://{base}");
        process::exit(1);
      }

      RunOptions {
        benchmark_path: cli.benchmark.clone(),
        report_path: cli.report.clone(),
        base_url,
        url_path,
        concurrency: run_args.concurrency,
        iterations: run_args.iterations,
        duration: run_args.duration.as_deref().map(parse_duration),
        rampup: run_args.rampup,
        worker_threads: run_args.worker_threads,
        relaxed_interpolations: cli.relaxed_interpolations,
        no_check_certificate: cli.no_check_certificate,
        quiet: cli.quiet || json_mode,
        nanosec: cli.nanosec,
        timeout,
        verbose: cli.verbose && !json_mode,
        machine_readable: json_mode,
        tags,
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
        url_path: None,
        concurrency: None,
        iterations: None,
        duration: None,
        rampup: None,
        worker_threads: None,
        relaxed_interpolations: cli.relaxed_interpolations,
        no_check_certificate: cli.no_check_certificate,
        quiet: cli.quiet || json_mode,
        nanosec: cli.nanosec,
        timeout,
        verbose: cli.verbose && !json_mode,
        machine_readable: json_mode,
        tags,
      }
    }
  };

  let benchmark_result = match driller::run(&options) {
    Ok(result) => result,
    Err(e) => {
      eprintln!("error: {e}");
      process::exit(1);
    }
  };
  let assertion_failures = benchmark_result.assertion_failures;
  let list_reports = benchmark_result.reports;
  let duration = benchmark_result.duration;

  show_stats(&list_reports, cli.stats, cli.stats_format, cli.nanosec, cli.verbose, duration);

  // A failed `assert` check fails the whole run, ahead of any `--compare`
  // perf verdict, so CI sees a non-zero exit code.
  if assertion_failures > 0 {
    eprintln!("{}: {} assertion(s) failed", "error".red().bold(), assertion_failures);
    process::exit(1);
  }

  compare_benchmark(&list_reports, cli.compare.as_deref(), cli.threshold);

  process::exit(0)
}

fn compare_benchmark(list_reports: &[Vec<Report>], compare_path_option: Option<&str>, threshold_option: Option<f64>) {
  if let Some(compare_path) = compare_path_option {
    if let Some(threshold) = threshold_option {
      // A regression verdict has already printed its per-request slowness
      // lines; any other error is a bad/missing baseline file, reported here.
      match checker::compare(list_reports, compare_path, threshold) {
        Ok(()) => process::exit(0),
        Err(Error::Regressions(_)) => process::exit(1),
        Err(e) => {
          eprintln!("error: {e}");
          process::exit(1);
        }
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
  fn cli_stats_format_defaults_to_text() {
    // Without --stats-format, existing behaviour is unchanged: text.
    let cli = Cli::try_parse_from(["driller", "run", "http://example.com"]).unwrap();
    assert_eq!(cli.stats_format, StatsFormat::Text);
  }

  #[test]
  fn cli_stats_format_json_parses() {
    let cli = Cli::try_parse_from(["driller", "run", "http://example.com", "--stats-format", "json"]).unwrap();
    assert_eq!(cli.stats_format, StatsFormat::Json);
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

  #[test]
  fn cli_threshold_accepts_numeric_value() {
    let cli = Cli::try_parse_from(["driller", "--threshold", "100", "--compare", "baseline.yml"]).unwrap();
    assert_eq!(cli.threshold, Some(100.0));
  }

  #[test]
  fn cli_threshold_rejects_non_numeric_at_parse_time() {
    // Regression: previously '-stats' was parsed as bundled shorts '-s -t ats',
    // feeding 'ats' into --threshold; the parse failure only surfaced after
    // the benchmark had already run. The value parser now rejects this up front.
    let result = Cli::try_parse_from(["driller", "run", "http://example.com", "-stats"]);
    let err = match result {
      Ok(_) => panic!("expected parse error for bundled '-stats'"),
      Err(e) => e,
    };
    let msg = err.to_string();
    assert!(msg.contains("--threshold"), "error should mention --threshold, got: {msg}");
    assert!(msg.contains("'ats'"), "error should quote the rejected value 'ats', got: {msg}");
  }

  #[test]
  fn split_url_with_path() {
    let (base, path) = split_url("http://example.com/api/users");
    assert_eq!(base, "http://example.com");
    assert_eq!(path, "/api/users");
  }

  #[test]
  fn split_url_no_path() {
    let (base, path) = split_url("http://example.com");
    assert_eq!(base, "http://example.com");
    assert_eq!(path, "/");
  }

  #[test]
  fn split_url_with_port_and_path() {
    let (base, path) = split_url("http://localhost:3000/health");
    assert_eq!(base, "http://localhost:3000");
    assert_eq!(path, "/health");
  }

  // -- stdin ad-hoc target (`driller run -`) ----------------------------------

  #[test]
  fn read_url_from_reader_returns_trimmed_url() {
    let input = std::io::Cursor::new("  http://example.com/health  \n".as_bytes());
    assert_eq!(read_url_from_reader(input).unwrap(), Some("http://example.com/health".to_string()));
  }

  #[test]
  fn read_url_from_reader_skips_leading_blank_lines() {
    let input = std::io::Cursor::new("\n   \nhttp://example.com\nhttp://ignored\n".as_bytes());
    assert_eq!(read_url_from_reader(input).unwrap(), Some("http://example.com".to_string()));
  }

  #[test]
  fn read_url_from_reader_empty_input_is_none() {
    let input = std::io::Cursor::new("   \n\n".as_bytes());
    assert_eq!(read_url_from_reader(input).unwrap(), None);
  }

  #[test]
  fn read_url_from_reader_strips_leading_bom() {
    // A URL piped from a UTF-8-with-BOM file must not carry the BOM into the URL.
    let input = std::io::Cursor::new("\u{feff}http://example.com/health\n".as_bytes());
    assert_eq!(read_url_from_reader(input).unwrap(), Some("http://example.com/health".to_string()));
  }

  #[test]
  fn read_url_from_reader_non_utf8_is_err() {
    // Invalid UTF-8 on stdin must surface as a read error, not a missing URL.
    let input = std::io::Cursor::new([0xff, 0xff, b'\n'].as_slice());
    assert!(read_url_from_reader(input).is_err());
  }

  #[test]
  fn cli_run_dash_parses_as_url_arg() {
    // `-` must reach the positional `url` (not be rejected as a flag) so the
    // Run arm can route it to stdin.
    let cli = Cli::try_parse_from(["driller", "run", "-"]).unwrap();
    match cli.command {
      Some(Commands::Run(ref args)) => assert_eq!(args.url.as_deref(), Some("-")),
      _ => panic!("expected Run command"),
    }
  }
}
