# Hypervisor Contract and Frame Base

## Worker Request Shape

`RealBackend::inject_input` must translate one `InputScheduleRequest` into one
worker scheduled event:

```rust
dh::InjectInputsRequest {
    lease: Some(session.lease.clone()),
    events: vec![dh::ScheduledEvent {
        at: Some(dh::scheduled_event::At::AtFrame(target_frame_u32)),
        event: Some(dh::scheduled_event::Event::PadSet(dh::PadSet {
            port: 0,
            buttons: u32::from(request.pad_word.raw()),
        })),
    }],
}
```

The bridge owns this mapping. There is no reference-workload helper that
converts padlog rows into hypervisor events.

## Frame Counter Sources

Only these sources may update the real session's authoritative frame base:

- `RestoreSnapshotResponse.frame_counter`
- `CreateVmResponse` implies frame `0`
- `GetFramebufferResponse.frame_counter`
- `TakeSnapshotResponse.frame_counter`
- `RunResponse.fb_info.frame_counter` when a capture request returns it

`RunResponse.frames_elapsed` must not update `current_frame`.

## Unknown Frame Base

After a real bounded `Run` without `fb_info.frame_counter`, the session may have
advanced but the bridge does not know the new absolute frame counter.

Represent that explicitly in `RealSession`, for example:

```rust
frame_base_known: bool,
```

Suggested semantics:

- start from snapshot: `frame_base_known = true`
- create VM: `frame_base_known = true` with `current_frame = 0`
- successful framebuffer refresh: `frame_base_known = true`
- successful snapshot refresh: `frame_base_known = true`
- bounded run without `fb_info`: `frame_base_known = false`
- bounded run with `fb_info`: update frame and keep `frame_base_known = true`

Do not expose `frame_base_known` directly in public JSON. Public staleness should
stay conservative through existing `preview_stale` behavior.

## Refresh Strategy

Before sending `InjectInputs`, if `frame_base_known` is false, refresh the frame
base while the slot is paused. Prefer a helper on the worker thread such as:

```rust
RealWorkerThread::frame_counter(lease) -> RealWorkerResult<RealFrameCounterOutcome>
```

The implementation may use one of these worker calls:

- `GetFramebuffer`, reusing the 0i9 path but discarding pixels after reading
  `frame_counter` and `icount`;
- `TakeSnapshot { seal_input_log: Some(false), capture: None }`, reading
  `frame_counter` and `icount`.

Use `GetFramebuffer` first if it is reliable for the target runtime because it
does not create a snapshot. Fall back to `TakeSnapshot` only if the implementer
confirms the side effects are acceptable for this bridge session and tests cover
the artifact/privacy behavior.

If no refresh source is available, return `BackendError::BackendUnavailable` for
operator-visible failures or `BackendError::FrameStale` for the scheduler retry
path when the worker specifically rejected a stale frame.

## Stale Retry Contract

The scheduler already retries once when `backend.inject_input` returns:

```rust
BackendError::FrameStale {
    requested_frame,
    current_frame,
}
```

Make the real backend feed that path:

- worker `Code::InvalidArgument` from `InjectInputs` should map to
  `FrameStale` only for the bridge-constructed frame request shape;
- after a stale rejection, mark `frame_base_known = false`;
- refresh the frame base before or during the stale error handling when possible;
- update `current_frame` before returning `FrameStale` so the scheduler's retry
  status sees a newer base.

If the retry is stale again, the existing scheduler will write a private dropped
input rejection and return a public `Input rejected.` message.

## Bounds

The hypervisor `at_frame` field is `u32`. Validate before constructing the
request.

Recommended behavior:

- if `request.target_frame >= u32::MAX`, reject locally before calling the
  worker;
- `u32::MAX` is the reserved `FRAME_HINT_NONE` sentinel and must never be sent
  as `ScheduledEvent.at_frame`;
- treat local target-frame bounds failures as sanitized unavailable unless the
  implementer adds a narrow internal backend error that preserves public
  sanitization;
- never wrap or truncate the target frame.

The browser-facing scheduler still uses `FrameCounter = u64`; the real backend
is responsible for safely crossing into the hypervisor wire type.

This validation is also what makes worker `InvalidArgument` safe to interpret as
stale for the bridge-constructed request shape. Without the local
`FRAME_HINT_NONE` guard, a bridge bug could be misclassified as stale input.
