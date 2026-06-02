//! driller -- HTTP load-testing engine with Ansible-style YAML plans.
//!
//! This crate is the reusable engine behind the `driller` binary. The binary
//! (`src/main.rs`) owns the command-line interface -- argument parsing, version
//! strings, the stats/compare presentation, and process exit codes -- while
//! everything that actually loads a plan and runs a benchmark lives here, so
//! integration tests, benchmarks, and sibling tools can drive the engine
//! directly without shelling out to the binary.
//!
//! # Entry point
//!
//! [`run`] is the single typed entry point: build a [`RunOptions`] and call it.
//!
//! ```no_run
//! use driller::{run, RunOptions};
//!
//! # fn build_options() -> RunOptions { unimplemented!() }
//! let options: RunOptions = build_options();
//! let result = run(&options);
//! println!("{} assertion(s) failed", result.assertion_failures);
//! ```
//!
//! # Module surface
//!
//! The engine modules are exposed at module level for in-repo consumers
//! (tests, benchmarks, and sibling crates); their members keep their original
//! visibility. `interpolator` and `writer` stay crate-private as they appear in
//! no public interface.

pub mod actions;
pub mod benchmark;
pub mod checker;
pub mod config;
pub mod expandable;
pub mod reader;
pub mod tags;

mod interpolator;
mod writer;

pub use benchmark::{BenchmarkResult, RunOptions};

/// Runs a benchmark to completion and returns its aggregated result.
///
/// This is the library's single entry point for executing a run. The `driller`
/// binary constructs a [`RunOptions`] from parsed command-line arguments and
/// calls this; tests and benchmarks can do the same without going through the
/// CLI layer.
///
/// The call builds its own tokio runtime (current-thread when `worker_threads`
/// is `None`/`1`, multi-thread when it is `2` or more) and blocks until the run
/// finishes, so it is safe to call from a synchronous context.
///
/// # Process exit on fatal input
///
/// Some invalid inputs are currently treated as fatal and terminate the
/// process via `std::process::exit(1)` rather than returning -- an empty plan,
/// and the unreadable/malformed-file paths in [`reader`]. A caller embedding
/// the engine should pre-validate its [`RunOptions`] accordingly. Migrating
/// these paths to returned errors is tracked as a follow-up.
pub fn run(options: &RunOptions) -> BenchmarkResult {
  benchmark::execute(options)
}
