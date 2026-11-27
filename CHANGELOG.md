# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.14.3] - 2026-11-27

### Fixed
- Mistakes in a request's method or headers stop the run when the plan
  loads, before any request is sent. Before, the first request crashed the
  whole run mid-load, or a wrong value was sent:
  - an unsupported method (`method: OPTIONS`) reports `Unsupported method
    "OPTIONS": use GET, POST, PUT, PATCH, DELETE or HEAD`;
  - a header value with a line break or another control character reports
    `Invalid value for header "Authorization": ...`. The usual cause is a `|`
    block scalar, which keeps its final line break; use `|-`;
  - two header names that differ only in case (`Accept` and `accept`) report
    `Header "accept" is set twice (also as "Accept")`. Only one of the two
    values could be sent, and which one changed from run to run.
- A header value that gets a line break from a `{{ }}` interpolation (a
  `with_items` entry, a CSV row or an `assign`ed response) fails that request
  with `ERR invalid value for header <name>`, counted like a connection
  error, and the run continues. Before, the whole run crashed.
- Cookies are sent back exactly as the server set them. driller added quotes
  around each value (`Cookie: sid="s0"` for `Set-Cookie: sid=s0`), so a
  server that does not strip quotes saw a different session ID. Multiple
  cookies are now separated by `"; "`, as RFC 6265 specifies. The example
  server no longer strips quotes either, so the cookies example plan checks
  this in CI.
- Plan files reach the YAML parser unchanged. driller trimmed each file and
  split it at every `---` followed by a line break before parsing, which
  broke three things:
  - a block scalar at the end of the file lost its final line break
    (`body: |` sent `line one` instead of `line one` plus a newline; `|+`
    lost all its trailing line breaks);
  - a `---` line inside an indented block scalar, such as a Markdown rule in
    a request body, split the plan and failed to parse;
  - a multi-document file with Windows (CRLF) line endings failed to parse.

  The YAML parser now finds the documents itself, at `---` and `...` markers,
  as the YAML standard says. This applies to plans, included files and
  `--compare` report files.

### Added
- Library: `driller::yaml::from_str_multi` parses every document of a YAML
  stream with the same rules as `driller::yaml::from_str`.

### Changed
- Custom headers are sent in the order the plan lists them, instead of in a
  different order on every run.
- A parse error in a multi-document file says `failed to parse YAML content`,
  like one in a single-document file, instead of `failed to parse YAML
  document`.

## [0.14.2] - 2026-11-27

### Fixed
- `driller --version` shows the release commit again for `cargo install
  driller` builds. Crates.io releases 0.13.0 to 0.14.1 printed `unknown`
  instead (`driller 0.14.1 (unknown ...)`), because the hash had to be added to
  the package by hand at publish time and that step was skipped. `build.rs` now
  reads it from the `.cargo_vcs_info.json` file that `cargo publish` writes
  into every package, so no manual step is left. Release binaries and
  Homebrew installs were not affected.
- The 0.14.1 entry below is dated 2026-11-27, the day it was released.

## [0.14.1] - 2026-11-27

### Fixed
- YAML tags no longer break plans. Since 0.14.0, a tagged value such as
  `url: !path /x` stopped the run with ``Unknown node `url` ...``, a tagged
  `with_items` entry panicked with "Unknown Yaml node" when its request ran,
  and tagged `tags:` entries were ignored. Tags are now removed when a plan is
  parsed: `!path /x` reads as `/x`, and `!!binary` / `!!timestamp` values read
  as their text, as they did before 0.14.0. `--list-tasks` shows the untagged
  value.
- Header names are checked when the plan is loaded, so a bad one stops the run
  before any request is sent instead of panicking inside a worker on the first
  request. A missing or null name (`: value`, `~:`) reports "Header name is
  empty or null"; an explicitly empty name (`"": value`) or one with characters
  not allowed in a header name (such as a space) reports "Invalid header name
  ...".
- Duplicate-key parse errors say where the duplicate is again (`duplicate entry
  with key "url" at line 6 column 7`), and every YAML error position now counts
  lines from the top of the file. Plans that start with `---` or contain
  several documents used to report lines too low.
- The README and the getting-started guide no longer say driller links
  OpenSSL. TLS uses rustls; no OpenSSL headers are needed to build from source
  (a C compiler is, for rustls' bundled cryptography library).

### Changed
- **Plan format, since 0.14.0:** a multi-line quoted string or inline list
  must indent its continuation lines deeper than its key. A JSON body written
  as `body: '{` ... `}'` with the closing `}'` lined up under `body:`, or a
  `with_items: [` list whose items line up with the key, fails to parse with
  "continuation must be indented more than the parent block" or "flow content
  must be indented more than the surrounding block". driller 0.13 and earlier
  accepted these. Indent the continuation lines, or write the body as a block
  scalar (`body: |`); `SYNTAX.md` shows an example. This comes from the YAML
  parser and cannot be switched off in driller.
- **Library, since 0.14.0:** `driller::yaml::from_str` rejects a duplicate key
  for every target type, as YAML 1.2 requires, with the error
  `duplicate entry with key "a"` and the duplicate's position. With
  `serde_yaml`, a `HashMap` or other typed map kept the last entry, and a
  repeated struct field failed with ``duplicate field `a` ``. The 0.14.0 notes
  did not mention this. Tags are removed only from `Value` and `Mapping`
  results; tags inside other targets are kept.

### Added
- Homebrew: `brew install zoosky/tap/driller` installs the release binaries on
  macOS (Apple silicon and Intel) and Linux x86_64.

## [0.14.0] - 2026-11-26

### Security
- Benchmark plans are now parsed with [`noyalib`](https://crates.io/crates/noyalib)
  instead of the archived `serde_yaml` 0.9. That drops `unsafe-libyaml` from the
  dependency tree, so `cargo audit` no longer reports RUSTSEC-2024-0449
  (unmaintained), and the `deny.toml` ignore for it is gone. `noyalib` is pure
  Rust with no `unsafe` code. It is pinned exactly (`=0.0.64`) because every
  `0.0.x` release is semver-incompatible. driller keeps the two parser limits
  `serde_yaml` had (nesting depth 128, and the alias-expansion limit that stops
  "billion laughs" files) and does not add `noyalib`'s other size limits, so
  plans that reuse an anchor many times and large `--report` baselines read by
  `--compare` load as before.

### Changed
- **Breaking (library):** the YAML types in driller's public API now come from
  `noyalib`. The new `driller::yaml` module re-exports them (`Value`,
  `Mapping`, `Number`, `Sequence`, `Tag`, `TaggedValue`, `Location`, `Error`,
  `from_str`, `to_string`), and `driller::Error::Yaml` carries a
  `driller::yaml::Error`. Code that passed `serde_yaml::Value`s into driller
  should switch to `driller::yaml::Value`.
- **YAML mapping keys are now always strings, in a standard form.** Plans that
  use ordinary string keys, as plans normally do, are not affected.
  - Number, boolean and null keys are rewritten: `1:` and `+1:` become `1`,
    `0x10:` becomes `16`, `1e3:` becomes `1000.0`, `true:` becomes `true`, and
    `~:`, `null:` or an empty key (`: value`) become `null`. Keys that are
    already strings, such as `01:` or `yes:`, stay as written.
  - A plan with two keys that end up as the same string (`1:` next to `"1":`,
    or `1:` next to `+1:`) no longer loads: parsing fails with a
    key-collision or duplicate-key error. So does a list or mapping used as a
    key (`? [a, b]`), even in a part of the file driller ignores.
  - A header name that YAML reads as a number or boolean is sent in that form
    (`1:` as header `1`) instead of stopping the run with "Header keys must be
    strings". An empty or null header name now stops the run with "Header
    names must not be empty or null".
  - In `with_items` entries, such keys are kept for `{{ item.* }}`
    interpolation (`1: one` becomes the key `"1"`) instead of being dropped, so
    a body template that renders a whole entry, such as `body: '{{ item }}'`,
    now includes them.
- `{{ item.* }}` keeps unsigned integers above `i64::MAX` exact instead of
  turning them into floats (`18446744073709551615`, not
  `1.8446744073709552e19`).
- `--list-tasks` indents nested lists under their key (`  - tag1` instead of
  `- tag1`) and quotes strings with double quotes (`"123"` instead of
  `'123'`). Items are still separated by a blank line.
- Error and panic messages that used to dump a YAML value's internal debug form
  now describe it: a non-sequence document reports `got: a mapping`, and an
  unsupported node shows the value as YAML.
- YAML parse errors keep the `failed to parse YAML ...` prefix, but the text
  after it now comes from `noyalib` and is worded differently.

### Fixed
- The docs now give the license as GPL-3.0-only everywhere (README, FORK.md,
  CONTRIBUTING.md, the website footer, and the fork-baseline changelog entry),
  matching `Cargo.toml` and upstream drill, which declares `GPL-3.0`. They
  previously said GPL-3.0-or-later. The license itself is unchanged.

## [0.13.4] - 2026-11-26

### Security
- Bump `cookie` 0.18.2 -> 0.18.3 to clear RUSTSEC-2026-0594 (private cookie
  nonces can repeat in processes that fork). `cookie` reaches driller only
  through `reqwest`'s cookie store, without the `private` feature, so the
  affected `PrivateJar` functions are not compiled into driller, and driller
  does not fork. Lockfile-only change; no driller source is affected. Released
  so that `--locked` installs and `cargo audit` runs against the published
  crate no longer flag `cookie` 0.18.2.

## [0.13.3] - 2026-11-23

### Security
- Bump `url` 2.5.8 -> 2.5.9 to clear RUSTSEC-2026-0592 (stack overflow in
  `Url::origin()` on nested `blob:` URLs). driller depends on `url`
  directly, and through `reqwest`, `cookie_store`, `hickory-proto`, and
  `tower-http`. The same update takes `form_urlencoded` 1.2.3, which
  `url` 2.5.9 requires.

## [0.13.2] - 2026-11-06

### Security
- Bump `rustls-webpki` 0.103.15 -> 0.103.16 to clear RUSTSEC-2026-0382
  (certificate verification may accept distrusted intermediates) and
  `webpki-root-certs` 1.0.9 -> 1.0.11 to clear RUSTSEC-2026-0383 (bundled
  root certificates allow trust in distrusted emSign intermediates). Both
  sit on driller's certificate-verification path for every `https://`
  target. The same `cargo update` refreshes all remaining lockfile
  dependencies to their latest semver-compatible releases, including
  `tokio` 1.54.1, which clears the RUSTSEC-2026-0386 unsoundness warning
  (`AsyncFd::new` I/O-safety violation). Lockfile-only change; no driller
  source is affected.
- Bump `rustls` 0.23.45 -> 0.23.46 to clear RUSTSEC-2026-0346 (`Debug` impls
  for client session values expose TLS secrets), RUSTSEC-2026-0347 (buffered
  application data sent to the outer server after ECH rejection),
  RUSTSEC-2026-0348 (Brotli certificate decompression can allocate up to 1 GiB
  before authentication) and RUSTSEC-2026-0349 (an unauthenticated ServerHello
  can downgrade later connections to classical key exchange). `rustls` is
  driller's TLS stack for every `https://` target (via `reqwest`); driller's
  build does not enable the `brotli` certificate-compression feature.
  Lockfile-only change; no driller source is affected.
- `cargo deny` now ignores RUSTSEC-2024-0449, which flags `unsafe-libyaml` as
  unmaintained. It reaches the build only through `serde_yaml`, which parses
  benchmark plans; no patched release exists, and the advisory reports no
  vulnerability. The entry in `deny.toml` is to be removed once `serde_yaml` is
  replaced with a maintained YAML crate.

## [0.13.1] - 2026-10-04

### Added
- Project website at <https://zoosky.github.io/driller>, built with
  [Accent CMS](https://accentcms.dev) from the repository's own markdown
  (`docs/`, `SYNTAX.md`, `example/README.md`, `FORK.md`, `CONTRIBUTING.md`,
  `CHANGELOG.md`) plus
  a landing page and a getting-started guide under `site/`. The new `Pages`
  workflow builds it on every push to `main` (and checks links on pull
  requests) and publishes the output to GitHub Pages. The mounted markdown
  files gained a short frontmatter block that sets each page's title,
  summary, and position in the docs navigation. `site/` is excluded from the
  crates.io package.

### Changed
- A failed request now prints a concise, classified line instead of a raw
  `reqwest::Error` `Debug` dump. The line matches the success line format
  (step name, URL, then a red `ERR <cause>` marker where the status would be) --
  for example `ERR request timed out`, `ERR connection refused`,
  `ERR DNS resolution failed`, or `ERR TLS error`. The cause is classified from
  reqwest's own predicates plus a walk of the error's `source()` chain (no new
  dependency). Under `--verbose` the full underlying `source()` chain is appended
  on a dimmed `cause:` line, so the detail from the old dump is still available
  on demand. The suppression gate (`--quiet`), the synthetic `520` recorded for
  `--stats`, and the status-code breakdown are all unchanged.
- Internal: the library error type (`driller::Error`) now derives its
  `Display`/`Error` implementations with `thiserror` instead of hand-written
  `impl` blocks. User-facing messages and the `source()` chain are byte-for-byte
  unchanged, and the public error API is identical (non-breaking). `thiserror`
  was already present transitively (via `reqwest`'s hickory-dns feature), so
  this adds no new crate to the build.
- Refresh all dependencies to their latest semver-compatible releases
  (`cargo update`; no `Cargo.toml` changes). Notable direct bumps: `tokio`
  1.53.2, `reqwest` 0.13.5, `clap` 4.6.7, `regex` 1.13.1, `hdrhistogram` 7.6.0,
  `rand` 0.10.3, `serde_json` 1.0.151, `thiserror` 2.0.21. This also replaces
  the yanked `chacha20` 0.10.0 with 0.10.2, so `cargo audit` reports no
  warnings.

### Security
- Bump `h2` 0.4.14 -> 0.4.19 to clear RUSTSEC-2026-0258, in which the HTTP/2
  implementation accepted and queued empty DATA frames without limit --
  unbounded memory growth on a stream that is not drained, or a panic from an
  integer overflow. Pulled in transitively via `reqwest`, and reachable on any
  run whose target negotiates HTTP/2, so a hostile or malfunctioning target
  server could exhaust the load generator. Lockfile-only change; no driller
  source is affected.
- Bump `rustls` 0.23.40 -> 0.23.45 to clear RUSTSEC-2026-0285, in which TLS 1.3
  handshake messages were incorrectly accepted across encryption level
  boundaries. `rustls` is driller's TLS stack for every `https://` target (via
  `reqwest`), so a hostile target server could exercise the flaw during the
  handshake. The fix also requires `rustls-webpki` 0.103.15 and `aws-lc-rs`
  1.18.1 (`aws-lc-sys` 0.45.0, which adds `pkg-config` as a build dependency).
  Lockfile-only change; no driller source is affected.

## [0.13.0] - 2026-07-03

### Added
- `--stats-format <text|json>` selects the statistics output format (default
  `text`, unchanged). `--stats-format json` emits the same global and per-step
  statistics as a single JSON document to stdout -- and nothing else on stdout:
  the run banner and any warnings are routed to stderr and per-request progress
  is suppressed (as with `--quiet`), so
  `driller run ... --stats-format json | jq .` always sees valid JSON. It
  implies `--stats`. The document carries a top-level integer `schema` version;
  each bucket reports totals, a `status_counts` map (exact code to count,
  including the synthetic `520`), a `class_rollup` (`2xx`/`3xx`/`4xx`/`5xx` plus
  a separate `connection_errors` for `520`), and `latency_ms` percentiles as raw
  milliseconds (`--nanosec` does not affect the JSON). `--report` is unaffected;
  `--stats-format json` cannot be combined with `--compare` (both write to
  stdout), the same restriction `--stats` already has.
- `driller run -` reads the ad-hoc target URL from standard input, so a
  single-endpoint load test composes in a shell pipeline -- for example
  `echo http://localhost:9000/health | driller run - --duration 10s --stats`.
  The URL is the first non-empty line of stdin (trimmed of surrounding
  whitespace and a leading UTF-8 BOM) and runs through the same synthetic-GET
  path as `driller run <URL>`. Because `-` is an ad-hoc source it cannot be
  combined with `--benchmark` (that pairing exits `1` with a clear message);
  empty stdin prints the standard `error: either a URL or --benchmark is
  required`; and unreadable or non-UTF-8 stdin exits `1` with `error: couldn't
  read URL from stdin: ...` rather than a misleading missing-URL message.

### Changed
- The ad-hoc runner now rejects a target URL without a scheme (for example
  `example.com`) up front with `error: URL must include a scheme, e.g.
  http://example.com` and exit `1`, instead of building a malformed base URL
  that fails every request. Applies to a positional `driller run <URL>`, a URL
  piped via `driller run -`, and a `--base-url` override; driller drives
  HTTP(S) only.
- **Library API (source-breaking):** `RunOptions` (re-exported from the crate
  root) gains a public `machine_readable` field, so downstream code that
  constructs it with a struct literal must set the new field. This is why
  0.13.0 is a minor rather than a patch: the library surface is not
  source-compatible with 0.12.x, even though every CLI flag and plan file is.

## [0.12.0] - 2026-07-01

### Added
- A reusable **library target** (`src/lib.rs`). driller was a binary-only
  crate; the load-testing engine is now exposed as a library with a single
  public entry point, `driller::run(&RunOptions) -> Result<BenchmarkResult,
  driller::Error>`, alongside the `actions`, `benchmark`, `checker`, `config`,
  `expandable`, `reader`, and `tags` modules. Integration tests, benchmarks,
  and sibling crates can drive the engine directly instead of shelling out to
  the binary. The CLI (`src/main.rs`) keeps its behavior and now drives the
  engine through this public API.

### Changed
- The library no longer calls `std::process::exit` on bad input. The engine
  (`driller::run` and the `reader`/`config`/`tags`/`checker` helpers) now
  returns a typed `driller::Error` for an unreadable or malformed plan, an
  invalid configuration, an empty plan, an empty/malformed `--compare`
  baseline, and an empty tag listing, leaving the exit decision to the binary.
  Embedding driller as a crate no longer risks having the whole host process
  terminated from deep inside the engine. Fatal-input messages printed by the
  CLI are now uniformly prefixed with `error:` (for example a missing
  `--benchmark` file still prints `error: couldn't open <path>: ...` and exits
  `1`, with no Rust panic backtrace).

### Fixed
- `driller --version` now embeds the correct commit hash for installs from
  crates.io. The hash was derived only from `git rev-parse`, which returns
  nothing when building the published tarball (it carries no `.git`), so
  `cargo install driller` reported `unknown` (and once printed a bare
  `0.11.0 ()`). The publish step now writes the release commit's short hash
  into the packaged tarball and `build.rs` prefers it, so an installed binary
  is traceable back to its source. The hash is still resolved via `git
  rev-parse` first in a normal checkout, so developer builds are unaffected.

### Security
- Bump `quinn-proto` 0.11.14 -> 0.11.15 to address RUSTSEC-2026-0185, a
  high-severity remote memory exhaustion from unbounded out-of-order QUIC
  stream reassembly. Pulled in transitively via `reqwest`'s optional QUIC
  support; driller is not directly affected, but the bump keeps `--locked`
  builds and the security audit clean.
- Bump `anyhow` 1.0.102 -> 1.0.103 to clear RUSTSEC-2026-0190, an unsoundness
  in `Error::downcast_mut()`. Present only transitively through build-time
  `wit-bindgen`/`getrandom` tooling; driller does not use `anyhow` directly.

## [0.11.1] - 2026-06-01

### Changed
- `--report` now runs the full benchmark and writes every request (all
  iterations, in completion order) to the report file, honoring
  `concurrency`/`iterations`/`duration` like any other run. Previously report
  mode executed a single hard-coded iteration and ignored those properties, so
  the report captured only one request per plan step (fcsonline/drill#87).
  `--report` composes with `--stats`, which now reports over the full run.
- `--compare` now averages both the baseline and the current run per request
  `name` and compares each name's mean duration, instead of comparing by
  position in the file. This keeps the verdict stable regardless of iteration
  count or the order concurrent iterations finished in (with `concurrency > 1`
  positions are not reproducible). A request with no matching baseline name is
  skipped, records missing a `name`/`duration` are skipped rather than
  panicking, and an empty or malformed baseline file now exits with a clean
  error instead of silently reporting success.

### Fixed
- A failed `assert` no longer aborts the run with a Rust panic and backtrace
  hint. Instead, driller prints a single `FAIL: <key> -- expected <x>, got <y>`
  line, continues the run so any remaining checks still report, and finishes
  with a non-zero exit code so CI can detect the failure. A passing run still
  exits `0`. Strict-equality semantics are unchanged.
- `--stats --report` together no longer prints `NaN` requests-per-second and an
  all-zero stats block. Report mode now produces real timing data, and the
  requests-per-second divide is guarded against a zero-duration run
  (fcsonline/drill#87).
- `--report` no longer silently writes an empty file when a run completes no
  requests (e.g. a plan with no `request` items, or a `--duration` shorter than
  a single request); it prints a warning and skips the write instead.
- `{{ index }}` now resolves in a plain request (one with no
  `with_items`/`with_items_range`/`with_items_from_csv`/`with_items_from_file`).
  Previously it only existed inside those expansions, so a plain plan that
  referenced `{{ index }}` panicked in the default strict mode ("Unknown
  'index' variable") or interpolated to an empty string under
  `--relaxed-interpolations` (fcsonline/drill#186). In a plain request `index`
  is the iteration counter; inside an items expansion it remains the item's
  position in the list.

### Added
- Documented the built-in interpolation variables (`base`, `index`,
  `iteration`, `item`) in `SYNTAX.md`.

## [0.11.0] - 2026-05-31

### Changed
- Request latency now measures time-to-last-byte. driller reads the full
  response body before stopping its timer, matching `wrk`, `k6`, `vegeta` and
  other load-testing tools. Previously the timer stopped as soon as the response
  headers arrived, and the body was only read when a request used `assign`, so
  endpoints serving non-trivial bodies (files, large JSON) were reported as far
  faster than they really were (fcsonline/drill#74). Reported latencies for
  body-heavy endpoints will increase to reflect true end-to-end time.
  - Because the body read is now part of the timed request, a response whose
    body does not finish transferring within `--timeout` is reported as a
    connection error (synthetic status `520` / the `conn` total) rather than its
    HTTP status. Previously such a request reported its status (e.g. `200`)
    because the body was never read. Loosen `--timeout` if body transfer for a
    slow or large-bodied endpoint legitimately needs more time.
  - `assign` bodies are decoded using the response's `Content-Type` charset
    (defaulting to UTF-8), preserving the previous charset-aware behaviour now
    that driller drains the body itself instead of calling reqwest's `text()`.
  - The body is streamed and discarded chunk by chunk; it is only buffered in
    memory when a request uses `assign`. Peak memory therefore stays bounded per
    in-flight request rather than scaling with the full response size, so testing
    large-body endpoints at high concurrency does not balloon memory.
- In `--duration` mode, iterations that complete before the deadline are now
  counted even when the deadline falls mid-batch; previously the entire
  in-flight batch was discarded when the duration elapsed. Only requests still
  in flight at the deadline are dropped.
- In `--verbose` mode, connection and body-read failures now also print the
  `<<<` response marker (with no body), so failed requests are visible in the
  request/response log instead of only the inline error line.
- `cargo-deny` now bans `native-tls`, `openssl`, and `openssl-sys`, so CI fails
  if OpenSSL is ever pulled back into the dependency tree. TLS stays on rustls;
  this guards against the prebuilt-musl OpenSSL segfault class
  (fcsonline/drill#168, #190).

## [0.10.3] - 2026-05-30

### Added
- `--stats` output now includes a per-status-code breakdown: each HTTP status
  mapped to its request count, followed by a `2xx/3xx/4xx/5xx` class rollup. The
  synthetic status `520` is labelled as a connection error and reported as a
  separate `conn` total (not folded into `5xx`), so dropped connections are
  distinguishable from server `5xx` responses (e.g. `example/benchmark.yml` now
  shows its 202 "failures" as 200 expected 404s + 2 flaky 500s). With
  `--verbose` each plan step also prints a compact per-step breakdown.

### Changed
- Example server is now a small Rust (axum) binary at `example/server`, serving
  the `responses/` fixtures and a few dynamic endpoints; running the examples
  needs only `cargo`. The previous Node/Express example server, its `npm`
  dependency tree, and its Docker files were removed.
- CI: a new `examples` job builds the example server and runs every standalone
  `example/*.yml` plan against it, gating on a clean exit and no connection
  errors -- turning the example suite into a regression test.

### Fixed
- `--version` no longer prints an empty commit hash in release binaries (e.g. `driller 0.10.2 ()`). `build.rs` now requires a successful, non-empty `git rev-parse` and otherwise falls back to `$GITHUB_SHA` (then `unknown`), so CI-built binaries always embed a real commit identifier. A `Cross.toml` passes `GITHUB_SHA` into the musl container build for the same reason.
- Release workflow: build the `x86_64-apple-darwin` target on `macos-latest` (Apple-silicon, cross-compiling) instead of the frequently-unavailable `macos-13` runner, which had left the Intel macOS asset missing from the 0.10.2 release.
- `example/headers.yml`: corrected the base URL port (`3000` -> `9000`) so the custom-headers example reaches the example server instead of failing with connection-refused.
- `example/benchmark.yml`: fixed the CSV `quote_char` (`"\'"` -> `"'"`, which had decoded to a backslash) so the CSV-driven POST step issues requests instead of silently parsing nothing; corrected the matching `quote_char` example in `SYNTAX.md`.

### Documentation
- `README.md`: document the `--worker-threads` / `-w` flag and link `docs/cli-reference.md` for the full flag list and the runtime workload-tuning guide.
- `example/README.md`: document running the examples against the Rust server with `driller run --benchmark … --stats`.

## [0.10.2] - 2026-05-29

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
See [FORK.md](https://github.com/zoosky/driller/blob/main/FORK.md) for rationale and migration instructions.

### Changed
- Renamed crate and binary from `drill` to `driller`
- Updated package metadata (repository, description, authors)
- Trimmed publish payload (exclude `.github/`, example server)

### Added
- `FORK.md` explaining the fork's purpose and relationship to upstream
- Local CI script (`local-ci.sh`)

### Unchanged
- License remains GPL-3.0-only
- Benchmark YAML format and CLI flags are fully compatible with drill 0.9.0
- Full upstream git history preserved

[Unreleased]: https://github.com/zoosky/driller/compare/0.14.3...HEAD
[0.14.3]: https://github.com/zoosky/driller/compare/0.14.2...0.14.3
[0.14.2]: https://github.com/zoosky/driller/compare/0.14.1...0.14.2
[0.14.1]: https://github.com/zoosky/driller/compare/0.14.0...0.14.1
[0.14.0]: https://github.com/zoosky/driller/compare/0.13.4...0.14.0
[0.13.4]: https://github.com/zoosky/driller/compare/0.13.3...0.13.4
[0.13.3]: https://github.com/zoosky/driller/compare/0.13.2...0.13.3
[0.13.2]: https://github.com/zoosky/driller/compare/0.13.1...0.13.2
[0.13.1]: https://github.com/zoosky/driller/compare/0.13.0...0.13.1
[0.13.0]: https://github.com/zoosky/driller/compare/0.12.0...0.13.0
[0.12.0]: https://github.com/zoosky/driller/compare/0.11.1...0.12.0
[0.11.1]: https://github.com/zoosky/driller/compare/0.11.0...0.11.1
[0.11.0]: https://github.com/zoosky/driller/compare/0.10.3...0.11.0
[0.10.3]: https://github.com/zoosky/driller/compare/0.10.2...0.10.3
[0.10.2]: https://github.com/zoosky/driller/compare/0.10.1...0.10.2
[0.10.1]: https://github.com/zoosky/driller/compare/0.10.0...0.10.1
[0.10.0]: https://github.com/zoosky/driller/compare/0.10.0-alpha.2...0.10.0
[0.10.0-alpha.2]: https://github.com/zoosky/driller/compare/0.10.0-alpha.1...0.10.0-alpha.2
[0.10.0-alpha.1]: https://github.com/zoosky/driller/releases/tag/0.10.0-alpha.1
