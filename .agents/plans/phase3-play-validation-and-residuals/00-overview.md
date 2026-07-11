# Phase 3 Play Validation And Residuals Plan

## Outcome

Finish the still-open round-1 request without reimplementing the Play stack or
duplicating the newer EQB amendment work:

1. land the three bridge-side regression tests tracked by `4zn`, `y4g`, and
   the bridge half of `k1b`;
2. hand the worker-side `FrameBudget(1) + CaptureSpec` assertion to
   determinism-hypervisor with a durable tracker reference;
3. complete `pea` with observable Play metrics and an evidence-based frame
   bandwidth decision;
4. execute the private real-worker validation through the existing
   `phase3-eqb-amendment-and-capture-smoke` plan; and
5. make the bead ledger and the request resolution reflect what is actually on
   `main` and what remains operator-gated.

## Current Baseline

Treat repository state, not the request's dated status table, as authoritative.
At plan time the current branch contains:

- `9c36909`, which added contained-EQB reopen telemetry and the round-1 rider;
- `2c420e7`, which raised the stream instruction budget after the worker fix;
- `548d528`, `2121fc8`, and `83abcb1`, which classify and fault unexpected
  stream endings; and
- `3f3358b`, which merged that work.

Commit `9c36909` also satisfied the amendment prerequisite that
`04-current-status-2026-07-10.md` still describes as missing. The checked-in
`02a-eqb-rider-2026-07-07.md` is the authoritative rider despite that dated
status file's “round-2 rider” wording; do not create another rider.

The private contained and raised-budget EQB runs have not been performed.
`eqb` is in progress; `4zn`, `y4g`, `k1b`, `pea`, `9xo`, `9mk`, `qh4`, and
`l1w` remain open; `bvq` and `aaw` remain in progress. Re-check every bead at
execution time because ledger state may change independently of this plan.

## Plan Files

| File | Implementation responsibility |
|---|---|
| `01-current-state-and-boundaries.md` | Baseline audit, ownership, and sequencing |
| `02-bridge-regression-tests.md` | `4zn`, `y4g`, and `/ws/events` throttle coverage |
| `03-metrics-and-bandwidth-decision.md` | `pea` telemetry and encoding decision |
| `04-private-validation-and-evidence.md` | EQB integration, rollback, determinism, and privacy |
| `05-ledger-resolution-and-closeout.md` | Cosmetic fix, bead dispositions, resolution, gates, and publication |
| `06-subagent-review-summary.md` | Independent plan reviews and incorporated findings |

## Dependency Order

```text
baseline audit
  -> bridge regression tests
  -> metrics instrumentation
  -> focused + full local quality gates
  -> private EQB main/delta/rollback validation when authorized
  -> bandwidth decision using measured data
  -> cosmetic/ledger disposition
  -> 04-resolution.md + commit/sync/push
```

Tests and instrumentation do not wait for the private operator window. The
final `pea` encoding decision consumes the private measurements, but its metric
surface and tests should be prepared first. If the operator window is not
authorized, land all ungated work, keep only genuinely gated beads open, and
record the exact gate rather than manufacturing synthetic closure evidence.

## Non-Goals

- Do not redesign B1/B2, pacing, segment reopen, or worker OOM handling.
- Do not chase 60 fps; current acceptance remains the rider's measured
  real-worker baseline.
- Do not add native `canvas` merely to unit-test frame ordering.
- Do not commit raw frames, cookies, hostnames, private paths, capture IDs,
  replay tuples, or worker logs.
- Do not close a cross-repository test item until a real hypervisor tracker or
  landed assertion can be cited.
