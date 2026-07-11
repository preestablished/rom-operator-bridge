# Implementation Resolution

## Decisions

- The bridge uses versioned write-ahead intent and token-bearing active records, keyed by independent UUID v4 operation IDs, with lease-write-before-intent-removal promotion and destroy-before-record-removal cleanup.
- Startup and lazy reconciliation destroy rather than re-adopt. Re-adoption remains unsafe because there is no client attach protocol, process-local session/run IDs restart, and `RealSession` derived state is not reconstructible.
- The paired hypervisor decision deferred tokenless reconciliation. Unmatched intents therefore remain fail-closed evidence until the bridge is stopped, the worker restarted, full capacity verified, and selected intents acknowledged with the audited bridge command.
- Exact protobuf `stale_lease` and `no_such_slot` details are benign for destruction; bare or differently coded failed-precondition responses retain evidence.
- Allocation intents are cleared only for `RESOURCE_EXHAUSTED` with exact worker `ErrorDetail.code` `no_free_slot` or `not_enough_cores`. The paired worker authority is `dh-worker/src/service.rs::slot_error_code`, `slot_error_to_status`, and `install_allocated_runtime`: those codes arise from allocation preflight and failed runtime construction is rolled back before an error response. Generic status codes, mismatched details, and lost responses retain the intent.
- Adjacent sequence persistence is tracked by `rom-operator-bridge-u4f` and is not part of this implementation.

## Automated evidence

On 2026-07-11:

- `cd service && cargo test --test lease_store`: 9 passed, including strict per-field schema/token validation, atomic replacement/removal, crash-temporary handling, runtime-lock contention, and operator-command refusals/success.
- `cd service && cargo test --test real-backend`: 45 passed, including the required nine crash/recovery shapes, permission-injected persistence/removal failures, RAM-held token recovery, malformed/wrong-state details, exact allocation error classification, malformed returned-token rollback, recursive private-artifact token auditing, wrong-session stop preservation, two-slot mock reconciliation, concurrent starts, and concurrent stop/start.
- `PATH="$HOME/.nvm/versions/node/v22.22.0/bin:$PATH" bash scripts/quality-gate.sh`: passed. This included all service tests, 89 UI tests, the production UI build, and the static redaction scan.
- `git diff --check`: passed.
- `cargo clippy --all-targets --all-features` passes for the changed code after allowing the repository's existing unrelated lint classes. The pre-existing all-target `-D warnings` cleanup is tracked by `rom-operator-bridge-w21`.

The matrix covers restart with an active session, empty restart, proven pre-allocation rejection, dangling/lost-response intent, stale token, missing slot, repeated reconciliation, failed-stop retry, and initially unavailable worker recovery. The mock carries multiple independently tokened slots, validates slot/token pairs, models recycled tokens, and returns encoded stale/no-slot details.

## Independent review

Two independent post-implementation reviews were applied. The crash/lifecycle review found and verified fixes for wrong-session stop dropping the real active session and overly broad allocation-error intent clearing. The security/acceptance review found and verified recursive private-artifact token auditing, focused strict-schema tests, exact worker token length, and write-time validation. Its follow-up also prompted `PendingCleanup` to retain the original `dh::Lease` separately so even a malformed returned token remains retryable verbatim when persistence and immediate destruction both fail.

The operator tool continues to permit any explicitly enumerated unique subset of operation IDs. It has no wildcard or implicit all-record mode; `--all` is explicitly tested as a refusal. This preserves selected recovery of a single remaining intent without introducing broad deletion.

## Remaining live evidence

The required live SIGKILL exercise remains restricted to an explicitly owned operator window and must not use a user session. It has not been performed in this session. Bead `rom-operator-bridge-72o` therefore remains in progress until sanitized before/after `ListSlots` capacity and no-worker-restart evidence are captured. The historical restart-orphan caveats in the sibling request choreography are not yet retired.
