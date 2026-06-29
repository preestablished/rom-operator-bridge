# Acceptance And Beads Handoff

## Acceptance Checklist

Close `rom-operator-bridge-o73` only when all of these are true:

1. `snapstore-server` manifest lookup for the private snapshot ref succeeded;
   full restoreability was proven by bridge start.
2. `dh-workerd` ran with snapstore enabled, not `--no-snapstore`.
3. The bridge ran in real mode with `BRIDGE_REAL_SNAPSHOT_REF` set,
   `BRIDGE_CREATE_VM_CONFIG_REF` absent, and private preflight booleans recorded
   without values.
4. `POST /api/session/start` succeeded through RestoreSnapshot.
5. `GET /api/session` reported an active session.
6. `GET /api/run/status` reported `backend_mode: real` and state `paused` or
   `running`.
7. `POST /api/session/stop` returned `state: stopped`.
8. Worker slot count after stop matched the pre-start count.
9. Public API responses and sanitized notes did not leak private paths, refs,
   credentials, cookie values, lease tokens, or raw worker errors.
10. Required tests or quality gates from `05-tests-and-quality-gates.md` passed,
    or any inability to run them is explicitly recorded.

## Bead Update Template For Success

Use a sanitized update like this:

```text
Live RestoreSnapshot acceptance passed on 2026-06-24.

Bridge commit: <short sha>
Worker checkout: determinism-hypervisor <short sha>
Snapstore checkout: snapshot-store <short sha>
Worker transport: UDS /run/dh/grpc.sock
Snapstore transport: <UDS or loopback TCP, no private path>

Results:
- RestoreSnapshot branch preflight: snapshot_ref_configured=yes, create_vm_config_ref_configured=no;
- snapstore manifest lookup for private snapshot ref succeeded;
- start returned HTTP 200 and state <paused|running>;
- /api/session returned active true;
- /api/run/status returned backend_mode real and state <paused|running>;
- stop returned state stopped;
- worker slot count returned from <before> -> <active> -> <after>;
- backend_unavailable probe returned sanitized envelope with empty details;
- forbidden literal sweep found no private values in public responses or repo.

Tests:
- <commands run and pass/fail summary>
```

Then:

```bash
bd update rom-operator-bridge-o73 --append-notes "$(cat /path/to/sanitized-o73-summary.txt)"
bd close rom-operator-bridge-o73 --reason "Live RestoreSnapshot acceptance passed on snapstore-enabled worker"
```

## Bead Update Template For Blocked Outcome

If the acceptance cannot run, leave the bead open and update it with a sanitized
blocker:

```text
Live RestoreSnapshot acceptance still blocked on <component>.

Observed on 2026-06-24:
- snapstore readiness: <sanitized pass/fail>
- worker readiness: <sanitized pass/fail>
- bridge start/status/stop: <sanitized pass/fail or not reached>
- public error sanitization: <sanitized pass/fail or not reached>

Private values were not included. Raw logs are retained only under the private
host evidence directory.

Next unblock step:
- <single concrete external action or bridge follow-up bead>
```

If a bridge-owned defect is found, file a new bead with a narrow title and link
it from o73.

Apply the blocked note before ending the session:

```bash
bd update rom-operator-bridge-o73 --append-notes "$(cat /path/to/sanitized-o73-blocker.txt)"
```

## Session Close Protocol

Follow the repository `AGENTS.md` close protocol before ending the session:

```bash
git status --short
git add .agents/plans/live-restore-snapshot-acceptance-o73
git commit -m "Plan live RestoreSnapshot acceptance"
git pull --rebase
bd dolt push
git push
git status --short --branch
```

Final status must show the branch up to date with origin. Do not leave bead or
plan changes only in the local worktree.
