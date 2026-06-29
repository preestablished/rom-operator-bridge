# Acceptance Checklist

## Code Acceptance

Before closing `rom-operator-bridge-bp8`, verify:

- `RealBackend::start_session` reaches `dh-workerd` and calls
  `RestoreSnapshot` when `BRIDGE_REAL_SNAPSHOT_REF` is configured.
- `RealBackend::start_session` reaches `dh-workerd` and calls `CreateVm` when
  `BRIDGE_CREATE_VM_CONFIG_REF` is configured.
- Returned worker leases are stored only in private backend state.
- `stop_session` calls `DestroyVm` and clears active bridge state.
- `api.rs` stop handling and `cleanup_runtime_session` clear browser-facing
  runtime state even when real `DestroyVm` fails, while still returning
  sanitized `backend_unavailable`.
- `pause` calls worker `Pause`.
- `resume` calls a bounded worker `Run`, returns a paused boundary, and never
  derives an absolute frame from `frames_elapsed`.
- `status` uses `WatchSlots` with `ListSlots` resync on lag or missing cache
  before returning a real-mode `RunStatus`.
- faulted, absent, `DATA_LOSS`, or lease-invalid worker slots end the bridge
  session with sanitized `backend_unavailable`.
- worker failures map to `BackendError::BackendUnavailable`.
- public API errors still use `{ "code": "backend_unavailable", "details": {} }`.
- synthetic tests still pass.

## Privacy Acceptance

Build a temporary forbidden-literals file from the actual private values used in
tests and smoke. Include the UDS path, private root, snapshot ref, create-vm
config ref, workload image ref, capture spec ref, operator credential, session
secret, any generated lease token, HTTP endpoint URI, and any generated 64-hex
snapshot/config hashes.

```bash
rg -n -F -f /tmp/bridge-forbidden-literals.txt \
  --glob '!target/**' \
  --glob '!node_modules/**' \
  service docs contracts ui
```

The search may intentionally exclude the private fixture file itself. It must
not find concrete private values, lease tokens, raw snapshot refs, endpoint URIs,
or private-root paths in committed source, docs, UI assets, public API response
fixtures, websocket event fixtures, or handoff evidence.

Add tests that explicitly assert HTTP response bodies, websocket events, and
public evidence snippets do not contain configured private literals.

## Runtime Acceptance

Runtime acceptance is split by what it proves.

Socket readiness:

- `dh-workerd --preflight` passes.
- `GetWorkerInfo` over `unix:///run/dh/grpc.sock` returns successfully.

Real RestoreSnapshot:

1. Capture starting worker slot counts with `GetWorkerInfo`.
2. Run worker with snapshot-store enabled.
3. Configure `BRIDGE_REAL_SNAPSHOT_REF` to a real private snapshot.
4. Start a real bridge session.
5. Confirm bridge returns a real session id and run id.
6. Confirm worker slot free count decreases by one.
7. Stop the bridge session.
8. Confirm worker slot free count returns to the starting value.
9. Repeat start/stop once to prove stale lease state is not retained.

Real CreateVm:

1. Capture starting worker slot counts with `GetWorkerInfo`.
2. Configure a valid private `BRIDGE_CREATE_VM_CONFIG_REF` file.
3. Start a real bridge session.
4. Confirm bridge returns a real session id and run id.
5. Confirm worker slot free count decreases by one.
6. Confirm status reports `current_frame = 0`.
7. Stop the bridge session.
8. Confirm worker slot free count returns to the starting value.
9. Repeat start/stop once to prove stale lease state is not retained.

If any stop path fails to release the worker slot, do not close bp8.

## Beads Protocol

When implementation is complete:

1. Update `rom-operator-bridge-bp8` notes with sanitized evidence only.
2. Close `rom-operator-bridge-bp8` only after live start/stop acceptance passes.
3. Run:

```bash
git pull --rebase
bd dolt push
git push
git status
```

`git status` must say the branch is up to date with origin and the working tree
is clean.
