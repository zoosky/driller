# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.3] - 2026-05-29

### Added
- `driller run --worker-threads N` (short `-w N`): selects the tokio runtime. `N = 1` (default) uses the current-thread runtime; `N >= 2` uses the multi-thread runtime with `N` worker threads. `N = 0` is rejected at CLI parse time. See `docs/cli-reference.md` for the workload-vs-N guidance table.

### Changed
- Default tokio runtime is now explicitly `current_thread`. This matches the behavior every previous release shipped with -- the prior derivation `min(num_cpus, concurrency)` was paired with `Builder::new_current_thread()`, which silently ignored the computed worker count. Behavior is therefore identical for users who do not pass `--worker-threads`; only the manifest is now honest.

### Removed
- `num_cpus` dependency. No longer needed now that the worker count is taken directly from the CLI flag.

### Changed
- `actions::request`: shrink the connection-pool `Mutex` window to cover only the `HashMap` lookup and a cheap `reqwest::Client` clone (the inner state is `Arc`-shared). The per-request `RequestBuilder` is now constructed after the lock is released. Originally pursued as a candidate fix for a multi-thread-runtime throughput regression at moderate response sizes; a clean-machine sweep of the patched binary did not show the regression closing, so this lands as a cleanup rather than a perf fix.
- `Cargo.toml`: declare tokio's `rt` and `rt-multi-thread` features explicitly. The runtime builder requires both, and they were previously available only via reqwest's transitive feature enablement.

### Added
- `Cargo.toml`: a `profiling` cargo profile that inherits from `release` and keeps debug symbols (`debug = true, strip = false`). Use with `cargo build --profile profiling` or `cargo install --path . --profile profiling --force` for samply / instruments stack walking. Default `release` build is unchanged.

## [0.10.1] - 2026-05-28

### Fixed
- `--threshold` rejects non-numeric values at CLI parse time instead of after running the benchmark, with an error that hints at the bundled-short-flags gotcha (e.g. `-stats` is parsed as `-s -t ats`, not as `--stats`).
- File-not-found and YAML/CSV parse errors in `reader.rs` are now reported as clean `error: ...` lines on stderr with exit code 1, instead of Rust panics with backtrace hints. Affects `--benchmark`, `--compare`, and any benchmark step that reads an `iterate` / `csv` source file.

### Changed (release pipeline)
- Release workflow rewritten to use `taiki-e/upload-rust-binary-action`, replacing the previous Docker-only action that failed on macOS and Windows runners. The 0.10.0 release shipped without binary assets as a result; 0.10.1 restores cross-platform artifacts for `x86_64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`.

## [0.10.0] - 2026-05-28

### Added
- `driller run <URL>` subcommand for ad-hoc HTTP testing without a benchmark file
- CLI override flags: `--concurrency`, `--iterations`, `--duration`, `--rampup`, `--base-url`
- Duration-based runs (`--duration 30s`) that loop the plan for a fixed wall-clock period
- Three-layer config precedence: hard-coded defaults < YAML file < CLI flags
- `docs/cli-reference.md` with full CLI documentation

### Changed
- `benchmark::execute()` accepts a `RunOptions` struct instead of positional parameters
- `Tags` struct owns its data (removed lifetime parameter)
- Synthetic plan built programmatically via `Request::simple_get` instead of YAML construction
- Duration loop bounded by `tokio::time::timeout` to prevent overshooting the deadline
- Terminal output colors changed from purple to cyan
- Concurrency > iterations validation produces a clear error message instead of a panic
- `checker::compare()` accepts `threshold` as `f64` (parsed at CLI boundary)
- Positional URL split into base and path components for correct request targeting
- README quick-start section tightened; example updated to use `run` subcommand
- Upgrade `reqwest` 0.12 to 0.13, bump MSRV to 1.95
- Upgrade `colored` 2 to 3, `rand` 0.8 to 0.10
- Add `cargo-deny` configuration for license and advisory auditing

### Fixed
- Histogram panic on response durations above 3.6 seconds (upper bound raised to 1 hour)
- Duration mode no longer overshoots by a full batch latency

### Added (infrastructure)
- SECURITY.md, CONTRIBUTING.md, issue templates, CODEOWNERS
- Cross-platform release workflow (Linux, macOS, Windows)
- Security audit and cargo-deny CI checks

## [0.10.0-alpha.2] - 2026-05-25

### Changed
- Upgrade `clap` 2 to 4 (colored help output, better error messages, typed derive API)
- Upgrade to Rust edition 2024, set MSRV to 1.85
- Update release workflow toolchain from 1.83.0 to 1.85.0
- Bump all dependencies via `cargo update`

### Fixed
- Clear all RUSTSEC vulnerabilities: `bytes` (integer overflow), `rustls-webpki` (4 CVEs), `time` (stack exhaustion DoS), `rand` (unsound with custom logger)
- Remove unmaintained `ansi_term` and `atty` transitive dependencies (were pulled in by clap 2)
- Fix clippy `unnecessary_unwrap` lint in request handling

## [0.10.0-alpha.1] - 2026-05-22

Friendly fork of [fcsonline/drill](https://github.com/fcsonline/drill) 0.9.0.
See [FORK.md](./FORK.md) for rationale and migration instructions.

### Changed
- Renamed crate and binary from `drill` to `driller`
- Updated package metadata (repository, description, authors)
- Trimmed publish payload (exclude `.github/`, example server)

### Added
- `FORK.md` explaining the fork's purpose and relationship to upstream
- Local CI script (`local-ci.sh`)

### Unchanged
- License remains GPL-3.0-or-later
- Benchmark YAML format and CLI flags are fully compatible with drill 0.9.0
- Full upstream git history preserved

[Unreleased]: https://github.com/zoosky/driller/compare/0.10.0...HEAD
[0.10.0]: https://github.com/zoosky/driller/compare/0.10.0-alpha.2...0.10.0
[0.10.0-alpha.2]: https://github.com/zoosky/driller/compare/0.10.0-alpha.1...0.10.0-alpha.2
[0.10.0-alpha.1]: https://github.com/zoosky/driller/releases/tag/0.10.0-alpha.1
