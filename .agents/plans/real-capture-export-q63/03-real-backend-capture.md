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

Capture prerequisites include a privately resolved `dh::CaptureSpec`, verified
`layout.json` metadata, feature-map order/decoder metadata, and a private root
that can write payloads and append `captures/index.jsonl`.

## 2. Add Real Capture State

Extend `RealBackendInner` and `RealSession` with the minimum state required for
idempotent real capture jobs:

- active capture job id;
- capture id;
- idempotency key mapping scoped to session id;
- status: pending/running/completed/failed;
- sanitized failure code;
- payload/index write completion flag.
- public projection fields needed by the API, excluding private refs.

Keep raw worker errors private. Public job failure responses should use existing
sanitized error envelopes.

## 3. Add Worker Command For Capture

Extend the real worker command enum and worker thread to call the chosen
hypervisor capture RPC using:

- active `Lease`;
- privately resolved `dh::CaptureSpec`;
- current frame/boundary information if required by the RPC;
- private runtime inputs from `RealRuntimeConfig`.

Implementation constraints:

- Do not call `RunWithFrameCapture`; the local worker path is not implemented
  for q63.
- Call `TakeSnapshot` with `capture: Some(spec)` on the active lease.
- Set `seal_input_log = Some(true)` explicitly. Persist returned snapshot/log
  identifiers only in private backend state or private manifests, and update the
  active session state from the returned snapshot, frame counter, icount, vns,
  state hash, machine config hash, and determinism class as needed.
- Do not hold the real backend mutex during blocking gRPC calls.
- Convert tonic/transport failures to `BackendUnavailable`.
- Never include endpoint paths, refs, or worker error text in `BackendError`.
- Treat empty `feature_bytes`, missing `fb_info`, empty `fb_lz4`, or a
  framebuffer metadata mismatch as sanitized capture failure.

## 4. Trigger Capture Lifecycle

Implement `RealBackend::trigger_capture`:

1. Validate there is an active real session matching the request session id.
2. Enforce one active capture per session.
3. Return the existing job for the same `(session_id, idempotency_key)`.
4. Create a stable public job id and capture id.
5. Call the worker capture command with `TakeSnapshot`.
6. Decode feature values, write private payloads, and append index artifacts
   durably.
7. Mark the job `Completed` only after durable writes succeed.
8. Upsert a public-safe projection record for the API.
9. Update active session status so `active_capture_job_id` clears after terminal
   status.

If the worker or private writer fails after job creation, store a failed job
with sanitized failure status. Do not leave a permanent active capture lock.
The stored job must retain enough backend-private state for `capture_job` polling
without exposing payload refs or worker errors.

Idempotent replay must return the original job and must not call the worker,
rewrite payloads, or append a duplicate index row after completion.

## 5. Capture Job Lookup

Implement `RealBackend::capture_job` so API polling can observe terminal job
state. The returned `CaptureJob` must contain only:

- public job id;
- public status;
- public capture id when complete.

It must not include payload refs, private paths, raw bytes, capture spec refs, or
worker error strings.

Recommended backend-private/public split:

```text
RealCaptureJobState
  private: lease token/slot, snapshot/log refs, payload refs, worker diagnostics
  public: RealCapturePublicProjection

RealCapturePublicProjection
  job_id
  status
  capture_id
  frame_counter or frame_index
  capture_source = "real"
  layout_hash
  public provenance classes/hashes only
  has_preview
  features_available
  labelable
```

Build the projection from explicit public-safe backend output. Do not copy the
private `captures/index.jsonl` row into API state.

## 6. Session Cleanup

On stop or fault cleanup:

- clear active capture state;
- do not delete already completed private capture artifacts;
- ensure subsequent sessions cannot label stale captures from the previous
  active session unless the API capture state explicitly still owns them.
