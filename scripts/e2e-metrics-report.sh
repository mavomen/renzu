#!/usr/bin/env bash
set -euo pipefail

# End-to-end verification of the metrics and report half of the observability
# proof. Prerequisites: the full Compose stack is up and healthy.
#
# Usage:
#   ./scripts/e2e-metrics-report.sh
#
# The script generates gateway traffic, asserts that the request counter is
# queryable in Prometheus, runs one short baseline load test, and asserts that
# the generated JSON report exists and parses. Compose is left running.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATEWAY_URL="${RENZU_GATEWAY_URL:-http://127.0.0.1:8080}"
PROMETHEUS_URL="${RENZU_PROMETHEUS_URL:-http://127.0.0.1:9090}"
REPORTS_DIR="${RENZU_REPORTS_DIR:-${REPO_ROOT}/reports}"
POLL_TIMEOUT=60
POLL_INTERVAL=3
PROBE_ID="e2e-metrics-$(date +%s)-$$-${RANDOM}"

fail() {
    echo "ERROR: $*" >&2
    exit 1
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

echo "==> checking gateway and Prometheus"
wait_for_http "${GATEWAY_URL}/health" "Gateway" 15
wait_for_http "${PROMETHEUS_URL}/-/healthy" "Prometheus" 15

echo "==> generating request traffic with probe ${PROBE_ID}"
for i in 1 2 3; do
    curl -fsS --max-time 10 "${GATEWAY_URL}/health" >/dev/null \
        || fail "gateway health request ${i} failed"
    curl -fsS --max-time 10 "${GATEWAY_URL}/order/${PROBE_ID}-${i}" >/dev/null \
        || fail "gateway order request ${i} failed"
done

echo "==> polling Prometheus for renzu_http_requests_total (timeout ${POLL_TIMEOUT}s)"
deadline=$(( $(date +%s) + POLL_TIMEOUT ))
metric_found=false
while [[ $(date +%s) -lt $deadline ]]; do
    prom_json=$(curl -sS --max-time 10 -G "${PROMETHEUS_URL}/api/v1/query" \
        --data-urlencode 'query={__name__=~"renzu_http_requests_total.*"}' 2>/dev/null) || prom_json=""

    if printf '%s' "$prom_json" | python3 -c '
import json
import sys

try:
    data = json.load(sys.stdin)
except (json.JSONDecodeError, TypeError):
    raise SystemExit(1)

if data.get("status") != "success":
    raise SystemExit(1)

for item in data.get("data", {}).get("result", []):
    name = item.get("metric", {}).get("__name__", "")
    value = item.get("value", [None, "0"])[1]
    try:
        count = float(value)
    except (TypeError, ValueError):
        continue
    if name.startswith("renzu_http_requests_total") and count > 0:
        raise SystemExit(0)

raise SystemExit(1)
' 2>/dev/null; then
        metric_found=true
        break
    fi

    echo "   metric not in Prometheus yet, retrying in ${POLL_INTERVAL}s..."
    sleep "${POLL_INTERVAL}"
done

if [[ "$metric_found" != "true" ]]; then
    fail "renzu_http_requests_total not queryable in Prometheus within ${POLL_TIMEOUT}s"
fi

echo "==> running a short baseline load test"
cd "$REPO_ROOT"
export RENZU_REPORTS_DIR="$REPORTS_DIR"
mkdir -p "$REPORTS_DIR"
load_output=$(cargo run -q -p renzu -- load-test --scenario baseline --duration 2 --rate 5 2>&1) || {
    printf '%s\n' "$load_output" >&2
    fail "load test failed"
}
run_id=$(printf '%s\n' "$load_output" | sed -n 's/^run id: //p' | tail -1)
[[ -n "$run_id" ]] || fail "load test did not print a run id"
report_path="${REPORTS_DIR}/${run_id}.json"

echo "==> asserting report ${report_path}"
[[ -f "$report_path" ]] || fail "report file missing: ${report_path}"
python3 - "$report_path" "$run_id" <<'PY'
import json
import sys

path, expected_run_id = sys.argv[1:]

try:
    with open(path, encoding="utf-8") as handle:
        report = json.load(handle)
except (json.JSONDecodeError, OSError) as exc:
    print(f"FAIL: report is not valid JSON: {exc}", file=sys.stderr)
    raise SystemExit(1)

if report.get("run_id") != expected_run_id:
    print(
        f"FAIL: report run_id {report.get('run_id')!r} != {expected_run_id!r}",
        file=sys.stderr,
    )
    raise SystemExit(1)
if report.get("schema_version") != 1:
    print(f"FAIL: unexpected schema_version {report.get('schema_version')!r}", file=sys.stderr)
    raise SystemExit(1)
if not isinstance(report.get("telemetry_references"), list):
    print("FAIL: telemetry_references is missing or not a list", file=sys.stderr)
    raise SystemExit(1)

print(f"PASS: report {expected_run_id} written with schema v1")
PY

echo "PASS: request metric is queryable in Prometheus"
echo "==> metrics/report E2E verification complete"
