# Tests and Acceptance

## Unit / integration (no worker required)

- Synthetic streaming adapter: the reworked `play_loop` on the synthetic
  backend publishes monotonically increasing frame counters at the paced
  rate, honors stop-flag/TTL teardown, and blanks the frame slot on exit
  (extend the existing `play.rs` and `api.rs` play tests).
- B1 decode path: golden test — lz4-compressed XRGB8888 fixture →
  `PlayStepOutcome.png_bytes` decodes to the expected pixels (reuse the
  `framebuffer_png` test fixtures).
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
  matching standalone emulators;
- sustained ~60fps at the `/ws/frames` client without frame-counter gaps;
- input-to-effect latency ≤ ~2 frames with M3 (≤ ~8 frames on the
  fallback path);
- Pause → frame counter freezes at a consistent boundary; Resume
  continues without visual glitches; Stop tears down cleanly;
- quality gate (`bash scripts/quality-gate.sh`) and redaction gate pass.

## Sequencing note

Do not benchmark B1/B2 against a debug worker: determinism-hypervisor
plan M1 (release builds in the ops runbook) must land first or the
numbers are meaningless. The hypervisor-side plan is
`determinism-hypervisor/.agents/plans/play-60fps-decouple-hash-from-frames/`.
