# How to play with driller

These examples run driller against a small local fixture server that serves the
`server/responses/` files and a few dynamic endpoints (sessions, a flaky POST,
etc.). The server is a Rust [axum](https://github.com/tokio-rs/axum) binary --
no Node toolchain required. The same example plans run against it in CI.

Compile driller:

```
cargo build --release
```

Start the example server in another terminal (listens on port 9000):

```
cd example/server
cargo run --release
```

To exercise the delayed-response behavior, add a delay (milliseconds):

```
cargo run --release -- --delay-ms 100        # or: DELAY_MS=100 cargo run --release
```

Then, from the `example` directory, run any plan:

```
cd example

# Example 1 -- delayed responses (run with the server started with --delay-ms)
../target/release/driller run --benchmark benchmark.yml --stats

# Example 2 -- cookies / session counter
../target/release/driller run --benchmark cookies.yml --stats

# Example 3 -- custom headers
../target/release/driller run --benchmark headers.yml --stats
```

Other ready-to-run plans in this directory: `delay.yml`, `iterations.yml`,
`tags.yml`, `throughput.yml`, and `env.yml` (set `ITERATIONS` and `EDITOR`, e.g.
`ITERATIONS=3 EDITOR=users ../target/release/driller run --benchmark env.yml --stats`).
The `comments.yml`, `subcomments.yml`, and `subtags.yml` files are include
fragments used by the other plans, not standalone plans.

> The legacy `driller --benchmark <file>` form still works; `driller run --benchmark <file>` is the current canonical invocation.

## Legacy Node server (fallback)

The original Node/Express server is still present (`server/server.js`,
`server/package.json`) and serves the same routes, if you prefer it or need to
cross-check behavior:

```
cd example/server
npm install
DELAY_MS=100 node server.js
```

It will be removed once the Rust server has proven itself in CI.
