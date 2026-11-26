#!/usr/bin/env bash
set -euo pipefail

# End-to-end verification of PRD Journey B: investigate slow requests.
# Prerequisites: the full Compose stack is up and healthy.
#
# Usage:
#   ./scripts/e2e-incident-journey.sh
#
# The script runs a baseline load test and a faulted (db-latency) load test
# against the gateway's /ready endpoint, which is where the 200 ms fault is
# injected. It asserts that:
#   - both runs produce schema-v1 reports with the expected scenario names;
#   - the faulted run has a materially higher p95 latency;
#   - `renzu compare` shows a positive p95 delta;
#   - the injected fault is cleaned up after both runs;
#   - a trace generated during the faulted workload is queryable in Tempo.
#
# A fresh temporary reports directory is used so the proof is isolated from
# earlier runs. Compose is left running.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY_URL="${RENZU_GATEWAY_URL:-http://127.0.0.1:8080}"
TEMPO_URL="${RENZU_TEMPO_URL:-http://127.0.0.1:3200}"
REPORTS_DIR="$(mktemp -d)"
DURATION=5
RATE=10

cleanup() {
    rm -rf "$REPORTS_DIR"
}
trap cleanup EXIT

fail() {
    echo "ERROR: $*" >&2
    exit 1
}

run_id_from_output() {
    printf '%s\n' "$1" | sed -n 's/^run id: //p' | tail -1
}

wait_for_http() {
    local url="$1" label="$2" timeout="$3"
    local deadline=$(( $(date +%s) + timeout ))
    while [[ $(date +%s) -lt $deadline ]]; do
        if curl -sf --max-time 3 "$url" >/dev/null 2>&1; then
            echo "==> ${label} reachable"
            return 0
        fi
        sleep 1
    done
    fail "${label} did not become reachable within ${timeout}s: ${url}"
}

measure_ready_ms() {
    curl -sS --max-time 10 -o /dev/null -w '%{time_total}' \
        "${GATEWAY_URL}/ready" | python3 -c \
        'import sys; print(float(sys.stdin.read().strip()) * 1000.0)'
}

echo "==> checking gateway and Tempo"
wait_for_http "${GATEWAY_URL}/health" "Gateway" 15
wait_for_http "${TEMPO_URL}/ready" "Tempo" 15

echo "==> verifying clean baseline before fault injection"
baseline_pre_ms=$(measure_ready_ms)
python3 - "$baseline_pre_ms" <<'PY'
import sys
ms = float(sys.argv[1])
if ms >= 150:
    print(f"FAIL: /ready already slow before fault injection ({ms:.1f} ms)", file=sys.stderr)
    raise SystemExit(1)
print(f"PASS: /ready baseline latency {ms:.1f} ms")
PY

echo "==> running baseline load test against /ready"
cd "$REPO_ROOT"
export RENZU_REPORTS_DIR="$REPORTS_DIR"
baseline_output=$(cargo run -q -p renzu -- load-test \
    --scenario baseline \
    --url "${GATEWAY_URL}/ready" \
    --duration "$DURATION" --rate "$RATE" 2>&1) || {
    printf '%s\n' "$baseline_output" >&2
    fail "baseline load test failed"
}
baseline_id=$(run_id_from_output "$baseline_output")
[[ -n "$baseline_id" ]] || fail "baseline load test did not print a run id"

echo "==> running db-latency load test against /ready"
faulted_start=$(date +%s)
faulted_output=$(cargo run -q -p renzu -- load-test \
    --scenario db-latency \
    --url "${GATEWAY_URL}/ready" \
    --duration "$DURATION" --rate "$RATE" 2>&1) || {
    printf '%s\n' "$faulted_output" >&2
    fail "db-latency load test failed"
}
faulted_id=$(run_id_from_output "$faulted_output")
[[ -n "$faulted_id" ]] || fail "db-latency load test did not print a run id"
faulted_end=$(date +%s)

echo "==> asserting reports and latency delta"
python3 - "$REPORTS_DIR" "$baseline_id" "$faulted_id" <<'PY'
import json
import sys

reports_dir, baseline_id, faulted_id = sys.argv[1:4]

def load(run_id, expected_scenario):
    path = f"{reports_dir}/{run_id}.json"
    try:
        with open(path, encoding="utf-8") as handle:
            report = json.load(handle)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"FAIL: cannot read report {path}: {exc}", file=sys.stderr)
        raise SystemExit(1)
    if report.get("run_id") != run_id:
        print(f"FAIL: report run_id mismatch in {path}", file=sys.stderr)
        raise SystemExit(1)
    if report.get("schema_version") != 1:
        print(f"FAIL: report schema_version != 1 in {path}", file=sys.stderr)
        raise SystemExit(1)
    if report.get("scenario") != expected_scenario:
        print(
            f"FAIL: scenario {report.get('scenario')!r} != {expected_scenario!r}",
            file=sys.stderr,
        )
        raise SystemExit(1)
    return report

baseline = load(baseline_id, "baseline")
faulted = load(faulted_id, "db-latency")

base_p95 = baseline["latency"]["p95_ms"]
fault_p95 = faulted["latency"]["p95_ms"]
delta = fault_p95 - base_p95
print(f"baseline p95: {base_p95:.1f} ms")
print(f"faulted p95:  {fault_p95:.1f} ms")
print(f"p95 delta:    {delta:.1f} ms")
if delta < 150:
    print(
        f"FAIL: expected >=150 ms p95 increase from the 200 ms fault, got {delta:.1f} ms",
        file=sys.stderr,
    )
    raise SystemExit(1)
print("PASS: injected latency is visible in the faulted report")

if not faulted.get("telemetry_references"):
    print("FAIL: faulted report has no telemetry references", file=sys.stderr)
    raise SystemExit(1)
PY

echo "==> asserting compare output"
compare_output=$(cargo run -q -p renzu -- compare "$baseline_id" "$faulted_id" 2>&1) || {
    printf '%s\n' "$compare_output" >&2
    fail "compare command failed"
}
python3 - "$compare_output" <<'PY'
import sys
text = sys.argv[1]
lines = [line for line in text.splitlines() if line.strip().startswith("Latency p95 (ms)")]
if len(lines) != 1:
    print("FAIL: compare output does not contain exactly one p95 row", file=sys.stderr)
    raise SystemExit(1)
columns = lines[0].split()
# Columns: label words, Run A, Run B, Abs delta, [% delta ...]
if len(columns) < 6:
    print(f"FAIL: malformed p95 row: {lines[0]!r}", file=sys.stderr)
    raise SystemExit(1)
try:
    absolute = float(columns[5])
except ValueError:
    print(f"FAIL: p95 absolute delta is not numeric: {columns[5]!r}", file=sys.stderr)
    raise SystemExit(1)
if absolute <= 0:
    print(f"FAIL: p95 absolute delta not positive: {absolute}", file=sys.stderr)
    raise SystemExit(1)
print(f"PASS: compare reports a positive p95 delta ({absolute:.1f} ms)")
PY

echo "==> verifying fault cleanup"
post_ms=$(measure_ready_ms)
python3 - "$post_ms" <<'PY'
import sys
ms = float(sys.argv[1])
if ms >= 150:
    print(f"FAIL: /ready still slow after fault cleanup ({ms:.1f} ms)", file=sys.stderr)
    raise SystemExit(1)
print(f"PASS: /ready latency restored after cleanup ({ms:.1f} ms)")
PY

echo "==> verifying a faulted-workload trace is queryable in Tempo"
# Report deep links use client-generated traceparent IDs and are best-effort
# because the services do not install a global W3C propagator. Search Tempo by
# the faulted run's time window instead, then fetch the trace by its real ID.
trace_id=""
deadline=$(( $(date +%s) + 60 ))
while [[ $(date +%s) -lt $deadline && -z "$trace_id" ]]; do
    search_json=$(curl -sS --max-time 10 -G "${TEMPO_URL}/api/search" \
        --data-urlencode "start=${faulted_start}" \
        --data-urlencode "end=$(( faulted_end + 10 ))" \
        --data-urlencode 'limit=50' 2>/dev/null) || search_json=""
    trace_id=$(python3 - "$search_json" "$(( faulted_start * 1000000000 ))" <<'PY'
import json
import sys

try:
    data = json.loads(sys.argv[1])
    started_after = int(sys.argv[2])
except (json.JSONDecodeError, TypeError, ValueError):
    raise SystemExit(0)
for trace in data.get("traces") or []:
    if trace.get("rootTraceName") != "http.request":
        continue
    try:
        start_nanos = int(trace.get("startTimeUnixNano") or 0)
    except ValueError:
        continue
    if start_nanos >= started_after and trace.get("traceID"):
        print(trace["traceID"])
        break
PY
)
    if [[ -z "$trace_id" ]]; then
        echo "   faulted-workload trace not in Tempo yet, retrying in 3s..."
        sleep 3
    fi
done
[[ -n "$trace_id" ]] || fail "no faulted-workload trace found in Tempo within 60s"

trace_json=$(curl -sS --max-time 10 "${TEMPO_URL}/api/traces/${trace_id}") || \
    fail "Tempo did not return trace ${trace_id}"
python3 - "$trace_json" "$trace_id" <<'PY'
import json
import sys

try:
    data = json.loads(sys.argv[1])
except json.JSONDecodeError as exc:
    print(f"FAIL: Tempo response is not valid JSON: {exc}", file=sys.stderr)
    raise SystemExit(1)
for batch in data.get("batches") or []:
    for scope_span in batch.get("scopeSpans") or []:
        for span in scope_span.get("spans") or []:
            start = span.get("startTimeUnixNano") or 0
            end = span.get("endTimeUnixNano") or 0
            if isinstance(start, str):
                try:
                    start = int(start)
                except ValueError:
                    start = 0
            if isinstance(end, str):
                try:
                    end = int(end)
                except ValueError:
                    end = 0
            if span.get("name") and span.get("kind") and end > start:
                print(f"PASS: trace {sys.argv[2]} queryable in Tempo with a timed span")
                raise SystemExit(0)
print(f"FAIL: trace {sys.argv[2]} has no span with name, kind, and duration > 0", file=sys.stderr)
raise SystemExit(1)
PY

echo "==> incident investigation E2E verification complete"
