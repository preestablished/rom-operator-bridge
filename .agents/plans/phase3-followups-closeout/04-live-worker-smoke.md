# Step 04 — Live-Worker Smoke Coverage For `refwork-dh-client`

Repo: `~/git/preestablished/reference-workload`. Bead required.

## The Gap, Scoped Honestly

The live (non-mock) gRPC path — `refwork-dh-client` against a real
`dh-workerd` — has zero automated coverage; every existing test runs
against the in-process mock. The **full** exercise (boot the real image,
restore, run frames) is the operator-coordinated step and stays out of
scope here. But most of the untested surface is reachable *without* a
bootable snapshot: transport, connection, request encoding, error-code
mapping, and the failure paths `vm-first-room` reports on. That slice is
testable today and is what this step covers.

## Approach

Add an env-gated integration test (`REFWORK_VM_TESTS=1` — note
`vm-gates.yaml` only *mentions* this gate in a comment today; you are
introducing its first real use, so also wire the workflow's real-worker
leg to set it; plain `cargo test` skips the test) that:

1. Locates `dh-workerd` via an env var (`REFWORK_DH_WORKERD_BIN`),
   skipping with a clear message if unset. Locally, point it at the
   pre-built binary in the operator's clean worktree
   (`~/git/preestablished/.dh-clean-ff1e88c/target/debug/dh-workerd`) —
   **do not run `cargo build` inside that worktree**; writing to its
   `target/` counts as modifying the artifact backing the deployed
   binary. If a rebuild is ever needed, make your own `git worktree`.
   Never assume the main hypervisor checkout is clean — it carries
   in-flight edits.
2. Launches it with scratch paths and **`--no-snapstore`** (the flag
   exists — resolved, no snapstore process needed for this smoke):
   `dh-workerd serve --uds <tmpdir>/grpc.sock
   --image-cache <tmpdir>/cache --no-snapstore --skip-preflight` (keep
   or drop `--skip-preflight` based on runner; note the binary's
   *defaults* are the deployed paths — every path flag must be passed
   explicitly). **Never** touch `/run/dh/grpc.sock`.
3. Through `refwork-dh-client` over the scratch UDS, asserts:
   - `worker_info()` round-trip (transport + codec proof; the client
     exposes no `list_slots` — do not add one for this test);
   - `restore_snapshot` with a bogus ref → a distinct, sanitized
     failure mapping (under `--no-snapstore` this may surface as a
     snapstore-unavailable class rather than bad-ref — assert whichever
     the worker actually returns, by its stage/code, and record it);
   - connection-refused (worker stopped) → the client's
     unavailable-path error, not a hang (timeout proof).
4. Tears the worker down and leaves no state outside the tempdir.

CI provisioning for the binary (decide and record which): either the
`vm-gates.yaml` real-worker leg checks out determinism-hypervisor at a
pinned rev and `cargo build -p dh-worker` in a job step, setting
`REFWORK_DH_WORKERD_BIN` to the result, or the CI leg is deferred to a
follow-up bead if the build is too heavy for the lane — do not leave the
workflow referencing a host-local path like `.dh-clean-ff1e88c`.

Wire the test into `vm-gates.yaml`'s existing self-hosted lane
(`[self-hosted, intel, kvm]`) alongside the other gated legs.

## Explicitly Not This Step

- Booting the real image, READY snapshot regeneration, frame execution —
  operator-coordinated (07-verification note, "What Remains").
- Any interaction with the deployed worker's socket, slots, or snapstore.

## Exit Criteria

- Gated test green locally via `REFWORK_VM_TESTS=1 cargo test …` on this
  host and wired into `vm-gates.yaml`.
- Plain `cargo test --workspace` unaffected (test skipped, suite count
  noted in the commit message).
- Bead closed with the run evidence (command + result) in its reason.
- When the coordinated boot later lands, this test's harness (scratch
  worker launcher) should be the reusable substrate for the full
  vm-first-room live leg — note that in the bead so the next agent finds
  it.
