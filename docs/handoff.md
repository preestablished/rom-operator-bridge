# ROM Operator Bridge Handoff

Generated: 2026-06-27T21:18:05Z

## Current Repository State

- Branch: `ralph/iteration-28-implement-real-backend-attachment-lifecycle`.
- Git remote state: branch was up to date with origin before this handoff file was added.
- Beads remote state: `bd dolt push` succeeded after the live UI backend-mode fix was closed.
- Issue export requested for cross-machine handoff: `issues.jsonl` in the repository root.

Recent commits to know about:

- `1b8a0f1` - fixed the operator UI so session start uses the backend mode discovered from `/health`.
- `30d8057` - recorded live `birb.homes` deployment state and sanitized deployment evidence.
- `34554e0` - documented static publish readiness.

## Live Deployment State

- Operator URL: `https://rombridge.birb.homes/`.
- Live static bundle verified through the public HTTPS route:
  - JavaScript: `index-Ds6jVeRa.js`
  - CSS: `index-DFESM0Rc.css`
- `/health` returns schema version 1, `ok: true`, `backend_mode: real`, and runtime API version 1.
- Unauthenticated `/api/session` returns sanitized `session_inactive`.
- `rom-operator-bridge.service` is active and running on the expected host.
- The live route was updated after the service initially kept serving the prior bundle; direct and public checks now both see `index-Ds6jVeRa.js`.

Do not record operator credentials, session secrets, cookie jars, private endpoint addresses, private artifact contents, raw verifier output, screenshots, or private evidence paths in Git.

## What Was Just Fixed

The deployed UI accepted the operator credential but appeared to do nothing because the frontend sent `backend_mode: synthetic` by default. The real deployment requires `backend_mode: real`, so `/api/session/start` rejected the request and the UI stayed in a generic unavailable/faulted state.

The fix:

- `ui/src/app.ts` now refreshes `/health` and tracks the service backend mode.
- `ui/src/authSession.ts` passes that backend mode into `startSession`.
- `ui/src/runtimeClient.ts` preserves the backend mode in the start-session model.
- Tests cover default synthetic behavior and real backend start behavior.

Verification run before deployment:

- `npm --prefix ui test -- --run tests/ui-auth/authSession.test.ts tests/ui-auth/mount.test.ts tests/runtimeClient.test.ts tests/synthetic-smoke/syntheticOperatorSmoke.test.ts`
- `npm --prefix ui test -- --run tests/privacy.test.ts tests/securityHeaders.test.ts`
- `npm --prefix ui run typecheck`
- `bash scripts/redaction-gate.sh`

## Bead State

No beads are currently ready:

- `bd ready --json` returned `[]`.

In progress:

- `rom-operator-bridge-44c` - Write operator runbook and handoff docs.

Open but blocked:

- `rom-operator-bridge-13h` - Complete final acceptance review.
- Blocked by `rom-operator-bridge-0wo`, `rom-operator-bridge-44c`, `rom-operator-bridge-opw`, and `rom-operator-bridge-r77`.

Deferred private/operator-dependent blockers:

- `rom-operator-bridge-0wo` - Document and run real backend smoke.
- `rom-operator-bridge-r77` - Run real one-capture label smoke.
- `rom-operator-bridge-opw` - Validate bridge-produced private bundle; also depends on `rom-operator-bridge-r77`.

Recently closed:

- `rom-operator-bridge-son` - Use live backend mode for UI session start.
- `rom-operator-bridge-38v` - Deploy private UI through `birb.homes`.
- `rom-operator-bridge-eqi` - Static publish readiness docs.
- `rom-operator-bridge-kut` - Deployment network isolation validation.

## Suggested Next Session

1. On the next machine, pull both Git and beads:

   ```sh
   git pull --rebase
   bd dolt pull
   bd prime
   ```

2. Confirm current state:

   ```sh
   git status --short --branch
   bd ready
   bd show rom-operator-bridge-44c
   bd show rom-operator-bridge-13h
   ```

3. Continue `rom-operator-bridge-44c` first. It is the only in-progress bead and is part of the final acceptance blocker chain.

4. Treat `rom-operator-bridge-0wo`, `rom-operator-bridge-r77`, and `rom-operator-bridge-opw` as operator-private validation work. They require approved private runtime data and should only produce sanitized public notes.

5. Before final acceptance, re-run the relevant quality gates and avoid committing any private values or raw private evidence.
