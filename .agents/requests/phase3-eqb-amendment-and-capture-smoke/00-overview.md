# Request: Amend eqb For The Segmented World, Then The First Real Capture Smoke (Thin, Mostly Gated)

> **CURRENT STATUS (2026-07-10):** Still open. Read
> `04-current-status-2026-07-10.md` before executing this request.

## Who Is Asking

The phases track, round 2 (2026-07-07). Deliberately thin: round-1
(`phase3-play-validation-and-residuals/`) remains this repo's work
queue and is unexecuted; the OOM incident since reshaped part of its
terms, and one genuinely new, gated chunk exists. This request carries
exactly those two things and nothing else.

## Why rom-operator-bridge, Why Now

**1. The OOM changed eqb's terms after round-1 froze them.** Round-1's
item 1 (eqb real-worker streaming validation) was written pre-incident.
Since then: `fbd38d1` clamps streaming to ~200M-instruction segments
with seamless reopen (~50 ms hash-link stall per boundary), new beads
`l1w` (incident record — closes on eqb passing on the fixed stack) and
`9bx` (raise the budget when the hypervisor green-lights) exist, and
the hypervisor owes the leak fix under its round-2 request. Executing
round-1's eqb as written would validate a stack that no longer exists
in that shape. This request is the amendment rider: eqb measures
*across segment reopens*, records the segment budget and worker build
as live variables, counts a segment-boundary stall correctly (a pacing
perturbation, not a "drop"), and closes `l1w` with it.

**2. The Phase-4 capture smoke = executing deferred bead `r77`.** The
export mechanism (`q63`) landed 2026-06-25 — real `trigger_capture`,
`captures/index.jsonl`, truthful capabilities — but no *real* capture
has ever flowed through it (`r77`/`opw`/`0wo` deferred on
operator-private access is the affirmative evidence). Reference-
workload's round-2 corpus request names this repo's export path as a
**contingency** route ("if the direct harness route stalls"). One
operator-private run — which is precisely what bead `r77` already
specifies, so this request executes and closes `r77` rather than
shadowing it — proves the route and advances the `13h`
final-acceptance chain.

## The Ask In One Paragraph

Write `02a-eqb-rider-2026-07-07.md` into the round-1 dir (plus a
one-line pointer in its `02-`): main run at a budget yielding ≥3
reopens (clamp to ≤200M if `9bx` already raised it), reopen-count
cross-check, boundary-stall cap (single ≤250 ms, p95 ≤ ~2× baseline),
budget + worker build recorded as variables, determinism spot-check in
both the main run and the delta re-run, and the `l1w` closure
definition pinned (closes on the contained-stack pass; the delta
addendum is the hypervisor's post-fix confirmation note). Then, once
the refwork cutover yields a real non-blank frame and a private window
is granted, execute and close bead `r77` through the `q63` path
(sanitized record, redaction gate green, cited in refwork's corpus
request dir and on the `r77`/`13h` beads); and action `9bx` when the
hypervisor's green light arrives, with the delta re-run per the rider.

## Files In This Request

| File | Contents |
|---|---|
| `01-current-state.md` | What moved since round-1: fbd38d1, l1w/9bx, the hypervisor's owed fix |
| `02-requested-work.md` | The rider, the smoke, entry conditions, acceptance criteria |
| `03-verification-offer.md` | Choreography with hypervisor/refwork requests; handback |
