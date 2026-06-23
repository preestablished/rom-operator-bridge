# Backend Trait Contract

Date: 2026-06-23
Schema version: 1

## Purpose

This file defines the service-side boundary that both `SyntheticBackend` and the
future real hypervisor backend must implement. It is a contract document until
`service/Cargo.toml` exists; the service scaffold must translate it into Rust
traits without weakening the privacy or fail-closed behavior.

## Common Types

```rust
pub const RUNTIME_API_SCHEMA_VERSION: u16 = 1;
pub const PAD_LAYOUT_ID: &str = "console16-12btn-v1";
pub const PAD_LAYOUT_VERSION: u16 = 1;

pub enum BackendMode {
    Synthetic,
    Real,
}

pub enum SessionState {
    Idle,
    Starting,
    Running,
    Paused,
    CapturePending,
    Stopping,
    Stopped,
    Faulted,
}

pub struct FrameCounter(pub u64);

pub struct PadWord(pub u16); // must be <= 0x0fff
```

All service/UI JSON payloads are validated against
`contracts/runtime-api.schema.json`. Any inbound payload whose
`schema_version` is not `1` is rejected with `bad_request`; any UI response or
event whose `schema_version` is not `1` is rejected client-side before state is
mutated.

## Backend Trait

```rust
pub trait BridgeBackend {
    fn mode(&self) -> BackendMode;
    fn capabilities(&self) -> BackendCapabilities;

    fn start_session(&mut self, request: StartBackendSession)
        -> Result<BackendSession, BackendError>;

    fn stop_session(&mut self, session_id: SessionId, reason: StopReason)
        -> Result<StoppedSession, BackendError>;

    fn status(&self, session_id: SessionId)
        -> Result<RunStatus, BackendError>;

    fn pause(&mut self, session_id: SessionId)
        -> Result<RunBoundary, BackendError>;

    fn resume(&mut self, session_id: SessionId)
        -> Result<RunBoundary, BackendError>;

    fn inject_input(&mut self, request: InputScheduleRequest)
        -> Result<InputScheduleReceipt, BackendError>;

    fn framebuffer(&mut self, session_id: SessionId)
        -> Result<FramePreview, BackendError>;

    fn trigger_capture(&mut self, request: CaptureRequest)
        -> Result<CaptureJob, BackendError>;

    fn capture_job(&self, job_id: CaptureJobId)
        -> Result<CaptureJob, BackendError>;
}
```

## Synthetic Backend Requirements

The synthetic backend must:

- use `BackendMode::Synthetic`;
- increment a fake `FrameCounter`;
- generate a deterministic browser-safe `image/png` preview;
- accept `PadWord` values only when reserved bits are clear;
- write or expose padlog frames through the same service artifact interface as
  the real backend will use;
- create synthetic capture ids that are visibly synthetic in API state;
- never claim synthetic capture as real Phase 4 acceptance.

## Real Backend Requirements

The real backend must:

- use `BackendMode::Real`;
- attach to the configured worker endpoint, default
  `unix:///run/dh/grpc.sock`;
- fail closed with sanitized `backend_unavailable` until private runtime config
  from `docs/real-backend-availability.md` is present and proven;
- own exactly one worker lease per bridge session;
- keep the worker lease token server-side only;
- schedule input with absolute `FRAME_COUNTER` values and `lead_frames = 1`;
- retry stale `InjectInputs` once after refreshing the frame base;
- mark preview stale whenever `preview_frame_counter < current_frame_counter`;
- return only sanitized status, counts, hashes, booleans, and operator-approved
  labels to browser APIs.

## Capture Boundary

`CaptureJob.status = "completed"` is legal only after the private capture writer
has fsynced payload files, appended and fsynced a `captures/index.jsonl` row, and
kept raw payload refs server-side. Until the writer exists, real capture export
work remains deferred by `docs/real-backend-availability.md`.

## Error Mapping

Backends must map internal failures to the public error codes in
`contracts/runtime-api.schema.json`:

```text
auth_rejected
origin_rejected
session_inactive
session_active_elsewhere
backend_unavailable
frame_stale
capture_in_progress
capture_failed
label_conflict
validation_failed
bad_request
```

Backend errors must not include credentials, private paths, worker lease tokens,
capture ids for private artifacts, decoded values, raw command stderr, or stack
traces in browser-visible fields.
