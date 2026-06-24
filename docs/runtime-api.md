# Runtime API And Backend Traits

Date: 2026-06-23
Agent: Codex / Ralph iteration 8

## Canonical Contract

The canonical shared UI/service schema is:

```text
contracts/runtime-api.schema.json
```

The service backend trait contract is:

```text
contracts/backend-traits.md
```

The Rust service scaffold and TypeScript UI scaffold must derive or validate
their public runtime payloads from `contracts/runtime-api.schema.json`. No
separate service-only or UI-only public type definitions may diverge from this
schema.

## Version Rule

Every JSON request, JSON response, and WebSocket message crossing the UI/service
boundary includes:

```json
{ "schema_version": 1 }
```

Service behavior:

- reject inbound HTTP JSON with a missing or non-`1` `schema_version` as
  `bad_request`;
- reject inbound WebSocket JSON with a missing or non-`1` `schema_version` using
  `input_reject` and a `bad_request` error envelope when a reply is possible;
- emit only `schema_version: 1` runtime payloads.

UI behavior:

- reject HTTP responses and WebSocket events whose `schema_version` is not `1`
  before mutating state;
- show a sanitized schema mismatch error instead of trying to coerce major
  versions.

## Routes

All private runtime routes must emit:

```text
Cache-Control: no-store
Pragma: no-cache
X-Content-Type-Options: nosniff
```

Routes frozen for MVP:

```text
GET  /health
GET  /api/session
POST /api/session/start
POST /api/session/stop
GET  /api/run/status
POST /api/run/pause
POST /api/run/resume
GET  /api/frame/current
GET  /api/frame/current/image
POST /api/capture/trigger
GET  /api/capture/jobs/<job_id>
GET  /api/capture/recent
GET  /api/capture/<capture_id>
GET  /api/capture/<capture_id>/preview
POST /api/labels
GET  /api/labels
```

Route-to-schema map:

| Route | Request | Success response | Notes |
| --- | --- | --- | --- |
| `GET /health` | none | `healthResponse` | Public liveness; no private paths. |
| `GET /api/session` | none | `sessionResponse` | Requires authenticated session cookie when active. |
| `POST /api/session/start` | `startSessionRequest` | `startSessionResponse` | Only route that accepts `operator_credential`. |
| `POST /api/session/stop` | `stopSessionRequest` | `stopSessionResponse` | Releases backend lease/slot when present. |
| `GET /api/run/status` | none | `runStatusResponse` | Runtime no-store headers required. |
| `POST /api/run/pause` | `sessionOnlyRequest` | `runStateResponse` | Real backend maps to `Pause`; synthetic mirrors state. |
| `POST /api/run/resume` | `sessionOnlyRequest` | `runStateResponse` | Real backend resumes on next bounded `Run`. |
| `GET /api/frame/current` | none | `frameCurrentResponse` | Metadata only. |
| `GET /api/frame/current/image` | none | PNG body | `Content-Type: image/png`; no-store; query `frame` is only a cache-busting hint. |
| `POST /api/capture/trigger` | `captureTriggerRequest` | `captureTriggerResponse` | Idempotent per session/idempotency key. |
| `GET /api/capture/jobs/<job_id>` | none | `captureJobResponse` | Job ids are service-local runtime ids, not private artifact refs. |
| `GET /api/capture/recent` | query `limit`, `cursor` | `captureRecentResponse` | `limit` defaults to 50 and maxes at 200. |
| `GET /api/capture/<capture_id>` | none | `captureDetailResponse` | Browser-safe detail only. |
| `GET /api/capture/<capture_id>/preview` | none | PNG body | `Content-Type: image/png`; no-store; never an artifact ref. |
| `POST /api/labels` | `labelsRequest` | `labelsResponse` | Private notes may be submitted but are stored server-side. |
| `GET /api/labels` | none | `labelsSnapshotResponse` | Typed target labels, status labels, and dedup groups. |

All non-2xx JSON responses use `errorEnvelope`.

WebSocket channels:

```text
/ws/input
/ws/events
```

## Runtime API Deviations From Planning Source

Phase 0 freezes these deviations from the initial
`11-runtime-api-contract.md` source:

- Public paths are rooted at `https://rombridge.birb.homes/`, so preview URLs are
  `/api/...`, not `/rom-bridge/api/...`.
- Preview freshness is stricter than the planning-time 120-frame or 2-second
  threshold. For MVP, any preview with
  `preview_frame_counter < session.current_frame_counter` is stale because the
  real backend exposes boundary samples rather than live streaming.
- Real backend mode may be represented in capabilities but must fail closed with
  sanitized `backend_unavailable` until the prerequisites in
  `docs/real-backend-availability.md` are available.
- `HttpOnly; Secure; SameSite=Strict` cookie auth is the selected MVP transport;
  credentials are never accepted in URLs.
- The planning-time privileged decoded-feature route is not part of schema
  version 1. Browser runtime APIs must not expose decoded feature arrays.

## Auth And Session

- `POST /api/session/start` is the only route that accepts
  `operator_credential`.
- Successful session start sets an `HttpOnly; Secure; SameSite=Strict` cookie
  scoped to `/`.
- WebSocket handshakes authenticate with the same cookie.
- Runtime routes reject missing, expired, or mismatched sessions.
- Default session TTL is 4 hours.
- MVP allows one active operator session.
- Browser `Origin` is checked before cookie/session acceptance for runtime HTTP
  and WebSocket requests; absent, `null`, and wrong origins are rejected unless a
  future non-browser admin path explicitly says otherwise.
- Credentials are never accepted in query strings or URLs.

## Common Error Envelope

All non-2xx JSON errors use the schema's `errorEnvelope` shape:

```json
{
  "schema_version": 1,
  "error": {
    "code": "backend_unavailable",
    "message": "Backend unavailable.",
    "retryable": true,
    "details": {}
  }
}
```

Browser-visible `details` must stay sanitized. Do not include credentials,
private paths, worker lease tokens, artifact refs, decoded arrays, command
stderr, or stack traces.

## WebSocket Rules

All WebSocket messages use the discriminated `wsEnvelope` union from the schema.
The `type` field selects the only legal `payload` schema.

Input channel rules:

- `client_seq` is monotonically increasing per `source_id`.
- duplicate `client_seq` values return the original result and are not applied
  twice;
- accepted input receives `input_ack`;
- invalid input receives `input_reject` with a sanitized error envelope in
  `payload`;
- the queue limit is 120 pending input messages per session;
- the UI sends a zero-button state on reconnect before any pressed state.

Event channel rules:

- `server_seq` is monotonically increasing per session;
- reconnect snapshots continue from the active session's last assigned
  `server_seq`;
- the UI ignores events older than the last processed `server_seq`;
- event payloads must be browser-safe summaries.

Schema version 1 event payloads are limited to:

```text
input_state -> inputStatePayload
input_ack -> inputAckPayload
input_reject -> errorEnvelope
session_updated -> sessionUpdatedPayload
run_updated -> runUpdatedPayload
capture_updated -> captureUpdatedPayload
label_updated -> labelUpdatedPayload
validation_updated -> validationUpdatedPayload
```

## Backend Traits

The service scaffold must implement the `BridgeBackend` boundary documented in
`contracts/backend-traits.md` with at least:

```text
SyntheticBackend
RealBackend
```

`SyntheticBackend` is the first runnable backend and may be used for API/UI
tests. `RealBackend` may be compiled behind config, but real sessions stay
unavailable until `docs/real-backend-availability.md` undefer conditions are met.

## Privacy Boundary

The runtime API may expose:

- sanitized status;
- frame counters;
- boolean capabilities;
- browser-safe preview URLs;
- high-level capture job state;
- labels approved for the authenticated operator session;
- sanitized hashes and provenance names.

The runtime API must not expose:

- private filesystem paths;
- worker lease tokens;
- operator credentials;
- raw framebuffer bytes except through no-store image response bodies;
- feature bytes;
- decoded feature arrays;
- raw verifier/scorer errors;
- private artifact refs;
- real private capture ids in public handoff material.

## Number Representation

Schema version 1 transports frame counters and WebSocket sequence numbers as
JSON integers that fit inside JavaScript's safe integer range. The Rust service
may keep wider `u64` values internally, but browser-visible values must be
validated before serialization. A future schema version may switch these fields
to decimal strings if full `u64` range is required in the UI.

## Future Generation

When `service/Cargo.toml` and `ui/package.json` exist, the quality gate must
validate that service and UI generated types are synchronized with
`contracts/runtime-api.schema.json`. The frozen command contract is recorded in
`docs/phase0-contract-freeze.md`.

Future schema fixtures must cover missing `schema_version`, `schema_version: 2`,
malformed WebSocket payloads for each `type`, private-field rejection, unknown
capability rejection, stale-frame errors, and image URL pattern rejection.
