# Test And Verification Plan

## Required Nine-Case Matrix

Implement these primarily in `service/tests/real-backend/main.rs`, using fresh
backend instances over the same temporary private root and mock worker to model
process restart:

| Case | Setup | Required assertion |
|---|---|---|
| Restart with active session | Start, retain worker, drop backend, construct another | startup destroys lease, records clear, full capacity returns before a new start |
| Restart with no sessions | Empty store and worker | no destroy, clean zero-count summary, starts remain enabled |
| W1 spurious intent | In-process RPC returns a contractually proven pre-allocation error | durably clear the intent; after restart an unmatched intent is indistinguishable from W2 and is conservatively retained |
| W2 dangling intent | Intent exists and worker has an allocated slot | retain/count, do not call tokenless destroy, block real starts, emit recovery guidance |
| W3 stale token | Lease record token differs from recycled worker lease | exact `stale_lease` detail clears record; bare/different failed-precondition does not |
| W4 destroyed but record remains | Lease record exists, slot absent | clear record without destroy and remain ready |
| W5 crash mid-reconcile | Leave one cleaned and one retained record, run twice | no duplicate harm; counts and remaining files converge deterministically |
| Stop destroy failure | First destroy unavailable, later succeeds | record survives failure; next start reconciles it before allocating |
| Worker unreachable at startup | Seed a lease record, make List/destroy unavailable | records survive, construction is time-bounded, real start fails closed, later retry unblocks after worker recovery |

The original request describes a spurious intent as clearable on lookup miss,
but an intent deliberately lacks a slot ID. With the worker's accepted decision
to defer tokenless reconciliation, no safe lookup can prove which allocation an
intent represents. The implementation should therefore use the conservative
W1 expectation above unless the worker contract gains an allocation correlation
key before coding. Only the same process observing a contractually guaranteed
pre-allocation failure can prove W1. Record this resolved ambiguity in the
request resolution.

## Additional Focused Tests

- Promotion crash points: intent only; intent plus lease; lease only.
- Allocation RPC failure removes a provably spurious pre-RPC intent when the
  same process observed the failure; removal failure leaves it accounted.
- Lease write failure triggers immediate destroy and never publishes a session.
- Lease write failure followed by destroy failure preserves the token in a
  durable retry or in-memory pending cleanup and blocks all allocation.
- Response-lost-after-allocation retains the intent as dangling.
- Manifest/event failure keeps the lease record if rollback destroy fails.
- Wrong-state `FAILED_PRECONDITION`, malformed details, empty details, and a
  different detail code never delete a record.
- Duplicate records, unknown schema, corrupt JSON, unsafe token encoding, and
  symlinked paths fail closed without leaking contents.
- Reconcile order is deterministic and does not hold backend mutexes during
  RPC/filesystem work.
- Empty-store startup performs no worker RPC; intent-only startup also avoids
  `ListSlots` and blocks conservatively.
- The operator intent-clear command refuses a running bridge, absent explicit
  worker-restart/full-capacity acknowledgement, active lease files, invalid
  records, and broad/all-record deletion; its selected success path fsyncs
  removal and emits only audit-safe identifiers/counts.
- Concurrent stop/start and concurrent starts serialize across the complete
  lifecycle transition.
- Destroy success plus record-removal failure returns unavailable, leaves no
  in-memory active session, and converges on the next reconcile.
- Every public response and reconcile log passes the sanitizer with the token,
  private root, worker endpoint, snapshot reference, and session secret loaded
  as forbidden literals.
- Existing real-backend start/stop, create/restore, cancel-window, capture,
  input, and websocket tests remain green.

## Local Quality Gates

Run focused tests while iterating, then the repository gate with the remembered
Node 22 requirement:

```bash
cd service && cargo test --test real-backend
PATH="$HOME/.nvm/versions/node/v22.22.0/bin:$PATH" bash scripts/quality-gate.sh
git diff --check
```

Adjust the quality-gate invocation to repository root if the script assumes
that working directory. Capture exact commands and results in
`.agents/requests/slot-lease-persistence-and-orphan-reconcile/04-resolution.md`.
File beads for unrelated pre-existing failures; do not hide them or expand this
change to repair unrelated code.

## Live Verification

Only after automated gates pass and an operator-owned restart window is
confirmed:

1. Record sanitized `ListSlots` capacity before the test.
2. Start a clearly synthetic operator-driven real-worker session.
3. Confirm the active lease record exists without printing its contents.
4. `SIGKILL` the bridge; do not perform graceful shutdown.
5. Restart only the bridge.
6. Capture the sanitized reconciliation summary showing the orphan destroyed.
7. Record `ListSlots` at full capacity afterward and prove no worker restart
   occurred in the interval.

Store only sanitized evidence. Never copy a token, private path, endpoint, or
user payload into git. If no window is available, implementation can merge but
`72o` and the request remain open with the live step explicitly blocked; do not
claim full acceptance.
