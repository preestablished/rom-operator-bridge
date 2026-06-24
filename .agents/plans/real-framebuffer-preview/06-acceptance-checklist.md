# Acceptance Checklist

Use this file as the final implementation audit for `rom-operator-bridge-0i9`.
Do not close the bead until every applicable item has concrete evidence in the
bead notes or handoff.

## Code Behavior

Real backend:

- `RealBackend::capabilities()` grants preview only for real MVP.
- `RealBackend::framebuffer()` calls worker `GetFramebuffer`.
- The worker request uses the private stored lease.
- Backend locks are not held while waiting on worker thread replies.
- Returned `frame_counter` updates `last_preview_frame`.
- Returned `frame_counter` updates `current_frame` monotonically.
- Returned `icount` updates `current_icount`.
- Real run without `fb_info` marks preview stale/unknown until a framebuffer
  refresh.
- Preview failure does not destroy an otherwise active real session.
- Faulted or missing sessions still return sanitized backend unavailable.

Conversion:

- XRGB8888 converts to PNG correctly.
- Row padding is handled.
- Unsupported formats are rejected.
- Non-`256x224` real route dimensions are rejected unless schema/docs are
  intentionally updated.
- Bad dimensions, stride, length, and overflow are rejected without panic.
- Raw worker pixels are not persisted.

API:

- `GET /api/frame/current` returns stable browser-safe metadata.
- Real metadata validates against `contracts/runtime-api.schema.json`.
- `GET /api/frame/current/image` returns PNG bytes only.
- Both routes retain no-store/nosniff protections.
- Metadata includes no raw pixel payloads or private refs.
- Image route serves the exact frame advertised by metadata.
- Stale is true when preview frame is older than current frame.
- Frame query hint validation remains strict.

Privacy:

- Public responses contain no worker endpoint, private root, lease token,
  snapshot ref, create-vm config ref, raw worker status, or raw framebuffer
  bytes.
- Public logs and handoff notes redact private values.
- Redaction gate passes.

## Required Tests

Minimum targeted commands:

```bash
cargo fmt --check
cargo test --test framebuffer
cargo test --test frame
cargo test --test real-backend
```

Full service gate:

```bash
cargo test
```

Privacy gate:

```bash
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> bash scripts/redaction-gate.sh
```

## Bead Closeout

When the implementation is complete:

1. Update `rom-operator-bridge-0i9` with concise evidence.
2. Mention whether live real preview was run, skipped, or blocked.
3. If live preview is blocked by missing framebuffer-published snapshot or
   runtime state, file a follow-up bead before closing or leave `0i9` blocked
   with exact host evidence.
4. Run repository closeout:

```bash
git pull --rebase
bd dolt push
git push
git status
```

`git status` must show the branch up to date with origin.
