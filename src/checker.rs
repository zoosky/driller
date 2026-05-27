use colored::*;

use crate::actions::Report;
use crate::reader;

/// Compares benchmark reports against a baseline YAML file.
///
/// Returns `Ok(())` when every request's duration delta stays within
/// `threshold` milliseconds of the baseline, or `Err(n)` where `n` is
/// the number of requests that exceeded it.
pub fn compare(list_reports: &[Vec<Report>], filepath: &str, threshold: f64) -> Result<(), i32> {
  let docs = reader::read_file_as_yml(filepath);
  let doc = &docs[0];
  let items = doc.as_sequence().unwrap();
  let mut slow_counter = 0;

  println!();

  for report in list_reports {
    for (i, report_item) in report.iter().enumerate() {
      let recorded_duration = items[i].get("duration").and_then(|v| v.as_f64()).unwrap();
      let delta_ms = report_item.duration - recorded_duration;

      if delta_ms > threshold {
        println!("{:width$} is {}{} slower than before", report_item.name.green(), delta_ms.round().to_string().red(), "ms".red(), width = 25);

        slow_counter += 1;
      }
    }
  }

  if slow_counter == 0 {
    Ok(())
  } else {
    Err(slow_counter)
  }
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

  fn comparison_file(durations: &[f64]) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    let items: Vec<String> = durations.iter().map(|d| format!("- duration: {d}")).collect();
    write!(f, "{}", items.join("\n")).unwrap();
    f.flush().unwrap();
    f
  }

  #[test]
  fn all_within_threshold_returns_ok() {
    let f = comparison_file(&[100.0, 200.0]);
    let reports = vec![vec![report("a", 110.0, 200), report("b", 205.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(result.is_ok());
  }

  #[test]
  fn exceeding_threshold_returns_err() {
    let f = comparison_file(&[100.0, 200.0]);
    let reports = vec![vec![report("a", 200.0, 200), report("b", 205.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert_eq!(result.unwrap_err(), 1);
  }

  #[test]
  fn exact_threshold_not_exceeded() {
    let f = comparison_file(&[100.0]);
    let reports = vec![vec![report("a", 150.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(result.is_ok());
  }

  #[test]
  fn faster_than_baseline_returns_ok() {
    let f = comparison_file(&[200.0]);
    let reports = vec![vec![report("a", 100.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert!(result.is_ok());
  }

  #[test]
  fn multiple_slow_requests_counted() {
    let f = comparison_file(&[100.0, 100.0, 100.0]);
    let reports = vec![vec![report("a", 200.0, 200), report("b", 200.0, 200), report("c", 105.0, 200)]];
    let result = compare(&reports, f.path().to_str().unwrap(), 50.0);
    assert_eq!(result.unwrap_err(), 2);
  }
}
