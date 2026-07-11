# Design And Invariants

## Durable Layout

Add a small lease store owned by the real backend, preferably in a focused
module such as `service/src/lease_store.rs`, backed by the existing
`BridgePrivateConfig` safety and atomic-write primitives.

Use these private-root paths:

```text
leases/intents/<operation_id>.json
leases/active/<operation_id>.json
```

`operation_id` must be collision-resistant and generated independently for
each allocation attempt (for example UUID v4). Do not key files by
`session_id`, `run_id`, slot ID, timestamp alone, or the in-memory sequence:
session/run sequences reset after restart, and slot IDs are recycled.

Both record types have `schema_version: 1`, `operation_id`, `session_id`,
`run_id`, base snapshot identity or an explicit create-VM source marker,
`created_at`, and an allocation kind (`restore_snapshot` or `create_vm`). The
lease record additionally has `slot_id`, the raw lease token, and
`lease_recorded_at`. Use an unambiguous token encoding such as lowercase hex;
reject malformed schema, paths, slot IDs, and token encodings without ever
printing their values.

Create `leases`, `leases/intents`, and `leases/active` with the existing private
directory mode. Files remain mode `0600`. Extend `BridgePrivateConfig` with a
safe durable removal operation: validate the relative path and existing file,
unlink it, and fsync the parent directory. Treat not-found as idempotent success
only after path validation. Add focused tests for modes, symlink rejection,
atomic replacement, durable removal, malformed records, and unknown schema.

## Allocation State Machine

The required ordering is:

```text
write+fsync intent
  -> CreateVm/RestoreSnapshot
  -> write+fsync lease record
  -> remove+fsync matching intent
  -> publish RealSession in memory
```

Writing the lease record before deleting the intent is mandatory. A crash may
leave both records, but must never create a new no-record gap. Reconciliation
deduplicates them by `operation_id`: the lease record is authoritative, and the
matching intent is removed only after that lease is reconciled. Include the
`operation_id` in `RealSession` so normal stop removes exactly the right file.

Failure behavior:

- Intent persistence fails: do not call the worker.
- Allocation RPC returns a failure proven by the worker contract to occur
  before allocation: durably remove the intent; if removal fails, retain it
  for startup accounting and return unavailable. Timeouts, disconnects,
  cancellation, malformed/missing responses, and other ambiguous errors may
  happen after allocation and must retain the intent as dangling.
- Lease persistence fails after the RPC returns: retry the atomic write once;
  if it still fails, immediately destroy using the in-memory token. Clear the
  intent only if destroy succeeds. If destroy fails too, retry persistence and
  retain an in-memory pending-cleanup entry holding the token for the life of
  the process. Block allocation and never voluntarily discard the last usable
  token copy. Return unavailable only after destruction is confirmed or the
  token-bearing record is durable; if storage remains unavailable, keep retrying
  on later lifecycle operations and emit only sanitized classification/counts.
- Manifest/event setup fails after the lease record exists: destroy first;
  remove the lease record and matching intent only after successful destroy.
  On destroy failure, keep the records for reconciliation.
- Never expose the session through `inner.active` until the lease record is
  durable and the intent promotion has completed or left only a harmless
  duplicate intent.

## Destruction Invariant

All cleanup paths use one helper with the invariant:

```text
DestroyVm(recorded token) -> durable record removal
```

Never remove first. This helper must cover explicit stop, start rollback,
input-artifact quarantine, and startup/lazy reconciliation. On a transport or
ordinary worker failure it retains the record. On a specifically decoded
`ErrorDetail.code == "stale_lease"`, exact `no_such_slot`, or after `ListSlots`
proves the slot ID is absent, it treats the lease as already clean and removes
the record. This also closes the race in which a slot disappears after the
initial list and before destroy.

Do not classify every `FAILED_PRECONDITION` as stale: wrong-state failures use
the same gRPC status. Decode `dh::ErrorDetail` from `Status::details()` and add
a distinct internal result such as `RealWorkerFailure::StaleLease`; malformed,
missing, or differently coded details remain failures and retain the record.

For explicit stop, do not lose the session before cleanup. It is acceptable to
remove it from `inner.active` to preserve current API semantics only after the
durable lease record is known to exist; a failed destroy returns unavailable,
keeps the record, and makes the next reconciliation attempt run before any new
allocation. Avoid holding the backend mutex during worker RPCs or filesystem
I/O.

## Startup And Retry Policy

Construct the worker thread and synchronously run one bounded reconciliation
attempt during real-backend construction in `AppState::from_config`, before
`router` begins serving. Keep `RealBackend::new -> Self`; store the failed/clean
readiness report internally so health and diagnostic HTTP can still start.
Existing worker connect/RPC deadlines bound this attempt; add a focused test
that a hung/unreachable worker cannot block construction indefinitely.

Load and validate records deterministically first. If the store is empty, mark
ready without contacting the worker. If it contains only unmatched intents,
retain/block without `ListSlots`. Call `ListSlots` once only when at least one
valid lease record requires presence classification, then:

1. For each valid lease whose slot is absent, remove it as already clean.
2. For each valid lease whose slot exists, call `DestroyVm`; remove only on
   success or discriminated stale lease.
3. For an intent with a same-operation lease, defer its removal until the
   lease outcome is clean; then remove both.
4. For an intent without a lease, count and retain it as dangling. Log that an
   unaccounted slot may exist and name the complete operator recovery: stop the
   bridge, restart the worker, verify empty/full capacity, use the audited
   intent-clear tool, then resume the bridge.
5. Quarantine malformed/unknown-version files by leaving them in place,
   reporting counts and blocking real allocations; do not delete evidence.

Choose fail-closed behavior for real sessions: if the worker is unreachable,
any record is malformed, or a recoverable lease remains, the HTTP service may
start for health/diagnostic surfaces but `start_session` in real mode returns
the existing sanitized unavailable response. Retain all records. Retry
reconciliation at the beginning of every later real `start_session` attempt;
this provides recovery without adding an unbounded background thread. A set
containing only dangling intents is also allocation-blocking because capacity
cannot be proven safe under the hypervisor's accepted deferral.

Add a narrowly scoped, bridge-owned one-shot operator command/tool that lists
only counts and operation IDs, validates the private-root marker/modes, requires
the bridge to be stopped plus explicit confirmation that the worker was
restarted and `ListSlots` is empty/full-capacity, and durably removes selected
dangling intents. It must refuse active lease records and malformed files, log
an audit-safe action, and have tests for every refusal and success path. This is
record-state acknowledgement after external recovery, not tokenless destroy.

Add `lifecycle: Arc<Mutex<()>>` separately from `inner` and hold it across each
complete start/reconcile/stop/quarantine transaction, including filesystem and
worker calls. Never hold `inner` during those calls; re-check `inner.active`
after acquiring the lifecycle guard. Lazy reconcile runs with the guard held,
only when no session is active, and before allocation begins. Reconciliation
is idempotent and cannot race destruction against stop or start.

Emit one sanitized summary per pass with numeric fields only:
`found_leases`, `found_intents`, `destroyed`, `stale_cleaned`, `missing_cleaned`,
`dangling`, `invalid`, `retained`, and `ready_for_real_sessions`. Never include
tokens, private paths, worker endpoints, snapshot IDs, session IDs, or raw
worker messages.
