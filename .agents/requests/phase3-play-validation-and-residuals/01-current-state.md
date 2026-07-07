# Current State (Evidence-Based)

Assessed 2026-07-07, post-`fb2a7fc` (tip of `main`). This file corrects
the original draft, which was written against a pre-`fb2a7fc` snapshot
of the repo and mis-stated four of five items.

## Landed (Do Not Re-Request)

- **Play B1 + B2 + pacing** — `fb2a7fc`: one captured `Run` per frame
  (B1), streaming lane restructure + `play_stream_start` +
  `PlayStreamSession` + synthetic adapter (B2), ~60.0988 Hz pacer,
  `ROM_OPERATOR_BRIDGE_PLAY_STREAMING` rollback toggle (the fallback
  stop/restart path wasn't needed). Beads `zbf`/`ffx`/`3gp`/`713`/`qr6`
  closed with detailed reasons.
- **Review code fixes** — `960e4cc` ("Apply Play-mode review findings"):
  I1 frames-slot blank + deregister on fault/self-exit; I2 the
  `/ws/frames` unauthenticated-handshake rejection test (the branch's
  security claim — now tested); the `stop()` `JoinHandle::join()`
  decision documented as a bounded ~1–2 frame park. Review source:
  `reviews/feat-play-mode-continuous-run-2026-07-06/` (repo root — note:
  *not* under `.agents/`).
- **q63 real capture export** — closed 2026-06-25, `bd5f834` +
  hardening `01966f2`: `RealBackend::trigger_capture` real
  (idempotency, capture-spec resolution, `captures/index.jsonl`),
  capabilities truthful (`real_capture_mvp()`), redaction discipline
  enforced by the static gate. `13h` final acceptance waits only on the
  deferred operator-private smokes (`0wo`/`r77`/`opw`).
- **Worker redeploy** — hypervisor `4285b45` (2026-07-07): operator
  worker now serves `RunWithFrameCapture` + live frame-hold input,
  release build from their `bdd476b` merge; `30d0cb9` kept as rollback.
- **bvq's "gaps," resolved by its own notes** (2026-07-06): the
  `preview=false → 503` report was a repro artifact (empty
  `requested_capabilities`; the UI requests preview and gets HTTP 200
  `image/png`) — not a bug. `current_frame` on the resume *response*
  lags, but the UI polls `frame/current`, which is correct — cosmetic.
  Only the content gap is real: deployed `game.img` ~96% zeros.

## Actually Open

| Item | State | What it is |
|---|---|---|
| `eqb` | open, now unblocked | Real-worker validation of streaming play (operator-private) — the landed stack has only synthetic-adapter evidence |
| `4zn` | open P2 | Play-lifecycle integration test (would have caught the fault-exit stale-frame gap) |
| `y4g` | open P2 | UI `handleLiveFrame` ordering test — **blocked on jsdom canvas** per its own title (no `createImageBitmap`/2d context; needs the `canvas` package or a decode/paint seam) |
| `k1b` | open P2 | The two `qr6` close-reason seams: `/ws/events` throttle-rate assertion (this repo) + worker-side FrameBudget+CaptureSpec combination test (**lands in determinism-hypervisor**, per the bead) |
| `pea` | open | B3: bridge play metrics + the PNG bandwidth decision (~172 KB/frame ⇒ ~81 Mbps at 60 fps; DEFLATE level / downscale / format question) |
| `9xo` | **open P0** | "no post-Ready frame under no-tick" — headline symptom now contradicted by `bvq`'s own verification (frames advance on the deployed worker); residue is the snapshot regen + cutover owned by refwork's request. Needs disposition, same as `bvq` |
| `9mk` | open P2 parent | The play-mode feature bead — implementation is on `main`; needs disposition |
| `bvq` | in progress | Needs re-scope: cosmetic `current_frame` fix optional; then only the content gap remains (refwork's request) |
| `qh4` | open P3 | `run_updated` throttling polish — suggestions-tier |
| `aaw` | in progress | Passwordless deploy build (notes-level non-interactive-sudo blocker) — the redeploy happened anyway; still worth closing out with the operator |
| `72o`, `0wo`/`r77`/`opw` | open P2 / deferred | Slot leases; operator-private smokes — listed for completeness, out of scope here |

## Performance Reality (So Nobody Chases 60fps Here)

The hypervisor's `38b6` measurements: ~8.5 fps end-to-end today; their
deferred M4 (epoch-hash pipeline) would buy ~11 fps; under streaming the
hash link amortizes to ~28 ms/frame (not the ~50 ms/frame of the
per-frame-Run era); 60 fps requires a reference-workload emulator
speedup (~90–115 ms/frame guest execution) that no repo currently owns.
Validation targets below are set against *these* numbers.

## Dependencies

- `eqb` needs an operator-private window against the deployed worker —
  no code dependency remains.
- The visible first room still waits on reference-workload's request
  (regenerated snapshot + real ROM) and the operator cutover; `bvq`
  closes then.
