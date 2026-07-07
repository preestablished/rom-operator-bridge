# Current State (Evidence-Based)

Assessed 2026-07-07 (round 3). Repo HEAD `6d70ea2`; two open requests
precede this one — round-1 lists `72o` as "tracked separately … listed
for completeness, out of scope here"; round-2 *mentions* the caveat
("we own the restart windows (`72o`) — one calendar, we broker it") but
covers none of the fix.

## The Leak, Per The Bead And The Code

`rom-operator-bridge-72o` (P2, open, `bd ready`): the bridge holds VM
leases **in memory only** — `RealBackendInner` keeps one
`active: Option<RealSession>` behind a mutex (`backend.rs:2126`); the
worker-minted lease token lives nowhere else. Start =
CreateVm/RestoreSnapshot (token exists only in the RPC *response* —
`proto/hypervisor.proto:116–131`); stop = DestroyVm. A bridge restart
mid-session drops the lease; DestroyVm never fires; the slot stays
PAUSED_S forever (the worker's reclaim engine exists but nothing calls
it — hypervisor round-3 evidence).

Observed 2026-07-01: `ListSlots` showed **4 paused slots at identical
icount 641343512** (the worker has exactly 4 slots) — a day's bridge
redeploys, cleared only by a worker restart. Provenance:
`../determinism-hypervisor/.agents/requests/rom-bridge-getframebuffer-region-contract/04-related-slot-leak.md`.

**A second leak path needs no restart at all**: `stop_session`
(`backend.rs:1278–1308`) `take()`s the session *then* calls
`worker.stop` — if that RPC fails, the session is already dropped and
the lease is gone with the bridge still running.

## What Exists To Build On — And One Invariant This Deliberately Bends

- **Private persistent root**: real
  (`service/src/private_config.rs` — `ROM_OPERATOR_BRIDGE_PRIVATE_ROOT`,
  `PRIVATE_RUN_DIRS`; runbook `/var/lib/rom-operator-bridge/private`),
  already carrying manifests/events/captures under the static
  redaction gate (`scripts/quality-gate.sh`).
- **But note**: the current discipline for lease tokens is *stricter*
  than "not in git": `assert_private_artifacts_do_not_contain_lease`
  (`tests/real-backend/main.rs:2177–2197`) asserts the token appears
  in **no private run artifact at all**, and `LEASE_TOKEN` is a
  sanitizer `forbidden_literal`. Persisting a token-bearing lease
  record is a **deliberate exception to an intentional invariant** —
  the request requires amending that test consciously (scoped
  exception for the lease-record path) with the rationale recorded,
  not silently.
- **Worker API primitives**: `ListSlots` (per-slot state + icount),
  `DestroyVm` (lease-validated; `slot_manager.rs:295–320,580`). No
  lease-less destroy exists (`force_destroy` is internal). Wire
  nuance: `StaleLease` maps to gRPC `FAILED_PRECONDITION`, which is
  *shared* with wrong-slot-state — reconcile must discriminate via
  `ErrorDetail.code`, not the status alone.
- **The mock worker models less than hoped**: it has the cancel-window
  / FailedPrecondition modeling from `fbd38d1`, but its `destroy_vm`
  ignores the lease entirely (`main.rs:2485–2495`) — no
  StaleLease/bad-token path exists. The matrix's lease-validation
  cases are **built**, not extended.
- **No re-adopt surface exists**: no reconnect/attach route
  (`api.rs:951–959`; `reset_session` on every start); session ids
  (`real-session-{sequence:04}`) come from an in-memory counter that
  resets — ids are *reused* across restarts; `RealSession` carries
  derived state with no reconstruction path.

## The Crash Windows (Under The Write-Ahead Intent Protocol)

The token-only-exists-after-the-RPC fact forces a two-record design
(see `02-` item 1). The windows, correctly enumerated against it:

1. **Spurious intent**: intent persisted, CreateVm/RestoreSnapshot
   failed → intent refers to nothing; reconcile tolerates
   (no matching slot / lookup-miss) and clears it.
2. **Dangling intent**: RPC succeeded, crash before the lease record
   landed (microseconds) → the bridge knows *a slot may exist* but
   holds no token. Bridge-side undestroyable; the requirement for a
   destroy-by-slot-id path was delivered to the hypervisor round-3
   dir at filing (`06-bridge-requirement.md` there); absent it, the
   documented runbook residual (worker restart) applies.
3. **Worker restarted underneath**: persisted token now stale / slot
   recycled → reconcile treats discriminated-StaleLease/missing as
   already-clean and clears the record.
4. **Destroyed but record remains**: DestroyVm succeeded, crash before
   record removal → stale record; benign *only* under a mandated
   **destroy-then-remove** ordering (remove-first would recreate
   window 2 with no trace) plus window-3 tolerance on the next pass.
5. **Crash mid-reconcile**: reconcile must be idempotent /
   re-runnable — mostly free once ordering + tolerance hold, but
   stated and tested, not assumed.

Plus two live-bridge behaviors (not crash windows): the stop-path
destroy failure above (record retained + retried — lazy sweep or next
start), and **worker unreachable during startup reconcile** (defined
behavior: serve or refuse, records retained either way).
