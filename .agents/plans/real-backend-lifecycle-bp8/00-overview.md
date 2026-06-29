# Real Backend Lifecycle Plan for bp8

## Goal

Finish `rom-operator-bridge-bp8` by replacing the current fail-closed
`RealBackend` scaffold with a real `dh-workerd` client behind the existing
`BridgeBackend` service interface.

The current branch already has:

- real-mode private config gating for the required `BRIDGE_*` values;
- `BRIDGE_PRIVATE_ROOT` alias support;
- redacted `RealRuntimeConfig`;
- `AppState::from_config` constructing `RealBackend`;
- a sanitized `backend_unavailable` public error path;
- `service/tests/real-backend/main.rs` covering unavailable real mode.

This plan covers the remaining implementation work:

- add `dh-proto` and tonic UDS/TCP client wiring;
- call `RestoreSnapshot` or `CreateVm` during `start_session`;
- keep lease token and slot id server-side only;
- call `DestroyVm` during stop and cleanup;
- implement status, pause, resume, and fault cleanup mapping;
- map worker failures to sanitized `BackendError::BackendUnavailable`.

## Current Runtime State

As of June 24, 2026, `dh-workerd` can run on this host and bind the expected
socket. The proven command shape is:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
target/debug/dh-workerd serve \
  --tcp 127.0.0.1:7400 \
  --http 127.0.0.1:7401 \
  --uds /run/dh/grpc.sock \
  --no-snapstore
```

For real `RestoreSnapshot`, do not use `--no-snapstore`; run snapshot-store and
start `dh-workerd` with its snapstore transport.

## Non-Goals

Do not implement real framebuffer preview, real input injection, or real capture
export in bp8. Those are owned by downstream beads:

- `rom-operator-bridge-0i9`: real framebuffer preview source;
- `rom-operator-bridge-3dr`: real frame-boundary input injection;
- `rom-operator-bridge-q63`: real capture export integration.

Do not expose lease tokens, snapshot refs, image refs, capture spec refs,
private paths, raw worker errors, or private config values in public API
responses, UI events, logs intended for public notes, or docs.

## Implementation Strategy

Keep `BridgeBackend` synchronous for this bead. Tonic is async, so put the tonic
client behind a private real-worker command loop running on its own Tokio
runtime thread. Synchronous `BridgeBackend` methods send commands to that thread
and wait for sanitized results.

Never hold `RealBackendInner` locks while waiting for the worker thread or tonic
RPCs. Clone the lease/session data needed for the command, release the lock,
send the command, then reacquire the lock only to apply the sanitized result.
This applies to start, stop, pause, resume, and status resync.

This avoids a broad async trait migration across every existing backend fake and
test double. It is acceptable for bp8 because lifecycle operations are low
frequency. Revisit the backend interface when high-frequency real input
injection is implemented.

## Expected File Touches

Primary files:

- `service/Cargo.toml`
- `service/src/private_config.rs`
- `service/src/backend.rs`
- `service/src/api.rs`
- `service/src/artifacts.rs`
- `service/tests/config/main.rs`
- `service/tests/real-backend/main.rs`

Possible supporting files:

- `service/tests/auth/main.rs`
- `service/tests/session/main.rs`
- `docs/real-backend-availability.md`

Keep edits scoped to real backend lifecycle. Avoid unrelated refactors.
