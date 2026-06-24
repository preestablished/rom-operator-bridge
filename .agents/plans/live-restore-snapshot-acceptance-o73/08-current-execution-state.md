# Current Execution State

Last updated: 2026-06-24.

## Verified On Host

- `rom-operator-bridge` worktree was clean and up to date before this update.
- `dh-workerd --preflight` passed.
- Bridge mock RestoreSnapshot lifecycle coverage passed:
  `cargo test --manifest-path service/Cargo.toml --test real-backend real_restore_snapshot_lifecycle_calls_worker_and_stays_sanitized`.
- Existing worker RPC is reachable with this host's `grpcurl` address form:
  `unix:///run/dh/grpc.sock`.
- Existing worker reported 4 total slots and 4 free slots.
- Existing worker command line includes `--no-snapstore`, so it cannot satisfy
  o73.
- No `snapstore-server` process was present.
- No private bridge env file, start request, snapstore config,
  `BRIDGE_REAL_SNAPSHOT_REF`, or snapstore data root was discoverable in the o73
  private workspace or current session environment.

## Disposable Snapstore-Enabled Stack Attempt

A non-invasive smoke stack was attempted on alternate private endpoints under
the o73 private workspace:

- `snapstore-server` started successfully on loopback/private endpoints and
  `/healthz` returned success.
- `dh-workerd serve` was started with alternate TCP/HTTP/UDS endpoints and
  `--snapstore-uds` pointing at the disposable snapstore UDS.
- `dh-workerd` preflight passed and it printed its serving line.
- The process then panicked before worker RPC readiness.

Sanitized panic:

```text
Cannot start a runtime from within a runtime.
```

The panic site is in the sibling `snapshot-store` blocking client:
`crates/snapstore-client/src/blocking.rs`, where
`SnapstoreClient::connect` calls `rt.block_on(...)`.

The `dh-workerd` call path is in the sibling `determinism-hypervisor` worker:

- `crates/dh-worker/src/bin/dh-workerd.rs` has an async `main` / `run`.
- `dh_worker::service::serve(...)` constructs `WorkerService`.
- `WorkerService::new` calls
  `snapstore_client::blocking::SnapstoreClient::connect` when snapstore is
  configured.

## Current Blockers

Live o73 acceptance is not yet complete. It requires all of the following:

1. A `dh-workerd` process that can run with snapstore enabled without the nested
   Tokio runtime panic above.
2. A running snapstore instance containing the operator-approved private
   snapshot.
3. Private bridge inputs:
   `BRIDGE_REAL_SNAPSHOT_REF`, operator credential, session secret, workload
   image ref, capture spec ref, private root, start request body, and snapstore
   config.

Do not close `rom-operator-bridge-o73` until those blockers are resolved and the
real bridge start/status/stop acceptance passes.

## Cleanup Status

The disposable snapstore and worker process groups were signaled after the smoke
attempt. No listeners remained on the alternate smoke ports.
