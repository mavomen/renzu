#!/usr/bin/env bash
set -euo pipefail

# End-to-end verification of F-013 log/trace correlation.
#
# Prerequisites: the full Compose stack is up and healthy, including gateway,
# Loki, and Tempo:
#   docker compose up -d
#
# The script sends one traced gateway request with a unique order ID, finds the
# matching application log in Loki, reads its native trace ID, and asserts that
# the same trace ID is retrievable from Tempo with a span for that order ID.
# Compose is left running.

GATEWAY_URL="${RENZU_GATEWAY_URL:-http://127.0.0.1:8080}"
LOKI_URL="${RENZU_LOKI_URL:-http://127.0.0.1:3100}"
TEMPO_URL="${RENZU_TEMPO_URL:-http://127.0.0.1:3200}"
PROBE_ID="e2e-$(date +%s)-$$-${RANDOM}"
POLL_TIMEOUT=60   # seconds
POLL_INTERVAL=2   # seconds

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

echo "==> checking gateway, Loki, and Tempo"
wait_for_http "${GATEWAY_URL}/health" "Gateway" 10
wait_for_http "${LOKI_URL}/ready" "Loki" 10
wait_for_http "${TEMPO_URL}/ready" "Tempo" 10

echo "==> sending traced request with probe ID ${PROBE_ID}"
status=$(curl -sS --max-time 10 -o /dev/null -w '%{http_code}' "${GATEWAY_URL}/order/${PROBE_ID}")
[[ "$status" == "200" ]] || fail "gateway returned HTTP ${status} for /order/${PROBE_ID}"

echo "==> polling Loki for the matching log record (timeout ${POLL_TIMEOUT}s)"
trace_id=""
deadline=$(( $(date +%s) + POLL_TIMEOUT ))
while [[ $(date +%s) -lt $deadline ]]; do
    loki_json=$(curl -sS --max-time 10 -G "${LOKI_URL}/loki/api/v1/query" \
        --data-urlencode "query={service_name=~\".+\"} |= \"${PROBE_ID}\"" \
        --data-urlencode 'limit=50' 2>/dev/null) || loki_json=""

    trace_id=$(printf '%s' "$loki_json" | python3 -c '
import json
import re
import sys

probe = sys.argv[1]
try:
    data = json.load(sys.stdin)
except (json.JSONDecodeError, TypeError):
    raise SystemExit(0)

for stream in data.get("data", {}).get("result", []):
    for _timestamp, line in stream.get("values", []):
        if probe not in line or "order lookup requested" not in line:
            continue
        try:
            record = json.loads(line)
        except (json.JSONDecodeError, TypeError):
            continue
        candidate = record.get("traceid", "")
        if re.fullmatch(r"[0-9a-f]{32}", candidate):
            print(candidate)
            raise SystemExit(0)
' "$PROBE_ID" 2>/dev/null) || trace_id=""

    if [[ -n "$trace_id" ]]; then
        break
    fi
    echo "   matching log not in Loki yet, retrying in ${POLL_INTERVAL}s..."
    sleep "${POLL_INTERVAL}"
done

if [[ -z "$trace_id" ]]; then
    fail "log for ${PROBE_ID} not queryable in Loki within ${POLL_TIMEOUT}s"
fi

echo "==> Loki log carries trace_id=${trace_id}; polling Tempo"
deadline=$(( $(date +%s) + POLL_TIMEOUT ))
trace_found=false
while [[ $(date +%s) -lt $deadline ]]; do
    trace_json=$(curl -sS --max-time 10 "${TEMPO_URL}/api/traces/${trace_id}" 2>/dev/null) || trace_json=""
    if printf '%s' "$trace_json" | grep -q '"batches"'; then
        trace_found=true
        break
    fi
    echo "   trace not in Tempo yet, retrying in ${POLL_INTERVAL}s..."
    sleep "${POLL_INTERVAL}"
done

if [[ "$trace_found" != "true" ]]; then
    fail "trace ${trace_id} not retrievable from Tempo within ${POLL_TIMEOUT}s"
fi

printf '%s' "$trace_json" | python3 -c '
import base64
import json
import sys

probe = sys.argv[1]
trace_id = sys.argv[2]
data = json.load(sys.stdin)


def attribute_value(attribute):
    value = attribute.get("value", {})
    for key in ("stringValue", "intValue", "doubleValue", "boolValue"):
        if key in value:
            return str(value[key])
    return ""


def normalize_trace_id(value):
    """Tempo returns OTLP JSON trace IDs as base64; Loki carries hex."""
    raw = str(value).strip()
    try:
        decoded = base64.b64decode(raw, validate=True)
        if len(decoded) == 16:
            return decoded.hex()
    except Exception:
        pass
    return raw.lower()


candidates = []
for batch in data.get("batches", []):
    for scope_span in batch.get("scopeSpans", []):
        for span in scope_span.get("spans", []):
            attributes = {
                attribute.get("key", ""): attribute_value(attribute)
                for attribute in span.get("attributes", [])
            }
            if attributes.get("order.id") == probe or attributes.get("route") == f"/order/{probe}":
                candidates.append(span)

for span in candidates:
    if normalize_trace_id(span.get("traceId", "")) == trace_id:
        print(f"PASS: Loki log and Tempo trace {trace_id} share the traced request {probe}")
        raise SystemExit(0)

if candidates:
    found = [normalize_trace_id(span.get("traceId", "")) for span in candidates]
    print(f"FAIL: no span for {probe} has trace ID {trace_id}; found {found}", file=sys.stderr)
else:
    print("FAIL: no Tempo span found for probe", probe, file=sys.stderr)
raise SystemExit(1)
' "$PROBE_ID" "$trace_id"

echo "==> log/trace correlation verification complete"
