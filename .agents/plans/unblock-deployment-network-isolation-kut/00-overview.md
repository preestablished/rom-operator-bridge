# Unblock Deployment Network Isolation For kut

## Target Bead

Unblock and complete `rom-operator-bridge-kut`: verify deployment network
isolation.

Closing `kut` should unblock `rom-operator-bridge-eqi` (`Gate static publish
readiness`). `eqi` then becomes the next practical P0 gate for publish readiness.

## Why This Bead Is The Best First Unblock

Current `bd ready` reports no ready work because every open bead depends on
deferred/private-host evidence. `kut` is the smallest useful unblock because:

- it is P0;
- it blocks `eqi`, another P0;
- its implementation can be mostly repeatable checks and sanitized docs;
- it does not require real ROM capture contents, screenshots, labels, or
  verifier reports;
- it can be completed with deployment host access plus redacted pass/fail
  evidence.

## Current Blocker

`kut` is deferred because it requires private operator data, private host/network
state, or deployment access. Dependencies `25u` and `3kf` are already closed, so
the remaining blocker is evidence collection on the expected deployment host.

## Success Criteria

`kut` can be closed when sanitized evidence proves:

- the service bind is localhost or the documented trusted interface;
- the health route is reachable and sanitized;
- absent, null, and unrelated browser `Origin` values are rejected for runtime
  surfaces;
- unauthenticated runtime requests are rejected without private details;
- runtime HTTP responses include `Cache-Control: no-store` and no-cache headers;
- WebSocket access requires allowed origin plus authenticated session;
- browser-facing deployment output has no mixed-content path back to `http://`
  or `ws://` runtime endpoints;
- outside-network access is unavailable or rejected, based on the selected
  deployment topology;
- the public repo contains only sanitized summaries, placeholders, and pass/fail
  status.

## Planned Outputs

The future coding agent should produce:

- `docs/deployment-checks.md` with sanitized procedures and a sanitized results
  table;
- optionally `scripts/deployment-network-check.sh` to make the checks
  repeatable;
- private evidence under an operator-approved private validation directory,
  outside the repo;
- bead notes that mention only sanitized pass/fail outcomes and commit ids.

## Non-Goals

- Do not deploy the UI; `38v` owns deployment.
- Do not gate static publish; `eqi` owns publish readiness.
- Do not run real capture or labeling smoke; `r77` owns that.
- Do not include private command output, IPs, cookies, paths, screenshots,
  exact capture ids, or proxy logs in committed docs or bead notes.

## Plan Files

| File | Purpose |
|---|---|
| `00-overview.md` | Target bead, blocker, success criteria |
| `01-prerequisites-and-privacy.md` | Required host state and evidence boundaries |
| `02-repeatable-checks.md` | Commands and optional script design |
| `03-sanitized-docs.md` | `docs/deployment-checks.md` content plan |
| `04-closeout.md` | Tests, bead closure, unblocking `eqi`, and push protocol |
