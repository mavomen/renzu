#!/usr/bin/env bash
set -euo pipefail

# E2E verification that traces flow into Tempo and are inspectable via the
# Tempo API.
#
# Prerequisites: docker compose is up and healthy (tempo).
#   docker compose up -d
#
# Usage:
#   ./scripts/e2e-trace-visualization.sh
#
# The script sends a synthetic OTLP trace directly to Tempo's OTLP HTTP
# receiver (127.0.0.1:4319), then polls the Tempo API until the trace is
# retrievable by trace ID. It asserts the trace has a span with a name,
# kind, and duration greater than zero. Compose is left running.

TEMPO_HOST="127.0.0.1"
TEMPO_PORT=3200
TEMPO_OTLP_PORT=4319
TEMPO_API="http://${TEMPO_HOST}:${TEMPO_PORT}"
TRACE_ID="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
SPAN_ID="aaaaaaaaaaaaaaaa"
START_NANO="1000000000"
END_NANO="2000000000"
TEMPO_TIMEOUT=60   # seconds to poll Tempo for traces
TEMPO_INTERVAL=3   # seconds between Tempo polls
COMPOSE_WAIT=5     # seconds to wait for compose ports before failing fast

fail() {
    echo "ERROR: $*" >&2
    exit 1
}

# Wait until a TCP port accepts connections.
wait_for_port() {
    local host="$1" port="$2" label="$3" timeout="$4"
    local deadline=$(( $(date +%s) + timeout ))
    while [[ $(date +%s) -lt $deadline ]]; do
        if (echo > "/dev/tcp/${host}/${port}") >/dev/null 2>&1; then
            echo "==> ${label} accepting connections on ${host}:${port}"
            return 0
        fi
        sleep 1
    done
    fail "${label} did not accept connections on ${host}:${port} within ${timeout}s"
}

echo "==> checking compose dependencies are reachable"
wait_for_port "${TEMPO_HOST}" "${TEMPO_PORT}" "Tempo HTTP API" "${COMPOSE_WAIT}"
wait_for_port "${TEMPO_HOST}" "${TEMPO_OTLP_PORT}" "Tempo OTLP HTTP receiver" "${COMPOSE_WAIT}"

echo "==> sending synthetic OTLP trace to Tempo (trace_id=${TRACE_ID})"
curl -sf --max-time 10 \
    -H "Content-Type: application/json" \
    -d "{\"resourceSpans\":[{\"scopeSpans\":[{\"spans\":[{\"traceId\":\"${TRACE_ID}\",\"spanId\":\"${SPAN_ID}\",\"name\":\"http.request\",\"kind\":1,\"startTimeUnixNano\":\"${START_NANO}\",\"endTimeUnixNano\":\"${END_NANO}\",\"status\":{\"code\":1}}]}]}]}" \
    "http://${TEMPO_HOST}:${TEMPO_OTLP_PORT}/v1/traces" >/dev/null || fail "failed to send trace to Tempo"

echo "==> polling Tempo for trace ${TRACE_ID} (timeout ${TEMPO_TIMEOUT}s)"
deadline=$(( $(date +%s) + TEMPO_TIMEOUT ))
trace_found=false

while [[ $(date +%s) -lt $deadline ]]; do
    trace_json=$(curl -s --max-time 10 "${TEMPO_API}/api/traces/${TRACE_ID}" 2>/dev/null) || trace_json=""

    if [[ -n "${trace_json}" ]] && echo "${trace_json}" | grep -q '"batches"'; then
        trace_found=true
        break
    fi
    echo "   trace not yet retrievable, retrying in ${TEMPO_INTERVAL}s..."
    sleep "${TEMPO_INTERVAL}"
done

if [[ "${trace_found}" != "true" ]]; then
    fail "trace ${TRACE_ID} not retrievable from Tempo within ${TEMPO_TIMEOUT}s"
fi

echo "==> trace found, asserting span data: name, kind, duration > 0"

# Re-fetch the trace and validate span data
trace_json=$(curl -s --max-time 10 "${TEMPO_API}/api/traces/${TRACE_ID}")

python3 - "${trace_json}" "${TRACE_ID}" <<'PY'
import json
import sys

raw = sys.argv[1]
expected_trace_id = sys.argv[2]

try:
    data = json.loads(raw)
except json.JSONDecodeError as exc:
    print(f"FAIL: Tempo response is not valid JSON: {exc}", file=sys.stderr)
    print(raw[:2000], file=sys.stderr)
    sys.exit(1)

batches = data.get("batches") or []
if not batches:
    print("FAIL: no batches found in Tempo response", file=sys.stderr)
    sys.exit(1)

ok = False
for batch in batches:
    scope_spans = batch.get("scopeSpans") or []
    for scope_span in scope_spans:
        spans = scope_span.get("spans") or []
        for span in spans:
            span_name = span.get("name") or ""
            span_kind = span.get("kind") or ""
            start_nanos = span.get("startTimeUnixNano") or 0
            end_nanos = span.get("endTimeUnixNano") or 0
            if isinstance(start_nanos, str):
                try:
                    start_nanos = int(start_nanos)
                except:
                    start_nanos = 0
            if isinstance(end_nanos, str):
                try:
                    end_nanos = int(end_nanos)
                except:
                    end_nanos = 0
            duration_nanos = end_nanos - start_nanos
            if span_name and span_kind and duration_nanos > 0:
                ok = True
                break
        if ok:
            break
    if ok:
        break

if not ok:
    print("FAIL: no span with name, kind, and duration > 0 found", file=sys.stderr)
    sys.exit(1)

print(f"PASS: trace {expected_trace_id} found with span name, kind, and duration > 0")
PY

echo "==> e2e trace visualization verification complete"