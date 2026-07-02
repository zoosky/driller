# CLI reference

## `driller run` -- execute a benchmark or ad-hoc request

```
Usage: driller run [OPTIONS] [URL]
```

### Arguments

| Argument | Description |
|---|---|
| `[URL]` | Target URL for ad-hoc testing (creates a synthetic GET request). Must include a scheme (`http://` or `https://`). Pass `-` to read the URL from standard input. |

`driller run -` reads the target URL from the first non-empty line of stdin
(trimmed of surrounding whitespace and a leading UTF-8 BOM), so a single-endpoint
test drops into a shell pipeline. Because `-` is an ad-hoc source it cannot be
combined with `--benchmark`. Empty stdin prints the standard `error: either a URL
or --benchmark is required`; unreadable or non-UTF-8 stdin exits with `error:
couldn't read URL from stdin: ...`.

### Run-specific options

| Flag | Short | Description |
|---|---|---|
| `--base-url <URL>` | `-u` | Override the base URL from the benchmark file |
| `--concurrency <N>` | `-p` | Number of concurrent requests (default: 1) |
| `--iterations <N>` | `-i` | Number of iterations (default: 1) |
| `--duration <DURATION>` | `-d` | Run for a fixed wall-clock duration (e.g. `30s`, `5m`, `1h`) |
| `--rampup <N>` | `-e` | Ramp-up time in seconds (default: 0) |

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

# Pipe the target URL in from stdin (ad-hoc)
echo http://localhost:3000/health | driller run - --duration 10s --stats

# Run a benchmark file for 60 seconds with overridden concurrency
driller run --benchmark bench.yml --duration 60s --concurrency 20 --stats

# Override the base URL to point at staging
driller run --benchmark bench.yml --base-url http://staging:3000 --stats
```

## Global options

These flags work with both `driller run` and the legacy `driller --benchmark`
form:

| Flag | Short | Description |
|---|---|---|
| `--benchmark <FILE>` | `-b` | Sets the benchmark file |
| `--stats` | `-s` | Shows request statistics |
| `--report <FILE>` | `-r` | Sets a report output file |
| `--compare <FILE>` | `-c` | Sets a comparison baseline file |
| `--threshold <MS>` | `-t` | Threshold in ms for comparison |
| `--relaxed-interpolations` | | Do not panic on missing interpolations |
| `--no-check-certificate` | | Disables SSL certificate verification |
| `--tags <TAGS>` | | Comma-separated tags to include |
| `--skip-tags <TAGS>` | | Comma-separated tags to exclude |
| `--list-tags` | | List all benchmark tags |
| `--list-tasks` | | List benchmark tasks (applies tag filters) |
| `--quiet` | `-q` | Disables output |
| `--timeout <SECONDS>` | `-o` | Request timeout in seconds (default: 10) |
| `--nanosec` | `-n` | Shows statistics in nanoseconds |
| `--verbose` | `-v` | Toggle verbose output |

### Statistics output (`--stats`)

`--stats` prints totals, latency percentiles, and a **status-code breakdown**:
each distinct HTTP status mapped to its request count, followed by a class
rollup that sums each family (`2xx`/`3xx`/`4xx`/`5xx`). The synthetic status
`520` denotes a connection error (driller could not reach the target); it is
labelled as such and reported as a separate `conn` total rather than folded
into `5xx`. With `--verbose`, each plan step also prints its own compact
breakdown. For example:

```
Successful requests       2598
Failed requests           202
Status codes
  200                     2598
  404                     200
  500                     2
  2xx 2598 · 4xx 200 · 5xx 2
```

### How latency is measured

Each request's latency is measured as **time-to-last-byte**: driller starts the
timer before sending the request and stops it only after the entire response
body has been read. This matches the behaviour of `wrk`, `k6`, `vegeta` and
other load-testing tools, and means endpoints that serve large bodies (files,
large JSON) are timed for the full transfer rather than just time-to-headers.

## Configuration precedence

When using `driller run` with a benchmark file, values are resolved in three
layers (last wins):

1. **Defaults** -- concurrency=1, iterations=1, rampup=0
2. **Benchmark YAML** -- values from the file override defaults
3. **CLI flags** -- `--concurrency`, `--iterations`, etc. override the file

## Proxies

driller honors the standard proxy environment variables (`HTTP_PROXY`,
`HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY`, plus their lowercase forms) with no
flag or configuration. See [proxy.md](./proxy.md) for details and an enterprise
`NO_PROXY` example.

## Tokio runtime selection

`--worker-threads N` (alias `-w N`) selects which tokio runtime drives the
benchmark:

| `--worker-threads` | Runtime built                                 |
|--------------------|------------------------------------------------|
| omitted or `1`     | current-thread runtime (single OS thread)      |
| `N >= 2`           | multi-thread runtime with `N` worker threads   |
| `0` or invalid     | rejected at CLI parse time                     |

The default (current-thread) has the lowest per-request overhead and the most
predictable performance across workload sizes. The multi-thread runtime can
win significantly on some workloads but loses significantly on others. There
is no single `N` that dominates across payload sizes; tune to your workload
and measure.

Rough guidance from internal measurements on a 28-core macOS host hitting a
local target server with `-p 256` over a 10 s window:

| Response body size | Best `N`    | Multi-thread Δ vs current-thread (best `N`) |
|--------------------|-------------|----------------------------------------------|
| Small (~3 B - 8 KB)   | 2        | +55 % to +65 %                               |
| Medium (~64 KB)       | 1 (or 2) | -2 % to -9 % (multi-thread cannot beat ct)   |
| Large (~512 KB - 2 MB) | 16 - 28 | +17 % to +39 %                              |
| Very large (~10 MB)   | (insensitive) | -4 % to -6 %                            |
| Latency-bound (slow server) | 2  | +5 % to +11 %                                |

These numbers are illustrative, not portable -- the exact crossover points
depend on host CPU topology, network stack, and target behavior. Measure on
your own setup if `--worker-threads` matters to you.

### Examples (continued)

```bash
# Default current-thread runtime (matches behavior before 0.10.2)
driller run http://localhost:3000/api -p 64 -i 1000 --stats

# Try multi-thread with 4 workers for a large-body workload
driller run http://localhost:3000/large-payload -p 256 -d 30s -w 4 --stats
```

## Legacy invocation

The original `driller --benchmark <FILE>` form continues to work. It is
equivalent to `driller run --benchmark <FILE>`.
