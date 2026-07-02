//! Aggregated run statistics: computation, human-readable text rendering, and
//! the machine-readable JSON view.
//!
//! A run produces one [`Report`] per executed step per iteration. This module
//! folds those into per-step and global aggregates ([`DrillStats`]) and renders
//! them one of two ways, selected by [`StatsFormat`]:
//!
//! - `text` (the default) -- the colored, column-aligned summary written for a
//!   terminal, unchanged from earlier releases.
//! - `json` -- a single structured document written to stdout and nothing else,
//!   so a downstream consumer (a CI gate, a dashboard, a trending script) can
//!   `jq` the numbers instead of scraping ANSI-decorated text.
//!
//! The JSON path exists because [`DrillStats`] owns an
//! [`hdrhistogram::Histogram`], which is not `Serialize`; a set of small view
//! structs ([`StatsView`] and friends) is populated from the histogram's
//! accessors and serialized in its place. Latency figures are emitted as raw
//! milliseconds (floats); `--nanosec` is a text-display concern and does not
//! change the JSON numbers.

use std::collections::BTreeMap;
use std::collections::HashMap;

use clap::ValueEnum;
use colored::*;
use driller::actions::Report;
use hdrhistogram::Histogram;
use linked_hash_map::LinkedHashMap;
use serde::Serialize;

/// Version of the JSON stats document. Bumped when the shape changes so a
/// downstream consumer can pin to a layout it understands. The value is carried
/// in the top-level `schema` field of every JSON summary.
const STATS_SCHEMA_VERSION: u32 = 1;

/// Rendering format for the statistics summary.
///
/// `Text` reproduces the colored, column-aligned terminal output byte-for-byte;
/// `Json` emits a single structured document to stdout. `Json` also implies that
/// statistics are produced, so a caller need not additionally request them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, ValueEnum)]
pub enum StatsFormat {
  /// Colored, column-aligned text designed for a terminal (default).
  #[default]
  Text,
  /// A single JSON document written to stdout, with everything else on stderr.
  Json,
}

/// Aggregated statistics for one bucket of requests (a single plan step, or the
/// whole run). Latency is held in an [`hdrhistogram::Histogram`] recorded in
/// microseconds; the accessors convert to milliseconds.
struct DrillStats {
  total_requests: usize,
  successful_requests: usize,
  failed_requests: usize,
  /// Count of requests per exact HTTP status code, sorted ascending. The
  /// synthetic status 520 represents a connection error (see actions::request).
  status_counts: BTreeMap<u16, usize>,
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

/// Folds a slice of per-request reports into a single [`DrillStats`] bucket:
/// request totals, the success/failure split (2xx is success), a per-code count
/// map, and a latency histogram.
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

  // Count of each exact status code (BTreeMap keeps them sorted for display).
  let mut status_counts: BTreeMap<u16, usize> = BTreeMap::new();
  for req in sub_reports {
    *status_counts.entry(req.status).or_insert(0) += 1;
  }

  let total_requests = sub_reports.len();
  let successful_requests = group_by_status.entry(2).or_insert_with(Vec::new).len();
  let failed_requests = total_requests - successful_requests;

  DrillStats {
    total_requests,
    successful_requests,
    failed_requests,
    status_counts,
    hist,
  }
}

/// Latency percentiles and moments for one bucket, in raw milliseconds.
#[derive(Serialize)]
struct LatencyView {
  mean: f64,
  median: f64,
  stdev: f64,
  p99: f64,
  p995: f64,
  p999: f64,
}

/// Requests bucketed by HTTP family, machine-readable form of the text rollup.
///
/// Each present family (`2xx`/`3xx`/`4xx`/`5xx`) is a key with count > 0;
/// `connection_errors` (the synthetic status 520) is always present and kept
/// out of `5xx`, mirroring the text summary's `conn` total.
#[derive(Serialize)]
struct ClassRollup {
  #[serde(flatten)]
  classes: BTreeMap<String, usize>,
  connection_errors: usize,
}

/// The per-bucket JSON shape shared by the global summary and each step.
#[derive(Serialize)]
struct BucketView {
  total_requests: usize,
  successful_requests: usize,
  failed_requests: usize,
  status_counts: BTreeMap<String, usize>,
  class_rollup: ClassRollup,
  latency_ms: LatencyView,
}

/// A single plan step's statistics, tagged with the step `name`, in plan order.
#[derive(Serialize)]
struct StepStatsView {
  name: String,
  #[serde(flatten)]
  bucket: BucketView,
}

/// The full JSON stats document emitted by `--stats-format json`.
#[derive(Serialize)]
struct StatsView {
  schema: u32,
  duration_s: f64,
  requests_per_second: f64,
  global: BucketView,
  steps: Vec<StepStatsView>,
}

/// Buckets status codes by HTTP family, splitting the synthetic connection-error
/// status 520 into its own tally. Shared source of truth for both the text
/// rollup line and the JSON `class_rollup` object.
fn class_rollup(status_counts: &BTreeMap<u16, usize>) -> ClassRollup {
  let mut classes: BTreeMap<String, usize> = BTreeMap::new();
  let mut connection_errors = 0;
  for (code, count) in status_counts {
    if *code == 520 {
      connection_errors += count;
    } else {
      *classes.entry(format!("{}xx", code / 100)).or_insert(0) += count;
    }
  }
  ClassRollup {
    classes,
    connection_errors,
  }
}

/// Renders a [`ClassRollup`] as the text summary's parts, e.g.
/// `["2xx 8", "4xx 3", "5xx 1", "conn 4"]` -- families in ascending order with
/// connection errors appended only when non-zero.
fn status_class_rollup_parts(rollup: &ClassRollup) -> Vec<String> {
  let mut parts: Vec<String> = rollup.classes.iter().map(|(class, count)| format!("{class} {count}")).collect();
  if rollup.connection_errors > 0 {
    parts.push(format!("conn {}", rollup.connection_errors));
  }
  parts
}

/// Builds the class-rollup parts for the status-code summary line: each HTTP
/// family (`2xx`/`3xx`/`4xx`/`5xx`) summed across all its codes (so 200, 201,
/// 204 fold into one `2xx` total), in ascending family order. The synthetic
/// status 520 is kept out of the `5xx` bucket and appended as a separate `conn`
/// total so dropped connections stay distinct from server errors.
fn status_class_rollup(status_counts: &BTreeMap<u16, usize>) -> Vec<String> {
  status_class_rollup_parts(&class_rollup(status_counts))
}

/// Projects a [`DrillStats`] bucket into its serializable [`BucketView`],
/// pulling latency figures (raw ms) from the histogram accessors.
fn bucket_view(stats: &DrillStats) -> BucketView {
  BucketView {
    total_requests: stats.total_requests,
    successful_requests: stats.successful_requests,
    failed_requests: stats.failed_requests,
    status_counts: stats.status_counts.iter().map(|(code, count)| (code.to_string(), *count)).collect(),
    class_rollup: class_rollup(&stats.status_counts),
    latency_ms: LatencyView {
      mean: stats.mean_duration(),
      median: stats.median_duration(),
      stdev: stats.stdev_duration(),
      p99: stats.value_at_quantile(0.99),
      p995: stats.value_at_quantile(0.995),
      p999: stats.value_at_quantile(0.999),
    },
  }
}

/// Assembles the full [`StatsView`] from the run's per-iteration reports: one
/// `steps` entry per distinct step name in plan order, plus the global rollup
/// across every request. `duration` is the wall-clock run time in seconds.
fn build_stats_view(list_reports: &[Vec<Report>], duration: f64) -> StatsView {
  // Group by step name in plan order (LinkedHashMap preserves insertion order),
  // matching the per-name grouping the text summary builds.
  let mut group_by_name = LinkedHashMap::new();
  for req in list_reports.concat() {
    group_by_name.entry(req.name.clone()).or_insert_with(Vec::new).push(req);
  }

  let steps = group_by_name
    .into_iter()
    .map(|(name, reports)| StepStatsView {
      name,
      bucket: bucket_view(&compute_stats(&reports)),
    })
    .collect();

  let allreports = list_reports.concat();
  let global_stats = compute_stats(&allreports);
  // Guard the divide so a zero-duration or empty run reports 0 rather than NaN.
  let requests_per_second = if duration > 0.0 {
    global_stats.total_requests as f64 / duration
  } else {
    0.0
  };

  StatsView {
    schema: STATS_SCHEMA_VERSION,
    duration_s: duration,
    requests_per_second,
    global: bucket_view(&global_stats),
    steps,
  }
}

/// Prints the per-status-code breakdown for a stats bucket, followed by a
/// class rollup that buckets codes by family (`2xx`/`3xx`/`4xx`/`5xx`) -- so a
/// run returning a mix of e.g. 200/201/204 sums them into a single `2xx`
/// total. The synthetic status 520 is labelled as a connection error and kept
/// out of the `5xx` bucket, reported as a separate `conn` total instead.
///
/// With `name = None` (the global summary) it prints the per-code lines plus
/// the rollup. With `name = Some(step)` (a per-step summary, shown only under
/// `--verbose`) it prints a single compact `code:count` line under the step's
/// named columns to keep the per-step output tight.
fn show_status_codes(stats: &DrillStats, name: Option<&str>) {
  if stats.status_counts.is_empty() {
    return;
  }

  if let Some(name) = name {
    let codes = stats.status_counts.iter().map(|(code, count)| format!("{code}:{count}")).collect::<Vec<_>>().join(" ");
    println!("{:width$} {:width2$} {}", name.green(), "Status codes".yellow(), codes.cyan(), width = 25, width2 = 25);
    return;
  }

  println!("{}", "Status codes".yellow());
  for (code, count) in &stats.status_counts {
    let label = if *code == 520 {
      format!("{code} (connection error)")
    } else {
      code.to_string()
    };
    println!("  {:width$} {}", label.cyan(), count.to_string().cyan(), width = 23);
  }

  println!("  {}", status_class_rollup(&stats.status_counts).join(" · ").dimmed());
}

fn format_time(tdiff: f64, nanosec: bool) -> String {
  if nanosec {
    (1_000_000.0 * tdiff).round().to_string() + "ns"
  } else {
    tdiff.round().to_string() + "ms"
  }
}

/// Renders the run statistics in the selected [`StatsFormat`].
///
/// In `Json` mode a single document is written to stdout regardless of
/// `stats_option` (JSON implies stats), and nothing else is written to stdout;
/// `nanosec` and `verbose` do not affect the JSON. In `Text` mode the summary is
/// printed only when `stats_option` is set, reproducing the previous output.
pub fn show_stats(list_reports: &[Vec<Report>], stats_option: bool, format: StatsFormat, nanosec: bool, verbose: bool, duration: f64) {
  if format == StatsFormat::Json {
    let view = build_stats_view(list_reports, duration);
    match serde_json::to_string_pretty(&view) {
      Ok(json) => println!("{json}"),
      // A plain view of numbers and strings does not fail to serialize in
      // practice; report to stderr rather than corrupt stdout if it ever does.
      Err(e) => eprintln!("error: failed to serialize stats as JSON: {e}"),
    }
    return;
  }

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
    println!("{:width$} {:width2$} {}", name.green(), "Total requests".yellow(), substats.total_requests.to_string().cyan(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Successful requests".yellow(), substats.successful_requests.to_string().cyan(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Failed requests".yellow(), substats.failed_requests.to_string().cyan(), width = 25, width2 = 25);
    if verbose {
      show_status_codes(&substats, Some(&name));
    }
    println!("{:width$} {:width2$} {}", name.green(), "Median time per request".yellow(), format_time(substats.median_duration(), nanosec).cyan(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Average time per request".yellow(), format_time(substats.mean_duration(), nanosec).cyan(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "Sample standard deviation".yellow(), format_time(substats.stdev_duration(), nanosec).cyan(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.0'th percentile".yellow(), format_time(substats.value_at_quantile(0.99), nanosec).cyan(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.5'th percentile".yellow(), format_time(substats.value_at_quantile(0.995), nanosec).cyan(), width = 25, width2 = 25);
    println!("{:width$} {:width2$} {}", name.green(), "99.9'th percentile".yellow(), format_time(substats.value_at_quantile(0.999), nanosec).cyan(), width = 25, width2 = 25);
  }

  // compute global stats
  let allreports = list_reports.concat();
  let global_stats = compute_stats(&allreports);
  // Guard the divide so a zero-duration or empty run reports 0.00 rather than NaN.
  let requests_per_second = if duration > 0.0 {
    global_stats.total_requests as f64 / duration
  } else {
    0.0
  };

  println!();
  println!("{:width2$} {} {}", "Time taken for tests".yellow(), format!("{duration:.1}").cyan(), "seconds".cyan(), width2 = 25);
  println!("{:width2$} {}", "Total requests".yellow(), global_stats.total_requests.to_string().cyan(), width2 = 25);
  println!("{:width2$} {}", "Successful requests".yellow(), global_stats.successful_requests.to_string().cyan(), width2 = 25);
  println!("{:width2$} {}", "Failed requests".yellow(), global_stats.failed_requests.to_string().cyan(), width2 = 25);
  show_status_codes(&global_stats, None);
  println!("{:width2$} {} {}", "Requests per second".yellow(), format!("{requests_per_second:.2}").cyan(), "[#/sec]".cyan(), width2 = 25);
  println!("{:width2$} {}", "Median time per request".yellow(), format_time(global_stats.median_duration(), nanosec).cyan(), width2 = 25);
  println!("{:width2$} {}", "Average time per request".yellow(), format_time(global_stats.mean_duration(), nanosec).cyan(), width2 = 25);
  println!("{:width2$} {}", "Sample standard deviation".yellow(), format_time(global_stats.stdev_duration(), nanosec).cyan(), width2 = 25);
  println!("{:width2$} {}", "99.0'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.99), nanosec).cyan(), width2 = 25);
  println!("{:width2$} {}", "99.5'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.995), nanosec).cyan(), width2 = 25);
  println!("{:width2$} {}", "99.9'th percentile".yellow(), format_time(global_stats.value_at_quantile(0.999), nanosec).cyan(), width2 = 25);
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
  fn stats_records_status_breakdown() {
    let reports = vec![
      report("a", 10.0, 200),
      report("b", 11.0, 200),
      report("c", 12.0, 404),
      report("d", 13.0, 500),
      report("e", 14.0, 520), // connection error
    ];
    let stats = compute_stats(&reports);
    assert_eq!(stats.status_counts.get(&200), Some(&2));
    assert_eq!(stats.status_counts.get(&404), Some(&1));
    assert_eq!(stats.status_counts.get(&500), Some(&1));
    assert_eq!(stats.status_counts.get(&520), Some(&1));
    // every request is accounted for exactly once
    assert_eq!(stats.status_counts.values().sum::<usize>(), stats.total_requests);
  }

  #[test]
  fn class_rollup_buckets_codes_and_splits_connection_errors() {
    let counts: BTreeMap<u16, usize> = [(200, 5), (201, 2), (204, 1), (404, 3), (500, 1), (520, 4)].into_iter().collect();
    // 200+201+204 -> 2xx 8; 404 -> 4xx 3; 500 -> 5xx 1; 520 -> conn 4 (not 5xx).
    assert_eq!(status_class_rollup(&counts), vec!["2xx 8", "4xx 3", "5xx 1", "conn 4"]);
  }

  #[test]
  fn stats_format_defaults_to_text() {
    // The default rendering path is unchanged for existing users: no
    // `--stats-format` means text.
    assert_eq!(StatsFormat::default(), StatsFormat::Text);
  }

  #[test]
  fn json_view_shape_and_connection_error_split() {
    // A run with a mix of codes, including the synthetic 520 connection error,
    // across two named steps. Global and per-step JSON must carry the exact
    // counts, keep 520 out of 5xx (folding it into connection_errors), and
    // expose latency in milliseconds.
    let list_reports = vec![vec![report("fetch", 1.0, 200), report("save", 2.0, 500)], vec![report("fetch", 3.0, 404), report("save", 4.0, 520)]];
    let view = build_stats_view(&list_reports, 2.0);
    let json = serde_json::to_value(&view).unwrap();

    assert_eq!(json["schema"], STATS_SCHEMA_VERSION);
    assert_eq!(json["duration_s"], 2.0);
    // 4 requests over 2 seconds.
    assert_eq!(json["requests_per_second"], 2.0);

    let global = &json["global"];
    assert_eq!(global["total_requests"], 4);
    assert_eq!(global["successful_requests"], 1); // only the 200
    assert_eq!(global["failed_requests"], 3);
    assert_eq!(global["status_counts"]["200"], 1);
    assert_eq!(global["status_counts"]["404"], 1);
    assert_eq!(global["status_counts"]["500"], 1);
    assert_eq!(global["status_counts"]["520"], 1);

    // 520 is a connection error, not a 5xx: the class rollup splits them.
    assert_eq!(global["class_rollup"]["2xx"], 1);
    assert_eq!(global["class_rollup"]["4xx"], 1);
    assert_eq!(global["class_rollup"]["5xx"], 1);
    assert_eq!(global["class_rollup"]["connection_errors"], 1);
    // No 3xx occurred, so that family is absent (not zero-filled).
    assert!(global["class_rollup"].get("3xx").is_none());

    // Latency is emitted in milliseconds as raw numbers.
    for key in ["mean", "median", "stdev", "p99", "p995", "p999"] {
      assert!(global["latency_ms"][key].is_number(), "latency_ms.{key} should be a number");
    }

    // Two steps, in plan order, each tagged with its name.
    let steps = json["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["name"], "fetch");
    assert_eq!(steps[1]["name"], "save");
    // The `save` step saw a 500 and a 520: one 5xx, one connection error.
    assert_eq!(steps[1]["class_rollup"]["5xx"], 1);
    assert_eq!(steps[1]["class_rollup"]["connection_errors"], 1);
  }

  #[test]
  fn json_document_is_the_only_thing_on_a_json_render() {
    // `to_string_pretty` produces a single well-formed object (parses back).
    let view = build_stats_view(&[vec![report("ping", 1.0, 200)]], 1.0);
    let rendered = serde_json::to_string_pretty(&view).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert!(parsed.is_object());
    assert_eq!(parsed["global"]["total_requests"], 1);
  }
}
