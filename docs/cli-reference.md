# CLI reference

## `driller run` -- execute a benchmark or ad-hoc request

```
Usage: driller run [OPTIONS] [URL]
```

### Arguments

| Argument | Description |
|---|---|
| `[URL]` | Target URL for ad-hoc testing (creates a synthetic GET request) |

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

## Configuration precedence

When using `driller run` with a benchmark file, values are resolved in three
layers (last wins):

1. **Defaults** -- concurrency=1, iterations=1, rampup=0
2. **Benchmark YAML** -- values from the file override defaults
3. **CLI flags** -- `--concurrency`, `--iterations`, etc. override the file

## Legacy invocation

The original `driller --benchmark <FILE>` form continues to work. It is
equivalent to `driller run --benchmark <FILE>`.
