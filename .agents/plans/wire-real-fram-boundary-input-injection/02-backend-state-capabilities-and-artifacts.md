# Backend State, Capabilities, and Artifacts

## Capabilities

Change the real backend advertised capability shape from preview-only to
input-plus-preview:

```rust
impl BackendCapabilities {
    pub const fn real_input_preview_mvp() -> Self {
        Self {
            input: true,
            preview: true,
            capture: false,
            labels: false,
            privileged_features: false,
            validation_runner: false,
        }
    }
}
```

Use this from `RealBackend::capabilities()`.

Keep capability filtering where it is today:

- `RealBackend::capabilities()` reports what real MVP supports.
- `/api/session/start` intersects requested and supported capabilities.
- `RealSession.capabilities` stores the granted capabilities passed in
  `StartBackendSession.requested_capabilities`.

`RealBackend::inject_input` must reject sessions where `capabilities.input` is
false.

## Real Session Fields

Extend `RealSession` with state needed for input persistence and frame-base
handling:

```rust
frame_base_known: bool,
applied_inputs: Vec<AppliedInputFrame>,
```

The existing fields remain important:

- `current_frame`
- `current_icount`
- `last_applied_input_frame`
- `capabilities`
- private `lease`

Do not store the lease token in public artifacts.

## Accepting Input

For real MVP, the slot is normally paused at a frame boundary between short
bounded runs. Treat that paused boundary state as the input-accepting state for
real sessions.

`RealBackend::inject_input` should require:

- active session id matches;
- granted input capability is true;
- session state is `Paused`;
- frame target is a valid future `u32` frame;
- worker schedules exactly one event.

Do not accept input while a real bounded `Run` is already in progress. See
`04-scheduler-and-run-boundary-flow.md` for the state-transition guard.

## Private Padlog Writes

After the worker confirms `scheduled == 1`, persist the same private padlog
artifacts that synthetic uses:

- append or rewrite the session padlog snapshot using `PadLog::from_applied_frames`;
- append one `PadLogEventRow`;
- update `last_applied_input_frame`;
- push the applied frame into `RealSession.applied_inputs`.

Prefer extracting a shared helper from the synthetic implementation rather than
duplicating fragile private-write ordering.

The write order must remain fail-closed:

1. Clone previous applied input state.
2. Build the new applied input vector.
3. Write private artifacts.
4. Only then mutate `RealSession.applied_inputs` and
   `last_applied_input_frame`.

If artifact writing fails after the worker accepted the input, fail closed more
strongly than a normal backend error. At that point the worker has a queued input
that may later be consumed without a matching private padlog row.

Required quarantine behavior:

- mark the real session `Faulted` or remove it from `active`;
- prevent any later `resume()` from running the unlogged queued input;
- best-effort `DestroyVm` the private lease if the session is removed;
- append a private bridge event such as `input_artifact_failed` or
  `cleanup_failed` only through safe existing private helpers;
- return sanitized `BackendUnavailable` publicly.

Do not leave the session paused and runnable after post-worker artifact failure.
That would let a retry duplicate input or let the original queued input land
without a durable private padlog row.

## Input Receipt

Return:

```rust
InputScheduleReceipt {
    session_id: request.session_id,
    assigned_frame: request.target_frame,
    pad_word: request.pad_word,
}
```

Only return the receipt after both worker scheduling and private artifact writes
succeed.

## Status Updates

`RunStatus.last_applied_input_frame` must reflect the last successfully
persisted real input frame.

`RunStatus.current_frame` must remain the latest authoritative frame base known
to the bridge. Do not advance it to `request.target_frame` just because an input
was scheduled for that future frame; the guest has not necessarily consumed it
yet.
