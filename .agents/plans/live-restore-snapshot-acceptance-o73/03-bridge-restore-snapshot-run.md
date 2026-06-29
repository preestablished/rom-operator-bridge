# Bridge RestoreSnapshot Run

## Start The Bridge

From the bridge repository:

```bash
(
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
    ROM_OPERATOR_BRIDGE_CONFIG_FILE="$O73_PRIVATE_ROOT/bridge/real-restore-snapshot.env" \
    cargo run --manifest-path service/Cargo.toml \
    > "$O73_PRIVATE_ROOT/evidence/bridge.private.log" 2>&1 &
  echo $! > "$O73_PRIVATE_ROOT/runtime/bridge.pid"
)
```

Use `http://127.0.0.1:7420` for the local acceptance commands below unless the
private env file uses a different bind address.

The bridge hard-codes browser runtime origin validation to
`https://rombridge.birb.homes`, so include that `Origin` header in runtime API
requests even when calling the local loopback address.

## Start Session

Send the private start body and save headers/body privately:

```bash
BRIDGE_URL="http://127.0.0.1:7420"
ORIGIN="https://rombridge.birb.homes"

curl -sS --connect-timeout 2 --max-time 300 \
  -D "$O73_PRIVATE_ROOT/evidence/start.headers.private.txt" \
  -o "$O73_PRIVATE_ROOT/evidence/start.body.private.json" \
  -w '%{http_code}' \
  -H "Origin: $ORIGIN" \
  -H "Content-Type: application/json" \
  --data @"$O73_PRIVATE_ROOT/evidence/start-request.json" \
  "$BRIDGE_URL/api/session/start" \
  > "$O73_PRIVATE_ROOT/evidence/start.status.private.txt"

test "$(cat "$O73_PRIVATE_ROOT/evidence/start.status.private.txt")" = "200"
```

Expected start response:

- HTTP 200;
- `schema_version: 1`;
- `backend_mode` is not in `startSessionResponse`, but the configured backend is
  real and subsequent status must report real;
- `state` is `paused` or `running`;
- `current_frame` is a non-negative integer;
- capabilities reflect the requested and granted real backend support;
- `Set-Cookie` is present.

Extract only the cookie name/value from the private header file into a private
curl config. Do not export, commit, print, or share the cookie:

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
```

Avoid relying on a normal curl cookie jar for local HTTP because the service sets
`Secure; SameSite=Strict` on the session cookie.

Extract the session id privately:

```bash
jq -r '.session_id' "$O73_PRIVATE_ROOT/evidence/start.body.private.json" \
  > "$O73_PRIVATE_ROOT/evidence/session-id.private.txt"
test -s "$O73_PRIVATE_ROOT/evidence/session-id.private.txt"
test "$(cat "$O73_PRIVATE_ROOT/evidence/session-id.private.txt")" != "null"
```

## Confirm Session And Run Status

Call both status routes with the origin and cookie:

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

Expected:

- `GET /api/session` returns `active: true`;
- `GET /api/run/status` returns `backend_mode: real`;
- state is `paused` or `running`;
- `current_frame` is a non-negative integer;
- no response contains private snapshot refs, credential material, private root
  paths, lease token material, or raw worker error text.

Capture worker info while the session is active:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
timeout 20s grpcurl -plaintext \
  -import-path proto \
  -proto hypervisor.proto \
  -d '{}' \
  unix:///run/dh/grpc.sock \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo \
  > "$O73_PRIVATE_ROOT/evidence/worker-info-active.private.json" \
  2> "$O73_PRIVATE_ROOT/evidence/worker-info-active.private.err"
```

Verify privately that the active slot/lease count changed as expected.

## Stop Session

Create the private stop body:

```bash
jq -n --rawfile session_id "$O73_PRIVATE_ROOT/evidence/session-id.private.txt" \
  '{schema_version: 1, session_id: ($session_id | rtrimstr("\n")), reason: "operator_stop"}' \
  > "$O73_PRIVATE_ROOT/evidence/stop-request.private.json"
```

Stop the session:

```bash
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

Expected stop response:

- HTTP 200;
- `schema_version: 1`;
- same `session_id`;
- `state: stopped`;
- `final_frame` is a non-negative integer.

Capture worker info after stop:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
timeout 20s grpcurl -plaintext \
  -import-path proto \
  -proto hypervisor.proto \
  -d '{}' \
  unix:///run/dh/grpc.sock \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo \
  > "$O73_PRIVATE_ROOT/evidence/worker-info-after-stop.private.json" \
  2> "$O73_PRIVATE_ROOT/evidence/worker-info-after-stop.private.err"
```

The post-stop worker slot count must match the pre-start count. If not, treat it
as a cleanup failure and do not close the bead.

After the post-stop checks and backend-unavailable probe, stop the bridge process
group:

```bash
kill -- "-$(cat "$O73_PRIVATE_ROOT/runtime/bridge.pid")" 2>/dev/null || true
```
