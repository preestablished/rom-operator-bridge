# Requested Work

## Suggested Sequencing

Item 1 (the rider) is ungated — write it today; it must exist before
round-1's eqb executes. Items 2–3 fire on their gates in any order.

## What We Need (Behavioral)

1. **The eqb rider (ungated).** Create
   `02a-eqb-rider-2026-07-07.md` in the round-1 request dir (the
   `04-`/`05-` slots stay reserved for resolution/verification), and
   add one bracketed pointer line at the top of round-1's `02-` item 1
   ("[Amended — read 02a-eqb-rider first]") so a linear reader can't
   miss it. The rider's terms:
   - **Budget clamp rule (the collision fix):** the main eqb run
     exercises segment-reopen behavior at a budget yielding ≥3
     boundaries in the window — if `9bx` has already raised the
     deployed default, clamp `PLAY_STREAM_SEGMENT_ICOUNT_BUDGET` to
     ≤200M for this run. The later delta re-run measures at the raised
     budget with boundary count recorded; ≥3 is *not* required there.
   - **Reopen-count cross-check:** record actual reopens and assert
     consistency with `window × fps × instr/frame ÷ budget` within
     tolerance — catches early-ending segments, which the bare floor
     never would.
   - **Boundary-stall cap, not just recording:** no single boundary
     stall > 250 ms; p95 ≤ ~2× the ~50 ms plan baseline. Applies to
     the main run *and* the delta re-run — post-raise, windows may
     hold 0–2 boundaries and the fps bar alone would never see a
     stall regression.
   - Drops stay round-1's definition (WS disconnect; frame_counter
     gap); boundary stalls are pacing tax, measured and capped as
     above.
   - The record states the live budget and deployed worker build as
     variables.
   - The determinism spot-check (plan 02's verifier flow) runs in the
     main run **and in the delta re-run** — a budget raise changes
     segment boundaries, which is exactly the chain-continuity seam.
   - **`l1w` closure, pinned across requests:** `l1w` closes on eqb
     passing on the *contained* stack (the bridge-side incident
     record); the post-fix confirmation the hypervisor's AC3 wants is
     the **delta addendum** — which their AC already accepts as
     "a handback note." State this in the rider so one bead doesn't
     carry two closure definitions.
2. **Execute `r77` (gated — this smoke IS that bead, not its
   shadow).** Entry: refwork's cutover has produced a real non-blank
   frame; an operator-private window granted. Un-defer `r77` and run
   it per its own text: one real capture through the `q63` path,
   private `captures/index.jsonl` row confirmed, the `needs_review`
   label drafted, session stopped cleanly; sanitized record filed
   (capture id, hashes, capability state, redaction-gate output).
   Close `r77` with that record — advancing the `13h` chain — unless
   a specific residual (name it) keeps it open. Citation locations,
   named: a mirror note in
   `../reference-workload/.agents/requests/phase4-real-capture-corpus-fast-follow/`
   and a note on the `r77`/`13h` beads. Share the operator window
   with refwork's lab session and the eqb run where calendars allow —
   but never run eqb concurrent with snapshot-store's 1000× session
   on the same box.
3. **`9bx` follow-through (gated on the hypervisor's green light).**
   Raise the segment budget per their number + build, run the eqb
   delta re-run per the rider (fps, boundary count, stall cap,
   spot-check), file the addendum, note both beads.

## Acceptance Criteria

(AC1↔item 1, AC2↔item 2, AC3↔item 3.)

1. `02a-eqb-rider-2026-07-07.md` exists with every term above; the
   pointer line added; round-1's executor works from it.
2. `r77` closed (or its named residual recorded) with the sanitized
   record filed and cited at both named locations; redaction gate
   green; index row verifiable by id.
3. Delta addendum filed iff the budget changed (verifiable: diff the
   deployed budget against 200M); `l1w` closed per the pinned
   definition; both beads noted.

## Out Of Scope For This Request

- Round-1's whole queue (test debt, pea, ledger hygiene) — untouched;
  this is a rider plus two gated items, not a re-plan.
- The OOM fix itself — the hypervisor's round-2.
- The remaining operator-private smokes (`0wo`/`opw`) and `13h` —
  advanced by item 2, not absorbed.
- Corpus production — refwork's round-2; this proves the contingency
  route only.
- This request dir gets committed per repo hygiene (as round-1 was).
