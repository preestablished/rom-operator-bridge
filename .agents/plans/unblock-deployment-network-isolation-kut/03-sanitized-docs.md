# Sanitized Documentation Plan

## 1. Create `docs/deployment-checks.md`

Use the reserved docs path from the bead notes:

```text
docs/deployment-checks.md
```

The document should be safe to commit and safe to include in handoff. It should
not be a raw transcript.

## 2. Recommended Structure

```md
# Deployment Network Isolation Checks

## Scope

This document records sanitized deployment-network verification for
`rom-operator-bridge-kut`.

## Evidence Boundary

Raw command output is stored in an operator-private validation directory outside
this repository. This public document records only pass/fail summaries and
placeholder command shapes.

## Environment

| Field | Sanitized Value |
|---|---|
| Deployment origin | `https://rombridge.birb.homes/` |
| Runtime API | `https://rombridge.birb.homes/api/...` |
| WebSocket route | `wss://rombridge.birb.homes/ws/...` |
| Service bind | `<localhost-or-trusted-interface>` |
| Evidence directory | `<private-validation-dir>` |

## Results

| Check | Result | Sanitized Evidence |
|---|---|---|
| DNS and TLS | PASS | Host resolves and TLS responds for the deployment origin. |
| Service bind | PASS | Bound only to localhost or documented trusted interface. |
| Health sanitization | PASS | Health response contains no private values. |
| Unauthenticated rejection | PASS | Runtime session endpoint rejects unauthenticated requests. |
| Wrong origin rejection | PASS | Unrelated origins do not receive session state. |
| Runtime no-store | PASS | Runtime routes include no-store/no-cache headers. |
| WebSocket origin/auth | PASS | WSS requires allowed origin and authentication. |
| Outside-network access | PASS | Outside access unavailable or rejected. |

## Commands

Show placeholder command shapes only.

## Residual Risks

Record any non-blocking limitation, such as outside-network probing represented
by firewall/proxy evidence instead of a remote probe.
```

## 3. Evidence References

If references are needed, use stable private evidence labels rather than paths:

- `private evidence: deployment-network-kut/dns`;
- `private evidence: deployment-network-kut/runtime-no-store`;
- `private evidence: deployment-network-kut/websocket-origin-auth`.

Do not commit actual filenames if they include private paths, timestamps, host
names, usernames, or IPs.

## 4. Cross-Reference Existing Docs

Link to existing sanitized docs:

- `docs/deployment-note.md`;
- `docs/deployment-security-shape.md`;
- `docs/runtime-api.md`;
- `docs/redaction.md`;
- `docs/runbook.md`.

Keep any repeated command examples placeholder-only.

## 5. Sanitization Checks Before Commit

Run:

```sh
rg -n 'Cookie|Set-Cookie|Authorization|Bearer |10\\.|192\\.168\\.|172\\.(1[6-9]|2[0-9]|3[0-1])\\.|/home/|/run/dh|/tmp/' docs/deployment-checks.md scripts .agents/plans/unblock-deployment-network-isolation-kut
bash scripts/redaction-gate.sh
```

If a private forbid file is available, run:

```sh
ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1 \
ROM_OPERATOR_BRIDGE_FORBID_FILE=<private-forbid-file> \
ROM_OPERATOR_BRIDGE_VALIDATION_DIR=<private-validation-dir>/redaction \
bash scripts/redaction-gate.sh
```

The redaction validation directory must be outside the repo.
