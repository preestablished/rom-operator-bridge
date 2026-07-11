# Subagent Review Summary

Two subagents independently reviewed the initial plan:

1. code/test architecture and repository-state correctness; and
2. operational validation, evidence privacy, bead semantics, and closeout.

## Code And Test Architecture Review

Accepted changes:

- added the spawn-before-register/self-deregister race as an implementation
  fix and deterministic lifecycle regression;
- required run changes to close the retained old bitmap and clear its canvas;
- retained binary frame counters as `bigint`, including `2^53` edge tests;
- replaced flaky websocket inter-arrival assertions with a pure throttle-policy
  test plus a conservative live integration assertion;
- corrected Tokio `watch` observability: socket counter gaps are indicative,
  not an exact skipped-version metric;
- split aggregate producer metrics from per-socket sink metrics and prohibited
  per-frame tracing spam or sensitive correlation IDs; and
- corrected the recommended service test targets and reused the existing auth
  test clock with a synchronization-controlled fake stream.

The malformed-frame suggestion was accepted with an explicit newest-received
policy: prefix-only messages are rejected, decode failures retain the current
painted bitmap, and same-counter retransmissions remain ignored.

## Operations, Privacy, And Closeout Review

Accepted changes:

- added atomic bead claims, coordination with assigned work, and independent
  sibling-repository transactions;
- reconciled the stale status file with the checked-in authoritative rider and
  required a pre-live diff against the amendment plan;
- narrowed private collection to minimum EQB evidence, separated all `r77`
  capture material, and added file modes, quiet scans, retention ownership, and
  build-ID publication approval;
- required current concrete proof before closing stale ledger items;
- made a hypervisor handoff remotely durable before it can satisfy `k1b`; and
- restored the mandatory `git pull --rebase`, beads push, git push, and
  up-to-date status sequence, treating a missing authorized upstream as a
  blocker rather than silently skipping publication.

## Reframed Or Rejected Findings

No finding was rejected outright. The UI decoder-counter finding was reframed
to preserve the existing render-if-newer/newest-received contract rather than
introducing retry semantics that the protocol does not promise.

Subagents made no file edits; the parent agent applied all agreed changes.
