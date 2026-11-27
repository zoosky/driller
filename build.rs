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
  //   1. `.cargo_vcs_info.json` -- `cargo package` / `cargo publish` writes
  //      this file into every published crate with the full commit hash, so a
  //      `cargo install` from crates.io (which builds from the tarball, with
  //      no `.git`) still knows its commit. The file exists only in packaged
  //      sources, never in a checkout, so it is authoritative when present.
  //      Checking it before git also keeps a crate unpacked inside some other
  //      repository (a vendored dependency, or a home directory under version
  //      control) from embedding that repository's HEAD.
  //   2. `git rev-parse --short HEAD` -- any build from a checkout. It can
  //      fail to spawn (no git on PATH), exit non-zero (not a working tree, or
  //      "dubious ownership" in a CI checkout), or return nothing; in every
  //      such case the output is empty and must not be embedded verbatim
  //      (that is how a release binary once printed `0.10.2 ()`).
  //   3. GitHub Actions' `GITHUB_SHA` (short) -- containers without `git`,
  //      e.g. the `cross` musl build (forwarded via Cross.toml).
  //   4. "unknown", only when every source above is unavailable.
  let git_hash = packaged_commit_hash().or_else(checkout_commit_hash).or_else(ci_commit_hash).unwrap_or_else(|| "unknown".to_string());

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
  // `.cargo_vcs_info.json` is not watched: it exists only in a packaged
  // crate, whose sources never change. Listing a path that does not exist
  // would make Cargo rerun this script on every build in a checkout, and the
  // fresh BUILD_TIME would then recompile the crate each time.
}

/// Reads the release commit from the `.cargo_vcs_info.json` that `cargo
/// package` writes into a published crate, returned as a 7-character short
/// hash (`b1f7df3`), with `-dirty` appended when the crate was published from
/// a working tree with uncommitted changes.
///
/// The file looks like `{"git": {"sha1": "<40 hex digits>", "dirty": true},
/// "path_in_vcs": ""}` (`dirty` appears only when true). It is scanned by hand
/// so the build script stays dependency-free.
///
/// Returns `None` when the file is absent (any checkout) or holds no
/// recognisable hash.
fn packaged_commit_hash() -> Option<String> {
  let text = std::fs::read_to_string(".cargo_vcs_info.json").ok()?;
  let after_key = &text[text.find("\"sha1\"")? + "\"sha1\"".len()..];
  let value = &after_key[after_key.find('"')? + 1..];
  let hash: String = value.chars().take_while(char::is_ascii_hexdigit).collect();
  if hash.len() < 7 {
    return None;
  }
  let dirty = text.replace(char::is_whitespace, "").contains("\"dirty\":true");
  let suffix = if dirty {
    "-dirty"
  } else {
    ""
  };
  Some(format!("{}{suffix}", &hash[..7]))
}

/// Runs `git rev-parse --short HEAD` for a build from a checkout.
///
/// Returns `None` when git cannot be spawned, exits non-zero (not a working
/// tree, or "dubious ownership" in a CI checkout), or prints nothing, so an
/// empty hash is never embedded.
fn checkout_commit_hash() -> Option<String> {
  let output = Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok()?;
  if !output.status.success() {
    return None;
  }
  let hash = String::from_utf8(output.stdout).ok()?.trim().to_string();
  (!hash.is_empty()).then_some(hash)
}

/// Shortens GitHub Actions' `GITHUB_SHA` to 7 characters, for container builds
/// without git (the `cross` musl build, which receives it via Cross.toml).
///
/// Returns `None` when the variable is unset or empty.
fn ci_commit_hash() -> Option<String> {
  let hash: String = std::env::var("GITHUB_SHA").ok()?.chars().take(7).collect();
  (!hash.is_empty()).then_some(hash)
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
