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
- Initially, the process then panicked before worker RPC readiness.

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

This startup blocker was fixed in sibling `determinism-hypervisor` commit
`8b59bbf` (`Fix snapstore worker startup`) by constructing `WorkerService` from
`dh_worker::service::serve` via `tokio::task::spawn_blocking`.

Post-fix verification:

- `cargo test -p dh-worker worker_info -- --nocapture` passed in
  `determinism-hypervisor`.
- Disposable `snapstore-server` reached `/healthz`.
- Disposable snapstore-enabled `dh-workerd` responded to `GetWorkerInfo` and
  `ListSlots` on alternate private endpoints.
- The fixed worker reported 4 total slots and 4 free slots.

## Bridge Missing-Snapshot Probe

After the worker startup fix, a bridge probe was run against disposable
snapstore and snapstore-enabled worker endpoints with a non-secret placeholder
snapshot ref that is not present in snapstore.

Results:

- bridge `/health` returned success;
- `POST /api/session/start` returned HTTP `503`;
- response error code was `backend_unavailable`;
- `retryable` was `true`;
- error `details` was empty;
- response body did not contain the worker socket path, snapstore socket path,
  disposable snapstore data root, bridge private root, placeholder snapshot ref,
  placeholder credential, placeholder session secret, placeholder workload ref,
  or placeholder capture ref.

This proves the sanitized unavailable path for a real RestoreSnapshot attempt
against a snapstore-enabled worker, but it does not satisfy o73 because the
operator-approved private snapshot was not available.

## Current Blockers

Live o73 acceptance is not yet complete. It requires all of the following:

1. A running snapstore instance containing the operator-approved private
   snapshot.
2. Private bridge inputs:
   `BRIDGE_REAL_SNAPSHOT_REF`, operator credential, session secret, workload
   image ref, capture spec ref, private root, start request body, and snapstore
   config.
3. A snapstore-enabled `dh-workerd` attached to that private snapstore. The
   startup path itself was verified after the `determinism-hypervisor` fix, but
   the host's currently running `/run/dh/grpc.sock` worker still used
   `--no-snapstore` during this check.

Do not close `rom-operator-bridge-o73` until those blockers are resolved and the
real bridge start/status/stop acceptance passes.

## Cleanup Status

The disposable snapstore, worker, and bridge process groups were signaled after
the smoke/probe attempts. No listeners remained on the alternate smoke ports.
