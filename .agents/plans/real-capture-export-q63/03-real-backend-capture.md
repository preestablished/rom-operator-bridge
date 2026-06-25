# Real Backend Capture

## 1. Extend Real Backend Capabilities

`BackendCapabilities::real_input_preview_mvp()` currently sets `capture: false`.
Add a real capture capability constructor or update the real capability selected
after config validation so real sessions advertise:

```text
input=true
preview=true
capture=true
labels=true
```

Only enable this when the real runtime config has all capture prerequisites.
If `BRIDGE_CAPTURE_SPEC_REF` is absent or invalid, fail closed.

## 2. Add Real Capture State

Extend `RealBackendInner` and `RealSession` with the minimum state required for
idempotent real capture jobs:

- active capture job id;
- capture id;
- idempotency key mapping scoped to session id;
- status: pending/running/completed/failed;
- sanitized failure code;
- payload/index write completion flag.

Keep raw worker errors private. Public job failure responses should use existing
sanitized error envelopes.

## 3. Add Worker Command For Capture

Extend the real worker command enum and worker thread to call the chosen
hypervisor capture RPC using:

- active `Lease`;
- configured `BRIDGE_CAPTURE_SPEC_REF`;
- current frame/boundary information if required by the RPC;
- private runtime inputs from `RealRuntimeConfig`.

Implementation constraints:

- Do not call `RunWithFrameCapture` unless preflight proved it is implemented
  and returns enough schema data. Prefer the implemented `Run` or `TakeSnapshot`
  capture path with `capture: Some(spec)` on the active lease.
- Do not hold the real backend mutex during blocking gRPC calls.
- Convert tonic/transport failures to `BackendUnavailable`.
- Never include endpoint paths, refs, or worker error text in `BackendError`.
- If the capture RPC streams events, consume until terminal success/failure or
  timeout.

## 4. Trigger Capture Lifecycle

Implement `RealBackend::trigger_capture`:

1. Validate there is an active real session matching the request session id.
2. Enforce one active capture per session.
3. Return the existing job for the same `(session_id, idempotency_key)`.
4. Create a stable public job id and capture id.
5. Call the worker capture command.
6. Write private payloads and index artifacts durably.
7. Mark the job `Completed` only after durable writes succeed.
8. Update active session status so `active_capture_job_id` clears after terminal
   status.

If the worker or private writer fails after job creation, store a failed job
with sanitized failure status. Do not leave a permanent active capture lock.
The stored job must retain enough backend-private state for `capture_job` polling
without exposing payload refs or worker errors.

## 5. Capture Job Lookup

Implement `RealBackend::capture_job` so API polling can observe terminal job
state. The returned `CaptureJob` must contain only:

- public job id;
- public status;
- public capture id when complete.

It must not include payload refs, private paths, raw bytes, capture spec refs, or
worker error strings.

## 6. Session Cleanup

On stop or fault cleanup:

- clear active capture state;
- do not delete already completed private capture artifacts;
- ensure subsequent sessions cannot label stale captures from the previous
  active session unless the API capture state explicitly still owns them.
