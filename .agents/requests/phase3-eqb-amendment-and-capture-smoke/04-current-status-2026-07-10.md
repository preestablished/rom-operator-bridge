# Current Status - 2026-07-10

This request remains open, with its gates changed by work completed after
filing.

## Item Status

1. **eqb rider: open and ungated.** The required
   `../phase3-play-validation-and-residuals/02a-eqb-rider-2026-07-07.md`
   and pointer in that request's `02-requested-work.md` do not exist.
2. **real capture smoke (`r77`): operator-gated only.** Reference-workload
   completed the real-image cutover, first-room proof, and M5 20/20 stamp on
   2026-07-07. The old refwork-cutover gate is satisfied. Bead `r77` remains
   deferred for private operator data, host/network state, and deployment
   access.
3. **budget raise/delta run: implementation green light exists, deployment
   evidence does not.** Hypervisor commit `c0337ab` fixed the unbounded agenda
   materialization OOM and its resolution explicitly green-lit removing the
   bridge's 200M segment clamp. Bridge beads `9bx`, `eqb`, and `l1w` remain
   open, so do not treat the code landing as a deployed eqb pass.

## Execution Ruling

Write the rider before any `eqb` run. Schedule `eqb` and `r77` in an
operator-private window against a worker that includes `c0337ab` or later,
record the deployed build, and use the rider's contained-stack/delta evidence
rules. This request is not waiting on reference-workload anymore.

The request commits `6d70ea2` and the round-3 bridge request commit `6eb7a1e`
are currently local-only: bridge `main` is two commits ahead of `origin/main`.
