# Driller

[![Crates.io](https://img.shields.io/crates/v/driller.svg)](https://crates.io/crates/driller)
[![License: GPL-3.0](https://img.shields.io/crates/l/driller.svg)](./LICENSE)
[![CI](https://github.com/zoosky/driller/actions/workflows/general.yml/badge.svg)](https://github.com/zoosky/driller/actions/workflows/general.yml)

A clean HTTP load-test drill. Ansible-style YAML plans, Rust runtime,
RPS and percentiles per run -- no fancy bits.

## Quick start

```bash
cargo install driller

driller run http://localhost:9000/api/users --stats
driller run --benchmark benchmark.yml --stats
```

Or grab a binary from the
[latest release](https://github.com/zoosky/driller/releases/latest).

## Example

Create a file called `benchmark.yml`:

```yaml
---
concurrency: 4
base: 'http://localhost:9000'
iterations: 5
rampup: 2

plan:
  - name: Fetch users
    request:
      url: /api/users.json

  - name: Fetch account
    request:
      url: /api/account
    assign: account

  - name: Fetch manager
    request:
      url: /api/users/{{ account.body.manager_id }}

  - name: Assert status
    assert:
      key: account.status
      value: 200

  - name: Create order
    request:
      url: /api/orders
      method: POST
      body: '{"user": {{ account.body.id }}}'
      headers:
        Content-Type: application/json
```

Run it:

```bash
driller run --benchmark benchmark.yml --stats
```

For the full benchmark file syntax, see [SYNTAX.md](./SYNTAX.md).

## Writing plans

A plan is a YAML file with a `plan:` list of steps. The fastest way to learn the
format is to read the runnable plans in [`example/`](./example), which build up
features roughly in order of complexity:

- `benchmark.yml` -- requests, `assign`, `assert`, `{{ }}` interpolation, and
  CSV-driven requests
- `cookies.yml` -- session/cookie propagation and chaining one request's
  response into the next
- `headers.yml` -- custom and templated request headers
- `iterations.yml` / `throughput.yml` -- iteration counts and concurrency
- `tags.yml` -- including sub-plans and selecting steps with `--tags`
- `delay.yml`, `env.yml` -- pacing and environment-variable interpolation

Each plan runs against the example server in [`example/server`](./example/server)
(`cd example/server && cargo run --release`). The two references are
[SYNTAX.md](./SYNTAX.md) for the plan format (every action, the
`with_items`/CSV/`include` forms, and interpolation rules) and
[docs/cli-reference.md](./docs/cli-reference.md) for CLI flags and the
`--worker-threads` tuning guide.

## Features

- **Ad-hoc URL testing** -- `driller run <URL>` sends requests without a
  benchmark file.
- **CLI overrides** -- `--concurrency`, `--iterations`, `--duration`,
  `--rampup`, and `--base-url` override values from the benchmark file.
- **Duration-based runs** -- `--duration 30s` loops the plan for a fixed
  wall-clock period instead of a fixed iteration count.
- **Concurrency and ramp-up** -- control parallel iterations and ramp-up time.
- **Dynamic interpolation** -- URLs, headers, and bodies can reference
  variables, CSV data, environment variables, and previous responses.
- **Request chaining** -- assign response data to variables and use them in
  later requests.
- **Response checks** -- `assert` plan items compare response fields against
  expected values; a mismatch prints a `FAIL` line and the run finishes with a
  non-zero exit code, so CI can detect it.
- **Multiple data sources** -- loop over inline lists, ranges,
  CSV files, or included YAML files.
- **Statistics** -- mean, median, standard deviation, and p99/p99.5/p99.9
  latency percentiles.
- **Benchmark comparison** -- `--report` runs the full benchmark and records
  every request (all iterations) to a file; `--compare` checks a later run
  against that saved report, comparing mean duration per request name with a
  configurable threshold. `--report` composes with `--stats`.
- **Tags** -- run or skip specific plan items by tag.
- **Plan introspection** -- `--list-tags` and `--list-tasks` (with tag
  filters) dump the structure of a benchmark file without running it.
- **Common HTTP methods** -- GET, POST, PUT, PATCH, DELETE, HEAD.
- **Cookie propagation** -- session cookies carry across requests automatically.
- **Proxy support** -- routes through HTTP/HTTPS proxies via the standard
  `HTTP_PROXY`/`HTTPS_PROXY` environment variables and honors `NO_PROXY`
  (see [docs/proxy.md](./docs/proxy.md)).
- **Request timeout** -- per-request timeout via `--timeout`; default 10s.
- **Shell execution** -- run external commands and capture output into variables.

## CLI reference

### `driller run` -- execute a benchmark or ad-hoc request

```
Usage: driller run [OPTIONS] [URL]

Arguments:
  [URL]  Target URL for ad-hoc testing (creates a synthetic GET request)

Run-specific options:
  -u, --base-url <URL>           Override the base URL from the benchmark file
  -p, --concurrency <N>          Number of concurrent requests [default: 1]
  -i, --iterations <N>           Number of iterations [default: 1]
  -d, --duration <DURATION>      Run for a fixed wall-clock duration (e.g. "30s", "5m", "1h")
  -e, --rampup <N>               Ramp-up time in seconds [default: 0]
  -w, --worker-threads <N>       Tokio runtime workers: 1 = current-thread (default), N >= 2 = multi-thread [default: 1]
```

For the complete flag list and the `--worker-threads` workload-tuning guide,
see [docs/cli-reference.md](./docs/cli-reference.md).

`--duration` and `--iterations` are mutually exclusive. When neither is given,
the default is 1 iteration.

When both a URL and `--benchmark` are provided, the benchmark file supplies the
plan and the URL sets the base URL.

### Examples

```bash
# Quick smoke test
driller run http://localhost:3000/health

# 10 concurrent users, 100 total iterations
driller run http://localhost:3000/api -p 10 -i 100 --stats

# Run a benchmark file for 60 seconds with overridden concurrency
driller run --benchmark bench.yml --duration 60s --concurrency 20 --stats

# Override the base URL to point at staging
driller run --benchmark bench.yml --base-url http://staging:3000 --stats
```

### Global options

These flags work with both `driller run` and the legacy `driller --benchmark`
form:

```
  -b, --benchmark <FILE>        Sets the benchmark file
  -s, --stats                   Shows request statistics
  -r, --report <FILE>           Sets a report output file
  -c, --compare <FILE>          Sets a comparison baseline file
  -t, --threshold <MS>          Threshold in ms for comparison
      --relaxed-interpolations  Do not panic on missing interpolations
      --no-check-certificate    Disables SSL certificate verification
      --tags <TAGS>             Comma-separated tags to include
      --skip-tags <TAGS>        Comma-separated tags to exclude
      --list-tags               List all benchmark tags
      --list-tasks              List benchmark tasks (applies tag filters)
  -q, --quiet                   Disables output
  -o, --timeout <SECONDS>       Request timeout in seconds [default: 10]
  -n, --nanosec                 Shows statistics in nanoseconds
  -v, --verbose                 Toggle verbose output
  -h, --help                    Print help
  -V, --version                 Print version
```

### Configuration precedence

When using `driller run` with a benchmark file, values are resolved in three
layers (last wins):

1. **Defaults** -- concurrency=1, iterations=1, rampup=0
2. **Benchmark YAML** -- values from the file override defaults
3. **CLI flags** -- `--concurrency`, `--iterations`, etc. override the file

## Building from source

```bash
git clone https://github.com/zoosky/driller.git && cd driller
cargo build --release
./target/release/driller --benchmark benchmark.yml --stats
```

### OpenSSL dependency

Driller links against OpenSSL for TLS. Install the development headers for
your platform:

| Platform | Command |
|---|---|
| Debian / Ubuntu | `apt install libssl-dev pkg-config` |
| Fedora / RHEL | `dnf install openssl-devel` |
| macOS (Homebrew) | `brew install openssl` |
| Windows (vcpkg) | `vcpkg install openssl:x64-windows-static-md` |

## Testing locally

The `example/` directory contains a small Rust (axum) fixture server and sample
benchmark plans. See the [example README](./example) for setup instructions. The
server lives in the git repository; it is not part of the published crate, so
install via a clone (`git clone https://github.com/zoosky/driller.git`) rather
than `cargo install` if you want to run the examples.

**Disclaimer:** do not run intensive benchmarks against production environments.

## Similar tools

Driller is one of many HTTP load testers. If you're shopping around, these are
the usual suspects:

| Tool                                                                       | Language          | Niche                                                                  |
|----------------------------------------------------------------------------|-------------------|------------------------------------------------------------------------|
| [ab](https://httpd.apache.org/docs/current/programs/ab.html)               | C                 | ApacheBench. Ships with Apache HTTPD; ubiquitous; no scripting.        |
| [gatling](https://gatling.io/)                                             | Scala (JVM)       | Simulation-style DSL; rich HTML reports.                               |
| [hey](https://github.com/rakyll/hey)                                       | Go                | Small, single-binary CLI; a spiritual successor to `ab`.               |
| [jmeter](https://jmeter.apache.org/)                                       | Java              | GUI-driven; deep plugin ecosystem; protocols beyond HTTP.              |
| [k6](https://github.com/grafana/k6)                                        | Go + JavaScript   | JS scripting for complex flows; first-class CI / cloud story.          |
| [oha](https://github.com/hatoo/oha)                                        | Rust              | Real-time terminal UI with histogram and chart.                        |
| [vegeta](https://github.com/tsenart/vegeta)                                | Go                | Constant attack-rate model; pipeline-friendly text/JSON output.        |
| [wrk](https://github.com/wg/wrk)                                           | C                 | High-throughput single-host benchmarker; optional Lua scripting.       |

## Contributing

Pull requests are welcome. See [FORK.md](./FORK.md) for the relationship to
upstream and the goals of this fork.

## License

GPL-3.0-or-later. See [LICENSE](./LICENSE) for the full text.
