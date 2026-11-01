# Profiling Workflow (F-019)

This document describes how to profile the Renzu services for CPU, memory,
and async-task performance. Profiling is opt-in and adds no runtime overhead
when disabled.

## CPU profiling

CPU profiling uses [`perf`](https://perf.wiki.kernel.org/index.php/Main_Page)
(Linux `perf_events`), which is a standard kernel profiling tool. No Rust
crate is required in the service binary.

### Prerequisites

- Linux kernel with `perf_events` enabled
- `perf` binary installed (`linux-tools` package on Debian/Ubuntu)
- `perf_event_paranoid` set low enough to allow profiling (typically `1` or
  `-1`):

```bash
# Check current value
cat /proc/sys/kernel/perf_event_paranoid

# Temporarily lower it (requires root)
sudo sysctl -w kernel.perf_event_paranoid=1
```

### Capture a CPU profile

Build the gateway with the `profiling` feature disabled (the default) —
`perf` does not require any code changes:

```bash
cargo build --release -p gateway
```

Start the gateway (or the full environment via `renzu up`), then run a load
test while recording:

```bash
# Start the service
./target/release/gateway &

# Run a repeatable profiling workload
renzu load-test --scenario baseline --duration 60 --rate 100 \
    --url http://127.0.0.1:8080/health

# In another terminal, record CPU samples while the load test runs
sudo perf record -F 99 -a -g -- sleep 60
```

Inspect the results:

```bash
sudo perf report --stdio
```

Or generate a flamegraph:

```bash
sudo perf script | stackcollapse-perf.pl | flamegraph.pl > cpu.svg
```

### Overhead and limitations

- `perf` sampling adds minimal overhead (~1-3% at 99 Hz).
- Kernel stacks require `perf` to have access to `/proc/kallsyms`; without it,
  only user-space stacks are shown.
- `perf_event_paranoid` restrictions may require root.
- Results are machine-specific; do not compare profiles across different
  hardware or kernel versions.

## Memory investigation

Memory profiling uses [`valgrind massif`](https://valgrind.org/docs/manual/ms-manual.html)
(heap profiler) or [`heaptrack`](https://github.com/KDE/heaptrack). Both are
external tools that run the service under instrumentation.

### valgrind massif

```bash
valgrind --tool=massif ./target/release/gateway
ms_print massif.out.<pid>
```

### heaptrack

```bash
heaptrack ./target/release/gateway
heaptrack --annotate heaptrack.<pid>.gz
```

### Overhead and limitations

- Both tools slow the service significantly (5-50x). Do not use under load
  tests that measure latency or throughput.
- Results reflect the instrumented run, not production behavior.
- Use only for identifying allocation hotspots, not for measuring absolute
  memory usage under production load.

## Async-task performance

Async-task inspection uses [`tokio-console`](https://github.com/tokio-rs/console).
The gateway can be built with the `profiling` feature to enable
`console-subscriber`, which emits task telemetry that `tokio-console` reads.

### Build with profiling enabled

```bash
cargo build --release -p gateway --features profiling
```

### Start the service with console-subscriber

Set `RENZU_PROFILING=1` to initialize `console-subscriber` (default listen
address `127.0.0.1:6669`):

```bash
RENZU_PROFILING=1 ./target/release/gateway &
```

### Connect tokio-console

```bash
tokio-console --host 127.0.0.1 --port 6669
```

Inside `tokio-console`, use `t` to list tasks, `w` to view wakers, and `b`
to block the current task for inspection.

### Overhead and limitations

- `console-subscriber` adds per-task tracking overhead. Enable only when
  profiling async behavior; never in production.
- Requires the `tokio-console` binary installed separately
  (`cargo install tokio-console`).
- Task names are derived from the `#[instrument]` span name or the future
  type name; unnamed tasks appear as `<anonymous>`.

## Repeatable profiling workload

Use `renzu load-test` (F-017) to generate a consistent load profile:

```bash
# Baseline workload: 100 req/s for 60 seconds
renzu load-test --scenario baseline --duration 60 --rate 100

# Faulted workload: database latency scenario
renzu load-test --scenario db-latency --duration 60 --rate 100

# Concurrency workload: 20 concurrent clients
renzu load-test --scenario baseline --duration 60 --concurrency 20
```

Each run writes a report to `./reports/<run-id>.json` (F-018) that records
the scenario, duration, rate/concurrency, request counts, throughput, and
latency statistics. Pair the report with the profiling capture so results are
traceable to a specific workload.

## Verifying profiling works

1. Start the environment: `renzu up`
2. Build the gateway: `cargo build --release -p gateway`
3. Start the gateway: `./target/release/gateway &`
4. Run the workload: `renzu load-test --scenario baseline --duration 30 --rate 50`
5. Capture CPU samples in parallel: `sudo perf record -F 99 -g -- sleep 30`
6. Inspect: `sudo perf report --stdio`
7. Save the report: `renzu report <run-id>`

Profiling is considered working when `perf report` shows gateway frames
(e.g., `axum`, `tokio`, `reqwest`) and the experiment report contains the
workload parameters used during capture.
