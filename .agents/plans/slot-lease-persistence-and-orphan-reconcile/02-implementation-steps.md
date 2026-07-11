# Implementation Steps

## 1. Establish Tracking And Resolve Current Context

- Claim `rom-operator-bridge-72o` before implementation.
- File the requested adjacent bead for persisted session/run sequencing; note
  that IDs currently reset and are reused, but keep that fix out of this work.
- Re-read the paired hypervisor resolution and decision at implementation time.
  Its current accepted outcome is deferral of tokenless reconciliation, so the
  bridge must retain dangling intents and document worker restart recovery.
- Re-resolve all source anchors; the request's line numbers predate current
  `main`.

## 2. Add The Durable Lease Store

- Add serializable versioned intent/lease structs and strict parsing in a new
  focused module. Keep raw token fields private and implement custom `Debug`
  or omit `Debug` so accidental formatting cannot reveal them.
- Add collision-resistant operation-ID generation. If a new crate is used,
  update `service/Cargo.toml` and lockfile with only the needed features.
- Add the two lease directories to private-root preparation, or have the store
  create them through existing safe descendant-directory helpers.
- Add safe list/read/atomic-write/durable-remove operations. Reuse
  `write_private_file_atomic`; extend `BridgePrivateConfig` rather than using
  unchecked `std::fs` calls from backend logic.
- Unit-test record round trips, permissions, fsync-oriented atomic promotion,
  idempotent removal, duplicate intent+lease loading, malformed data, and path
  safety.

## 3. Preserve Worker Error Semantics

- Change the worker stop command/result so `DestroyVm` can return a distinct
  stale-lease result after decoding protobuf status details.
- Add a direct compatible `prost` dependency to `service/Cargo.toml`; decoding
  `Status::details()` as `dh::ErrorDetail` requires `prost::Message`.
- Add a small decoder that requires `FAILED_PRECONDITION`, successfully
  decoded `dh::ErrorDetail`, and exact code `stale_lease`. Everything else is
  unavailable/ordinary failed precondition, except exact `no_such_slot`, which
  is also benign for destroy reconciliation. Test worker-shaped encoded
  details, malformed/empty bytes, alternate codes, and code/status mismatch.
- Keep public API errors sanitized. Logging may include RPC operation and safe
  classification only, never `Status.message()`, token, endpoint, or record
  contents.
- Replace/augment the mock's single optional slot with at least two slot entries
  carrying the minted token/generation. Generate configurable tokens, validate
  slot and token on destroy, model slot reuse with a new token, return a
  protobuf-encoded `ErrorDetail { code: "stale_lease" }` for recycled tokens,
  and retain wrong-state modeling as a different detail.

## 4. Integrate Write-Ahead Allocation

- Resolve the start command and its source identity without leaking private
  values, allocate a new operation ID, and durably persist the intent before
  `worker.start`.
- After the worker returns, persist the full lease record, then durably remove
  the intent. Store `operation_id` in `RealSession`.
- Classify allocation failures by outcome certainty. Clear only errors the
  contract proves are pre-allocation; retain ambiguous timeout/disconnect/lost
  response intents. Add a mock mode that allocates and then drops the response.
- If lease persistence and immediate destroy both fail, retry persistence and
  keep an in-memory token-bearing pending cleanup under the lifecycle guard;
  never return to an allocation-capable state while the token is only in RAM.
- Refactor the manifest/event rollback to use the shared destroy-then-remove
  helper. Explicitly test failures at intent write, worker allocation, lease
  write, intent removal, manifest write, destroy, and record removal.
- Ensure `starting` is cleared on every return path, preferably through a guard
  or a single structured cleanup path.

## 5. Integrate Startup And Lazy Reconciliation

- Add a reconciliation report/state to `RealBackend`. Run a bounded attempt
  synchronously from `RealBackend::new` as called by `AppState::from_config`,
  before the router is returned, without changing the constructor to `Result`.
- Skip worker RPCs for an empty store or intent-only store. List slots only
  when valid lease records exist.
- Use `ListSlots` to distinguish missing slot IDs, followed by token-validating
  `DestroyVm` for present slots. Use exact stale-lease details for benign
  cleanup.
- Retain and count dangling intents because the paired worker decision deferred
  destroy-by-slot-id. Retain invalid records and fail closed. Add the audited
  operator command that clears selected dangling intents only after explicit
  stopped-bridge, restarted-worker, and empty/full-capacity confirmation.
- Gate new real starts on a clean report. When blocked and no in-memory session
  exists, rerun reconciliation before each start attempt so worker recovery can
  unblock without restarting the bridge.
- Log the fixed numeric summary and a separate sanitized dangling-intent
  operator message. Add the recovery policy to the runbook/service docs.

## 6. Repair Every Live Cleanup Path

- Refactor `stop_session` so a failed `DestroyVm` cannot discard the only lease
  copy. The durable record stays and the next start retries reconciliation.
- Route start rollback and `quarantine_after_input_artifact_failure` through
  the same helper. Search every `worker.stop` call and prove each is
  destroy-then-remove.
- Keep API session state internally consistent after a failed stop and ensure a
  new session cannot allocate until the retained record has reconciled.
- If destroy succeeds but durable record removal fails, return unavailable,
  leave no active in-memory session, retain the record, and let reconciliation
  prove the slot absent before removal. Emit `session_stopped` only after
  durable cleanup succeeds or is later reconciled.
- Add the dedicated lifecycle mutex described in the design and test concurrent
  stop/start plus two concurrent starts.

## 7. Amend The Token Invariant Deliberately

- Update `assert_private_artifacts_do_not_contain_lease` into an allowlist
  audit: decoded, validated `leases/active/<operation_id>.json` is the sole
  durable file class that may contain the token; run
  manifests, events, padlogs, captures, intents, summaries, public JSON, and
  every other private artifact may not.
- Add an assertion that the lease record is mode `0600` beneath the validated
  private root and disappears only after successful/benign cleanup.
- Keep the token as a sanitizer forbidden literal for every public/log
  projection. Update the static redaction gate only as narrowly as needed; do
  not broadly exempt `leases/` from secret scanning if a path-specific rule can
  express the intended exception.
- Assert atomic temporary files are `0600`, are never included in public
  evidence, and are cleaned or safely ignored after a crash. Do not log record
  contents, untrusted filenames, or operation IDs in routine summaries.
