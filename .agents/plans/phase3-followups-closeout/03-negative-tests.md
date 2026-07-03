# Step 03 — Negative Tests For The Disclosed Gaps

Repo: `~/git/preestablished/reference-workload`. Create one bead per gap
(or one bead with a checklist, per repo convention) before coding. These
are the reviewer-identified gaps from the 07-verification note: each is
**real, load-bearing code whose failure branch no test exercises**. The
standard is the one this project already uses elsewhere: a check that
cannot be shown to fail proves nothing (see the vm-suite `--nondet-test`
precedent, and guest-sdk's corrupted-region `ReverifyRegions` test).

## Gap A — Harness `RegionRegFailed` Hard-Fault Before Ready

- Where: `crates/refwork-harness/src/agent.rs` (~lines 103–108: agent-mode
  registration failure → `RegionRegFailed` fault + abort) and
  `runner.rs` (~264–272: `SetupError::AgentRegistration` propagation).
  Only the standalone-degrade branch (`AgentUnavailable → continue`) has
  a test today; the *safety-critical* branch — a failed registration must
  make `Ready` unreachable — is untested.
- Why it's untested: `detguest_sdk::register_region` isn't mockable from
  the harness crate.
- Approach: introduce a narrow test seam — e.g. a `RegionRegistrar`
  trait (or `fn` injection) owned by the harness with the production
  impl delegating to `detguest_sdk`, and a test impl returning a
  non-`AgentUnavailable` error. Assert: fault emitted, `Ready` never
  sent, process path aborts. Keep the seam private to the crate; do not
  change the production call shape.

## Gap B — Lock-File Mismatch Refusal (Both Locks)

- Where: `xtask/src/image.rs` — kernel BLAKE3 refusal (~526–546) and
  guest-sdk rev refusal (~551–607). Only shape-tests exist
  (`xtask/tests/image_inputs.rs:96-114`); neither refusal branch is
  exercised.
- Approach: refactor the two checks to take their inputs (artifact path /
  expected hash; checkout path / expected rev) as parameters, then unit
  test: (1) wrong-content bzImage in a tempdir → error message contains
  the "rebuild in guest-sdk or deliberately bump the lock" guidance and
  the mismatching hashes; (2) a scratch git repo at a different rev →
  rev-mismatch refusal naming both revs. Do not shell out to the real
  sibling checkouts in these tests.

## Gap C — Restore-Continuity Negative Test

- Where: `crates/refwork-verify/src/vm_suite.rs` restore-continuity leg
  (~276–284, 341–387) and its tests (`tests/vm_suite.rs`). The existing
  `--nondet-test` negative covers the double-run leg only.
- Approach: extend the mock worker to support a post-restore divergence
  mode (e.g. flip one region byte in the restored slot's hash stream at
  frame K), and add a test asserting the restore-continuity comparison
  fails naming the first divergent frame. Mirror the double-run negative
  test's structure (`tests/vm_suite.rs:94-113`).

## Gap D (Optional, Low) — Capture-Alarm Failure Stage

- Where: `crates/refwork-verify/src/vm_first_room.rs` — capture alarms
  currently surface under the generic `"run"` failure stage while other
  failure modes get distinct names.
- Approach: add a distinct `"capture"` stage string + a mock-worker test
  that triggers an alarm and asserts the stage. Do it only if it stays a
  small diff; otherwise fold into the bead as a note.

## Exit Criteria

- Each implemented gap has a test that **fails when the guard is
  reverted** (verify by temporarily breaking the branch, same discipline
  as this session's fetch-binding regression test).
- Workspace suite still green (`cargo test --workspace --locked`), fmt +
  clippy clean.
- Beads closed with reasons; commits reference the beads; push per the
  step-01 authorization pattern (docs+tests on `main`, verify
  `origin/main..main` contains only this plan's commits first).
