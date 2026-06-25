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

- real-mode `POST /api/capture/trigger` reaches the mock worker capture RPC
  instead of staying on synthetic/API-only capture state;
- real-mode `GET /api/capture/jobs/:id` polls backend job state and then updates
  only sanitized API projection state;
- real backend trigger calls the mock worker capture RPC with the active lease;
- completed real capture writes private payload artifacts;
- completed real capture appends `captures/index.jsonl`;
- the emitted `captures/index.jsonl` row contains matching payload
  hashes/lengths and no inline raw bytes;
- job is not completed when payload write fails;
- job is not completed when index append fails;
- idempotent replay returns the same job;
- concurrent real capture is rejected or returns the existing active job
  according to existing API behavior;
- public responses and websocket events contain no private values;
- `CaptureState` becomes labelable only after durable index append;
- completed real capture can receive a `needs_review` label draft;
- stale capture ids from previous sessions are rejected.

## 2. Mock Hypervisor Test Shape

Extend `service/tests/real-backend/main.rs` mock worker so it can assert:

- the bridge requested capture for the same lease token/slot as the active
  session;
- the configured capture spec ref is passed;
- the worker can return deterministic fake private payload bytes;
- worker failures are converted into sanitized public failures.

Mock payload bytes, lease tokens, slot ids, spec refs, endpoints, and worker
errors must all be synthetic sentinel values. Do not add real capture payload
samples or real operator refs to the repository.

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

Private smoke commands must be quiet/pass-fail only. Do not `cat`,
pretty-print, screenshot, paste, or log raw API bodies, `captures/index.jsonl`
rows, payload refs, private paths, capture ids from private runs, worker stderr,
or label draft contents. Public notes may only say whether authorized private
smoke was not run, failed with a sanitized class, or passed sanitized checks.
This private smoke may strengthen confidence, but `r77` owns the formal
operator-private one-capture label smoke. Do not close `r77` from this work.

## 5. Leak Sweeps

Before committing or writing bead notes, create a forbidden-literals file outside
the repository under a private run directory with mode `0600`. Never commit it.
Sweep source, docs, `.agents`, staged diffs, generated public response captures,
websocket captures, and bead note text before closeout. Use quiet scans only:

```bash
test -d "$PRIVATE_RUN_ROOT"
test -f "$FORBIDDEN_LITERALS_FILE"
git diff --check
set +e
rg -q -F -f "$FORBIDDEN_LITERALS_FILE" service docs .agents
source_sweep_status=$?
git diff --cached --name-only --diff-filter=ACMRT > "$PRIVATE_RUN_ROOT/staged-files.private.txt"
if [ -s "$PRIVATE_RUN_ROOT/staged-files.private.txt" ]; then
  xargs -r rg -q -F -f "$FORBIDDEN_LITERALS_FILE" \
    < "$PRIVATE_RUN_ROOT/staged-files.private.txt"
  staged_sweep_status=$?
else
  staged_sweep_status=1
fi
set -e
case "$source_sweep_status" in
  0) echo 'forbidden literal found in source/docs/agent plan files' >&2; exit 1 ;;
  1) ;;
  *) echo 'source forbidden-literal sweep errored' >&2; exit 1 ;;
esac
case "$staged_sweep_status" in
  0) echo 'forbidden literal found in staged files' >&2; exit 1 ;;
  1) ;;
  *) echo 'staged forbidden-literal sweep errored' >&2; exit 1 ;;
esac
```

For private run evidence, use a private forbidden-literals file and quiet
`rg -q -F -f` sweeps. Do not print matching lines.
