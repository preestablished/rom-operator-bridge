# Real Frame-Boundary Input Injection Plan for 3dr

## Goal

Finish `rom-operator-bridge-3dr` by wiring browser pad words to the
authoritative hypervisor `InjectInputs` path for real backend sessions.

The completed behavior should let an authenticated operator send websocket
input for a real session and have the bridge:

```text
validate browser pad word
choose a future absolute pv-pad frame
send HypervisorWorker.InjectInputs
record the applied input in private padlog artifacts
run the next bounded frame step without duplicate application
```

## Current State

The synthetic input path is already implemented:

- `service/src/ws_input.rs` parses and validates browser input envelopes.
- `service/src/input/scheduler.rs` assigns `current_frame + lead_frames`,
  preserves ordering, retries once on `BackendError::FrameStale`, and records
  dropped inputs through an `InputRejectionSink`.
- `SyntheticBackend::inject_input` writes private padlog artifacts only after an
  accepted backend receipt.

The real backend lifecycle is partially wired:

- `RealBackend` stores the private worker lease and slot id through the lease.
- real start, stop, pause, bounded `Run`, status, and framebuffer preview are
  implemented.
- `RealBackend::inject_input` still returns `BackendUnavailable`.
- real `resume()` runs one frame budget and returns a paused boundary; the real
  worker slot is paused between frame steps.

## Source Contracts

Use these contracts as authoritative:

- `docs/hypervisor-runtime-contracts.md`, `One Pad Word To A Running ROM` and
  `Frame Bases`
- `docs/bridge-discovery-note.md`, `Hypervisor input API`
- `contracts/backend-traits.md`, backend input expectations
- `../determinism-hypervisor/proto/hypervisor.proto`, `InjectInputsRequest`,
  `ScheduledEvent`, and `PadSet`
- `../determinism-hypervisor/crates/dh-worker/src/service.rs`,
  `queued_input_from_proto`

Frozen facts:

- Player 1 maps to `PadSet.port = 0`.
- `PadSet.buttons = pad_word as u32`.
- `ScheduledEvent.at_frame` is an absolute pv-pad `FRAME_COUNTER`, not a
  segment-relative count.
- The bridge must schedule `at_frame > current_frame_counter`.
- The MVP lead-frame policy is `lead_frames = 1`.
- Do not derive an absolute frame counter from `RunResponse.frames_elapsed`.
- `InjectInputs` is not live mid-run control; it must be queued before the
  bounded `Run` that should consume it.
- Worker `INVALID_ARGUMENT` for stale frame input must feed the scheduler's
  stale retry path.
- Worker `FAILED_PRECONDITION`, connection failures, lease failures, and private
  worker messages must remain sanitized as public `backend_unavailable`.

## Implementation Shape

Implement in four narrow layers:

1. Real backend capability and session state.
2. Real worker `InjectInputs` command and stale/error mapping.
3. Scheduler/API flow for real boundary-paused sessions.
4. Private padlog persistence and regression coverage.

Avoid async-trait changes. Reuse the existing real worker thread and blocking
reply channel pattern in `service/src/backend.rs`.

## Expected File Touches

Primary files:

- `service/src/backend.rs`
- `service/src/input/scheduler.rs`
- `service/src/api.rs`
- `service/tests/real-backend/main.rs`
- `service/tests/input_scheduler/main.rs`
- `service/tests/ws_input/main.rs`

Likely supporting files:

- `contracts/backend-traits.md`
- `docs/real-backend-smoke.md`

Avoid unrelated UI, capture, label, validation, and framebuffer refactors.

## Non-Goals

Do not implement real capture or label smoke. Those remain separate beads.

Do not implement long-running live input streaming. The MVP is explicit
frame-boundary injection before short bounded runs.

Do not expose worker request details, lease tokens, socket paths, private
artifact paths, snapshot refs, or raw worker status messages through public API
responses, websocket replies, docs, or test snapshots.
