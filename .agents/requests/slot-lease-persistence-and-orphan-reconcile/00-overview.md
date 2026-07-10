# Request: Stop Leaking Worker Slots On Bridge Restart — Persist Leases, Reconcile Orphans (Bead 72o)

> **CURRENT STATUS (2026-07-10):** Still open and still production-relevant.
> Read `04-current-status-2026-07-10.md` before implementation.

## Who Is Asking

The phases track, round 3 (2026-07-07). The round-3 skip-sweep caught
that bead `72o` — a live, observed production leak — is covered by
**neither** filed request in this repo: round-1 lists it as "tracked
separately"; round-2 mentions only the caveat it imposes on calendars;
the hypervisor-side cluster (their round-3
`lease-semantics-doc-and-orphan-slot-warn/`) delivers only the
worker-side WARN and semantics doc. The durable fix is bridge-owned
and unowned by any filing. Why this is a request and not a bead,
given the repo already carries two open requests: (a) two-way
choreography with an in-flight hypervisor *decision* (a requirement
delivered before their ruling — bead comments can't carry that);
(b) a production live-verification on a calendar four requests share;
(c) crash-window design that benefits from pre-implementation review —
which, in the event, found two missing windows and an ordering flaw
before a line was written.

## Why rom-operator-bridge, Why Now

- **The leak is real and recurring.** Observed 2026-07-01: the bridge
  holds VM leases in memory only; start does RestoreSnapshot/CreateVm,
  stop does DestroyVm — so a bridge restart mid-session loses the
  lease and DestroyVm never fires. Four slots sat paused at identical
  icount 641343512 across a day's deploys until a worker restart
  cleared them. Every bridge redeploy with an active session leaks a
  slot; the worker has only 4.
- **The bead's own remediation options are bridge-side**: persist
  leases to the private root and destroy orphans on startup, or an
  admin/reconcile path that destroys slots the bridge doesn't know
  about. The worker-side "also consider" tail (expiry, WARN) is the
  hypervisor round-3's territory.
- **Every choreography in the queue leans on this repo's restart
  windows** (`72o` is cited by four sibling requests as the caveat to
  schedule around). The leak is why restarts are scary; fixing it
  makes the shared-box calendar cheaper for everyone.

## The Ask In One Paragraph

Adopt a write-ahead two-record protocol (intent before the RPC; the
token-bearing lease record when it returns — the token only exists in
the response, so single-record designs leave an unbounded blind spot),
with destroy-then-remove ordering and a consciously amended
token-storage invariant; on startup, reconcile before serving traffic
— destroy-by-default (re-adopt is disqualified today, reasons
recorded), discriminated-StaleLease tolerance, dangling-intent
accounting; repair the restartless stop-path leak too; build the
mock's missing lease modeling and run the nine-case matrix; deliver
the window-2 destroy-by-slot-id requirement to the hypervisor's
decision **at filing** (done — `06-bridge-requirement.md` in their
round-3 dir); then one SIGKILL live verification on a synthetic
session during an owned window, closing `72o` and retiring the
restart caveat from the ecosystem's choreography notes.

## Files In This Request

| File | Contents |
|---|---|
| `01-current-state.md` | Evidence: the leak mechanics, current lease handling, prior incidents |
| `02-requested-work.md` | The ask, acceptance criteria, out of scope |
| `03-verification-offer.md` | Choreography with the hypervisor round-3; handback |
