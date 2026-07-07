# Implementation Plan

## B1 — Fold play_step into one captured Run (land first)

Files: `service/src/backend.rs`.

1. In `RealWorkerThread` / `WorkerClient`, add a
   `run_one_frame_captured(lease) -> RealCapturedFrameOutcome` that sends
   `RunRequest{ until: FrameBudget(1), capture: Some(CaptureSpec{
   framebuffer: true, ranges: [] }), hard_icount_cap: 0 }` and reads
   `fb_lz4 + fb_info + icount + reason` from the single `RunResponse`.
   This means a new `RealWorkerCommand` variant + match arm in
   `run_real_worker_thread` (the existing `Capture` variant maps to
   `TakeSnapshot`, not `Run`).
2. lz4-decompress (`fb_lz4`) → existing `framebuffer_png` (XRGB8888 path,
   honoring `fb_info.stride/format`) → `PlayStepOutcome` as today.
   No new dependency or format investigation: `lz4_flex` is already in
   `service/Cargo.toml`, and `backend.rs:997` already decodes worker
   `fb_lz4` with `lz4_flex::decompress_size_prepended` on the
   TakeSnapshot capture path — the worker's `capture_at_boundary` emits
   the same `compress_prepend_size` block format for Run captures.
   Caveat: `framebuffer_png` hard-codes a 256x224 dimension check —
   fine for the SNES target, but validate `fb_info.width/height` and
   surface a clean error rather than a panic if they ever differ.
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

### B2.0 — Restructure the worker channel FIRST (the hidden blocker)

`run_real_worker_thread` (`backend.rs:2173`) is a strict
one-command-at-a-time loop: `rx.recv()` → `block_on(timeout(rpc))` →
reply → next command, with 15s/20s timeouts. A `RunWithFrameCapture`
stream lives for the whole play session; if it is dispatched as just
another `RealWorkerCommand`, it occupies the single `block_on` slot
indefinitely and EVERY other command — Status, Pause, Stop, InjectInputs,
the input fallback's own stop/restart — queues behind it until the
session ends. That deadlocks the exact controls that are supposed to
interrupt the stream, and starves `/api/run/status` and capture polling
for the whole session (each caller burns the 20s reply timeout and gets
`BackendUnavailable`).

Design this before writing `play_stream_start`: give the stream its own
lane. Recommended shape — a dedicated streaming task spawned on the
worker thread's tokio runtime (or a second thread), holding the
`tonic::Streaming<FrameCaptureEvent>`, with:

- a bounded frame channel toward the play loop (`next_frame` reads it);
- an out-of-band cancel handle (drop the stream / abort the task) that
  `stop()` uses WITHOUT going through the command mpsc;
- the command loop untouched for all existing RPCs, which therefore stay
  responsive while a stream is open.

Locking note: the streaming task must not hold the `RealBackendInner`
mutex across awaits; it only touches shared session state at
frame-delivery and termination edges, same discipline as today's
`play_step` bookkeeping block.

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
`icount_budget` (M2's current proposal — not yet a settled decision — is
no `frame_budget` arm initially; stopping = dropping/cancelling the
stream, which leaves the slot Paused at a frame boundary). Each
`CapturedFrame` → lz4 → PNG → `PlayStepOutcome`. Terminal `RunResponse`
or stream error → map to the existing fault path (`mark_faulted`).

`input_in_flight` semantics: the flag exists to serialize input RPCs
against per-frame Runs; it has no meaning while a stream owns the slot.
Define it explicitly for B2: set for the duration of a fallback
stop/inject/restart cycle (so nothing else races the restart), never set
by frame delivery itself, and `play_stream_start` refuses to start while
it is set — do not leave the old `play_step` precondition comment as the
only documentation.

Synthetic implementation: trivial adapter that generates one synthetic
frame per `next_frame` call (reuses `synthetic_frame_png`), so the loop
rewrite is testable without a worker.

### Play loop (`api.rs::play_loop`)

Keep the dedicated-thread + stop-flag + watch-channel structure
(`play.rs` unchanged). Loop body becomes:

1. tick a 60Hz pacer (SNES NTSC ~60.0988Hz; use `fb_info`-derived timing
   if the meta region exposes it, else a 16.64ms fixed tick). Pacing by
   NOT reading the stream faster than the tick is the *intent* — but do
   not assume HTTP/2 gives frame-granular backpressure: connection/stream
   flow-control windows plus tonic/hyper buffering can hold several
   ~230 KB lz4 frames in flight, so the worker's bounded channel may not
   block until the transport buffer fills, and frames then arrive in
   bursts. Validate with a capture during B2 bring-up; the bridge's paced
   reads are the primary pacing mechanism, the worker's channel bound is
   the backstop, and if buffering proves deep, shrink the HTTP/2 window
   on this channel or have the worker pace on send timestamps;
2. flush pending input:
   - M3 available: `handle.inject(...)` (applied at next frame-hold);
   - fallback while M3 pending: accumulate inputs; every N frames
     `handle.stop()`, inject via the existing paused-slot `InjectInputs`
     path, restart the stream. **Do the math before shipping this to
     operators**: every restart ends a Run, and every Run stop pushes a
     ~50ms full-memory hash link on the worker — at N=6 (~100ms of play)
     that is a ~50% stall duty cycle, i.e. roughly the original problem
     in 6-frame bursts, with visible stutter. Treat the fallback as
     test-scaffolding only (or N≥30 with a visible "reduced input rate"
     UI banner); the real interactive input path is M3;
3. `next_frame(deadline)` → publish via `frame_message` → 
   `publish_run_boundary_event` (unchanged shape);
4. stop flag / TTL expiry → `handle.stop()`, then the existing teardown
   (`deregister`, blank frame slot).

Pause/Stop path: the streaming branch of `run_state_transition`
(`api.rs`) calls `handle.stop()` (stream cancel → slot Paused at the
current frame boundary, ≤1 frame latency) **instead of**
`state.backend.pause()` — not in addition to it. Today's handler calls
`backend.pause()` unconditionally after stopping the loop, relying on a
benign `FailedPrecondition` because per-frame Runs always leave the slot
paused; with a stream, that same call can race the asynchronous cancel
and dispatch a REAL worker Pause, which is epoch-grid-quantized (up to
~1s of extra play — exactly the latency the sibling plan's stop design
exists to avoid). Refresh `current_frame` from `handle.stop()`'s
returned boundary, mirroring what `play_stop` does today.

### Event cadence

`run_updated` per frame at 60Hz may be noisy for `/ws/events`
subscribers; throttle boundary events to ~4Hz while streaming (frame
counter still rides `/ws/frames` framing), preserving the final
boundary event on stop.

## B3 — Pacing & polish

- Pacer accuracy: measure end-to-end fps in the UI; drift-free tick
  (absolute-deadline scheduling, not `sleep(16ms)` per iteration). Base
  period matters, not just jitter: a 16.64ms tick against the SNES's
  true ~16.6398ms frame period drifts ~1 frame per ~12s of play. Pick
  the base period explicitly and resync it periodically against
  `fb_info.frame_counter` vs wall clock (slew, don't jump).
- `/ws/frames` bandwidth: `rgb8_png` currently emits stored
  (uncompressed) zlib blocks — ~172 KB/frame, ~81 Mbps at 60fps. Measure
  the operator's real link; if not comfortably under budget, either
  implement real DEFLATE in `rgb8_png` (a genuine sub-task — today's
  encoder has no compressor) or add an explicit adaptive frame-skip
  policy on WS send-buffer depth. Decide and document which.
- PNG encode cost at 60Hz on the loop thread: measure; if >2-3ms/frame,
  consider reusing encoder buffers first (avoid per-frame allocs in
  `framebuffer_png`) before reaching for threads.
- Fault/reconnect UX: a dropped worker stream surfaces the same
  terminal-state event flow the per-frame loop has today.
- Rollback toggle, concretely: a `play_streaming = bool` in the bridge
  runtime config (committed default `true` once soaked; NOT in the
  operator-private file — it is not a secret) selecting streaming vs the
  B1 per-frame path at `play_run` time. Keep for at least one release;
  removal criteria documented alongside the soak results.
- Bridge-side metrics (the operator debugs the bridge, not the worker):
  delivered fps, pacer overrun count, stream restarts (fallback
  injections), WS send-buffer depth / skipped frames, decode+encode time
  per frame — exposed on the existing telemetry surface
  (`telemetry.rs`).
- Artifact/labeling flows (`real_capture.rs`, `labels.rs`): they consume
  paused-slot captures, not the play-frame path, so they should be
  unaffected — verify once in the B2 smoke (take a labeled capture right
  after stopping a streamed session) and note the result.

## Suggested beads

1. `Fold play_step resume+framebuffer into one captured Run` (B1, impl,
   p0)
2. `Reuse lz4_flex decode for captured-run PNG path` (B1, impl, p0) —
   same PR as 1 if small
3. `Restructure RealWorkerThread: streaming lane that does not block the
   command loop` (B2.0, impl, p1) ← worker M2
4. `BridgeBackend streaming-play surface + synthetic adapter` (B2, impl,
   p1) ← 3
5. `Rework play_loop to paced streaming consumer` (B2, impl, p1) ← 4
6. `Rewire run_state_transition Pause/Stop to stream cancel (no
   backend.pause on the streaming path)` (B2, impl, p1) ← 5
7. `Frame-hold input injection wiring` (B2, impl, p1) ← worker M3, 5
8. `Throttle run_updated events during streaming play` (B2, impl, p2) ← 5
9. `Bridge play metrics + rollback toggle` (B3, impl, p2) ← 5
10. `Pacing accuracy, /ws/frames bandwidth, PNG encode measurements`
   (B3, testing, p2) ← 5
