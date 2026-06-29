# Tests and Smoke Plan

## Unit and Integration Tests

Extend `service/tests/real-backend/main.rs`. Keep tests sanitized and avoid
requiring a real ROM unless explicitly marked ignored.

Recommended tests:

1. Missing worker socket returns public `backend_unavailable`.
2. Complete real config with a mock worker can start via `RestoreSnapshot`.
3. Complete real config with a mock worker can start via `CreateVm`.
4. `CreateVm` parses the private JSON schema, sends the expected
   `CreateVmRequest`, includes the 32-byte entropy seed, stores the returned
   lease privately, and reports `current_frame = 0`.
5. Invalid CreateVm JSON, wrong private file mode, path traversal, and missing
   private file all return sanitized `backend_unavailable`.
6. Start response grants no preview/input/capture capabilities for real bp8.
7. Stop after start calls `DestroyVm`.
8. `DestroyVm` failure clears backend active state and the API clears
   browser-facing runtime state while returning sanitized `backend_unavailable`.
9. Start artifact failure best-effort destroys the worker lease.
10. Private artifacts do not contain lease tokens by default. If a future change
    persists a lease for crash cleanup, tests must prove that file lives under
    the private root with mode `0600` and is excluded from public evidence.
11. Pause maps `PauseResponse` to a paused boundary.
12. Resume calls bounded `Run`, updates `current_icount`, leaves state paused,
    and does not compute `current_frame += frames_elapsed`.
13. Faulted run response clears or faults the active session.
14. Status uses `WatchSlots` or `ListSlots` resync, and clears the session when
    the slot is missing, faulted, lagged beyond recovery, or invalid for the
    stored lease.
15. Public response bodies and UI event payloads after start, pause, resume,
    stop, status, and fault do not contain:
    - `/run/dh/grpc.sock`;
    - private root path;
    - snapshot ref;
    - create-vm config ref;
    - lease token;
    - worker tonic status message.
16. Public handoff notes and evidence snippets contain only sanitized values.

## Mock Worker

Use a test-local tonic server over a temp UDS. Follow the pattern in
`determinism-hypervisor/crates/dh-worker/tests/m6_full_api_uds.rs`, but do not
pull in `dh-worker`.

Implement only the RPCs needed by bp8:

- `RestoreSnapshot`
- `CreateVm`
- `DestroyVm`
- `Pause`
- `Run`
- `WatchSlots`
- `ListSlots`

The generated server type is available from:

```rust
dh_proto::v1::hypervisor_worker_server::{
    HypervisorWorker,
    HypervisorWorkerServer,
}
```

The test fake should record requests in an `Arc<Mutex<Vec<...>>>` so tests can
assert `DestroyVm` was called.

## Private Config Test Helpers

Use temp private roots with proper permissions. Existing config tests already
show the pattern:

- create tempdir;
- pass `BRIDGE_PRIVATE_ROOT` or `ROM_OPERATOR_BRIDGE_PRIVATE_ROOT`;
- set operator credential and session secret;
- set `BRIDGE_HYPERVISOR_ENDPOINT=<unix temp socket endpoint>`;
- set `BRIDGE_REAL_SNAPSHOT_REF=<64 hex>` or a private
  `BRIDGE_CREATE_VM_CONFIG_REF`.

For `CreateVm`, write the private config JSON under the private root and set
mode `0600`. Include helper fixtures for:

- valid `boot.elf`;
- invalid hash length;
- duplicate `cpuid_table` entries;
- duplicate or out-of-range `device_set` entries;
- invalid `hash_epochs`;
- both boot oneof variants present;
- non-`0600` private file mode.

## Live Smoke

Do this only after snapshot-store and private refs are available.

Separate the runtime checks into three levels. Passing level 1 only proves the
socket is reachable; it is not sufficient to close bp8.

Level 1, worker readiness:

```bash
cd /home/infra-admin/git/preestablished/determinism-hypervisor
cargo run -p dh-worker --bin dh-workerd -- --preflight
grpcurl -plaintext -unix -import-path proto -proto hypervisor.proto \
  -d '{}' unix:///run/dh/grpc.sock \
  determinism.hypervisor.v1.HypervisorWorker/GetWorkerInfo
```

Bridge env shape:

```bash
ROM_OPERATOR_BRIDGE_BACKEND=real
BRIDGE_HYPERVISOR_ENDPOINT=<unix UDS endpoint>
BRIDGE_PRIVATE_ROOT=<private root>
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<private credential>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<private secret>
BRIDGE_WORKLOAD_IMAGE_REF=<private image ref>
BRIDGE_CAPTURE_SPEC_REF=<private capture spec ref>
BRIDGE_REFERENCE_WORKLOAD_CHECKOUT=<absolute reference workload checkout>
BRIDGE_REAL_SNAPSHOT_REF=<64 hex private snapshot ref>
```

Do not commit or paste the actual private values into public notes.

Level 2, real RestoreSnapshot RPC:

- `POST /api/session/start` with `backend_mode=real` returns `200`;
- response has `backend_mode=real` through later session status;
- response capabilities are all false for bp8-owned paths;
- `POST /api/session/stop` returns success;
- worker `GetWorkerInfo` slot free count returns to the starting value.

If snapshot-store is not running or the snapshot ref is absent, the same start
request must return sanitized `backend_unavailable`.

Level 3, real CreateVm RPC:

- create a private `BRIDGE_CREATE_VM_CONFIG_REF` JSON file under
  `BRIDGE_PRIVATE_ROOT` with mode `0600`;
- start a real bridge session without `BRIDGE_REAL_SNAPSHOT_REF`;
- verify the worker slot count decreases by one;
- verify bridge status reports `current_frame = 0` and real mode;
- stop the bridge session and verify the slot count returns to the starting
  value.

## Quality Gates

Run from `service/`:

```bash
cargo fmt --check
cargo test --test real-backend
cargo test
```

Run the repository redaction gate if it remains available:

```bash
node scripts/redaction-gate.mjs
```
