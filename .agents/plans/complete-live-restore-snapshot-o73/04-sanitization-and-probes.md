# Sanitization And Probes

## 1. Forbidden-Literal Sweep Public Responses

Run the sweep with `rg -q` first so private matches are not printed.

```bash
public_bodies=(
  "$O73_PRIVATE_ROOT/evidence/bridge-health.private.json"
  "$O73_PRIVATE_ROOT/evidence/start.body.private.json"
  "$O73_PRIVATE_ROOT/evidence/session.body.private.json"
  "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json"
  "$O73_PRIVATE_ROOT/evidence/stop.body.private.json"
)

if rg -q -F -f "$O73_FORBID_FILE" "${public_bodies[@]}"; then
  rg -l -F -f "$O73_FORBID_FILE" "${public_bodies[@]}" \
    > "$O73_PRIVATE_ROOT/evidence/forbidden-response-files.private.txt"
  echo 'forbidden literals found in public response bodies; inspect private file list' >&2
  exit 1
fi
```

Do not run a command that prints matching lines.

## 2. Sanitized Backend-Unavailable Probe

After the successful run and stop path, prove the unavailable path still returns
a sanitized envelope.

Stop the bridge process group first:

```bash
bridge_pid="$(cat "$O73_PRIVATE_ROOT/runtime/bridge.pid")"
kill -- "-$bridge_pid" 2>/dev/null || true
for _ in $(seq 1 80); do
  if ! kill -0 "$bridge_pid" 2>/dev/null; then
    break
  fi
  sleep 0.25
done
```

Create a temporary private env file with a dummy worker endpoint under the
private runtime directory:

```bash
O73_UNAVAILABLE_ENV="$O73_PRIVATE_ROOT/bridge/backend-unavailable-probe.env"
cp -f "$O73_BRIDGE_ENV" "$O73_UNAVAILABLE_ENV"
chmod 0600 "$O73_UNAVAILABLE_ENV"
dummy_endpoint="unix://$O73_PRIVATE_ROOT/runtime/nonexistent-dh-grpc.sock"

tmp="$O73_UNAVAILABLE_ENV.tmp"
awk -v endpoint="$dummy_endpoint" '
  /^BRIDGE_HYPERVISOR_ENDPOINT=/ {
    print "BRIDGE_HYPERVISOR_ENDPOINT='\''" endpoint "'\''"
    next
  }
  { print }
' "$O73_UNAVAILABLE_ENV" > "$tmp"
mv -f "$tmp" "$O73_UNAVAILABLE_ENV"
chmod 0600 "$O73_UNAVAILABLE_ENV"
printf '%s\n' "$dummy_endpoint" >> "$O73_FORBID_FILE"
```

Restart the bridge with the probe env:

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
  ROM_OPERATOR_BRIDGE_CONFIG_FILE="$O73_UNAVAILABLE_ENV" \
  cargo run --manifest-path service/Cargo.toml \
  > "$O73_PRIVATE_ROOT/evidence/backend-unavailable-bridge.private.log" 2>&1 &
echo $! > "$O73_PRIVATE_ROOT/runtime/backend-unavailable-bridge.pid"

probe_pid="$(cat "$O73_PRIVATE_ROOT/runtime/backend-unavailable-bridge.pid")"
for _ in $(seq 1 120); do
  if ! kill -0 "$probe_pid" 2>/dev/null; then
    echo 'backend-unavailable probe bridge exited; inspect private log' >&2
    exit 1
  fi
  if curl -fsS --connect-timeout 2 --max-time 5 \
      "$BRIDGE_URL/health" \
      > "$O73_PRIVATE_ROOT/evidence/backend-unavailable-health.private.json"; then
    break
  fi
  sleep 0.5
done
test -s "$O73_PRIVATE_ROOT/evidence/backend-unavailable-health.private.json"
```

Call start and expect `503`:

```bash
curl -sS --connect-timeout 2 --max-time 120 \
  -D "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.headers.private.txt" \
  -o "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.body.private.json" \
  -w '%{http_code}' \
  -H "Origin: $ORIGIN" \
  -H "Content-Type: application/json" \
  --data @"$O73_PRIVATE_ROOT/evidence/start-request.private.json" \
  "$BRIDGE_URL/api/session/start" \
  > "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.status.private.txt"

test "$(cat "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.status.private.txt")" = "503"
jq -e '.error.code == "backend_unavailable"' "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.body.private.json" >/dev/null
jq -e '.error.retryable == true' "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.body.private.json" >/dev/null
jq -e '.error.details == {}' "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.body.private.json" >/dev/null
```

Sweep the probe response:

```bash
if rg -q -F -f "$O73_FORBID_FILE" "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.body.private.json"; then
  echo 'backend_unavailable response leaked a forbidden literal' >&2
  exit 1
fi
```

Stop the probe bridge:

```bash
kill -- "-$(cat "$O73_PRIVATE_ROOT/runtime/backend-unavailable-bridge.pid")" 2>/dev/null || true
```

## 3. Produce A Sanitized Summary

Write the private draft summary after the successful unavailable-path probe, so
it includes the complete acceptance evidence. Keep it to booleans, HTTP status
codes, state names, and slot counts.

```bash
{
  echo "Live RestoreSnapshot acceptance passed."
  echo
  echo "RestoreSnapshot branch preflight:"
  echo "- snapshot_ref_configured=yes"
  echo "- create_vm_config_ref_configured=no"
  echo "- snapstore_manifest_lookup=pass"
  echo
  echo "HTTP results:"
  echo "- start=$(cat "$O73_PRIVATE_ROOT/evidence/start.status.private.txt")"
  echo "- session=$(cat "$O73_PRIVATE_ROOT/evidence/session.status.private.txt")"
  echo "- run_status=$(cat "$O73_PRIVATE_ROOT/evidence/run-status.status.private.txt")"
  echo "- stop=$(cat "$O73_PRIVATE_ROOT/evidence/stop.status.private.txt")"
  echo "- backend_unavailable_probe=$(cat "$O73_PRIVATE_ROOT/evidence/backend-unavailable-start.status.private.txt")"
  echo
  echo "States:"
  echo "- start_state=$(jq -r '.state' "$O73_PRIVATE_ROOT/evidence/start.body.private.json")"
  echo "- run_status_backend=$(jq -r '.backend_mode' "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json")"
  echo "- run_status_state=$(jq -r '.state' "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json")"
  echo "- stop_state=$(jq -r '.state' "$O73_PRIVATE_ROOT/evidence/stop.body.private.json")"
  echo
  echo "Worker slot counts:"
  echo "- total_before=$(cat "$O73_PRIVATE_ROOT/evidence/slots-total-before.private.txt")"
  echo "- free_before=$(cat "$O73_PRIVATE_ROOT/evidence/slots-free-before.private.txt")"
  echo "- free_active=$(cat "$O73_PRIVATE_ROOT/evidence/slots-free-active.private.txt")"
  echo "- free_after=$(cat "$O73_PRIVATE_ROOT/evidence/slots-free-after.private.txt")"
  echo
  echo "Redaction:"
  echo "- forbidden_literal_sweep=pass"
  echo "- backend_unavailable_sanitized=pass"
} > "$O73_PRIVATE_ROOT/evidence/o73-sanitized-summary.private.txt"
chmod 0600 "$O73_PRIVATE_ROOT/evidence/o73-sanitized-summary.private.txt"
```

Before using any summary in a bead note, sweep it too:

```bash
if rg -q -F -f "$O73_FORBID_FILE" "$O73_PRIVATE_ROOT/evidence/o73-sanitized-summary.private.txt"; then
  echo 'sanitized summary contains a forbidden literal' >&2
  exit 1
fi
```

## 4. Sweep Changed Public Files Before Notes Or Commit

Before committing plan/doc changes or appending bead notes, sweep changed files
and the sanitized summary. Do not scan the entire repository blindly: the source
tree may contain known public constants that are also in the forbidden file.

```bash
cd /home/infra-admin/git/preestablished/rom-operator-bridge

{
  git diff --name-only --diff-filter=ACMRT
  git diff --cached --name-only --diff-filter=ACMRT
} | sort -u > "$O73_PRIVATE_ROOT/evidence/changed-files.private.txt"

if [ -s "$O73_PRIVATE_ROOT/evidence/changed-files.private.txt" ]; then
  set +e
  xargs -r -d '\n' rg --hidden --glob '!.git' -q -F -f "$O73_FORBID_FILE" \
    < "$O73_PRIVATE_ROOT/evidence/changed-files.private.txt"
  sweep_status=$?
  set -e
  if [ "$sweep_status" -eq 0 ]; then
    xargs -r -d '\n' rg --hidden --glob '!.git' -l -F -f "$O73_FORBID_FILE" \
      < "$O73_PRIVATE_ROOT/evidence/changed-files.private.txt" \
      > "$O73_PRIVATE_ROOT/evidence/forbidden-changed-files.private.txt"
    echo 'forbidden literals found in changed files; inspect private file list' >&2
    exit 1
  elif [ "$sweep_status" -ne 1 ]; then
    echo 'changed-file forbidden-literal sweep errored' >&2
    exit 1
  fi
fi

if rg -q -F -f "$O73_FORBID_FILE" "$O73_PRIVATE_ROOT/evidence/o73-sanitized-summary.private.txt"; then
  echo 'forbidden literals found in sanitized summary' >&2
  exit 1
fi
```

These sweeps must find no matches.
