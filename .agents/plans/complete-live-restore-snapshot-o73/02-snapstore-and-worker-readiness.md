# Snapstore And Worker Readiness

## 1. Confirm Snapstore Manifest Availability

If the handoff generator left snapstore running, `SNAPSTORE_GRPC_UDS_PATH` may
already be a live socket. First check whether the socket exists:

```bash
cd /home/infra-admin/git/preestablished/snapshot-store
if [ -S "$SNAPSTORE_GRPC_UDS_PATH" ]; then
  timeout 30s cargo run -p snapstore-cli --bin snapstorectl -- \
    --endpoint "uds:$SNAPSTORE_GRPC_UDS_PATH" \
    dump-manifest "$BRIDGE_REAL_SNAPSHOT_REF" \
    > "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.txt" \
    2> "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.err"
  snapstore_lookup_status=$?
else
  snapstore_lookup_status=127
fi
```

If the socket exists but manifest lookup fails, do not start a competing
snapstore. Treat it as a sanitized blocker: either the live service is wrong,
the snapshot is absent/corrupt, or the endpoint is stale and needs operator
cleanup.

```bash
if [ "$snapstore_lookup_status" -eq 0 ]; then
  echo 'snapstore manifest lookup: pass'
elif [ "$snapstore_lookup_status" -ne 127 ]; then
  echo 'snapstore socket exists but manifest lookup failed; inspect private stderr and do not start a competing snapstore' >&2
  exit 1
else
  nohup setsid env \
    -u BRIDGE_REAL_SNAPSHOT_REF \
    -u BRIDGE_WORKLOAD_IMAGE_REF \
    -u BRIDGE_CAPTURE_SPEC_REF \
    -u BRIDGE_HYPERVISOR_ENDPOINT \
    -u O73_OPERATOR_CREDENTIAL \
    -u O73_SESSION_SECRET \
    cargo run -p snapstore-server --bin snapstore-server -- \
    --config "$SNAPSTORE_CONFIG_PATH" \
    > "$O73_PRIVATE_ROOT/evidence/snapstore-server.private.log" 2>&1 &
  echo $! > "$O73_PRIVATE_ROOT/runtime/snapstore-server.pid"

  snapstore_pid="$(cat "$O73_PRIVATE_ROOT/runtime/snapstore-server.pid")"
  sleep 1
  if ! kill -0 "$snapstore_pid" 2>/dev/null; then
    echo 'snapstore exited; inspect private snapstore log' >&2
    exit 1
  fi

  for _ in $(seq 1 80); do
    [ -S "$SNAPSTORE_GRPC_UDS_PATH" ] && break
    sleep 0.25
  done
fi

test -S "$SNAPSTORE_GRPC_UDS_PATH"
```

Run the manifest lookup again. It must pass before continuing:

```bash
timeout 30s cargo run -p snapstore-cli --bin snapstorectl -- \
  --endpoint "uds:$SNAPSTORE_GRPC_UDS_PATH" \
  dump-manifest "$BRIDGE_REAL_SNAPSHOT_REF" \
  > "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.txt" \
  2> "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.err"
```

## 2. Decide Worker Endpoint

The handoff env supplies `BRIDGE_HYPERVISOR_ENDPOINT`. Prefer that endpoint if
the operator can prove it is owned by a snapstore-enabled worker. `GetWorkerInfo`
does not prove snapstore is enabled. For safety, default to a private alternate
worker UDS for this acceptance run unless the operator explicitly approves the
existing worker and proves its command line includes `--snapstore-uds` or
`--snapstore-tcp` and does not include `--no-snapstore`.

Do not include the worker UDS path in public bead notes.

Use a private alternate endpoint:

```bash
O73_WORKER_UDS="$O73_PRIVATE_ROOT/runtime/dh-grpc.sock"
BRIDGE_HYPERVISOR_ENDPOINT="unix://$O73_WORKER_UDS"

tmp="$O73_BRIDGE_ENV.tmp"
awk -v endpoint="$BRIDGE_HYPERVISOR_ENDPOINT" '
  BEGIN { replaced = 0 }
  /^BRIDGE_HYPERVISOR_ENDPOINT=/ {
    print "BRIDGE_HYPERVISOR_ENDPOINT='\''" endpoint "'\''"
    replaced = 1
    next
  }
  { print }
  END {
    if (!replaced) {
      print "BRIDGE_HYPERVISOR_ENDPOINT='\''" endpoint "'\''"
    }
  }
' "$O73_BRIDGE_ENV" > "$tmp"
mv -f "$tmp" "$O73_BRIDGE_ENV"
chmod 0600 "$O73_BRIDGE_ENV"
printf '%s\n' "$BRIDGE_HYPERVISOR_ENDPOINT" >> "$O73_FORBID_FILE"
```

If the operator explicitly approves an existing worker endpoint instead, derive
the UDS path from the approved endpoint and verify command-line ownership
privately before continuing:

```bash
case "$BRIDGE_HYPERVISOR_ENDPOINT" in
  unix://*) O73_WORKER_UDS="${BRIDGE_HYPERVISOR_ENDPOINT#unix://}" ;;
  *) echo 'this plan expects a UDS worker endpoint for o73' >&2; exit 1 ;;
esac

# Operator-approved existing workers only:
# privately identify the owning dh-workerd process and verify its command line
# contains --snapstore-uds or --snapstore-tcp and does not contain --no-snapstore.
```

## 3. Start dh-workerd With Snapstore Enabled

If a worker already responds on `$O73_WORKER_UDS`, verify it privately with
process/command-line evidence, not only `GetWorkerInfo`. If it was started with
`--no-snapstore`, it cannot satisfy o73.

Start a fresh snapstore-enabled worker only when the chosen UDS is free:

```bash
if [ -S "$O73_WORKER_UDS" ]; then
  echo 'worker UDS already exists; require private operator proof of snapstore-enabled ownership or choose another endpoint' >&2
  exit 1
else
  cd /home/infra-admin/git/preestablished/determinism-hypervisor
  cargo run -p dh-worker --bin dh-workerd -- --preflight

  nohup setsid env \
    -u BRIDGE_REAL_SNAPSHOT_REF \
    -u BRIDGE_WORKLOAD_IMAGE_REF \
    -u BRIDGE_CAPTURE_SPEC_REF \
    -u BRIDGE_HYPERVISOR_ENDPOINT \
    -u O73_OPERATOR_CREDENTIAL \
    -u O73_SESSION_SECRET \
    cargo run -p dh-worker --bin dh-workerd -- serve \
    --tcp 127.0.0.1:0 \
    --http 127.0.0.1:0 \
    --uds "$O73_WORKER_UDS" \
    --image-cache "$DH_M9_IMAGE_CACHE" \
    --snapstore-uds "$SNAPSTORE_GRPC_UDS_PATH" \
    > "$O73_PRIVATE_ROOT/evidence/dh-workerd.private.log" 2>&1 &
  echo $! > "$O73_PRIVATE_ROOT/runtime/dh-workerd.pid"

  worker_pid="$(cat "$O73_PRIVATE_ROOT/runtime/dh-workerd.pid")"
  sleep 1
  if ! kill -0 "$worker_pid" 2>/dev/null; then
    echo 'dh-workerd exited; inspect private worker log' >&2
    exit 1
  fi

  for _ in $(seq 1 80); do
    [ -S "$O73_WORKER_UDS" ] && break
    sleep 0.25
  done
fi

test -S "$O73_WORKER_UDS"
```

## 4. Capture Worker Slot Counts

Use the same endpoint the bridge will use:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
timeout 20s grpcurl -plaintext \
  -import-path proto \
  -proto hypervisor.proto \
  -d '{}' \
  "$BRIDGE_HYPERVISOR_ENDPOINT" \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo \
  > "$O73_PRIVATE_ROOT/evidence/worker-info-before.private.json" \
  2> "$O73_PRIVATE_ROOT/evidence/worker-info-before.private.err"
```

Privately assert there is at least one free slot and save counts for later:

```bash
jq -e '.slotsFree >= 1' "$O73_PRIVATE_ROOT/evidence/worker-info-before.private.json" >/dev/null
jq -r '.slotsFree' "$O73_PRIVATE_ROOT/evidence/worker-info-before.private.json" \
  > "$O73_PRIVATE_ROOT/evidence/slots-free-before.private.txt"
jq -r '.slotsTotal' "$O73_PRIVATE_ROOT/evidence/worker-info-before.private.json" \
  > "$O73_PRIVATE_ROOT/evidence/slots-total-before.private.txt"
```

The sanitized bead note should include only counts, not worker endpoint paths or
raw JSON.
