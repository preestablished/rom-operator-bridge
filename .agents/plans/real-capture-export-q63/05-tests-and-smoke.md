# Tests And Smoke

## 1. Focused Unit And Integration Tests

Add or extend tests in these areas:

```bash
cargo test --manifest-path service/Cargo.toml --test artifacts
cargo test --manifest-path service/Cargo.toml --test capture
cargo test --manifest-path service/Cargo.toml --test labels
cargo test --manifest-path service/Cargo.toml --test real-backend
```

Required test coverage:

- real backend trigger calls the mock worker capture RPC with the active lease;
- completed real capture writes private payload artifacts;
- completed real capture appends `captures/index.jsonl`;
- job is not completed when payload write fails;
- job is not completed when index append fails;
- idempotent replay returns the same job;
- concurrent real capture is rejected or returns the existing active job
  according to existing API behavior;
- public responses and websocket events contain no private values;
- completed real capture can receive a `needs_review` label draft;
- stale capture ids from previous sessions are rejected.

## 2. Mock Hypervisor Test Shape

Extend `service/tests/real-backend/main.rs` mock worker so it can assert:

- the bridge requested capture for the same lease token/slot as the active
  session;
- the configured capture spec ref is passed;
- the worker can return deterministic fake private payload bytes;
- worker failures are converted into sanitized public failures.

The mock payload should be obviously fake test data. Do not add real capture
payload samples to the repository.

## 3. Quality Gates

Run the focused gates first:

```bash
cargo fmt --manifest-path service/Cargo.toml -- --check
cargo test --manifest-path service/Cargo.toml --test artifacts
cargo test --manifest-path service/Cargo.toml --test capture
cargo test --manifest-path service/Cargo.toml --test labels
cargo test --manifest-path service/Cargo.toml --test real-backend
```

Then run the broader service gates:

```bash
cargo test --manifest-path service/Cargo.toml --all-targets
scripts/quality-gate.sh
```

If a host dependency prevents a broad gate from running, append a sanitized note
to `q63` with the exact command and non-private failure class.

## 4. Optional Private Host Smoke

If operator-approved private inputs are available, run a private smoke after the
mock tests:

1. Start a real bridge session.
2. Trigger one capture.
3. Poll until terminal status.
4. Verify `captures/index.jsonl` exists under the private root.
5. Verify required payload refs exist and are non-empty.
6. Apply a `needs_review` label to the public capture id.
7. Verify `label-draft.json` exists privately.
8. Stop the session.
9. Sweep public bodies and sanitized notes for forbidden literals.

This private smoke may strengthen confidence, but `r77` owns the formal
operator-private one-capture label smoke. Do not close `r77` from this work.

## 5. Leak Sweeps

Before committing or writing bead notes:

```bash
rg -n "private-root-placeholder|capture-spec-placeholder" service docs .agents || true
git diff --check
```

For private run evidence, use a private forbidden-literals file and quiet
`rg -q -F -f` sweeps. Do not print matching lines.
