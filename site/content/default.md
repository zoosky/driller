---
title: Driller
description: "A clean HTTP load-test drill: Ansible-style YAML plans, a Rust runtime, RPS and latency percentiles per run."
template: landing
menu:
  visible: false
---

# HTTP load testing, driven by YAML plans

Driller is a clean HTTP load-test drill. Describe a scenario as an
Ansible-style YAML plan, run it from a single Rust binary, and read
requests per second and latency percentiles per run. No fancy bits.

```bash
cargo install driller
driller run http://localhost:9000/api/users --stats
```

{% button variant="primary" href="/docs/getting-started" %}Get started{% /button %}
{% button href="https://github.com/zoosky/driller" %}Source on GitHub{% /button %}

---section: features---

## What you get

- **Plans as YAML.** Requests, assertions, includes, tags, and loops over
  inline lists, ranges, or CSV rows, in a file you can review and version.
- **Ad-hoc from the shell.** `driller run <URL>` needs no plan file, and
  `driller run -` reads the target from standard input.
- **Request chaining.** Assign a response to a variable and interpolate it
  into the next URL, header, or body.
- **Assertions that fail CI.** An `assert` mismatch prints a `FAIL` line and
  the run exits non-zero.
- **Stats you can script.** `--stats-format json` emits one JSON document
  with per-status counts and p99, p99.5, and p99.9 latencies.
- **Iterations or duration.** Fix the iteration count, or loop the plan for
  `30s`, `5m`, or `1h`.
- **Concurrency and ramp-up.** Parallel iterations with a ramp-up window,
  and a selectable multi-thread runtime for large-body workloads.
- **Compare runs.** `--report` records every request of a run; `--compare`
  checks a later run against it with a configurable threshold.

---section: example---

## A plan in thirty seconds

```yaml
---
concurrency: 4
base: 'http://localhost:9000'
iterations: 5
rampup: 2

plan:
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
```

```bash
driller run --benchmark benchmark.yml --stats
```

Benchmark files written for the original `drill` run unchanged; see
[Migrating from drill](docs/migrating-from-drill).

---section: cta---

## Read the documentation

Install driller, write a first plan, and look up every flag.

<a class="sc-button sc-button--primary" href="docs">Documentation</a>
