# Acceptance Audit

Use this file as the final implementation audit for `rom-operator-bridge-3dr`.
Record concrete evidence in the bead notes before closing the bead.

## Code Behavior

Real backend:

- `RealBackend::capabilities()` advertises input support for the real MVP.
- real session start stores only the granted input capability.
- `RealBackend::inject_input()` rejects ungranted input capability.
- `RealBackend::inject_input()` accepts real boundary-paused sessions.
- `RealBackend::inject_input()` does not inject while a bounded `Run` is already
  in progress.
- worker `InjectInputs` uses the private lease.
- worker request uses `ScheduledEvent.at_frame`.
- worker request uses `PadSet.port = 0`.
- worker request uses `PadSet.buttons = pad_word as u32`.
- worker request rejects `target_frame >= u32::MAX` locally before tonic.
- worker request never uses `frames_elapsed` as a frame base.
- stale worker rejection maps to `BackendError::FrameStale`.
- stale retry refreshes or updates the frame base before the retry target is
  computed.
- worker `FAILED_PRECONDITION` and unavailable paths stay sanitized as
  `backend_unavailable`.
- scheduled count must be exactly `1`.
- padlog artifacts are written only after worker acceptance.
- artifact failure after worker acceptance quarantines or removes the session.
- public status `last_applied_input_frame` updates after successful persistence.

Scheduler/API:

- synthetic paused input still queues.
- real paused input is scheduled for the next boundary.
- real running input is rejected with sanitized public shape.
- any pending real input is flushed before the bounded resume run.
- duplicate client sequence replay returns the original ack.
- pre- and post-resume flushes cannot duplicate one pending input.
- stale-after-retry returns the existing public input rejection shape.
- stale-after-retry writes a private input rejection row in runtime paths.

Privacy:

- public websocket replies contain no lease tokens, private roots, worker
  endpoints, snapshot refs, or raw worker status.
- private padlog rows contain frame and pad word data only.
- private rejection rows do not leak worker messages.
- redaction gate passes with operator canaries.

## Required Commands

Minimum targeted commands:

```bash
(cd service && cargo fmt --check)
(cd service && cargo test --test input_scheduler)
(cd service && cargo test --test ws_input)
(cd service && cargo test --test real-backend)
(cd service && cargo test --test padlog)
```

Full service gate:

```bash
(cd service && cargo test)
```

Privacy gate:

```bash
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> bash scripts/redaction-gate.sh
```

## Bead Closeout

Before closing `rom-operator-bridge-3dr`, update the bead with:

- worker request evidence from tests;
- stale retry evidence;
- padlog artifact evidence;
- websocket/API behavior evidence;
- redaction gate evidence;
- whether live real-host input smoke was run or deferred to `0wo`.

Repository closeout:

```bash
git pull --rebase
bd dolt push
git push
git status
```

The final `git status` must show the branch up to date with origin.
