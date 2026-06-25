# Closeout

## Quality Gates

For code, deploy artifact, and docs changes, run:

```sh
scripts/quality-gate.sh
```

If the future agent only changes deployment docs after host validation, at
minimum run:

```sh
git diff --check
bash scripts/redaction-gate.sh
bash -n scripts/deployment-network-check.sh
```

If shell scripts or K3s manifests are changed, add the available focused checks:

```sh
shellcheck scripts/deployment-network-check.sh
KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl apply --dry-run=server -f deploy/k8s/rombridge-ingress.yaml
```

If a tool is unavailable, record that explicitly in the handoff and run the
closest local validation that is available.

## Closing `rom-operator-bridge-kut`

Close `kut` only after:

- the route is live at `https://rombridge.birb.homes/`;
- trusted TLS works without insecure curl flags;
- static UI, API, health, and WSS routes work through the HTTPS origin;
- `scripts/deployment-network-check.sh` passes with private inputs;
- `docs/deployment-checks.md` contains sanitized PASS results;
- no private values are present in the git diff or bead notes.

Suggested bead update:

```sh
COMMIT="$(git rev-parse --short HEAD)"
bd update rom-operator-bridge-kut --append-notes "Deployment prerequisites and network-isolation evidence completed in ${COMMIT}. Sanitized results are in docs/deployment-checks.md; raw evidence remains private."
bd close rom-operator-bridge-kut --reason "Sanitized deployment network isolation evidence recorded"
bd dolt push
```

After closing, check the next bead:

```sh
bd ready
bd show rom-operator-bridge-eqi
```

Do not close `eqi` in this work unless the future agent performs the full
publish-readiness gate for that bead.

## If Still Blocked

If deployment cannot be made ready, leave `kut` deferred or open and update the
bead with one sanitized blocker. Keep it concrete.

Suggested blocked update:

```sh
bd update rom-operator-bridge-kut --append-notes "Still blocked after deployment prerequisite work: <sanitized blocker>. No private values were recorded."
bd defer rom-operator-bridge-kut --until="+7d"
bd dolt push
```

## Commit And Push Protocol

Follow the repository close protocol before ending the future session:

```sh
git status --short --branch
git add .agents/plans/fix-kut-deployment-prerequisites docs deploy service ui scripts
git commit -m "<sanitized commit message>"
git pull --rebase
bd dolt push
git push
git status --short --branch
```

Only stage files that were intentionally changed. The example `git add` line is
a reminder of likely paths, not permission to stage unrelated local work.

Work is not complete until both bead state and Git commits are pushed.
