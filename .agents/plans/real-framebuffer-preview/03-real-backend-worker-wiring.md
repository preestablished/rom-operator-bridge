# Real Backend Worker Wiring

## Capabilities

Add a real-preview capability shape instead of continuing to advertise all real
capabilities as false after this bead.

Recommended:

```rust
impl BackendCapabilities {
    pub const fn real_preview_mvp() -> Self {
        Self {
            input: false,
            preview: true,
            capture: false,
            labels: false,
            privileged_features: false,
            validation_runner: false,
        }
    }
}
```

Use it for `RealBackend::capabilities()`. Synthetic capabilities must not
change.

Be precise about where capabilities are filtered:

- `RealBackend::capabilities()` reports supported real backend capabilities.
- `/api/session/start` already computes granted capabilities from the requested
  capabilities and backend-supported capabilities.
- `RealSession.capabilities` should store the granted capabilities carried in
  `StartBackendSession.requested_capabilities`, not blindly store every
  supported backend capability.

Add a test where real start does not request preview and later status/events
keep `preview: false`.

## Worker Command

Extend the existing real worker command loop in `service/src/backend.rs`.

Add:

```rust
RealWorkerCommand::Framebuffer {
    lease: dh::Lease,
    reply: mpsc::Sender<RealWorkerResult<RealFramebufferOutcome>>,
}

struct RealFramebufferOutcome {
    frame: FrameCounter,
    icount: u64,
    width: u32,
    height: u32,
    png_bytes: Vec<u8>,
}
```

Add `RealWorkerThread::framebuffer(lease)` and a matching
`RealWorkerState::framebuffer(lease).await`.

Inside `RealWorkerState::framebuffer`:

1. Ensure the tonic client is connected.
2. Call `get_framebuffer(dh::GetFramebufferRequest { lease: Some(lease) })`.
3. Map tonic status failures to `RealWorkerFailure::BackendUnavailable`.
4. Convert `response.format` into the local raw framebuffer format.
5. Validate and convert `response.pixels` to PNG using `service/src/framebuffer.rs`.
6. Return only PNG bytes, dimensions, `frame_counter`, and `icount`.

Do not send raw worker status text or conversion detail through
`BackendError`.

## Backend Method

Implement `RealBackend::framebuffer`.

Recommended flow:

1. Lock `RealBackendInner`.
2. Clone the active session if `session_id` matches.
3. Reject missing, faulted, or non-paused sessions with `BackendUnavailable`.
4. Drop the lock.
5. Call `self.worker.framebuffer(session.lease.clone())`.
6. Reacquire the lock and confirm the same session is still active.
7. Update:
   - `active.last_preview_frame = outcome.frame`;
   - `active.current_frame = active.current_frame.max(outcome.frame)`;
   - `active.current_icount = outcome.icount`.
8. Return `FramePreview { session_id, frame, width, height, png_bytes }`.

Do not hold the backend mutex while blocking on the worker thread.

If the worker returns unavailable, keep the active session intact unless status
or another lifecycle method proves the slot is gone or faulted. Preview
unavailability should not destroy an otherwise valid session.

## Status Interaction

`RealBackend::status` currently updates `current_icount` from `ListSlots`, but
the slot info does not include a frame counter. Leave `current_frame` unchanged
there unless the hypervisor API grows a frame field.

`RealBackend::framebuffer` is the authoritative update point for
`last_preview_frame` and the latest known preview frame.

Do not leave real preview freshness tied only to
`last_preview_frame < current_frame`. After any real `Run` that may advance
frames without `fb_info.frame_counter`, the old preview must become stale or
unknown according to `docs/hypervisor-runtime-contracts.md`.

Preferred implementation:

- add an internal `preview_stale` or `preview_unknown` flag to `RealSession`;
- extend internal `RunStatus` so API and websocket payloads use that explicit
  boolean instead of recomputing freshness only from counters;
- synthetic status can keep computing freshness from counters;
- real `framebuffer()` clears the flag after a successful `GetFramebuffer`;
- real `resume()` sets the flag when `RunResponse.fb_info` is absent or a
  post-run framebuffer refresh fails.

Alternative implementation:

- after real `Run` without `fb_info`, immediately call `GetFramebuffer` while
  the slot is paused to refresh the authoritative frame counter;
- if that refresh fails, still mark preview stale/unknown through an explicit
  internal flag.

Do not compute a new absolute current frame by adding `frames_elapsed`; bp8
intentionally avoided that because the hypervisor contract says frame counters
come from framebuffer/pv-pad boundary data.

The existing API route calls `status` before `framebuffer`, then computes stale
using the status snapshot. This still works:

- if the preview frame is behind the status frame, the response is stale;
- if the preview reports a newer frame, the response is fresh and backend state
  will be up to date for later status calls.

If implementation changes this ordering, update tests to prove stale responses
are still conservative.

## Worker Failure Mapping

Keep public failures sanitized as `backend_unavailable`.

Cases that must map to sanitized unavailable:

- worker connection failure;
- missing lease in request construction;
- `GetFramebuffer` tonic status, including `FAILED_PRECONDITION`;
- unsupported pixel format;
- invalid dimensions, stride, or pixel length;
- conversion overflow;
- session disappeared while waiting for the worker reply.

Do not add a public `framebuffer_invalid` or `worker_failed_precondition` error
for this bead.
