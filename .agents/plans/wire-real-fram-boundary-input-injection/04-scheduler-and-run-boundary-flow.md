# Scheduler and Run-Boundary Flow

## Real Paused State Is Input-Accepting

The current scheduler applies input only when `RunStatus.state == Running`.
That is correct for synthetic sessions, but real MVP sessions are paused between
short bounded frame steps.

Add an input-acceptance helper in `service/src/input/scheduler.rs`, for example:

```rust
fn status_accepts_input(status: &RunStatus) -> bool {
    match status.backend_mode {
        BackendMode::Synthetic => status.state == SessionState::Running,
        BackendMode::Real => status.state == SessionState::Paused && status.capabilities.input,
    }
}
```

Then use it in:

- `InputScheduler::submit`
- `InputScheduler::flush_pending`
- `InputScheduler::apply_with_status`
- stale retry handling

Do not make synthetic paused sessions apply input immediately; keep their
existing queued behavior.

For real sessions, choose this policy:

- paused + input capability: apply immediately to the worker pending-input queue;
- running: reject with the existing sanitized input rejection shape;
- paused without input capability: reject rather than queue.

This keeps the real MVP explicit: input is accepted only at a boundary before
the next bounded run step.

## Pending Input and Resume

For real sessions, normal paused websocket input applies immediately, so the
usual path does not create queued real input. Keep pre-resume flushing as a
safety net for any already-pending entries, replacement-session edge cases, or
future UI changes that intentionally queue before resume.

Update `run_state_transition` for `RunTransition::Resume`:

1. Before calling `backend.resume`, call `ws_input.flush_pending(...)` with a
   real private rejection sink, not `NoopInputRejectionSink`.
2. Call `backend.resume`.
3. After resume, keep the existing flush call or restrict the post-resume flush
   to synthetic behavior.
4. Publish the boundary event after the backend returns.

With the real paused acceptance helper, pre-resume flush will schedule any
pending real input while the slot is paused. The subsequent bounded `Run` can
then pick up the worker's pending input at call start.

Guard against duplicate application:

- `flush_pending` must remove successfully applied queued entries.
- the existing scheduler run-state duplicate checks should remain in force.
- tests must prove a pending real input is scheduled once, not once before and
  once after resume.

## Direct Websocket Input While Paused

When a real session is paused and input capability is granted, direct websocket
input should schedule immediately for the next frame boundary and return an
`input_ack` with:

```json
{
  "status": "applied",
  "assigned_frame": current_frame + 1,
  "pad_word": <validated word>
}
```

The ack means "accepted by the hypervisor input queue", not "the ROM has already
consumed it." Keep that distinction in tests and docs.

## Input During Real Run

Prevent input from being injected behind an already-started real `Run`.

Recommended backend guard:

- set `RealSession.state = Running` immediately before sending the worker
  `Run` command;
- restore `Paused` after the worker returns with a boundary result;
- set `Faulted` on faulted run results.

The real input scheduler helper should not treat real `Running` as
input-accepting. A websocket input received during the short run should be
rejected with the existing sanitized input rejection, because the run should be
brief and the browser can send the next state at the next boundary.

If the worker `Run` call fails after the backend marks the session `Running`, do
not leave public status stuck at `running`. Required cleanup:

- set the session to `Faulted` or remove it from active;
- append a private cleanup/fault event best effort;
- return sanitized `BackendUnavailable`;
- require a test where worker `Run` fails and subsequent input is not accepted
  as if the VM were still safely paused.

## Stale Retry Flow

Expected stale path:

```text
status returns current_frame = F
scheduler assigns target_frame = F + 1
worker rejects because actual frame counter is already >= F + 1
real backend refreshes current_frame to F'
backend returns BackendError::FrameStale
scheduler refreshes status and assigns F' + 1
second InjectInputs succeeds or the scheduler records dropped frame_stale
```

Do not convert stale `InvalidArgument` into public `backend_unavailable` unless
the request shape was not a bridge-constructed `at_frame + PadSet` event.

## Private Rejection Sink

The current websocket input path uses `NoopInputRejectionSink`. This bead should
replace that in runtime paths with a private artifact sink.

Required shape:

- websocket submit should record scheduler drops through
  `PrivateArtifactStore::append_input_rejection`;
- API `flush_pending` should use the same private rejection sink;
- tests may keep `NoopInputRejectionSink` only for isolated unit cases where
  private artifacts are irrelevant.

Suggested implementation:

- pass `BridgePrivateConfig` or a small rejection-sink factory into
  `serve_input_socket`;
- change `WsInputState::flush_pending` to accept a rejection sink or private
  config from `api.rs`;
- construct `PrivateArtifactStore::new(state.config.private_config())` at the
  API boundary where the private config is already available.

If writing the rejection row fails, return sanitized backend unavailable for API
flush paths and sanitized input rejection for websocket submit paths. Do not
drop stale-input privacy evidence silently.

## API Response State

`RunStateResponse` can continue returning the boundary returned by
`backend.resume`.

For real bounded runs that stop at a boundary, that boundary should remain
`Paused` after the run completes. Do not report a long-lived public `running`
state unless the backend really leaves the VM running.
