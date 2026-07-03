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

Add an env-gated integration test (`REFWORK_VM_TESTS=1`, matching the
`vm-gates.yaml` convention; plain `cargo test` skips it) that:

1. Builds/locates `dh-workerd` from a **clean hypervisor worktree** —
   follow the deployed pattern: the operator keeps one at
   `~/git/preestablished/.dh-clean-ff1e88c` (do not modify it; it backs
   the deployed binary). Either use its existing `target/debug/dh-workerd`
   or build in a fresh `git worktree` of your own. Never assume the main
   hypervisor checkout is clean — it carries in-flight edits.
2. Launches it with scratch paths:
   `dh-workerd serve --uds <tmpdir>/grpc.sock --image-cache <tmpdir>/cache
   --snapstore-uds <tmpdir>/snapstore.sock` (a snapstore may need to be
   stubbed or launched likewise — check `--help`; if a live snapstore is
   required for startup, launch `snapstore-server` from its sibling repo
   the same scratch way). **Never** touch `/run/dh/grpc.sock`.
3. Through `refwork-dh-client` over the scratch UDS, asserts:
   - `GetWorkerInfo`/`ListSlots` round-trip (transport + codec proof);
   - `RestoreSnapshot` with a bogus ref → the distinct, sanitized
     failure `vm-first-room` maps for it (error-mapping proof);
   - connection-refused (worker stopped) → the client's
     unavailable-path error, not a hang (timeout proof).
4. Tears the worker down and leaves no state outside the tempdir.

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
