# Snapstore And Worker Readiness

## 1. Confirm Snapstore Manifest Availability

If the handoff generator left snapstore running, `SNAPSTORE_GRPC_UDS_PATH` may
already be a live socket. First try the manifest lookup privately:

```bash
cd /home/infra-admin/git/preestablished/snapshot-store
if cargo run -p snapstore-cli --bin snapstorectl -- \
    --endpoint "uds:$SNAPSTORE_GRPC_UDS_PATH" \
    dump-manifest "$BRIDGE_REAL_SNAPSHOT_REF" \
    > "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.txt" \
    2> "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.err"
then
  echo 'snapstore manifest lookup: pass'
else
  echo 'snapstore manifest lookup failed; starting snapstore from private config' >&2
fi
```

If the lookup failed because the server is not running, start snapstore using
the generated config:

```bash
if [ ! -S "$SNAPSTORE_GRPC_UDS_PATH" ] || \
   ! cargo run -p snapstore-cli --bin snapstorectl -- \
      --endpoint "uds:$SNAPSTORE_GRPC_UDS_PATH" \
      dump-manifest "$BRIDGE_REAL_SNAPSHOT_REF" \
      > /dev/null 2>&1
then
  nohup setsid cargo run -p snapstore-server --bin snapstore-server -- \
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
cargo run -p snapstore-cli --bin snapstorectl -- \
  --endpoint "uds:$SNAPSTORE_GRPC_UDS_PATH" \
  dump-manifest "$BRIDGE_REAL_SNAPSHOT_REF" \
  > "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.txt" \
  2> "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.err"
```

## 2. Decide Worker Endpoint

The handoff env supplies `BRIDGE_HYPERVISOR_ENDPOINT`. Prefer that endpoint if
it is available for a snapstore-enabled worker. If it points at
`unix:///run/dh/grpc.sock` and that socket is currently owned by a
`--no-snapstore` worker, do not kill it blindly. Choose one of:

1. Get operator approval to restart that worker snapstore-enabled.
2. Use a private alternate worker UDS and override `BRIDGE_HYPERVISOR_ENDPOINT`
   in `$O73_BRIDGE_ENV`.

For a private alternate endpoint:

```bash
export O73_WORKER_UDS="$O73_PRIVATE_ROOT/runtime/dh-grpc.sock"
export BRIDGE_HYPERVISOR_ENDPOINT="unix://$O73_WORKER_UDS"

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

If using the handoff endpoint as-is:

```bash
case "$BRIDGE_HYPERVISOR_ENDPOINT" in
  unix://*) export O73_WORKER_UDS="${BRIDGE_HYPERVISOR_ENDPOINT#unix://}" ;;
  *) echo 'this plan expects a UDS worker endpoint for o73' >&2; exit 1 ;;
esac
```

## 3. Start dh-workerd With Snapstore Enabled

If a worker already responds on `$O73_WORKER_UDS`, verify it privately with
`GetWorkerInfo`. If it was started with `--no-snapstore`, it cannot satisfy
o73.

Start a fresh snapstore-enabled worker only when the chosen UDS is free:

```bash
if [ -S "$O73_WORKER_UDS" ]; then
  echo 'worker UDS already exists; verify or choose another endpoint' >&2
else
  cd /home/infra-admin/git/preestablished/determinism-hypervisor
  cargo run -p dh-worker --bin dh-workerd -- --preflight

  nohup setsid cargo run -p dh-worker --bin dh-workerd -- serve \
    --tcp 127.0.0.1:7400 \
    --http 127.0.0.1:7401 \
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

Privately record total/free slot counts from the response. The sanitized bead
note should include only counts, not worker endpoint paths or raw JSON.

