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
GET  /api/capture/<capture_id>/features
POST /api/labels
GET  /api/labels
```

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

All WebSocket messages use `wsEnvelope` from the schema.

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
- the UI ignores events older than the last processed `server_seq`;
- event payloads must be browser-safe summaries.

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

## Future Generation

When `service/Cargo.toml` and `ui/package.json` exist, the quality gate must
validate that service and UI generated types are synchronized with
`contracts/runtime-api.schema.json`. The frozen command contract is recorded in
`docs/phase0-contract-freeze.md`.
