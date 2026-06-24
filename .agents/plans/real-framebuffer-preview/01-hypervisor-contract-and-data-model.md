# Hypervisor Contract and Data Model

## RPC Shape

`determinism.hypervisor.v1.GetFramebuffer` has this request and response:

```text
GetFramebufferRequest {
  lease: Lease
}

GetFramebufferResponse {
  width: u32
  height: u32
  stride: u32
  format: PixelFormat
  frame_counter: u32
  icount: u64
  pixels: bytes
}

PixelFormat {
  PF_UNSPECIFIED = 0
  XRGB8888 = 1
  RGB565 = 2
}
```

The request must use the private stored lease. The lease token must remain
private and must never appear in route responses, public logs, or plan evidence.

## Supported Preview Format

The discovery note records the reference workload framebuffer as:

```text
format: xrgb8888-256x224-stride1024
width: 256
height: 224
stride: 1024
size: 229376
```

Implement support for `XRGB8888` first. Before coding the channel mapping,
freeze byte order with a colored fixture test. The proto names the format but
does not spell out host byte order; the determinism-hypervisor nanokernel
framebuffer fixture publishes known XRGB8888 bytes and should be used as the
source of truth for whether the bridge strips the first byte as X and maps the
next bytes as RGB, or needs a different channel order.

Treat `PF_UNSPECIFIED`, unknown enum values, malformed stride, malformed byte
length, zero dimensions, and dimensions above a conservative bound as
unavailable real backend data. Public output should be sanitized
`backend_unavailable`, not a worker-specific validation message.

`RGB565` can be added only if it is small and covered by tests. It is not
required by the documented reference workload.

## Public Dimension Contract

`contracts/runtime-api.schema.json` currently fixes `frameCurrentResponse` to:

```text
width = 256
height = 224
format = image/png
```

That means a successful real preview route must return `256x224` unless the
schema and docs are intentionally updated in the same implementation. For
`0i9`, prefer strict validation: non-`256x224` real worker framebuffers map to
sanitized `backend_unavailable`.

## Validation Rules

For `XRGB8888`:

- `width == 256` and `height == 224` for public route success;
- `stride >= width * 4`;
- `stride * height == pixels.len()`;
- `stride * height` and derived RGB buffer sizes do not overflow `usize`;
- source rows may include padding bytes after `width * 4`;
- padding bytes are ignored and never copied into the PNG.

Add explicit tests for row padding. The M9 shape has no padding beyond the
XRGB bytes because `256 * 4 == 1024`, but the converter should not rely on that
coincidence.

The converter is the explicit dimension and stride enforcement point. Do not
rely on `validate_frame_preview`; that API helper currently checks session id
and JSON-safe frame counters, not width or height.

## Frame Counter and Icount

`GetFramebufferResponse.frame_counter` is an absolute pv-pad frame counter. Map
it to `FramePreview.frame` and to `RealSession.last_preview_frame`.

`GetFramebufferResponse.icount` should update `RealSession.current_icount`.

Update `RealSession.current_frame` to at least the returned `frame_counter`.
Do not decrement `current_frame` if a stale framebuffer response reports an
older frame than the session already knows. The route will mark it stale through
the existing `preview.frame < status.current_frame` rule.

## Session State

`GetFramebuffer` is valid for paused slots. In the current real backend,
`CreateVm`, `RestoreSnapshot`, `pause`, and the bounded one-frame `resume` path
all end in `SessionState::Paused`.

Do not add a hidden long-running preview loop. If a future state is `Running`
or `Faulted`, return sanitized backend unavailable instead of trying to stream.
If a worker returns `FAILED_PRECONDITION`, map it to sanitized backend
unavailable and leave browser output private-safe.
