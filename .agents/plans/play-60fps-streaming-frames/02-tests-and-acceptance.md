# Tests and Acceptance

## Unit / integration (no worker required)

- Synthetic streaming adapter: the reworked `play_loop` on the synthetic
  backend publishes monotonically increasing frame counters at the paced
  rate, honors stop-flag/TTL teardown, and blanks the frame slot on exit
  (extend the existing `play.rs` and `api.rs` play tests).
- B1 decode path: golden test — extend the existing XRGB8888 fixtures in
  `service/tests/framebuffer/main.rs` with an lz4 variant
  (`lz4_flex::compress_prepend_size` on the same raw pixels) feeding the
  same expected-pixel assertions; no lz4-aware test exists today, only
  raw-pixel ones.
- B1 worker-combination gap: no worker test combines
  `FrameBudget(1)` with `CaptureSpec` (the existing capture test uses an
  icount budget). Add one — worker-side in determinism-hypervisor or as
  a bridge integration test — asserting the combination returns
  `fb_lz4/fb_info` identically; do not rely on code inspection alone.
- Backend responsiveness while streaming (B2.0): with a stream open,
  `/api/run/status` and a capture-job poll must both complete within a
  bounded time (hundreds of ms, not the 20s reply timeout). If this test
  cannot pass, the worker-channel restructuring is wrong — fix the
  design, not the test.
- Pause-vs-stream race: Pause during streaming must terminate via stream
  cancel at a frame boundary and must NOT dispatch a worker `Pause` RPC
  (assert no epoch-quantized pause happens — e.g. via worker logs or a
  mock asserting the Pause command is never sent on the streaming path).
- Input fallback batching: with M3 absent, inputs buffered mid-stream are
  injected within N frames and produce the existing artifact rows
  (`write_input_artifacts` path unchanged).
- Event throttle: `/ws/events` sees ≤ the throttled rate while streaming
  plus exactly one terminal boundary event on stop.

## Real-worker validation (operator-private)

Follow the private validation reference (`deploy/` docs) — evidence goes
to the operator-private root, never committed:

1. B1 smoke: Start → Play for ~600 frames; confirm one Run RPC per frame
   in worker logs, identical UI behavior, no state regressions
   (Pause/Stop/preview still correct).
2. B2 smoke: streaming play for ≥60s; measure delivered fps at the
   `/ws/frames` client and input-to-effect latency (count frames between
   an injected pad press and its visible effect).
3. Determinism spot-check: after a played session, run the verifier flow
   (existing `verifier.rs` path) to confirm the session's chain/replay
   evidence is intact — full-fidelity hashing must be unaffected by
   streaming delivery.

## Acceptance (the bug-report scenario)

Loading the SNES ROM from `~/ROMs/SNES`, Start → Play:

- reach the reference game point in **~20s wall time (real-time speed)**,
  matching standalone emulators. Make this machine-checkable, not an
  eyeball test: capture the frame counter N at the reference point once
  (from a known-good run), then assert "frame_counter ≥ N within 20s ±1s
  of Play start" from the `/ws/frames` framing;
- sustained ~60fps at the `/ws/frames` client without frame-counter gaps,
  measured at the client end of the operator's real link (this also
  validates the ~81 Mbps uncompressed-PNG bandwidth question from B3 —
  if the link can't carry it, the chosen mitigation must be in place
  before acceptance);
- input-to-effect latency ≤ ~2 frames with M3 (the stop/restart fallback
  is test-scaffolding, not an acceptance path — see 01's duty-cycle
  analysis);
- CI: the synthetic streaming-loop tests and the B1 golden tests run in
  the normal quality gate; add a feature-flag matrix entry so both the
  streaming and B1-fallback paths compile and pass unit tests;
- Pause → frame counter freezes at a consistent boundary; Resume
  continues without visual glitches; Stop tears down cleanly;
- quality gate (`bash scripts/quality-gate.sh`) and redaction gate pass.

## Sequencing note

Do not benchmark B1/B2 against a debug worker: determinism-hypervisor
plan M1 (release builds in the ops runbook) must land first or the
numbers are meaningless. The hypervisor-side plan is
`determinism-hypervisor/.agents/plans/play-60fps-decouple-hash-from-frames/`.
