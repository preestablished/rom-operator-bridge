# Snapstore And Worker Readiness

## Build Or Preflight First

From `determinism-hypervisor`, run the worker preflight before starting services:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
cargo run -p dh-worker --bin dh-workerd -- --preflight
```

If preflight fails, stop and update `rom-operator-bridge-o73` with a sanitized
blocker. Do not close the bead.

## Start Snapstore

Prefer a UDS snapstore transport for the live acceptance run. Keep the config in
the private workspace, for example:
`$O73_PRIVATE_ROOT/snapstore/config.toml`.

Minimal config shape:

```toml
data_root = "<private snapstore data root>"
grpc_tcp_addr = "127.0.0.1:7410"
grpc_uds_path = "<private runtime dir>/snapstore.sock"
http_addr = "127.0.0.1:7411"
```

Start the server with logs and PID kept private:

```bash
(
  cd /home/infra-admin/git/preestablished/snapshot-store
  nohup setsid env RUST_LOG=info cargo run -p snapstore-server -- \
    --config "$O73_PRIVATE_ROOT/snapstore/config.toml" \
    > "$O73_PRIVATE_ROOT/evidence/snapstore-server.private.log" 2>&1 &
  echo $! > "$O73_PRIVATE_ROOT/runtime/snapstore-server.pid"
)
```

In another shell, check HTTP readiness:

```bash
curl -fsS --connect-timeout 2 --max-time 20 \
  http://127.0.0.1:7411/healthz \
  > "$O73_PRIVATE_ROOT/evidence/snapstore-health.private.txt"
```

Confirm the private snapshot ref manifest is present. Keep output private. This
proves manifest lookup and decoding; full restoreability is proven later by the
live bridge start:

```bash
cd /home/infra-admin/git/preestablished/snapshot-store
timeout 30s cargo run -p snapstore-cli --bin snapstorectl -- \
  --endpoint "uds:$O73_PRIVATE_ROOT/runtime/snapstore.sock" \
  dump-manifest "<private 64 hex snapshot ref>" \
  > "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.txt" \
  2> "$O73_PRIVATE_ROOT/evidence/snapstore-dump-manifest.private.err"
```

If `dump-manifest` reports that the snapshot is absent or corrupt, the blocker
is private snapstore content, not bridge code. Update the bead with a sanitized
blocker and do not close it.

## Start dh-workerd With Snapstore Enabled

Start `dh-workerd` with the bridge-visible UDS and the snapstore UDS. Do not use
`--no-snapstore`.

```bash
(
  cd /home/infra-admin/git/preestablished/determinism-hypervisor
  nohup setsid cargo run -p dh-worker --bin dh-workerd -- serve \
    --tcp 127.0.0.1:7400 \
    --http 127.0.0.1:7401 \
    --uds /run/dh/grpc.sock \
    --snapstore-uds "$O73_PRIVATE_ROOT/runtime/snapstore.sock" \
    > "$O73_PRIVATE_ROOT/evidence/dh-workerd.private.log" 2>&1 &
  echo $! > "$O73_PRIVATE_ROOT/runtime/dh-workerd.pid"
)
```

Loopback TCP is acceptable when UDS is not viable:

```bash
--snapstore-tcp http://127.0.0.1:7410
```

Record which transport class was used, not the private path.

## Worker Readiness Checks

Verify the worker UDS exists and is accessible to the bridge user:

```bash
test -S /run/dh/grpc.sock
```

Use the local hypervisor proto to call `GetWorkerInfo` before the bridge start.
Save raw JSON privately:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
timeout 20s grpcurl -plaintext \
  -unix \
  -import-path proto \
  -proto hypervisor.proto \
  -d '{}' \
  /run/dh/grpc.sock \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo \
  > "$O73_PRIVATE_ROOT/evidence/worker-info-before.private.json" \
  2> "$O73_PRIVATE_ROOT/evidence/worker-info-before.private.err"
```

The exact slot field names are owned by `determinism-hypervisor`. The executing
agent should inspect this JSON privately and record only sanitized numeric slot
counts in bead notes.

Do not proceed unless:

- `GetWorkerInfo` succeeds;
- the worker was not started with `--no-snapstore`;
- the snapshot manifest can be read from snapstore;
- at least one worker slot is available.

When the run is complete, stop private background processes with the recorded
PIDs, after the bridge stop path has already run:

```bash
kill -- "-$(cat "$O73_PRIVATE_ROOT/runtime/dh-workerd.pid")" 2>/dev/null || true
kill -- "-$(cat "$O73_PRIVATE_ROOT/runtime/snapstore-server.pid")" 2>/dev/null || true
```
