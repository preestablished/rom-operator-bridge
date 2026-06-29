# Lifecycle Method Implementation

## start_session

Flow:

1. Lock `RealBackendInner`.
2. If a session is already active, return `BackendUnavailable`.
3. Reserve the next local `session_id` and `run_id`.
4. Drop the lock before the worker RPC.
5. Dispatch either `RestoreSnapshot` or `CreateVm` to the worker command loop.
6. Validate the response has a lease.
7. Write private manifest and `session_started` event.
8. Store `RealSession` in `inner.active`.
9. Return `BackendSession`.

Snapshot request:

```rust
dh_proto::v1::RestoreSnapshotRequest {
    snapshot: Some(dh_proto::v1::SnapshotRef { hash: snapshot_hash.to_vec() }),
    entropy_seed: Vec::new(),
}
```

CreateVm request:

```rust
dh_proto::v1::CreateVmRequest {
    config: Some(machine_config),
    entropy_seed,
}
```

Frame and icount:

- Restore: use `RestoreSnapshotResponse.frame_counter` and leave icount at `0`
  unless the worker response exposes a specific cumulative icount later.
- CreateVm: current frame `0`, icount from `CreateVmResponse.icount`.

State:

- Use `SessionState::Paused` after `RestoreSnapshot`, because restored slots are
  paused.
- Use `SessionState::Paused` after `CreateVm`, unless worker docs prove created
  slots are running. Current worker tests treat allocated slots as paused.

Cleanup on partial failure:

- If worker returns a lease but private artifact writing fails, call `DestroyVm`
  best effort before returning `BackendUnavailable`.
- Do not hold the backend mutex while waiting for the worker command. Reserve
  IDs under lock, release the lock for the RPC, then reacquire it before
  publishing `inner.active`.

## stop_session

Flow:

1. Lock and remove the matching active session.
2. If no matching session, return `BackendUnavailable`.
3. Call `DestroyVm` with the stored lease.
4. If destroy succeeds, append `session_stopped` and return `StoppedSession`.
5. If destroy fails, append private `cleanup_failed`, keep no active backend
   session, and return `BackendUnavailable`.

Required API cleanup change:

- Current `api.rs` clears `runtime_session`, websocket state, preview state, and
  auth cookies only after `backend.stop_session(...)` returns `Ok`.
- Change the stop handler and `cleanup_runtime_session` so a stop request for
  the current runtime session clears browser-facing bridge state even when the
  backend reports a real `DestroyVm` cleanup failure.
- The public HTTP response for the failed destroy still returns sanitized
  `backend_unavailable`; the important invariant is that the bridge no longer
  accepts input/status as if that old session is usable.
- Preserve normal `BackendUnavailable` behavior when there was no matching
  runtime session to clear.

Reason mapping:

- `OperatorStop`: normal stop event.
- `SessionReplaced`: still destroy the old lease before replacing.
- `FaultCleanup`: destroy the lease and record a fault cleanup event.

Do not leave an active session after a destroy failure. The worker may still own
the slot, but the bridge must stop accepting browser input for that session.

## pause

Flow:

1. Find active matching session.
2. Call `Pause` with the lease.
3. On success, set `state = Paused` and update `current_icount`.
4. Append `session_paused` if state changed.
5. Return `RunBoundary`.

The worker `PauseResponse` does not include frame counter. Keep the previous
`current_frame`.

If the worker returns `FailedPrecondition` because the slot is already paused,
treat that as success and return the current paused boundary only if the bridge
already believes it is paused. Otherwise map to `BackendUnavailable`.

## resume

The current real runtime docs define resume as the next bounded `Run`, not a
separate `Resume` RPC.

For bp8, implement a conservative one-frame step:

```rust
RunRequest {
    lease: Some(lease),
    until: Some(run_request::Until::FrameBudget(1)),
    hard_icount_cap: 0,
    capture: None,
}
```

On success:

- if `RunResponse.reason == FAULTED`, mark session faulted and return
  `BackendUnavailable`;
- if capture output includes `fb_info.frame_counter`, update `current_frame`
  from that authoritative value;
- otherwise leave `current_frame` unchanged and mark any preview/frame-derived
  status as stale or unknown if the existing status model has such a flag;
- update `current_icount` from `RunResponse.icount`.

Recommended public semantics for bp8:

- Treat `resume` as "advance one bounded run step and return to a paused
  boundary."
- Set `state = Paused` after successful bounded `Run`, because
  `RunRequest.frame_budget` stops at a boundary and leaves the slot paused.
- Do not derive an absolute frame counter from `frames_elapsed`; it is elapsed
  frame marks for that run, not the final `FRAME_COUNTER`.

This may be refined by `rom-operator-bridge-3dr` when frame-boundary input
scheduling owns the stepping loop.

## status

Status must not report stale bridge-owned state for a slot the worker considers
missing, faulted, or invalid.

Worker synchronization requirements:

- Start a private `WatchSlots` task in the worker thread after the client
  connects, or before the first real session is published.
- Maintain a private slot-status cache keyed by slot id.
- On watch lag, stream error, `RESOURCE_EXHAUSTED`, or a status request with no
  fresh cache entry for the active slot, call `ListSlots` once to resync before
  answering status.
- If resync fails, map to sanitized `BackendUnavailable`.
- If the active slot is absent, faulted, reports `DATA_LOSS`, has a lease
  validation failure, or is otherwise not compatible with the stored lease,
  clear the bridge session and return `BackendUnavailable`.

Minimum fields:

- `backend_mode = Real`;
- `current_frame` from session state;
- `last_preview_frame = 0`;
- `last_applied_input_frame = 0`;
- `active_capture_job_id = None`;
- capabilities from the session.

Do not let status leak worker slot details.

## Unsupported Operations

For bp8, keep these returning `BackendUnavailable`:

- `inject_input`
- `framebuffer`
- `trigger_capture`
- `capture_job`

Downstream beads will replace those paths.
