# API Route and State Behavior

## Preserve Existing Routes

Keep the public route contract unchanged:

- `GET /api/frame/current` returns JSON metadata only.
- `GET /api/frame/current/image` returns PNG bytes only.
- The `frame` query parameter remains a cache-busting hint.
- Session mismatch remains unauthorized/session inactive behavior.

Invalid query behavior must match current auth and frame-hint handling:

- no query string is allowed for metadata;
- exactly `frame=<digits>` is allowed for the image route;
- empty `frame`, extra query keys, and credential-like query values are rejected
  before image handling;
- unexpected query strings return the sanitized auth rejection path, not an
  image response;
- oversized numeric `frame` returns `400 bad_request`;
- error responses must not carry `Content-Type: image/png`.

The primary change should be the real backend implementation behind
`BridgeBackend::framebuffer`. Small API projection changes are in scope if
needed to carry an explicit internal preview stale/unknown state into
`RunStatusResponse` and websocket payloads.

## Stale Semantics

The stale rule remains:

```text
preview.frame < status.current_frame
```

for simple counter-known cases. Real runs add one more requirement: if the
backend knows a run may have advanced frames but has no authoritative
framebuffer frame counter yet, public run-status and websocket payloads must set
`preview_stale=true`.

Plan tests around these cases:

- real status frame equals framebuffer frame: `stale = false`;
- real status frame is newer than framebuffer frame: `stale = true`;
- real framebuffer reports a newer frame than the current cached status:
  `stale = false`, and later status calls report at least that frame.
- real resume returns no `fb_info`: `/api/run/status` and run-updated events
  report `preview_stale=true` until a successful real framebuffer refresh.

Do not reintroduce the older 120-frame or 2-second stale threshold. The frozen
MVP contract says any older boundary sample is stale.

## Preview Store

The existing in-memory `FramePreviewStore` caches PNG bytes by session and frame.
Continue using it so `/api/frame/current/image?frame=<advertised>` serves the
same image advertised by metadata.

Do not cache raw worker pixels. Do not write previews to disk.

When a requested frame hint is absent from the in-memory store, the current
image route may fetch a fresh backend preview. Preserve the current behavior:
if the fresh preview is not the requested frame, return `400 Preview frame
unavailable`.

## Capabilities and UI

After this bead, real session start responses should grant `preview: true` when
preview is requested, while real input and capture remain false until their
own beads land.

This is important because the browser UI uses capabilities to decide whether to
show preview controls. Do not set `input` or `capture` true as part of `0i9`.

## Browser-Safe Error Behavior

For real preview failures, public responses should look like the existing
sanitized backend unavailable envelope:

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

The body must not include:

- worker endpoint path or URL;
- private root path;
- lease token;
- snapshot ref;
- create-vm config path;
- raw tonic status message;
- raw framebuffer payload length if it came from a private malformed response.

## Event Stream Interaction

No websocket event schema change is required for this bead.

The websocket payload schema already has a `preview_stale` boolean. If the
implementation adds an explicit internal stale/unknown flag, update websocket
projection to use that flag instead of recomputing solely from
`last_preview_frame < current_frame`.

Add tests only if a small route-level or event-level assertion is cheap. Avoid
turning this bead into a websocket feature change.
