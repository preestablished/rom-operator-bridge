# Subagent Review Summary

Two subagents independently reviewed the initial plan:

1. runtime/measurement correctness, especially boundary math, effective
   unbounded budget representation, determinism, and bead closure semantics;
2. operator/privacy/execution safety, especially private evidence handling,
   capture/index/label assertions, cross-repo citation, and closeout.

## Runtime And Measurement Review

Accepted changes:

- replaced the vague `verifier.rs` reference with an executable worker
  `VerifyReplay` flow: sealed input log, private base/log/end-hash tuple, zero
  Divergence, Done event, and matching end-state hash for both main and delta;
- decoupled code preparation from live deployment: create a telemetry-bearing
  200M intermediate commit and a raised successor now, deploy them in that
  order only when the private window exists;
- tightened expected-boundary tolerance from a loose 20% allowance to the
  mathematically justified window-edge/icount-variance bound and named the
  aggregate icount source;
- made both stall caps operate on one baseline-subtracted boundary-stall series;
- required an API/play-loop test for API-loop telemetry and prohibited treating
  every clean end as a budget end without preserving/classifying the reason;
- corrected bead ownership: the complete contained validation closes `eqb` and
  `l1w`; code/deployment/delta closes `9bx` and only cites the closed beads;
- required the mock worker to retain and assert the numeric
  `IcountBudget(u64::MAX / 4)` request.

The reviewer confirmed the numeric effectively-unbounded budget is correct:
the worker requires an `until` arm and its own RSS/agenda regressions use
`u64::MAX / 4`.

## Operator, Privacy, And Closeout Review

Accepted changes:

- documented the conflict between AC2's ID-verifiable record and the current
  publish-blocking capture-id redaction rule; added a two-tier private/public
  evidence contract, authorized alias-to-id attestation, and explicit
  operator/phases-track sign-off rather than weakening either requirement;
- made capture retry idempotency durable and prohibited a second capture after
  partial success without explicit operator disposition;
- split operator authorization into data, host/network, deploy/restart,
  capture/label mutation, and cross-repo publication grants;
- clarified deferred/open/claim/note/close ordering while deferring exact
  deferred-state syntax to the installed `bd` version's help;
- strengthened sibling-repository workflow, quality gates, independent
  commit/beads sync/push, and no-corpus-production boundary;
- added post-rollback readiness/session/slot/process invariants and o73
  escalation when cleanup cannot establish them;
- expanded final cleanup to inspect stashes without deleting user-owned state,
  prune remotes, and verify/push every touched repository separately.

## Partially Accepted Or Reframed

- The ops reviewer proposed a concrete `bd update ... --defer '' --status open`
  command. The plan instead requires consulting the installed CLI's help before
  clearing deferral because supported flags may vary; the lifecycle and
  note-preservation requirement are retained.
- The repository close instruction says to clear stashes. Because unrelated
  stashes belong to the user, the plan inspects all stashes and removes only a
  stash demonstrably created by this work.

## Rejected Findings

None. All other findings were applied directly or incorporated with equivalent
scope and safety.
