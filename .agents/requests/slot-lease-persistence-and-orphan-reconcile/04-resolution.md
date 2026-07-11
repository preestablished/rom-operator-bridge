# Implementation Resolution

## Decisions

- The bridge uses versioned write-ahead intent and token-bearing active records, keyed by independent UUID v4 operation IDs, with lease-write-before-intent-removal promotion and destroy-before-record-removal cleanup.
- Startup and lazy reconciliation destroy rather than re-adopt. Re-adoption remains unsafe because there is no client attach protocol, process-local session/run IDs restart, and `RealSession` derived state is not reconstructible.
- The paired hypervisor decision deferred tokenless reconciliation. Unmatched intents therefore remain fail-closed evidence until the bridge is stopped, the worker restarted, full capacity verified, and selected intents acknowledged with the audited bridge command.
- Exact protobuf `stale_lease` and `no_such_slot` details are benign for destruction; bare or differently coded failed-precondition responses retain evidence.
- Adjacent sequence persistence is tracked by `rom-operator-bridge-u4f` and is not part of this implementation.

## Automated evidence

On 2026-07-11:

- `cd service && cargo test --test lease_store`: 8 passed, including strict schema/token validation, atomic replacement/removal, crash-temporary handling, runtime-lock contention, and operator-command refusals/success.
- `cd service && cargo test --test real-backend`: 43 passed, including the required nine crash/recovery shapes, permission-injected persistence/removal failures, RAM-held token recovery, malformed/wrong-state details, two-slot mock reconciliation, concurrent starts, and concurrent stop/start.
- `PATH="$HOME/.nvm/versions/node/v22.22.0/bin:$PATH" bash scripts/quality-gate.sh`: passed. This included all service tests, 89 UI tests, the production UI build, and the static redaction scan.
- `git diff --check`: passed.
- `cargo clippy --all-targets --all-features` passes for the changed code after allowing the repository's existing unrelated lint classes. The pre-existing all-target `-D warnings` cleanup is tracked by `rom-operator-bridge-w21`.

The matrix covers restart with an active session, empty restart, proven pre-allocation rejection, dangling/lost-response intent, stale token, missing slot, repeated reconciliation, failed-stop retry, and initially unavailable worker recovery. The mock carries multiple independently tokened slots, validates slot/token pairs, models recycled tokens, and returns encoded stale/no-slot details.

## Remaining live evidence

The required live SIGKILL exercise remains restricted to an explicitly owned operator window and must not use a user session. It has not been performed in this session. Bead `rom-operator-bridge-72o` therefore remains in progress until sanitized before/after `ListSlots` capacity and no-worker-restart evidence are captured. The historical restart-orphan caveats in the sibling request choreography are not yet retired.
