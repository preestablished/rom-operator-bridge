# Budget Raise And Delta Validation

## 1. Prepare The Raised Successor Now; Deploy It After The Main Pass

Claim `9bx`. In `service/src/backend.rs`, replace the 200M OOM-containment
constant with an explicitly named effectively-unbounded value matching the
worker's proven incident shape:

```rust
const PLAY_STREAM_ICOUNT_BUDGET: u64 = u64::MAX / 4;
```

Continue sending it as
`run_with_frame_capture_request::Until::IcountBudget(...)`.
Do not omit `until`: the worker rejects a missing arm. Update comments to cite
the fixed-worker prerequisite and distinguish the remaining DHILOG/sealing
granularity concern from the resolved agenda OOM.

Do not delete seamless-reopen handling. Budget completion is still a legal
worker event, and the existing two-segment test is a regression guard. Extend
the `MockWorker` to retain `RunWithFrameCaptureRequest` rather than discard
`_request`, and assert the normal real-client path sends
`Until::IcountBudget(u64::MAX / 4)`. The reopen test should continue to
synthesize an early clean budget end and prove continuity.

Land this as a successor to the telemetry-bearing 200M intermediate commit from
`03-contained-eqb-run.md`. Code review and local gates are not operator-gated;
only deployment/evidence ordering is. Preserve both immutable commits/builds.
In the live window deploy the 200M intermediate first, then this successor.

## 2. Local Quality Gates

Run with Node 22 available (the repository memory notes that Node 18 cannot run
the current UI/Vitest toolchain):

```bash
cargo fmt --manifest-path service/Cargo.toml -- --check
cargo test --manifest-path service/Cargo.toml --test real-backend
cargo test --manifest-path service/Cargo.toml --all-targets
PATH="$HOME/.nvm/versions/node/v22.22.0/bin:$PATH" bash scripts/quality-gate.sh
git diff --check
```

Add or update focused assertions for the chosen budget and aggregate reopen
telemetry. Do not weaken the existing clean-end reopen test because production
windows will rarely reach the effectively-unbounded terminal budget.

## 3. Deploy Safely

Build a release bridge, acquire the existing restart/slot window, and deploy it
only against the already-proved `c0337ab`-or-later worker. Record the deployed
bridge SHA, worker SHA, and effective budget privately. Keep the prior 200M
release available for rollback.

Readiness/start failures, worker build regression, or unexpected RSS growth
trigger rollback and keep `9bx` open. Do not interpret absence of segment
boundaries as proof of health; worker RSS protection is supported by the
hypervisor guard, while this delta proves bridge behavior and determinism.

After rollback, require a private/sanitized record that the prior 200M bridge is
restored, the fixed worker remains deployed, readiness is green, no session/run
is active, the slot is released, and no orphan process group remains. Failure
to establish those invariants is an operator escalation, not routine residue.

## 4. Delta EQB Run

Repeat the same scripted-client window and executable `VerifyReplay` spot-check
from `03-contained-eqb-run.md` at the raised
budget. Record fps, no-drop checks, effective budget, worker build, boundary
count, and determinism result. Apply the stall caps to every boundary actually
observed; zero boundaries is expected and must be recorded as `0 / N/A`, not
silently omitted. The three-boundary floor applies only to the contained run.

Write a sanitized delta addendum adjacent to the main `eqb` record. It must
explicitly compare 200M with `u64::MAX / 4`, cite the hypervisor green light,
state that the determinism spot-check was rerun, and give the measured behavior
change. Add its reference to `eqb`, `9bx`, and `l1w` (the latter as confirmation,
not as a new closure rule). Close `9bx` only after code, deployment, and delta
evidence all pass.
