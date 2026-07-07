# Verification And Cross-Request Choreography

## Who Verifies What

This request inverts the usual direction — the bridge normally verifies
others — so the phases track takes the verifying side:

1. **Test debt (item 2):** we re-run the four tests from a clean
   checkout and check the two new beads' close reasons point at real
   assertions (a throttle test that never asserts a rate doesn't count).
2. **`eqb` (item 1):** the run itself is operator-private; we verify the
   sanitized record is internally consistent (fps math vs frame counts,
   the bar met or the miss explained) and that no raw capture material
   entered git (static redaction gate green).
3. **`pea` (item 3):** we check the decision cites the item-1
   measurements rather than the stale ~50 ms/frame figure (streaming
   amortizes to ~28 ms/frame per the hypervisor's `38b6` notes).

## Choreography With The Other Filed Requests

- `../reference-workload/.agents/requests/phase3-m4-first-room-gate-and-m5-stamp/`
  — their snapshot + the operator ROM decision close `bvq`'s content gap;
  schedule the `eqb` window with their cutover if timing allows (one
  operator window, fewer orphaned slots, `72o`).
- `../determinism-hypervisor/.agents/requests/phase3-frame-cap-retune-and-run-wallclock-backstop/`
  — if their wall-clock item lands as an implementation, we verify it
  from this operator surface (their `03-` names us); if it closes as
  confirm-no-hang, retire the `timeout(1)` client stopgap and note it.
  Their worker redeploy already happened (`4285b45`) — no window to
  coordinate for item 1 beyond operator availability.

## Handback Shape

Append `04-resolution.md` here (git SHAs, the sanitized `eqb` record,
bead dispositions, the `pea` decision, test list); we respond with
`05-verification.md` after the checks above.

## Contact / Tracking

- Beads covered: `eqb`, `4zn`, `y4g`, `k1b` (the `qr6` seams — already
  tracked, no new filing), `pea`, `9xo` (disposition), `9mk`, `bvq`
  (re-scope), `qh4` (disposition).
- Review source (correct path): `reviews/feat-play-mode-continuous-run-2026-07-06/`.
- Operator-attention items: the `eqb` private window; the real-ROM
  content decision; `aaw`'s non-interactive-sudo closeout.
