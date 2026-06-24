# Ralph Quality Gate

Ralph agents must use this root VERIFY command after branch work and before
review or merge:

```sh
scripts/quality-gate.sh
```

The command is the single repository contract for branch verification. Later
beads should extend this script instead of adding local, one-off test command
lists to handoffs or branch notes.

## Current Checks

The gate currently runs:

```sh
git diff --check
git diff --cached --check
git diff --check <base-branch>...HEAD  # skipped with an explicit message on the base branch
git show --check --stat --oneline HEAD
cargo fmt --manifest-path service/Cargo.toml -- --check
cargo test --manifest-path service/Cargo.toml --all-targets
npm --prefix ui ci
npm --prefix ui run typecheck
npm --prefix ui test -- --run
npm --prefix ui run build
```

The base branch defaults to `main`. If `.ralph` defines `main_branch=<branch>`,
the script uses that branch for the branch diff whitespace check. The configured
base branch must exist locally so a Ralph branch is compared against the same
base it will merge into.

## Degradation Rules

The script is fail-fast for checks whose scaffolds exist:

- If `service/Cargo.toml` exists, Rust service formatting and tests must pass.
- If `ui/package.json` exists, UI dependency sync, typecheck, tests, and build
  must pass. `ui/package-lock.json` is required because the command is
  `npm --prefix ui ci`.
- If `scripts/redaction-gate.sh` exists, the static redaction gate must pass.

The script may skip only missing scaffolds or missing future gates, and every
skip is printed with an explicit `SKIP` message:

- Missing `service/Cargo.toml` means the service scaffold is not available yet.
- Missing `ui/package.json` means the UI scaffold is not available yet.
- Missing `scripts/redaction-gate.sh` means the static redaction gate is still
  deferred to `rom-operator-bridge-25u`.

## Extension Points

Add new branch-wide checks here when the matching bead lands:

- `rom-operator-bridge-25u` should provide `scripts/redaction-gate.sh` and keep
  the public output limited to pass/fail status plus sanitized counts.
- Synthetic integration-test beads should add commands to this script after
  their tests are committed.
- Deployment or publish-readiness beads should add only agent-runnable checks;
  private real-host validation stays in operator-only runbook commands.
