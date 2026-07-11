# Slot Lease Persistence And Orphan Reconciliation Plan

## Outcome

Implement the bridge-owned half of `rom-operator-bridge-72o`: every real-worker
allocation is preceded by a durable intent, every returned lease is durably
recorded, and startup reconciliation destroys recoverable orphans before the
bridge permits another real session. Preserve records until destruction is
known to have succeeded or the worker proves that the recorded lease is stale
or absent.

The paired hypervisor decision is no longer pending. Its 2026-07-10 resolution
explicitly defers TTL, disconnect reclamation, and privileged tokenless
reconciliation. Therefore a dangling intent with no lease token remains an
operator-visible residual. Recovery requires stopping the bridge, restarting
the worker, verifying the slot table is empty/full-capacity, and then using an
audited bridge-owned tool to clear the selected intent records before the
bridge resumes. A worker restart alone is insufficient because the durable
intent remains. Do not invent or call a destroy-by-slot-id RPC in this change.

## Plan Files

1. `01-design-and-invariants.md` defines the durable format, state machine,
   crash behavior, and startup policy.
2. `02-implementation-steps.md` gives the implementation sequence and concrete
   repository touch points.
3. `03-test-and-verification.md` maps the required nine cases to automated and
   live evidence.
4. `04-rollout-and-handoff.md` covers compatibility, deployment, beads, and
   the request-directory resolution handoff.

## Scope Boundaries

- Change the bridge and its real-backend mock only. Do not change worker
  lease semantics or worker APIs.
- Destroy persisted leases; do not re-adopt them. There is no client reattach
  protocol, session/run IDs restart from zero, and `RealSession` contains
  derived state that cannot be reconstructed safely.
- File the session/run sequence persistence finding separately. This plan only
  prevents those reused IDs from colliding with lease persistence.
- Do not perform the live SIGKILL exercise outside an explicitly owned operator
  window, and never use a user session for it.

## Recommended Delivery Slices

Keep the implementation reviewable in four commits if practical:

1. durable record store and unit tests;
2. worker error classification and mock lease semantics;
3. start/stop/reconcile integration plus the nine-case matrix;
4. documentation, live-verification evidence, and bead/request resolution.
