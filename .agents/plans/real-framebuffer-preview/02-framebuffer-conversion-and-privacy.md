# Framebuffer Conversion and Privacy

## Conversion Helpers

Extend `service/src/framebuffer.rs` rather than embedding conversion logic in
`backend.rs`.

Recommended helper shape:

```rust
pub struct RawFramebuffer<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: RawFramebufferFormat,
    pub pixels: &'a [u8],
}

pub enum RawFramebufferFormat {
    Xrgb8888,
}

pub fn framebuffer_png(raw: RawFramebuffer<'_>) -> Result<Vec<u8>, FramebufferConvertError>
```

Keep error variants private or non-sensitive. They may be useful in unit tests,
but API code should not serialize their messages to the browser.

Refactor the current synthetic PNG function so the low-level PNG encoder can
write a PNG from RGB8 scanlines for arbitrary validated width and height. Keep
`synthetic_frame_png(frame)` behavior unchanged.

Even if the low-level encoder accepts arbitrary dimensions for unit tests, the
real route-facing converter must enforce the current public schema dimensions
of `256x224` unless the schema is deliberately changed.

## PNG Encoding

The existing hand-rolled PNG encoder is acceptable for this bead. It already
uses a no-compression zlib stream and avoids adding a new dependency.

Add a helper such as:

```rust
pub fn rgb8_png(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, ...>
```

Validation should ensure `rgb.len() == width * height * 3`.

If a dependency is added instead, justify it in the implementation notes and
keep the output deterministic enough for test hash assertions.

## Privacy Rules

Do not persist raw worker pixels.

Do not include raw pixels, raw byte lengths beyond public metadata, private
paths, lease tokens, worker endpoint URLs, snapshot refs, or worker error
messages in public API responses.

Real preview tests should run `PublicSanitizer` over success and error JSON,
with forbidden literals for the private root, worker endpoint, lease token,
snapshot ref, create-vm config ref, and an injected fake worker status string.
Malformed framebuffer responses must produce the generic `backend_unavailable`
envelope with `details: {}` and no raw length, stride, or status text.

The existing in-memory `FramePreviewStore` may cache PNG bytes only. That cache
is process-local and authenticated behind the runtime cookie. Do not add a
filesystem preview cache.

If temporary private debugging artifacts are introduced during implementation,
remove them before completion. If permanent private artifacts become necessary,
store them only under `BRIDGE_PRIVATE_ROOT` with private modes and add a separate
tracking bead; do not expand `0i9` without recording the scope change.

## Public Response Shape

Leave the runtime API shape stable:

```json
{
  "schema_version": 1,
  "frame": 123,
  "captured_at": "1970-01-01T00:00:00Z",
  "stale": false,
  "width": 256,
  "height": 224,
  "format": "image/png",
  "image_url": "/api/frame/current/image?frame=123",
  "preview_hash": "sha256:..."
}
```

`captured_at` is currently a stable placeholder. Do not replace it with host
wall-clock time unless the runtime API contract is updated and tests prove it
does not leak private operator timing.

Image responses must remain:

```text
Content-Type: image/png
Cache-Control: no-store
Pragma: no-cache
X-Content-Type-Options: nosniff
```

The generic runtime headers already add these protections. Tests should keep
asserting them for both metadata and image responses.
