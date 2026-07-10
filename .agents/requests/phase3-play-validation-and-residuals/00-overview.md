# Request: Validate The Landed Play Stack And Clear Its Residuals

> **CURRENT STATUS (2026-07-10):** Still open. Read
> `04-current-status-2026-07-10.md`; the refwork cutover is complete, but the
> validation and ledger work below has not landed.

## Who Is Asking

The phases track. Filed 2026-07-07 (revised the same day: the original
draft asked for B1/B2/q63 implementation — dual review showed all of it
had already landed, some of it *while the draft was being written*; this
revision is scoped to what is actually left). Unusual direction — this
repo normally files the requests — but the bridge is the human-visible
half of Phase 3 exit gate 3 and the capture surface Phase 4 leans on.

## Why rom-operator-bridge, Why Now

The implementation race is over; the validation isn't:

- **B1 + B2 + pacing landed** at `fb2a7fc` (tip of `main`, 2026-07-07):
  single captured `Run` per frame, the streaming lane,
  `PlayStreamSession` + synthetic adapter, paced `play_loop`,
  `ROM_OPERATOR_BRIDGE_PLAY_STREAMING` rollback toggle. Beads `zbf`,
  `ffx`, `3gp`, `713`, `qr6` all closed.
- **The dual review's code fixes landed** at `960e4cc`: frames slot
  blanked on fault/self-exit (I1), the `/ws/frames`
  unauthenticated-handshake rejection test (I2), the `stop()` join
  decision documented (bounded ~1–2 frame park).
- **q63 real-capture export landed** back on 2026-06-25 (`bd5f834`,
  hardened in `01966f2`): real `trigger_capture` with idempotency,
  capabilities truthful. `13h`'s only remaining blockers are the
  operator-private smokes, by design.
- **The worker is already redeployed** with the full
  `RunWithFrameCapture` + frame-hold surface (hypervisor `4285b45`,
  built from their `bdd476b` merge; `30d0cb9` retained as rollback).

What has NOT happened: the streaming stack has never been exercised
against the real worker (`eqb`, unblocked now); the review's follow-up
*test* beads are open (`4zn`, `y4g`); `qr6`'s close reason names two
untested seams; the B3 bandwidth/metrics decision (`pea`) is unmade —
and at ~172 KB/frame of PNG, faster streaming is an ~81 Mbps-at-60fps
question someone has to answer before it matters; and the bead ledger
(`9xo` open P0, `9mk` parent, `bvq`) no longer reflects reality.

## The Ask In One Paragraph

Run the real-worker streaming validation (`eqb`) against the redeployed
worker and record measured fps/latency against the known ceiling (the
hypervisor's `38b6` data: ~8.5 fps today, ~11 after their deferred M4;
60 needs an emulator speedup); land the review's follow-up tests (`4zn`
Play-lifecycle, `y4g` UI frame-ordering) plus the two seams `qr6`'s
close reason left untested (`/ws/events` throttle-rate assertion,
worker-side FrameBudget+CaptureSpec combination test); make the `pea`
metrics/bandwidth decision with numbers; apply the one real `bvq`
cosmetic (resume response's `current_frame` lag) and disposition
`9xo`/`bvq`/`9mk` so the ledger matches `main`; and commit this request
directory per the repo's own hygiene rules.

## Files In This Request

| File | Contents |
|---|---|
| `01-current-state.md` | Evidence: what landed where, what is actually open |
| `02-requested-work.md` | The ask, sequencing, acceptance criteria, out of scope |
| `03-verification-offer.md` | Phases-track verification and cross-request choreography |
