# Closeout And Bead Flow

## 1. Quality Gates

For docs-only execution:

```sh
git diff --check
bash scripts/redaction-gate.sh
```

If a shell script is added:

```sh
bash -n scripts/deployment-network-check.sh
shellcheck scripts/deployment-network-check.sh
```

If `shellcheck` is not installed, record that explicitly in the final handoff
and rely on `bash -n` plus a real private-host run.

If service or UI code changes unexpectedly, run the full gate:

```sh
scripts/quality-gate.sh
```

## 2. Closing `kut`

Only close `rom-operator-bridge-kut` after the deployment-network evidence is
complete and `docs/deployment-checks.md` contains a sanitized PASS summary.

Suggested sequence:

```sh
git status --short
git add docs/deployment-checks.md
if [[ -f scripts/deployment-network-check.sh ]]; then
  git add scripts/deployment-network-check.sh
fi
git commit -m "Document deployment network isolation checks"
git pull --rebase
COMMIT="$(git rev-parse --short HEAD)"
bd update rom-operator-bridge-kut --append-notes "Deployment network isolation checks completed in ${COMMIT}. Sanitized results are in docs/deployment-checks.md; raw evidence remains private."
bd close rom-operator-bridge-kut --reason "Sanitized deployment network isolation evidence recorded"
bd dolt push
git push
git status --short --branch
```

If other files changed, include only intentional sanitized files. Do not commit
raw private evidence.

## 3. Confirm `eqi` Is Unblocked

After closing `kut`:

```sh
bd ready
bd show rom-operator-bridge-eqi
```

Expected result:

- `rom-operator-bridge-eqi` should be ready or at least no longer blocked by
  `kut`.

If `eqi` is ready, add a short sanitized note:

```sh
bd update rom-operator-bridge-eqi --append-notes "kut is complete; static publish readiness can now evaluate redaction, browser no-persistence, rollback/restart, and sanitized evidence links."
bd dolt push
```

Do not close `eqi` in the `kut` session unless the executing agent also performs
the full `eqi` publish-readiness checklist.

## 4. If `kut` Cannot Be Closed

Leave the bead deferred/open and make the blocker concrete:

```sh
git status --short
git add <sanitized changed files>
git commit -m "Record blocked deployment network isolation handoff"
git pull --rebase
bd update rom-operator-bridge-kut --append-notes "Still blocked: <sanitized missing evidence>. No private command output or values were recorded."
bd defer rom-operator-bridge-kut --until="+7d"
bd dolt push
git push
git status --short --branch
```

If no files changed, skip `git add` and `git commit`, but still run
`git pull --rebase`, update/defer the bead, `bd dolt push`, `git push`, and the
final status check. The final status must show the branch up to date with
origin.

Examples of acceptable sanitized blockers:

- deployment route is not active yet;
- operator has not approved a private validation directory;
- no authenticated session is available for WebSocket checks;
- outside-network probe must be run by an operator from a separate network.

## 5. Final Handoff Template

Use this final response shape:

```text
Implemented the kut unblock plan.

Closed:
- rom-operator-bridge-kut

Evidence:
- docs/deployment-checks.md records sanitized PASS results.
- Raw command output stayed under the private validation directory.

Verification:
- <commands run>

Next:
- rom-operator-bridge-eqi is now ready for static publish readiness.
```

If blocked:

```text
Prepared kut evidence/docs, but did not close the bead.

Remaining blocker:
- <one sanitized blocker>

Verification:
- <commands run>
```

## 6. Mandatory Repository Close Protocol

Follow `AGENTS.md` exactly before ending the future session:

```sh
git status --short
git add <changed files>
git commit -m "<sanitized message>"
git pull --rebase
bd dolt push
git push
git status --short --branch
```

Work is not complete until both Git and bead state are pushed.
