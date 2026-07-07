# Requested Work

## What We Need (Behavioral)

1. **`eqb` — real-worker streaming validation.** Against the redeployed
   worker (`4285b45` build): a sustained streaming Play session from the
   browser through `/ws/frames`, with measured fps, frame latency, and
   stream stability recorded (sanitized notes only, per the redaction
   discipline / `scripts/quality-gate.sh`). The pass bar is honesty, not
   speed: **sustained ≥8 fps for ≥60 seconds with zero stream drops and
   scheduled input honored (frame-hold injection observed)** — i.e.
   within measurement noise of the hypervisor's ~8.5 fps ceiling. Record
   the delta from ceiling and where the time goes (worker vs bridge vs
   wire); that number is the baseline `pea` decides against. Also
   exercise the rollback toggle once
   (`ROM_OPERATOR_BRIDGE_PLAY_STREAMING=false`) and record the per-frame
   path still works — that comparison replaces the "B1 before/after" the
   original draft asked for.
2. **Test debt, all four (beads already exist — don't file duplicates).**
   - `4zn`: Play-lifecycle integration test (fault during Play → frames
     slot cleared and deregistered — the regression the `960e4cc` fix
     needs a guard for);
   - `y4g`: UI `handleLiveFrame` ordering test — first make the
     unblocking decision its title records (jsdom lacks canvas: adopt
     the `canvas` package, or introduce a decode/paint seam) and note it
     on the bead;
   - `k1b` (tracks both `qr6` seams): the `/ws/events` throttle-rate
     assertion lands here; the worker-side FrameBudget+CaptureSpec
     combination test lands in **determinism-hypervisor** — coordinate
     via their request dir or a bead in their tracker rather than
     forcing it into this repo's CI.
3. **`pea` — the metrics/bandwidth decision, with numbers.** Land the
   play metrics the bead describes, then decide the frame-encoding
   question against the `eqb` measurements: at ~172 KB/frame PNG, is the
   answer DEFLATE-level tuning, downscale, a different format, or
   "fine at current fps, revisit at M4" — decide, document, and either
   implement or file the follow-up bead with the decision recorded.
   Don't leave it as an open question for the 60fps era to trip over.
4. **Ledger hygiene.**
   - `9xo` (the ledger's only open P0): its headline symptom is
     contradicted by `bvq`'s own verification on the deployed worker;
     its residue (snapshot regen + cutover) is owned by refwork's
     request. Disposition it — close-with-pointer or re-scope to the
     cutover confirmation — so the P0 lane tells the truth;
   - `bvq`: apply the one real cosmetic if cheap (resume response
     `current_frame`), then re-scope the bead so its only open item is
     the content gap owned by reference-workload's request;
   - `9mk`: disposition the parent feature bead (close against
     `fb2a7fc`/`960e4cc` or re-scope to the B3 tail);
   - `qh4`: keep or fold into `pea` — your call, say which;
   - commit this request directory (it is currently untracked; the
     repo's own session rules say commit).

## Suggested Sequencing (Yours To Overrule)

2 (pure test code, no operator window) → 1 (needs the operator-private
window; schedule it with the refwork cutover if timing allows — one
window, both purposes) → 3 (consumes 1's numbers) → 4 as things close.

## Acceptance Criteria

1. `eqb` closed with the sanitized measurement record: fps/latency
   table, the ≥8 fps / ≥60 s / zero-drop bar met (or a recorded reason
   with the bottleneck named), input-injection observed, rollback-toggle
   check recorded.
2. The three bridge-side tests (`4zn`, `y4g`, the `/ws/events`
   throttle-rate assertion) exist and are green in this repo's CI, with
   `y4g`'s canvas-unblocking decision recorded; the worker-side
   FrameBudget+CaptureSpec test is either landed hypervisor-side or
   handed to them with a tracker reference; `4zn`/`y4g` closed and `k1b`
   closed or split accordingly.
3. `pea`'s decision written down with the supporting numbers; metrics
   visible (endpoint or log — say which); follow-up bead filed if
   implementation is deferred.
4. `9xo` dispositioned (the P0 lane truthful), `bvq` re-scoped, `9mk`
   dispositioned, `qh4` dispositioned, request dir committed —
   `bd list` for the play area matches `main` reality.

## Out Of Scope For This Request

- Any further Play implementation — B1/B2/pacing are landed; B3 beyond
  `pea`'s decision is future work.
- 60fps — gated on hypervisor M4 and an emulator speedup nobody owns
  yet; the validation bar above is deliberately set at today's ceiling.
- The READY-snapshot regeneration, real ROM, and cutover — reference-
  workload's request + operator decisions. A blank frame from a zeros
  image is not a bridge defect.
- The operator-private final smokes (`0wo`/`r77`/`opw`) and `13h` —
  operator-gated by design; `eqb`'s window is not a substitute for them.
- Slot-lease persistence (`72o`) — tracked separately.
