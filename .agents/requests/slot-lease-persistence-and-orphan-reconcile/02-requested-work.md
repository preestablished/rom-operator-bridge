# Requested Work

## What We Need (Behavioral)

1. **Write-ahead intent + lease records (two-phase, mandated).**
   (a) Persist an **intent record** to the private root *before* the
   CreateVm/RestoreSnapshot RPC (the token doesn't exist yet — the
   intent carries session_id, run_id, base-snapshot id, timestamp);
   (b) persist the **full lease record** (slot id, token, session_id,
   run_id, base-snapshot id, timestamps) immediately when the RPC
   returns, clearing the intent; (c) remove the lease record on clean
   DestroyVm — **destroy-then-remove, never remove-then-destroy**.
   This shrinks the undestroyable class to window 2's microsecond
   residual instead of leaving it unbounded. The start-failure
   rollback path (`backend.rs:1240–1247`) interacts with both
   records — specify it. Redaction: this deliberately bends the
   no-token-in-private-artifacts invariant
   (`assert_private_artifacts_do_not_contain_lease`,
   `LEASE_TOKEN` forbidden_literal) — amend the test with a scoped
   exception for the lease-record path and record the rationale;
   tokens still never reach git or sanitized logs.
2. **Reconcile on startup, before serving traffic — destroy is the
   default.** For each persisted lease: query the worker; token
   validates → **DestroyVm** (re-adopt is disqualified today: no
   client re-attach protocol exists, session ids reset and are
   reused, `RealSession` derived state has no reconstruction path —
   record these disqualifiers in the decision doc rather than
   re-deriving them); discriminated `StaleLease` (via
   `ErrorDetail.code`, *not* bare FAILED_PRECONDITION) or slot
   missing → already clean, clear the record. Dangling intents →
   log "unaccounted slot may exist," count them, and use the
   hypervisor destroy-by-slot-id path if their item-4 decision
   provides one. Worker unreachable → defined behavior (recommend:
   serve synthetic-only or refuse real sessions, retain records,
   retry on a timer — pick and document). Emit a startup reconcile
   summary (found / destroyed / stale-cleaned / dangling) to the
   sanitized log.
3. **Live-bridge repair, not just startup.** The stop-path leak
   (`stop_session` drops the session before a failed `worker.stop`)
   gets the same treatment: record retained on destroy failure and
   retried (lazy sweep or next start-session) — the 2026-07-01 class
   must not survive in its restartless variant. File the adjacent
   session-id finding (`next_sequence` not persisted ⇒ id reuse
   across restarts) as its own bead — one-line fix once records
   persist, not this request's scope.
4. **Test matrix on the mock worker — build the lease modeling.** The
   mock's `destroy_vm` currently ignores leases; add token validation
   + discriminated StaleLease to it, then cover:
   restart-with-active-session (the 2026-07-01 shape),
   restart-no-sessions, spurious intent (W1), dangling intent (W2),
   stale token (W3), destroyed-record-remains (W4), crash
   mid-reconcile / double-reconcile idempotence (W5), stop-path
   destroy-failure retry, worker-unreachable-at-startup.
5. **One live verification, then retire the caveat.** During an owned
   restart window on the deployed pair: an **operator-driven synthetic
   session** (never a user's), **SIGKILL** the bridge (a graceful stop
   wouldn't reproduce the deploy shape), restart, show the reconcile
   summary destroying the orphan and `ListSlots` back to full
   capacity **before/after captured**, no worker restart. Close `72o`
   citing it; list (in this dir's resolution) the sibling-request
   choreography sections whose "restarts orphan slots" caveat this
   retires — don't edit their history.

## Acceptance Criteria

(AC1↔item 1, AC2↔item 2, AC3↔items 3–4, AC4↔item 5.)

1. Two-phase records implemented with destroy-then-remove ordering;
   rollback-path interaction specified; the invariant exception
   amended in the test with recorded rationale; redaction gate green.
2. Reconcile implemented with destroy-default (disqualifiers
   recorded), discriminated-StaleLease handling, dangling-intent
   accounting, worker-unreachable behavior documented, idempotence
   stated and tested. The window-2 requirement was delivered to the
   hypervisor round-3 dir **at filing**
   (`06-bridge-requirement.md` there — already done); if their
   decision predates any needed revision, request a revisit note.
3. Mock lease-modeling built; the full nine-case matrix green in CI;
   the session-id bead filed.
4. Live verification record (sanitized): SIGKILL, synthetic session,
   reconcile summary, ListSlots before/after, no worker restart;
   `72o` closed citing it; caveat-retirement list in the resolution.

## Out Of Scope For This Request

- Worker-side anything (WARN, TTL, admin RPC) — the hypervisor
  round-3; the window-2 requirement is delivered, not implemented,
  from here.
- Session *re-adoption* / client re-attach — disqualified above;
  revisit only if a re-attach protocol ever exists.
- Round-1/round-2 scopes here — untouched; item 5's window rides an
  already-booked calendar slot.
