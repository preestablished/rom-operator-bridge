# Real Framebuffer Preview Plan for 0i9

## Goal

Finish `rom-operator-bridge-0i9` by wiring the real backend preview route to
the authoritative hypervisor framebuffer source while preserving the existing
browser-facing runtime API shape.

The completed behavior should let an authenticated real backend session call:

```text
GET /api/frame/current
GET /api/frame/current/image?frame=<frame>
```

and receive browser-safe preview metadata plus a no-store PNG image generated
from the real worker framebuffer.

## Current State

The synthetic preview routes are already implemented in `service/src/api.rs`.
They:

- authenticate runtime requests;
- call `backend.status(session_id)` and `backend.framebuffer(session_id)`;
- validate that returned session ids match the active session;
- cache `FramePreview` PNG bytes in memory for the image route;
- mark preview stale when `preview.frame < status.current_frame`;
- set no-store runtime headers and `Content-Type: image/png` for image bodies.

`SyntheticBackend::framebuffer` returns generated PNG bytes.
`RealBackend::framebuffer` currently returns `BackendError::BackendUnavailable`.

`rom-operator-bridge-bp8` already added the real worker command-loop pattern,
tonic client setup, UDS/TCP endpoint support, lease storage, `Pause`, bounded
`Run`, `DestroyVm`, `ListSlots`, and sanitized worker error mapping. Reuse that
shape; do not migrate `BridgeBackend` to async for this bead.

## Source Contracts

Use these contracts as authoritative:

- `docs/bridge-discovery-note.md`, `Framebuffer Contract`
- `docs/hypervisor-runtime-contracts.md`, `Frame Bases` and `GetFramebuffer`
- `docs/runtime-api.md`, frame routes and runtime API deviations
- `determinism-hypervisor/proto/hypervisor.proto`, `GetFramebuffer*` and
  `PixelFormat`

Important frozen facts:

- The MVP uses boundary samples, not live streaming.
- `RunWithFrameCapture` is unimplemented and must not be required.
- `GetFramebuffer` is available for paused slots.
- Real stale threshold is strict: `preview_frame_counter < current_frame_counter`.
- Raw pixels, private paths, worker status strings, lease tokens, and private
  artifact refs must not be exposed in public API responses, websocket events,
  logs copied into public notes, or docs.

## Implementation Strategy

Add real preview support in three narrow layers:

1. `service/src/framebuffer.rs`: add reusable framebuffer-to-PNG conversion
   helpers with strict validation.
2. `service/src/backend.rs`: add a real worker `Framebuffer` command that calls
   `GetFramebuffer`, converts the response to a `FramePreview`, and updates
   private session frame state.
3. `service/tests/...`: add mock-worker and route tests proving real preview
   success, stale handling, unsupported response sanitization, and privacy.

Prefer keeping raw worker pixels inside the worker command handling path. Return
only validated PNG bytes and public dimensions to the API layer.

## Non-Goals

Do not implement real input injection. That is `rom-operator-bridge-3dr`.

Do not implement real capture export or capture preview artifact wiring. That is
`rom-operator-bridge-q63`.

Do not introduce live streaming, polling loops, background framebuffer threads,
browser persistence, service-worker caches, localStorage, IndexedDB, or public
preview files.

Do not expose raw framebuffer bytes or private artifact references through the
runtime API. The existing `/api/frame/current/image` response body may contain a
PNG preview only.

## Expected File Touches

Primary files:

- `service/src/backend.rs`
- `service/src/framebuffer.rs`
- `service/tests/real-backend/main.rs`

Likely supporting files:

- `service/tests/framebuffer/main.rs` or a similar integration test folder
- `service/tests/frame/main.rs`
- `service/Cargo.toml` only if a conversion dependency is intentionally added

Avoid unrelated route, UI, capture, label, and input refactors.
