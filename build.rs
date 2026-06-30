//! Build script for driller.
//!
//! Captures git commit hash, build timestamp, and target triple at compile
//! time and exposes them as `GIT_HASH`, `BUILD_TIME`, and `BUILD_TARGET`
//! environment variables. `src/main.rs` reads them via `env!()` to build
//! the `--version` string so users (and the b-bug investigation harness)
//! can tell which exact build is running, not just which version was
//! published.
//!
//! Pattern lifted from accentcms's `build.rs`.

use std::process::Command;

fn main() {
  // Short git commit hash, resolved in priority order:
  //
  //   1. `git rev-parse --short HEAD` -- authoritative for any build from a
  //      checkout with a `.git`. It can fail to spawn (no git on PATH), exit
  //      non-zero (not a working tree, or "dubious ownership" in a CI
  //      checkout), or return nothing; in every such case the output is empty
  //      and must not be embedded verbatim (that is how a release binary once
  //      printed `0.10.2 ()`). Trying git first means a stray `git-hash` file
  //      left over in a working tree can never shadow the real commit.
  //   2. A packaged `git-hash` file. A `cargo install` from crates.io builds
  //      from the published tarball, which strips `.git`, so `git rev-parse`
  //      finds nothing and the hash must travel *inside* the package. The
  //      release/publish step writes the short hash to `git-hash` and stages it
  //      so `cargo publish` includes it (see CONTRIBUTING.md). This file is
  //      absent in normal checkouts, where step 1 already answered.
  //   3. GitHub Actions' `GITHUB_SHA` (short) -- containers without `git`,
  //      e.g. the `cross` musl build (forwarded via Cross.toml).
  //   4. "unknown", only when every source above is unavailable.
  let git_hash = Command::new("git")
    .args(["rev-parse", "--short", "HEAD"])
    .output()
    .ok()
    .filter(|o| o.status.success())
    .and_then(|o| String::from_utf8(o.stdout).ok())
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .or_else(|| std::fs::read_to_string("git-hash").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
    .or_else(|| std::env::var("GITHUB_SHA").ok().map(|s| s.chars().take(7).collect::<String>()))
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "unknown".to_string());

  println!("cargo:rustc-env=GIT_HASH={git_hash}");

  // Build timestamp in UTC, in a stable `YYYY-MM-DD HH:MM:SS UTC` format.
  // Pure Rust (no chrono/time dependency) so the build script stays
  // dependency-free and cross-platform.
  let build_time = {
    use std::time::SystemTime;
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
    let secs_per_day = 86400u64;
    let days = now / secs_per_day;
    let day_secs = now % secs_per_day;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;
    // Days since epoch comfortably fits in i64 for any realistic build time.
    #[allow(clippy::cast_possible_wrap)]
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02} {hours:02}:{minutes:02}:{seconds:02} UTC")
  };

  println!("cargo:rustc-env=BUILD_TIME={build_time}");

  // Target triple (cargo sets TARGET in the build script's environment).
  let target = std::env::var("TARGET").unwrap_or_default();
  println!("cargo:rustc-env=BUILD_TARGET={target}");

  // Trigger a rebuild when HEAD moves so the embedded git hash stays in
  // sync with the working tree without needing `cargo clean`.
  println!("cargo:rerun-if-changed=.git/HEAD");
  println!("cargo:rerun-if-changed=.git/refs/heads/");
  // Only watch `git-hash` when it actually exists (the publish/tarball path).
  // Listing a path that does not exist makes Cargo treat the build script as
  // always-dirty and rerun it on every build, which -- because this script
  // emits a fresh BUILD_TIME each run -- would recompile the crate on every
  // `cargo build` in a normal checkout.
  if std::path::Path::new("git-hash").exists() {
    println!("cargo:rerun-if-changed=git-hash");
  }
}

/// Converts days since the Unix epoch (1970-01-01) into a calendar
/// `(year, month, day)` triple, using Howard Hinnant's `civil_from_days`
/// algorithm.
///
/// Reference: <https://howardhinnant.github.io/date_algorithms.html>
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
  let z = days + 719_468;
  let era = (if z >= 0 {
    z
  } else {
    z - 146_096
  }) / 146_097;
  let doe = (z - era * 146_097) as u32;
  let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
  let y = i64::from(yoe) + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let m = if mp < 10 {
    mp + 3
  } else {
    mp - 9
  };
  let y = if m <= 2 {
    y + 1
  } else {
    y
  };
  (y, m, d)
}
