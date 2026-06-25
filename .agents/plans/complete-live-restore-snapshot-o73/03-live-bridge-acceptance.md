# Live Bridge Acceptance

## 1. Run Targeted Mock Coverage First

Before the live run, make sure the bridge-owned RestoreSnapshot path still
passes its mock UDS integration test:

```bash
cd /home/infra-admin/git/preestablished/rom-operator-bridge
cargo test --manifest-path service/Cargo.toml --test real-backend \
  real_restore_snapshot_lifecycle_calls_worker_and_stays_sanitized
```

## 2. Start The Bridge In Real Mode

Start from a clean environment so stale shell variables do not override the
private config file:

```bash
cd /home/infra-admin/git/preestablished/rom-operator-bridge
nohup setsid env \
  -u ROM_OPERATOR_BRIDGE_BACKEND \
  -u ROM_OPERATOR_BRIDGE_BIND_ADDR \
  -u ROM_OPERATOR_BRIDGE_PRIVATE_ROOT \
  -u BRIDGE_PRIVATE_ROOT \
  -u ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL \
  -u ROM_OPERATOR_BRIDGE_SESSION_SECRET \
  -u BRIDGE_HYPERVISOR_ENDPOINT \
  -u BRIDGE_WORKLOAD_IMAGE_REF \
  -u BRIDGE_CAPTURE_SPEC_REF \
  -u BRIDGE_REFERENCE_WORKLOAD_CHECKOUT \
  -u BRIDGE_REAL_SNAPSHOT_REF \
  -u BRIDGE_CREATE_VM_CONFIG_REF \
  ROM_OPERATOR_BRIDGE_CONFIG_FILE="$O73_BRIDGE_ENV" \
  cargo run --manifest-path service/Cargo.toml \
  > "$O73_PRIVATE_ROOT/evidence/bridge.private.log" 2>&1 &
echo $! > "$O73_PRIVATE_ROOT/runtime/bridge.pid"

bridge_pid="$(cat "$O73_PRIVATE_ROOT/runtime/bridge.pid")"
sleep 1
if ! kill -0 "$bridge_pid" 2>/dev/null; then
  echo 'bridge exited; inspect private bridge log' >&2
  exit 1
fi
```

Set request helpers:

```bash
export BRIDGE_URL="http://$O73_BRIDGE_BIND_ADDR"
export ORIGIN="https://rombridge.birb.homes"
```

Confirm health:

```bash
curl -fsS --connect-timeout 2 --max-time 20 \
  "$BRIDGE_URL/health" \
  > "$O73_PRIVATE_ROOT/evidence/bridge-health.private.json"
```

## 3. Start A Real Session Through RestoreSnapshot

```bash
curl -sS --connect-timeout 2 --max-time 300 \
  -D "$O73_PRIVATE_ROOT/evidence/start.headers.private.txt" \
  -o "$O73_PRIVATE_ROOT/evidence/start.body.private.json" \
  -w '%{http_code}' \
  -H "Origin: $ORIGIN" \
  -H "Content-Type: application/json" \
  --data @"$O73_PRIVATE_ROOT/evidence/start-request.private.json" \
  "$BRIDGE_URL/api/session/start" \
  > "$O73_PRIVATE_ROOT/evidence/start.status.private.txt"

test "$(cat "$O73_PRIVATE_ROOT/evidence/start.status.private.txt")" = "200"
```

Private assertions:

```bash
jq -e '.schema_version == 1' "$O73_PRIVATE_ROOT/evidence/start.body.private.json" >/dev/null
jq -e '.session_id != null and .session_id != ""' "$O73_PRIVATE_ROOT/evidence/start.body.private.json" >/dev/null
jq -e '.state == "paused" or .state == "running"' "$O73_PRIVATE_ROOT/evidence/start.body.private.json" >/dev/null
jq -e '.current_frame | type == "number"' "$O73_PRIVATE_ROOT/evidence/start.body.private.json" >/dev/null
```

Extract the cookie without printing it:

```bash
awk 'BEGIN{IGNORECASE=1}
     /^set-cookie:/ {
       sub(/\r$/, "");
       sub(/^[^:]+:[[:space:]]*/, "");
       split($0, parts, ";");
       printf "header = \"Cookie: %s\"\n", parts[1];
       exit
     }' "$O73_PRIVATE_ROOT/evidence/start.headers.private.txt" \
  > "$O73_PRIVATE_ROOT/evidence/session-cookie.private.curlconfig"
chmod 0600 "$O73_PRIVATE_ROOT/evidence/session-cookie.private.curlconfig"
test -s "$O73_PRIVATE_ROOT/evidence/session-cookie.private.curlconfig"

sed 's/^header = "Cookie: //' "$O73_PRIVATE_ROOT/evidence/session-cookie.private.curlconfig" \
  | sed 's/"$//' >> "$O73_FORBID_FILE"
```

Extract the session id privately:

```bash
jq -r '.session_id' "$O73_PRIVATE_ROOT/evidence/start.body.private.json" \
  > "$O73_PRIVATE_ROOT/evidence/session-id.private.txt"
test -s "$O73_PRIVATE_ROOT/evidence/session-id.private.txt"
test "$(cat "$O73_PRIVATE_ROOT/evidence/session-id.private.txt")" != "null"
```

## 4. Confirm Session And Run Status

```bash
curl -sS --connect-timeout 2 --max-time 30 \
  --config "$O73_PRIVATE_ROOT/evidence/session-cookie.private.curlconfig" \
  -D "$O73_PRIVATE_ROOT/evidence/session.headers.private.txt" \
  -o "$O73_PRIVATE_ROOT/evidence/session.body.private.json" \
  -w '%{http_code}' \
  -H "Origin: $ORIGIN" \
  "$BRIDGE_URL/api/session" \
  > "$O73_PRIVATE_ROOT/evidence/session.status.private.txt"
test "$(cat "$O73_PRIVATE_ROOT/evidence/session.status.private.txt")" = "200"

curl -sS --connect-timeout 2 --max-time 30 \
  --config "$O73_PRIVATE_ROOT/evidence/session-cookie.private.curlconfig" \
  -D "$O73_PRIVATE_ROOT/evidence/run-status.headers.private.txt" \
  -o "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json" \
  -w '%{http_code}' \
  -H "Origin: $ORIGIN" \
  "$BRIDGE_URL/api/run/status" \
  > "$O73_PRIVATE_ROOT/evidence/run-status.status.private.txt"
test "$(cat "$O73_PRIVATE_ROOT/evidence/run-status.status.private.txt")" = "200"
```

Private assertions:

```bash
jq -e '.active == true' "$O73_PRIVATE_ROOT/evidence/session.body.private.json" >/dev/null
jq -e '.backend_mode == "real"' "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json" >/dev/null
jq -e '.state == "paused" or .state == "running"' "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json" >/dev/null
```

Capture active worker info privately:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
timeout 20s grpcurl -plaintext \
  -import-path proto \
  -proto hypervisor.proto \
  -d '{}' \
  "$BRIDGE_HYPERVISOR_ENDPOINT" \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo \
  > "$O73_PRIVATE_ROOT/evidence/worker-info-active.private.json" \
  2> "$O73_PRIVATE_ROOT/evidence/worker-info-active.private.err"
```

## 5. Stop The Session

```bash
jq -n --rawfile session_id "$O73_PRIVATE_ROOT/evidence/session-id.private.txt" \
  '{schema_version: 1, session_id: ($session_id | rtrimstr("\n")), reason: "operator_stop"}' \
  > "$O73_PRIVATE_ROOT/evidence/stop-request.private.json"
chmod 0600 "$O73_PRIVATE_ROOT/evidence/stop-request.private.json"

curl -sS --connect-timeout 2 --max-time 120 \
  --config "$O73_PRIVATE_ROOT/evidence/session-cookie.private.curlconfig" \
  -D "$O73_PRIVATE_ROOT/evidence/stop.headers.private.txt" \
  -o "$O73_PRIVATE_ROOT/evidence/stop.body.private.json" \
  -w '%{http_code}' \
  -H "Origin: $ORIGIN" \
  -H "Content-Type: application/json" \
  --data @"$O73_PRIVATE_ROOT/evidence/stop-request.private.json" \
  "$BRIDGE_URL/api/session/stop" \
  > "$O73_PRIVATE_ROOT/evidence/stop.status.private.txt"
test "$(cat "$O73_PRIVATE_ROOT/evidence/stop.status.private.txt")" = "200"
```

Private assertions:

```bash
jq -e '.schema_version == 1' "$O73_PRIVATE_ROOT/evidence/stop.body.private.json" >/dev/null
jq -e '.state == "stopped"' "$O73_PRIVATE_ROOT/evidence/stop.body.private.json" >/dev/null
jq -e '.final_frame | type == "number"' "$O73_PRIVATE_ROOT/evidence/stop.body.private.json" >/dev/null
```

Capture after-stop worker info privately and verify slot counts returned to the
pre-start value:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
timeout 20s grpcurl -plaintext \
  -import-path proto \
  -proto hypervisor.proto \
  -d '{}' \
  "$BRIDGE_HYPERVISOR_ENDPOINT" \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo \
  > "$O73_PRIVATE_ROOT/evidence/worker-info-after-stop.private.json" \
  2> "$O73_PRIVATE_ROOT/evidence/worker-info-after-stop.private.err"
```

If the after-stop slot count does not match the before-start slot count, do not
close `o73`; classify it as a cleanup failure.

