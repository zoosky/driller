use std::collections::HashMap;

use colored::*;

use crate::actions::Report;
use crate::error::Error;
use crate::reader;

/// Compares the current run against a baseline report file.
///
/// The baseline is the flat list of request records that `--report` writes (one
/// record per request across the whole run). Both the baseline and the current
/// run are averaged per request `name`, and each name's mean duration is
/// compared. Averaging by name -- rather than by position in the file -- keeps
/// the verdict stable regardless of how many iterations either run used or the
/// order in which concurrent iterations completed (with `concurrency > 1` they
/// finish out of order, so a positional comparison would not be reproducible).
///
/// Returns `Ok(())` when every named request stays within `threshold`
/// milliseconds of its baseline mean. Records missing a `name` or numeric
/// `duration` are skipped rather than panicking.
///
/// # Errors
///
/// Returns [`Error::Regressions`] (carrying the count) when one or more named
/// requests regressed past the threshold; the per-request slowness lines are
/// printed before returning. A missing baseline file surfaces as [`Error::Io`],
/// and an empty or non-list baseline as [`Error::EmptyComparison`] -- the
/// library returns these rather than exiting the process.
pub fn compare(list_reports: &[Vec<Report>], filepath: &str, threshold: f64) -> Result<(), Error> {
  let docs = reader::read_file_as_yml(filepath)?;
  let items = match docs.first().and_then(|doc| doc.as_sequence()) {
    Some(items) if !items.is_empty() => items,
    _ => return Err(Error::EmptyComparison(filepath.to_string())),
  };

  // Mean baseline duration per request name. Records lacking a name or a numeric
  // duration (e.g. a hand-edited or truncated file) are skipped, not unwrapped.
  let mut baseline: HashMap<String, (f64, usize)> = HashMap::new();
  for item in items {
    if let (Some(name), Some(duration)) = (item.get("name").and_then(|v| v.as_str()), item.get("duration").and_then(|v| v.as_f64())) {
      accumulate(&mut baseline, name, duration);
    }
  }

  // Mean duration per request name for the current run.
  let mut current: HashMap<String, (f64, usize)> = HashMap::new();
  for report in list_reports.iter().flatten() {
    accumulate(&mut current, &report.name, report.duration);
  }

  println!();

  // Iterate in a stable (sorted) order so the output is deterministic.
  let mut names: Vec<&String> = current.keys().collect();
  names.sort();

  let mut slow_counter = 0usize;
  for name in names {
    let Some(baseline_mean) = mean(&baseline, name) else {
      continue; // this request has no baseline entry -- nothing to compare against
    };
    let delta_ms = mean(&current, name).expect("name came from the current map") - baseline_mean;

    if delta_ms > threshold {
      println!("{:width$} is {}{} slower than before", name.green(), delta_ms.round().to_string().red(), "ms".red(), width = 25);

      slow_counter += 1;
    }
  }

  if slow_counter == 0 {
    Ok(())
  } else {
    Err(Error::Regressions(slow_counter))
  }
}

/// Adds one `duration` sample for `name` to a running (sum, count) tally.
fn accumulate(means: &mut HashMap<String, (f64, usize)>, name: &str, duration: f64) {
  let entry = means.entry(name.to_string()).or_insert((0.0, 0));
  entry.0 += duration;
  entry.1 += 1;
}

/// Mean duration recorded for `name`, or `None` if it was never seen.
fn mean(means: &HashMap<String, (f64, usize)>, name: &str) -> Option<f64> {
  means.get(name).map(|(sum, count)| sum / *count as f64)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;
  use tempfile::NamedTempFile;

  fn report(name: &str, duration_ms: f64, status: u16) -> Report {
    Report {
      name: name.to_string(),
      duration: duration_ms,
      status,
    }
  }

  /// Writes a baseline file in the same `- name:/duration:` shape `--report`
  /// produces (the `status` line `--report` also writes is irrelevant here).
  fn comparison_file(records: &[(&str, f64)]) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    let items: Vec<String> = records.iter().map(|(name, d)| format!("- name: {name}\n  duration: {d}")).collect();
    write!(f, "{}", items.join("\n")).unwrap();
    f.flush().unwrap();
    f
  }

  #[test]
  fn all_within_threshold_returns_ok() {
    let f = comparison_file(&[("a", 100.0), ("b", 200.0)]);
    let reports = vec![vec![report("a", 110.0, 200), report("b", 205.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(result.is_ok());
  }

  #[test]
  fn exceeding_threshold_returns_err() {
    let f = comparison_file(&[("a", 100.0), ("b", 200.0)]);
    let reports = vec![vec![report("a", 200.0, 200), report("b", 205.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(matches!(result.unwrap_err(), Error::Regressions(1)));
  }

  #[test]
  fn exact_threshold_not_exceeded() {
    let f = comparison_file(&[("a", 100.0)]);
    let reports = vec![vec![report("a", 150.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(result.is_ok());
  }

  #[test]
  fn faster_than_baseline_returns_ok() {
    let f = comparison_file(&[("a", 200.0)]);
    let reports = vec![vec![report("a", 100.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(result.is_ok());
  }

  #[test]
  fn multiple_slow_requests_counted() {
    let f = comparison_file(&[("a", 100.0), ("b", 100.0), ("c", 100.0)]);
    let reports = vec![vec![report("a", 200.0, 200), report("b", 200.0, 200), report("c", 105.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(matches!(result.unwrap_err(), Error::Regressions(2)));
  }

  /// The verdict must not depend on the order iterations completed in -- which
  /// is non-deterministic under `concurrency > 1`. The same requests in two
  /// different iteration orders compare identically.
  #[test]
  fn compare_is_order_independent() {
    let f = comparison_file(&[("a", 100.0), ("b", 100.0)]);
    let order1 = vec![vec![report("a", 110.0, 200)], vec![report("b", 300.0, 200)]];
    let order2 = vec![vec![report("b", 300.0, 200)], vec![report("a", 110.0, 200)]];
    let r1 = compare(&order1, f.path().to_str().unwrap(), 50.0);
    let r2 = compare(&order2, f.path().to_str().unwrap(), 50.0);
    // Only `b` regressed (200ms over a 100ms baseline) in both orderings.
    assert!(matches!(r1.unwrap_err(), Error::Regressions(1)));
    assert!(matches!(r2.unwrap_err(), Error::Regressions(1)));
  }

  /// Multiple samples of the same name (multiple iterations, or a single-sample
  /// baseline vs a multi-iteration run) are averaged on each side before the
  /// comparison.
  #[test]
  fn samples_are_averaged_per_name() {
    let f = comparison_file(&[("a", 100.0), ("a", 100.0)]);
    // run mean for `a` = (140 + 160) / 2 = 150, baseline mean = 100, delta 50.
    let reports = vec![vec![report("a", 140.0, 200)], vec![report("a", 160.0, 200)]];
    assert!(matches!(compare(&reports, f.path().to_str().unwrap(), 40.0).unwrap_err(), Error::Regressions(1)));
    assert!(compare(&reports, f.path().to_str().unwrap(), 60.0).is_ok());
  }

  /// A request whose name is absent from the baseline is skipped, not compared
  /// against an unrelated record (and never panics).
  #[test]
  fn request_without_baseline_entry_is_skipped() {
    let f = comparison_file(&[("a", 100.0)]);
    let reports = vec![vec![report("a", 110.0, 200), report("z", 9999.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(result.is_ok());
  }
}
