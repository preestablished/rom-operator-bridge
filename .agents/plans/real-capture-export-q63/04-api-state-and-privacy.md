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
- Concurrent capture: existing `capture_in_progress` behavior.
- Idempotency replay: return the original job.

Public failure details must remain empty or sanitized.
