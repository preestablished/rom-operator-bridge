# API State And Privacy

## 1. Align API-Owned Capture Records

`service/src/api.rs` already keeps `CaptureState` and writes recent captures and
label drafts for completed API-owned jobs. Verify that real backend completion
flows through the same path as synthetic completion.

Required behavior:

- For `BackendMode::Real`, `POST /api/capture/trigger` must call
  `state.backend.trigger_capture(CaptureRequest { session_id, idempotency_key })`
  instead of completing through synthetic/API-owned capture logic.
- For `BackendMode::Real`, `GET /api/capture/jobs/:job_id` must call
  `state.backend.capture_job(job_id)` for real jobs and then upsert only
  sanitized public projection state.
- The backend owns real payload and `captures/index.jsonl` durability.
  `CaptureState` owns public recent/detail/labelability projection.
- `POST /api/capture/trigger` returns a public job id and `requested` or
  terminal status.
- `GET /api/capture/jobs/:job_id` returns `completed` only after the backend has
  completed private writes.
- `GET /api/capture/recent` lists only sanitized capture summaries.
- `GET /api/capture/:capture_id` never exposes private payload paths or raw
  capture bytes.

If backend-owned artifact writing and API-owned recent capture writing would
duplicate writes, make one layer authoritative. Prefer keeping raw payload and
`captures/index.jsonl` writes in the backend/private artifact layer, while API
state owns public projections and label draft updates.

Extend `CaptureState` with a real capture projection that stores only
public-safe metadata. Real capture detail must use `capture_source = "real"` or
an approved sanitized tool class, public hashes only, and must not store raw
payload bytes, decoded feature bytes, private refs, private paths, or screenshots
in API state.

Make the route branch explicit:

1. Authenticate and validate the active session as today.
2. Check real backend capabilities and active preview/session status.
3. If `BackendMode::Real`, call `BridgeBackend::trigger_capture`.
4. Map backend job statuses to API job statuses.
5. Upsert only `RealCapturePublicProjection` fields after backend completion.
6. Publish the existing sanitized `capture_updated` event.

The real branch must not call synthetic `CaptureState::trigger`,
`complete_capturing_job`, synthetic feature generation, or synthetic preview
generation.

`RealCapturePublicProjection` should contain only:

- public job id;
- API job status;
- public capture id;
- frame counter or frame index;
- `capture_source = "real"` or an approved sanitized tool class;
- public layout/provenance hashes or classes;
- `has_preview`;
- `features_available`;
- labelability state.

Build this projection from explicit backend-public output. Never build it by
copying private index rows, payload refs, worker responses, or manifest paths
into API state.

For `/api/capture/:capture_id/preview` and capture feature endpoints, real
captures must return no preview/features unless q63 also implements an approved
public-safe derivative. The default q63 behavior should be:

- preview unavailable, with no screenshot or framebuffer bytes exposed;
- features response reports `available = false` or the existing equivalent;
- capture detail may still report public hashes/classes and labelability.

Add route tests for these real-mode responses.

## 2. Preserve Labelability Contract

Labels should work against completed real capture ids exactly as synthetic
capture ids do:

- A real capture becomes labelable only after the durable private index append
  succeeds and the API public projection is upserted.
- `LabelState::apply` should accept real capture ids that belong to the active
  run.
- `needs_review` should write `captures/<capture-id>/label-draft.json`.
- Label notes remain private.
- Stale capture ids from prior sessions remain rejected.

Do not weaken `is_labelable_capture` validation to make tests pass.

`captures/<capture-id>/label-draft.json` is bridge-local UI state. It is not the
reference workload Phase 4 trace-label YAML artifact, and q63 must not claim
trace-label compatibility unless that separate artifact writer is implemented.

## 3. Websocket Events

Keep existing `capture_updated` event shape:

- `job_id`;
- public `status`;
- optional public `capture_id`.

No event should contain raw payload details, worker endpoints, artifact paths,
private refs, or worker stderr.

## 4. Sanitizer Coverage

Extend tests so public bodies are checked with `PublicSanitizer` configured with:

- private root;
- worker endpoint;
- capture spec ref;
- real snapshot or CreateVm config ref;
- payload filenames if they include private values;
- representative worker error text.

Use `inspect_json` or equivalent structured checks. Avoid substring-only checks
as the only proof when typed sanitizer helpers exist.

## 5. Failure Mapping

Map failures consistently:

- Missing/invalid real capture config: `backend_unavailable`.
- Worker capture RPC unavailable: `backend_unavailable` or sanitized capture job
  failed state, depending on where failure occurs.
- Private artifact write failure before completion: failed job or
  `backend_unavailable`, but never completed.
- Payload-written/index-failed: no public projection, no labelability, and no
  duplicate index row on idempotent replay.
- Concurrent capture: existing `capture_in_progress` behavior.
- Idempotency replay: return the original job.

Public failure details must remain empty or sanitized.
