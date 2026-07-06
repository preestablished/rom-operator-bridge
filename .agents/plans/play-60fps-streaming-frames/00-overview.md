# Play at 60fps: Bridge-Side Streaming Frame Consumption

## Problem

Continuous Play through the bridge runs at ~5 fps (~240s to reach a game
point zsnes reaches in ~20s). Target: real-time 60fps play with the
hypervisor's state-hash chain kept at full fidelity.

## Where the time goes (measured/confirmed 2026-07-06)

Per rendered frame the Play loop (`service/src/api.rs::play_loop`) does:

1. `flush_pending_input` (artifact writes on the real path);
2. `BridgeBackend::play_step` (`service/src/backend.rs:1404`), which is
   **two serialized worker RPCs**: `Run{frame_budget=1}` then
   `GetFramebuffer` — the code itself notes folding these into one
   captured Run as a known optimization;
3. on the worker side, every one-frame Run stop pushes a **full
   128 MiB blake3 state-hash link** (~50ms even in release; the live
   worker additionally runs a debug build today) — root causes and fixes
   are planned in
   `determinism-hypervisor/.agents/plans/play-60fps-decouple-hash-from-frames/`;
4. PNG encode (`framebuffer_png`) + `/ws/frames` publish (cheap).

Bridge-side levers cannot reach 60fps alone (cause 3 dominates), but they
halve RPC traffic now and are the consumer half of the 60fps design.

## Design direction (operator-approved)

Separate frame generation, frame delivery, and hash links: the worker's
`RunWithFrameCapture` streaming RPC (API.md §2.7; being implemented under
the determinism-hypervisor plan, milestone M2) emits one lz4 framebuffer
per FRAME_MARK during a single long Run; hash links happen per
epoch/terminal stop instead of per frame; stream backpressure holds the
vCPU at frame boundaries, which the bridge uses for exact 60Hz pacing.

## Milestones

- **B1 — Single captured Run per frame** (01). No worker changes needed:
  `RunRequest` already supports `capture{framebuffer: true}` and
  `RunResponse` returns `fb_lz4 + fb_info` (worker
  `capture_at_boundary` is implemented). Replaces the resume +
  GetFramebuffer pair in `play_step` with one RPC. Land now; keeps all
  current semantics.
- **B2 — Streaming Play** (01). Replace the per-frame RPC loop with a
  `RunWithFrameCapture` consumer: 60Hz-paced reads, PNG encode, publish
  to the existing `watch` channel; input via `InjectInputs` at
  frame-holds (hypervisor plan M3), with segment-boundary injection as
  the fallback while M3 is pending.
- **B3 — Pacing & polish** (01). Real-time pacing (the current loop
  free-runs — after the worker speedups it would run FASTER than real
  time), Pause/Stop semantics over a streaming run, fault handling.

## Constraints

- Keep the existing UI contract: `/ws/frames` binary framing
  `[u64 frame_counter LE][PNG]`, watch-channel latest-frame semantics,
  Pause/Stop behavior, TTL self-termination (`play_loop`'s
  `active_session_live` check).
- Synthetic backend (`play_step` synthetic path) must keep working for
  validation without a worker.
- Privacy: no operator-private env values, socket paths, refs, or raw
  worker errors in committed files or bead notes.

## Dependencies

- B1: none (worker capture path already live). Benefits from
  determinism-hypervisor M1 (release builds) landing first for honest
  measurements.
- B2: determinism-hypervisor plan M2 (`RunWithFrameCapture`
  implementation); input latency target needs M3.

## Prior art in this repo

`.agents/plans/play-mode-continuous-run/` (the plan that built the
current Play loop) and
`.agents/plans/wire-real-fram-boundary-input-injection/` — B2 must
preserve the invariants those plans established (single active loop,
stop-flag teardown, input rejection artifacts).
