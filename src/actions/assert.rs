use std::sync::atomic::Ordering;

use async_trait::async_trait;
use colored::*;
use serde_json::json;
use serde_yaml::Value;

use crate::actions::Runnable;
use crate::actions::extract;
use crate::benchmark::{Context, Pool, Reports};
use crate::config::Config;
use crate::interpolator;

#[derive(Clone)]
pub struct Assert {
  name: String,
  key: String,
  value: String,
}

impl Assert {
  pub fn is_that_you(item: &Value) -> bool {
    item.get("assert").and_then(|v| v.as_mapping()).is_some()
  }

  pub fn new(item: &Value, _with_item: Option<Value>) -> Assert {
    let name = extract(item, "name");
    let assert_val = item.get("assert").expect("assert field is required");
    let key = extract(assert_val, "key");
    let value = extract(assert_val, "value");

    Assert {
      name,
      key,
      value,
    }
  }
}

#[async_trait]
impl Runnable for Assert {
  async fn execute(&self, context: &mut Context, _reports: &mut Reports, _pool: &Pool, config: &Config) {
    if !config.quiet {
      println!("{:width$} {}={}?", self.name.green(), self.key.cyan().bold(), self.value.magenta(), width = 25);
    }

    let interpolator = interpolator::Interpolator::new(context);
    let eval = format!("{{{{ {} }}}}", &self.key);
    let stored = interpolator.resolve(&eval, true);
    let assertion = json!(self.value.to_owned());

    // A failed assertion is a result of the plan under test, not a fault in
    // driller itself, so report it as a clean per-check failure and let the
    // run continue (other checks in the same plan still get to report). The
    // shared failure counter drives a non-zero process exit code once the run
    // completes, so CI can detect it. Strict-equality semantics are unchanged.
    if !stored.eq(&assertion) {
      println!("{:width$} {}: {} -- expected {}, got {}", self.name.green(), "FAIL".red().bold(), self.key.cyan().bold(), format!("{:?}", self.value).magenta(), format!("{stored:?}").magenta(), width = 25);

      config.assertion_failures.fetch_add(1, Ordering::Relaxed);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::benchmark::{Context, Pool, PoolStore, Reports};
  use crate::config::Config;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::sync::{Arc, Mutex};

  fn test_config() -> Config {
    Config {
      base: String::new(),
      concurrency: 1,
      iterations: 1,
      relaxed_interpolations: false,
      no_check_certificate: false,
      rampup: 0,
      quiet: true,
      nanosec: false,
      timeout: 10,
      verbose: false,
      duration: None,
      assertion_failures: Arc::new(AtomicUsize::new(0)),
    }
  }

  fn assert_item(name: &str, key: &str, value: &str) -> Assert {
    let item: Value = serde_yaml::from_str(&format!("name: {name}\nassert:\n  key: {key}\n  value: {value}\n")).unwrap();
    Assert::new(&item, None)
  }

  /// Runs a single `assert` against a context seeded with `key=stored`, and
  /// returns the shared failure tally afterwards.
  fn run_one(key: &str, stored: serde_json::Value, expected: &str) -> Arc<AtomicUsize> {
    let assert = assert_item("Check", key, expected);

    let mut context: Context = Context::new();
    context.insert(key.to_string(), stored);
    let mut reports: Reports = Vec::new();
    let pool: Pool = Arc::new(Mutex::new(PoolStore::new()));
    let config = test_config();
    let failures = config.assertion_failures.clone();

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(assert.execute(&mut context, &mut reports, &pool, &config));

    failures
  }

  /// A mismatched assertion must report a clean failure and tally it, not
  /// `panic!` (the test thread completing at all proves there was no panic).
  #[test]
  fn assert_mismatch_reports_failure_not_panic() {
    let failures = run_one("status", json!(404), "200");
    assert_eq!(failures.load(Ordering::Relaxed), 1);
  }

  /// After a failed assertion the run continues, so a later check in the same
  /// iteration still executes; only the failing check is tallied.
  #[test]
  fn assert_match_continues_run() {
    let failing = assert_item("A", "status", "200");
    let passing = assert_item("B", "status", "404");

    let mut context: Context = Context::new();
    context.insert("status".to_string(), json!(404));
    let mut reports: Reports = Vec::new();
    let pool: Pool = Arc::new(Mutex::new(PoolStore::new()));
    let config = test_config();

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
      failing.execute(&mut context, &mut reports, &pool, &config).await;
      passing.execute(&mut context, &mut reports, &pool, &config).await;
    });

    assert_eq!(config.assertion_failures.load(Ordering::Relaxed), 1);
  }

  /// The tally `main` turns into the exit code: zero when every assertion
  /// matches, non-zero as soon as one fails.
  #[test]
  fn assert_exit_code_reflects_any_failure() {
    assert_eq!(run_one("status", json!(200), "200").load(Ordering::Relaxed), 0);
    assert_eq!(run_one("status", json!(500), "200").load(Ordering::Relaxed), 1);
  }
}
