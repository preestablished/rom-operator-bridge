# Phase 3 EQB Amendment And Capture Smoke Plan

## Outcome

Implement the request in three deliberately separate tracks:

1. amend the round-1 `eqb` contract before any live validation;
2. prepare and test both a telemetry-bearing 200M build and its raised-budget
   successor now; in the private window deploy the 200M build first, run `eqb`,
   and close the bridge-side OOM incident;
3. deploy the already-prepared raised-budget build, perform the required delta
   validation, and
   execute the already-tracked `r77` real-capture/label smoke when the operator
   grants a private deployment window.

This plan does not absorb the rest of
`.agents/requests/phase3-play-validation-and-residuals/`. Its tests, `pea`
metrics decision, and ledger cleanup remain owned there.

## Current Facts To Treat As Authoritative

- Reference-workload's real-image cutover and first-room proof are complete.
  `r77` is no longer blocked on a non-blank frame; it is blocked only on
  operator-private data, host/network state, and deployment access.
- Hypervisor commit `c0337ab` fixes agenda materialization and explicitly
  green-lights an effectively unbounded stream budget. It is not sufficient
  evidence by itself: record the deployed release-worker build.
- The bridge still sends `PLAY_STREAM_SEGMENT_ICOUNT_BUDGET = 200_000_000` in
  `service/src/backend.rs`. `RunWithFrameCaptureRequest.until` is required, so
  the post-raise value must be a very large numeric `IcountBudget`, consistent
  with the worker regression guard (`u64::MAX / 4`), not `None`.
- `eqb`, `l1w`, `9bx`, and `13h` are open. `r77` is deferred and blocks `13h`
  and `opw`.

## Deliverables

| File | Purpose |
|---|---|
| `01-rider-and-contract.md` | Exact round-1 request edits and rider contents |
| `02-private-preflight-and-observability.md` | Safe staging, worker/build gates, measurement method |
| `03-contained-eqb-run.md` | 200M main run, acceptance math, `l1w` disposition |
| `04-budget-raise-and-delta.md` | Effectively unbounded default, tests, redeploy, delta addendum |
| `05-r77-real-capture-smoke.md` | One real capture, `needs_review`, private index verification |
| `06-evidence-privacy-and-closeout.md` | Sanitized records, citations, beads, gates, commit/push |
| `07-subagent-review-summary.md` | Independent reviews and accepted/rejected feedback |

## Dependency And Stop Rules

```text
rider + contained telemetry commit + raised-budget successor prepared
  -> operator-private window + c0337ab-or-later deployed worker
      -> deploy contained commit -> 200M eqb pass -> close eqb + l1w
          -> deploy raised successor -> delta eqb -> close 9bx

operator-private window + real non-blank frame
  -> undefer/claim r77 -> capture + label + clean stop -> close r77 -> advance 13h
```

- Never run `eqb` concurrently with snapshot-store's 1000x session on the same
  host. Coordinate through the existing slot/restart ownership rather than
  improvising around it.
- Do not run a long stream against a worker older than `c0337ab`. If the build
  cannot be proved, retain the 200M containment, stop, and record the sanitized
  blocker.
- Do not close a gated bead from synthetic evidence or from the hypervisor's
  code-only proof.
