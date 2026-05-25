# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/zoosky/driller/compare/0.10.0-alpha.2...HEAD
[0.10.0-alpha.2]: https://github.com/zoosky/driller/compare/0.10.0-alpha.1...0.10.0-alpha.2
[0.10.0-alpha.1]: https://github.com/zoosky/driller/releases/tag/0.10.0-alpha.1
