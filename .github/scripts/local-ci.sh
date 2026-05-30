#!/usr/bin/env bash
# Parallel local mirror of driller's GitHub CI (.github/workflows/general.yml
# and audit.yml). Runs the cheap independent checks concurrently so a full
# pre-push verification finishes in roughly the time of the longest single job
# (typically `cargo test`).
#
# Coverage parity:
#   - cargo fmt --all -- --check          (Rustfmt workflow job)
#   - cargo clippy -- -D warnings          (Clippy workflow job)
#   - cargo test                           (Test workflow job)
#   - cargo audit                          (Security audit workflow job, if installed)
#   - example plans vs the example server  (Examples workflow job)
#   - cargo tarpaulin --out Xml --ignore-tests  (Code coverage workflow job; opt-in only)
#
# The release workflow is not mirrored — it only runs on tag pushes and
# produces a musl binary; building it locally has no signal value before push.
#
# Override knobs:
#   JOBS              cargo --jobs N for the parallel builds (default: half the cores)
#   SKIP_AUDIT=1      skip `cargo audit` even if installed
#   SKIP_EXAMPLES=1   skip building + running the example server / example plans
#   RUN_COVERAGE=1    additionally run `cargo tarpaulin` (slow; opt-in)
#   SKIP_HEARTBEAT=1  silence the 30s "still running" progress line
#
# Each cargo build uses its own CARGO_TARGET_DIR so clippy and test do not
# serialise on cargo's target lock.
set -euo pipefail

# --- env / sanity ---

if [ ! -f Cargo.toml ]; then
  echo "error: run from the driller crate root (Cargo.toml not found in \$PWD)" >&2
  exit 1
fi

CORES=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
JOBS="${JOBS:-$(( CORES > 1 ? CORES / 2 : 1 ))}"
LOG_DIR="$(mktemp -d -t driller-local-ci.XXXXXX)"
START_TS=$(date +%s)
HEARTBEAT_PID=""

cleanup() {
  if [ -n "$HEARTBEAT_PID" ] && kill -0 "$HEARTBEAT_PID" 2>/dev/null; then
    kill "$HEARTBEAT_PID" 2>/dev/null || true
  fi
  rm -rf "$LOG_DIR"
}
trap cleanup EXIT INT TERM

# --- parallel job tracking (Bash 3.2 compatible — no `wait -n`) ---

PIDS=()
NAMES=()

start() {
  local name=$1
  shift
  local log="$LOG_DIR/${name}.log"
  ( "$@" ) >"$log" 2>&1 &
  local pid=$!
  PIDS+=("$pid")
  NAMES+=("$name")
  echo "  [start ] $name (pid $pid)"
}

heartbeat() {
  while true; do
    sleep 30
    local now=$(date +%s)
    local elapsed=$((now - START_TS))
    local running=""
    for i in "${!PIDS[@]}"; do
      if kill -0 "${PIDS[$i]}" 2>/dev/null; then
        running="$running ${NAMES[$i]}"
      fi
    done
    if [ -n "$running" ]; then
      echo "  [${elapsed}s] still running:$running"
    fi
  done
}

await_all() {
  local fail=0
  local first_failed=""
  for i in "${!PIDS[@]}"; do
    local pid=${PIDS[$i]}
    local name=${NAMES[$i]}
    if wait "$pid"; then
      local now=$(date +%s)
      echo "  [ ok   ] $name (+$((now - START_TS))s)"
    else
      local status=$?
      local now=$(date +%s)
      echo "  [ FAIL ] $name (exit $status, +$((now - START_TS))s)"
      [ -z "$first_failed" ] && first_failed="$name"
      fail=1
    fi
  done
  if [ $fail -ne 0 ]; then
    for i in "${!PIDS[@]}"; do
      local name=${NAMES[$i]}
      echo
      echo "----- $name log -----"
      cat "$LOG_DIR/${name}.log"
      echo "----- end $name -----"
    done
    echo
    echo "FAILED: $first_failed (and possibly more above)"
    exit 1
  fi
  PIDS=()
  NAMES=()
}

# --- workers ---

run_fmt() {
  cargo fmt --all -- --check
}

run_clippy() {
  CARGO_TARGET_DIR=target-clippy cargo clippy --all-targets --jobs "$JOBS" -- -D warnings
}

run_test() {
  CARGO_TARGET_DIR=target-test cargo test --jobs "$JOBS"
}

run_audit() {
  if [ "${SKIP_AUDIT:-0}" = "1" ]; then
    echo "SKIP_AUDIT=1 — skipping cargo audit"
    return 0
  fi
  if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "cargo-audit not installed (install with 'cargo install cargo-audit') — skipping"
    return 0
  fi
  cargo audit
}

run_coverage() {
  if ! command -v cargo-tarpaulin >/dev/null 2>&1; then
    echo "cargo-tarpaulin not installed (install with 'cargo install cargo-tarpaulin') — skipping"
    return 0
  fi
  CARGO_TARGET_DIR=target-tarpaulin cargo tarpaulin --out Xml --ignore-tests --jobs "$JOBS"
}

# Mirror of the `examples` workflow job: build driller + the example server,
# start the server, and run every standalone example plan against it. Gates on
# a clean exit AND no connection errors (the latter catches a wrong-port /
# server-down regression that a bare exit code misses). Runs in its own
# CARGO_TARGET_DIR so it does not serialise on the clippy/test target locks.
run_examples() {
  if [ "${SKIP_EXAMPLES:-0}" = "1" ]; then
    echo "SKIP_EXAMPLES=1 — skipping example plans"
    return 0
  fi
  export ITERATIONS=2 EDITOR=users

  CARGO_TARGET_DIR=target-examples cargo build --release --jobs "$JOBS"
  cargo build --release --jobs "$JOBS" --manifest-path example/server/Cargo.toml

  local driller="target-examples/release/driller"
  local server="example/server/target/release/driller-example-server"

  # Not `local`: the EXIT trap fires after this function returns, so the pid
  # must still be in scope then. (Each worker is its own subshell, so this does
  # not leak into the parent.) The `${server_pid:-}` guard keeps the trap safe
  # under `set -u`.
  server_pid=""
  trap '[ -n "${server_pid:-}" ] && kill "$server_pid" 2>/dev/null; true' EXIT
  "$server" --responses-dir example/server/responses >/dev/null 2>&1 &
  server_pid=$!

  local up=0 _
  for _ in $(seq 1 50); do
    if curl -sf http://127.0.0.1:9000/api/users.json >/dev/null 2>&1; then up=1; break; fi
    sleep 0.2
  done
  if [ "$up" -ne 1 ]; then
    echo "example server did not become ready on :9000"
    return 1
  fi

  # Standalone plans only (top-level `plan:` key); comments/subcomments/subtags
  # are include fragments exercised via their parents.
  local rc=0 plan prc out
  for plan in $(cd example && grep -lE '^plan:' *.yml); do
    if out=$("$driller" run --benchmark "example/$plan" --stats 2>&1); then prc=0; else prc=$?; fi
    if [ "$prc" -ne 0 ]; then echo "$plan exited $prc"; rc=1; fi
    if printf '%s' "$out" | grep -q 'Error connecting'; then echo "$plan produced connection errors"; rc=1; fi
  done
  return "$rc"
}

# --- Phase 1: parallel CI mirror ---

echo "=== Phase 1: parallel CI mirror ==="
echo "    JOBS=$JOBS (cores=$CORES)"

start "fmt"      run_fmt
start "clippy"   run_clippy
start "test"     run_test
start "audit"    run_audit
start "examples" run_examples

if [ "${SKIP_HEARTBEAT:-0}" != "1" ]; then
  heartbeat &
  HEARTBEAT_PID=$!
fi

await_all

if [ -n "$HEARTBEAT_PID" ]; then
  kill "$HEARTBEAT_PID" 2>/dev/null || true
  HEARTBEAT_PID=""
fi

# --- Phase 2: optional code coverage (slow; opt-in via RUN_COVERAGE=1) ---

if [ "${RUN_COVERAGE:-0}" = "1" ]; then
  echo "=== Phase 2: cargo tarpaulin (coverage) ==="
  run_coverage
else
  echo "=== Phase 2: coverage skipped (set RUN_COVERAGE=1 to run cargo tarpaulin) ==="
fi

echo
echo "All local CI checks passed in $(( $(date +%s) - START_TS ))s."
