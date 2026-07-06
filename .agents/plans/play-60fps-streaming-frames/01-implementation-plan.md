# Implementation Plan

## B1 — Fold play_step into one captured Run (land first)

Files: `service/src/backend.rs`.

1. In `RealWorkerThread` / `WorkerClient`, add a
   `run_one_frame_captured(lease) -> RealCapturedFrameOutcome` that sends
   `RunRequest{ until: FrameBudget(1), capture: Some(CaptureSpec{
   framebuffer: true, ranges: [] }), hard_icount_cap: 0 }` and reads
   `fb_lz4 + fb_info + icount + reason` from the single `RunResponse`.
2. lz4-decompress (`fb_lz4`) → existing `framebuffer_png` (XRGB8888 path,
   honoring `fb_info.stride/format`) → `PlayStepOutcome` as today.
   Add the lz4 dependency to `service/Cargo.toml` (worker uses lz4 frame
   format — match whatever `dh-worker`'s capture engine emits; confirm
   block vs frame format against `capture_at_boundary`).
3. `play_step` (real backend, `backend.rs:1404`) switches to the new
   call; faulted/`StopReason` handling unchanged; the
   `GetFramebuffer`-based `resume` path stays for non-Play uses
   (`frame_counter`, preview).
4. Keep the synthetic backend untouched.

Expected effect: one worker round-trip per frame instead of two; the
worker still hashes full memory per frame until the hypervisor plan's M2
lands, so this is a latency/cleanliness win, not the 60fps fix.

## B2 — Streaming Play loop

Files: `service/src/backend.rs`, `service/src/api.rs`,
`service/src/play.rs` (minor), `service/src/ws_input.rs` (input timing).

### Backend surface

Extend `BridgeBackend` with a streaming-play session:

```rust
fn play_stream_start(&self, session_id: SessionId)
    -> BackendResult<PlayStreamHandle>;
// PlayStreamHandle:
//   next_frame(deadline) -> BackendResult<PlayStepOutcome>  // blocking read
//   inject(pad events)   -> BackendResult<...>              // frame-hold inject (M3)
//   stop() -> BackendResult<RunBoundary>                    // cancel stream, slot Paused
```

Real implementation: open `RunWithFrameCapture` with a large
`icount_budget` (worker M2 decision: no `frame_budget` arm initially;
stopping = dropping/cancelling the stream, which leaves the slot Paused
at a frame boundary). Each `CapturedFrame` → lz4 → PNG →
`PlayStepOutcome`. Terminal `RunResponse` or stream error → map to the
existing fault path (`mark_faulted`).

Synthetic implementation: trivial adapter that generates one synthetic
frame per `next_frame` call (reuses `synthetic_frame_png`), so the loop
rewrite is testable without a worker.

### Play loop (`api.rs::play_loop`)

Keep the dedicated-thread + stop-flag + watch-channel structure
(`play.rs` unchanged). Loop body becomes:

1. tick a 60Hz pacer (SNES NTSC ~60.10Hz; use `fb_info`-derived timing if
   the meta region exposes it, else a 16.64ms fixed tick). Pacing by NOT
   reading the stream faster than 60Hz is what holds the vCPU via
   backpressure — do not busy-read;
2. flush pending input:
   - M3 available: `handle.inject(...)` (applied at next frame-hold);
   - fallback while M3 pending: accumulate inputs; every N frames
     (configurable, default ~6 ≈ 100ms) `handle.stop()`, inject via the
     existing paused-slot `InjectInputs` path, restart the stream.
     Document the input-latency and extra-hash-link cost of the fallback;
3. `next_frame(deadline)` → publish via `frame_message` → 
   `publish_run_boundary_event` (unchanged shape);
4. stop flag / TTL expiry → `handle.stop()`, then the existing teardown
   (`deregister`, blank frame slot).

Pause/Stop path (`pause`/`stop` handlers) must route through
`handle.stop()` so the slot lands Paused at a frame boundary and
`current_frame` is refreshed — mirror what `play_stop` does today.

### Event cadence

`run_updated` per frame at 60Hz may be noisy for `/ws/events`
subscribers; throttle boundary events to ~4Hz while streaming (frame
counter still rides `/ws/frames` framing), preserving the final
boundary event on stop.

## B3 — Pacing & polish

- Pacer accuracy: measure end-to-end fps in the UI; drift-free tick
  (absolute-deadline scheduling, not `sleep(16ms)` per iteration).
- PNG encode cost at 60Hz on the loop thread: measure; if >2-3ms/frame,
  consider reusing encoder buffers first (avoid per-frame allocs in
  `framebuffer_png`) before reaching for threads.
- Fault/reconnect UX: a dropped worker stream surfaces the same
  terminal-state event flow the per-frame loop has today.
- Remove/feature-flag the per-frame `play_step` loop once streaming has
  soaked (keep B1 fallback behind a config toggle for one release).

## Suggested beads

1. `Fold play_step resume+framebuffer into one captured Run` (B1, impl,
   p0)
2. `Add lz4 framebuffer decode + captured-run PNG path` (B1, impl, p0) —
   same PR as 1 if small
3. `BridgeBackend streaming-play surface + synthetic adapter` (B2, impl,
   p1) ← worker M2
4. `Rework play_loop to paced streaming consumer` (B2, impl, p1) ← 3
5. `Frame-hold input injection wiring` (B2, impl, p1) ← worker M3, 4
6. `Throttle run_updated events during streaming play` (B2, impl, p2) ← 4
7. `Pacing accuracy + PNG encode measurements` (B3, testing, p2) ← 4
