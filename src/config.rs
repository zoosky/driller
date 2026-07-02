use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

use serde_yaml::Value;

use crate::benchmark::{Context, RunOptions};
use crate::error::Error;
use crate::interpolator;
use crate::reader;

const NITERATIONS: i64 = 1;
const NCONCURRENCY: i64 = 1;
const NRAMPUP: i64 = 0;

/// Runtime configuration for a benchmark execution, assembled from
/// hard-coded defaults, benchmark YAML values, and CLI flag overrides.
pub struct Config {
  pub base: String,
  pub concurrency: i64,
  pub iterations: i64,
  pub relaxed_interpolations: bool,
  pub no_check_certificate: bool,
  pub rampup: i64,
  pub quiet: bool,
  pub nanosec: bool,
  pub timeout: u64,
  pub verbose: bool,
  pub duration: Option<Duration>,
  /// Number of `assert` checks that failed during the run. Shared across all
  /// concurrent iterations (the run is one `Config` wrapped in an `Arc`) and
  /// read once after every iteration has joined, so a failed assertion can set
  /// a non-zero process exit code without aborting the run.
  pub assertion_failures: Arc<AtomicUsize>,
}

impl Config {
  fn validate(concurrency: i64, iterations: i64, duration: Option<Duration>) -> Result<(), String> {
    if duration.is_none() && concurrency > iterations {
      return Err("the concurrency can not be higher than the number of iterations".to_string());
    }
    Ok(())
  }

  /// Constructs configuration using three-layer precedence:
  /// hard-coded defaults < benchmark YAML file < CLI flags.
  ///
  /// # Errors
  ///
  /// Returns [`Error::Io`] / [`Error::Yaml`] if the benchmark file cannot be
  /// read or parsed, or [`Error::InvalidConfig`] if the resolved values fail
  /// validation (for example concurrency exceeding the iteration count).
  pub fn new(options: &RunOptions) -> Result<Config, Error> {
    // Layer 1: hard-coded defaults
    let mut base = String::new();
    let mut iterations = NITERATIONS;
    let mut concurrency = NCONCURRENCY;
    let mut rampup = NRAMPUP;

    // Layer 2: benchmark YAML file values (if a file was provided)
    if let Some(ref path) = options.benchmark_path {
      let config_docs = reader::read_file_as_yml(path)?;
      let config_doc = &config_docs[0];

      let context: Context = Context::new();
      let interpolator = interpolator::Interpolator::new(&context);

      iterations = read_i64_configuration(config_doc, &interpolator, "iterations", NITERATIONS);
      concurrency = read_i64_configuration(config_doc, &interpolator, "concurrency", iterations);
      rampup = read_i64_configuration(config_doc, &interpolator, "rampup", NRAMPUP);
      base = read_str_configuration(config_doc, &interpolator, "base", "");
    }

    // Layer 3: CLI flag overrides
    if let Some(c) = options.concurrency {
      concurrency = c as i64;
    }
    if let Some(i) = options.iterations {
      iterations = i as i64;
    }
    if let Some(r) = options.rampup {
      rampup = r as i64;
    }
    if let Some(ref u) = options.base_url {
      base = u.clone();
    }

    // Rampup is not meaningful in duration mode (iteration counter grows
    // without bound, causing ever-increasing delays).
    if options.duration.is_some() {
      rampup = 0;
    }

    Self::validate(concurrency, iterations, options.duration).map_err(Error::InvalidConfig)?;

    Ok(Config {
      base,
      concurrency,
      iterations,
      relaxed_interpolations: options.relaxed_interpolations,
      no_check_certificate: options.no_check_certificate,
      rampup,
      quiet: options.quiet,
      nanosec: options.nanosec,
      timeout: options.timeout,
      verbose: options.verbose,
      duration: options.duration,
      assertion_failures: Arc::new(AtomicUsize::new(0)),
    })
  }
}

fn read_str_configuration(config_doc: &Value, interpolator: &interpolator::Interpolator, name: &str, default: &str) -> String {
  match config_doc.get(name).and_then(|v| v.as_str()) {
    Some(value) => {
      if value.contains('{') {
        interpolator.resolve(value, true)
      } else {
        value.to_owned()
      }
    }
    None => {
      if config_doc.get(name).and_then(|v| v.as_str()).is_some() {
        println!("Invalid {name} value!");
      }

      default.to_owned()
    }
  }
}

fn read_i64_configuration(config_doc: &Value, interpolator: &interpolator::Interpolator, name: &str, default: i64) -> i64 {
  let value = if let Some(value) = config_doc.get(name).and_then(|v| v.as_i64()) {
    Some(value)
  } else if let Some(key) = config_doc.get(name).and_then(|v| v.as_str()) {
    interpolator.resolve(key, false).parse::<i64>().ok()
  } else {
    None
  };

  match value {
    Some(value) => {
      if value < 0 {
        println!("Invalid negative {name} value!");

        default
      } else {
        value
      }
    }
    None => {
      if config_doc.get(name).and_then(|v| v.as_str()).is_some() {
        println!("Invalid {name} value!");
      }

      default
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::benchmark::RunOptions;
  use std::io::Write;
  use tempfile::NamedTempFile;

  fn default_options() -> RunOptions {
    RunOptions {
      benchmark_path: None,
      report_path: None,
      base_url: None,
      url_path: None,
      concurrency: None,
      iterations: None,
      duration: None,
      rampup: None,
      worker_threads: None,
      relaxed_interpolations: false,
      no_check_certificate: false,
      quiet: false,
      nanosec: false,
      timeout: 10,
      verbose: false,
      machine_readable: false,
      tags: crate::tags::Tags::new(None, None),
    }
  }

  fn yaml_file(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    write!(f, "{content}").unwrap();
    f.flush().unwrap();
    f
  }

  // -- Layer 1: hard-coded defaults -------------------------------------------

  #[test]
  fn defaults_without_file_or_cli() {
    let config = Config::new(&default_options()).unwrap();
    assert_eq!(config.iterations, 1);
    assert_eq!(config.concurrency, 1);
    assert_eq!(config.rampup, 0);
    assert_eq!(config.base, "");
    assert!(config.duration.is_none());
    assert_eq!(config.timeout, 10);
  }

  // -- Layer 2: YAML file values ----------------------------------------------

  #[test]
  fn yaml_values_override_defaults() {
    let f = yaml_file("base: http://example.com\niterations: 50\nconcurrency: 10\nrampup: 5\nplan:\n  - name: t\n    request:\n      url: /\n");
    let options = RunOptions {
      benchmark_path: Some(f.path().to_str().unwrap().to_string()),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.base, "http://example.com");
    assert_eq!(config.iterations, 50);
    assert_eq!(config.concurrency, 10);
    assert_eq!(config.rampup, 5);
  }

  #[test]
  fn yaml_concurrency_defaults_to_iterations() {
    let f = yaml_file("iterations: 50\nplan:\n  - name: t\n    request:\n      url: /\n");
    let options = RunOptions {
      benchmark_path: Some(f.path().to_str().unwrap().to_string()),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.iterations, 50);
    assert_eq!(config.concurrency, 50);
  }

  // -- Layer 3: CLI flag overrides --------------------------------------------

  #[test]
  fn cli_overrides_yaml() {
    let f = yaml_file("base: http://file.com\niterations: 50\nconcurrency: 10\nrampup: 5\nplan:\n  - name: t\n    request:\n      url: /\n");
    let options = RunOptions {
      benchmark_path: Some(f.path().to_str().unwrap().to_string()),
      base_url: Some("http://staging:3000".to_string()),
      concurrency: Some(20),
      iterations: Some(100),
      rampup: Some(10),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.base, "http://staging:3000");
    assert_eq!(config.iterations, 100);
    assert_eq!(config.concurrency, 20);
    assert_eq!(config.rampup, 10);
  }

  #[test]
  fn partial_cli_override_preserves_yaml() {
    let f = yaml_file("base: http://file.com\niterations: 100\nconcurrency: 50\nrampup: 5\nplan:\n  - name: t\n    request:\n      url: /\n");
    let options = RunOptions {
      benchmark_path: Some(f.path().to_str().unwrap().to_string()),
      concurrency: Some(20),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.concurrency, 20);
    assert_eq!(config.iterations, 100);
    assert_eq!(config.rampup, 5);
    assert_eq!(config.base, "http://file.com");
  }

  #[test]
  fn cli_overrides_without_yaml() {
    let options = RunOptions {
      base_url: Some("http://test:8080".to_string()),
      concurrency: Some(5),
      iterations: Some(10),
      rampup: Some(2),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.base, "http://test:8080");
    assert_eq!(config.iterations, 10);
    assert_eq!(config.concurrency, 5);
    assert_eq!(config.rampup, 2);
  }

  // -- Duration mode ----------------------------------------------------------

  #[test]
  fn duration_zeroes_rampup() {
    let options = RunOptions {
      rampup: Some(10),
      duration: Some(Duration::from_secs(30)),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.rampup, 0);
    assert_eq!(config.duration, Some(Duration::from_secs(30)));
  }

  #[test]
  fn duration_mode_allows_high_concurrency() {
    let options = RunOptions {
      concurrency: Some(10),
      duration: Some(Duration::from_secs(30)),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.concurrency, 10);
    assert_eq!(config.iterations, 1);
  }

  #[test]
  fn duration_propagated_to_config() {
    let dur = Duration::from_secs(60);
    let options = RunOptions {
      duration: Some(dur),
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert_eq!(config.duration, Some(dur));
  }

  // -- Validation -------------------------------------------------------------

  #[test]
  fn concurrency_exceeds_iterations_is_error() {
    assert!(Config::validate(10, 5, None).is_err());
  }

  #[test]
  fn concurrency_exceeds_iterations_ok_with_duration() {
    assert!(Config::validate(10, 5, Some(Duration::from_secs(30))).is_ok());
  }

  #[test]
  fn concurrency_within_iterations_ok() {
    assert!(Config::validate(5, 10, None).is_ok());
  }

  // -- Boolean / scalar pass-through ------------------------------------------

  #[test]
  fn boolean_flags_pass_through() {
    let options = RunOptions {
      relaxed_interpolations: true,
      no_check_certificate: true,
      quiet: true,
      nanosec: true,
      verbose: true,
      timeout: 42,
      ..default_options()
    };
    let config = Config::new(&options).unwrap();
    assert!(config.relaxed_interpolations);
    assert!(config.no_check_certificate);
    assert!(config.quiet);
    assert!(config.nanosec);
    assert!(config.verbose);
    assert_eq!(config.timeout, 42);
  }
}
