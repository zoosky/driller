# Driller

[![Crates.io](https://img.shields.io/crates/v/driller.svg)](https://crates.io/crates/driller)
[![License: GPL-3.0](https://img.shields.io/crates/l/driller.svg)](./LICENSE)
[![CI](https://github.com/zoosky/driller/actions/workflows/general.yml/badge.svg)](https://github.com/zoosky/driller/actions/workflows/general.yml)

A fast, lightweight HTTP load testing tool written in Rust with an
Ansible-inspired YAML syntax. Friendly fork of
[fcsonline/drill](https://github.com/fcsonline/drill) -- see
[FORK.md](./FORK.md) for background.

## Quick start

```bash
cargo install driller
driller --benchmark benchmark.yml --stats
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
driller --benchmark benchmark.yml --stats
```

For the full benchmark file syntax, see [SYNTAX.md](./SYNTAX.md).

## Features

- **Concurrency and ramp-up** -- control parallel iterations and ramp-up time.
- **Dynamic interpolation** -- URLs, headers, and bodies can reference
  variables, CSV data, environment variables, and previous responses.
- **Request chaining** -- assign response data to variables and use them in
  later requests.
- **Assertions** -- validate response status codes and body values inline.
- **Multiple data sources** -- loop over inline lists, ranges,
  CSV files, or included YAML files.
- **Statistics** -- mean, median, standard deviation, and p99/p99.5/p99.9
  latency percentiles.
- **Benchmark comparison** -- compare runs against a saved report with
  configurable thresholds.
- **Tags** -- run or skip specific plan items by tag.
- **All HTTP methods** -- GET, POST, PUT, PATCH, DELETE, HEAD.
- **Cookie propagation** -- session cookies carry across requests automatically.
- **Shell execution** -- run external commands and capture output into variables.

## CLI reference

```
Usage: driller [OPTIONS] --benchmark <BENCHMARK>

Options:
  -b, --benchmark <BENCHMARK>   Sets the benchmark file
  -s, --stats                   Shows request statistics
  -r, --report <REPORT>         Sets a report file
  -c, --compare <COMPARE>       Sets a compare file
  -t, --threshold <THRESHOLD>   Sets a threshold value in ms amongst the compared file
      --relaxed-interpolations  Do not panic if an interpolation is not present. (Not recommended)
      --no-check-certificate    Disables SSL certification check. (Not recommended)
      --tags <TAGS>             Tags to include
      --skip-tags <SKIP_TAGS>   Tags to exclude
      --list-tags               List all benchmark tags
      --list-tasks              List benchmark tasks (executes --tags/--skip-tags filter)
  -q, --quiet                   Disables output
  -o, --timeout <TIMEOUT>       Set timeout in seconds for all requests
  -n, --nanosec                 Shows statistics in nanoseconds
  -v, --verbose                 Toggle verbose output
  -h, --help                    Print help
  -V, --version                 Print version
```

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

The `example/` directory contains a small Node.js server and sample benchmark
files. See the [example README](./example) for setup instructions.

**Disclaimer:** do not run intensive benchmarks against production environments.

## Contributing

Pull requests are welcome. See [FORK.md](./FORK.md) for the relationship to
upstream and the goals of this fork.

## License

GPL-3.0-or-later. See [LICENSE](./LICENSE) for the full text.
