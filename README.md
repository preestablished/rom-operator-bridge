# rom-operator-bridge

Private browser bridge for operating and validating ROM runs through a
sanitized web UI. The bridge is built to keep session secrets, private
runtime paths, raw captures, screenshots, verifier reports, and host details
out of committed files and shared handoff text.

## Operator Entry Points

- [Operator runbook](docs/operator-runbook.md) - start here for setup,
  start/stop, synthetic validation, real-host operation, capture labeling,
  verifier flow, deployment URL, rollback, and current gaps.
- [Deployment runbook](deploy/operator-kut-deployment-runbook.md) - install
  the service, static UI, systemd unit, and K3s route using placeholders.
- [Private validation reference](deploy/operator-kut-private-validation-reference.md)
  - run deployment validation with operator-private env, cookie, network
  evidence, and forbid files.
- [Current handoff](docs/handoff.md) - sanitized current state and remaining
  beads.

The active operator URL is:

```text
https://rombridge.birb.homes/
```

Do not commit or paste instantiated private env files,
session secrets, cookie jars, private endpoint manifests, absolute private
paths, raw command output, screenshots, capture ids, verifier reports, or
private evidence contents.

## Quality Gate

Before handoff or merge, run:

```sh
bash scripts/quality-gate.sh
```

For deployment or publish checks, also run the deployment-network checker and
redaction gate with an operator-private forbid file as documented in
[docs/redaction.md](docs/redaction.md) and
[docs/deployment-checks.md](docs/deployment-checks.md).
