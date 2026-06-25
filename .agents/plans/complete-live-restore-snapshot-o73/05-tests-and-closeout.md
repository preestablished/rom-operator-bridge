# Tests And Closeout

## If No Code Changes Are Needed

If live acceptance passes without bridge code changes, run only the targeted
mock regression plus repository status checks:

```bash
cd /home/infra-admin/git/preestablished/rom-operator-bridge
cargo test --manifest-path service/Cargo.toml --test real-backend \
  real_restore_snapshot_lifecycle_calls_worker_and_stays_sanitized
git status --short
```

The live run itself is the private acceptance evidence.

## If Bridge Code Changes Are Needed

Keep changes scoped to the bridge-owned defect. Then run:

```bash
cargo fmt --manifest-path service/Cargo.toml -- --check
cargo test --manifest-path service/Cargo.toml --test real-backend
cargo test --manifest-path service/Cargo.toml --all-targets
scripts/quality-gate.sh
```

If a gate cannot run because the host lacks a dependency, record the sanitized
reason in `rom-operator-bridge-o73` and keep the bead open unless acceptance is
otherwise complete and the missing gate is unrelated.

## Success Bead Note Template

Append only sanitized evidence. Do not include private paths, refs, endpoint
paths, credentials, cookies, lease tokens, raw JSON, screenshots, or raw worker
errors.

```text
Live RestoreSnapshot acceptance passed on 2026-06-25.

Bridge commit: <short sha>
Worker checkout: determinism-hypervisor <short sha>
Snapstore checkout: snapshot-store <short sha>
Worker transport: UDS (path private)
Snapstore transport: UDS (path private)

Results:
- RestoreSnapshot branch preflight: snapshot_ref_configured=yes, create_vm_config_ref_configured=no;
- snapstore manifest lookup for private snapshot ref succeeded;
- start returned HTTP 200 and state <paused|running>;
- /api/session returned active true;
- /api/run/status returned backend_mode real and state <paused|running>;
- stop returned state stopped;
- worker slot count returned from <before> -> <active> -> <after>;
- backend_unavailable probe returned sanitized 503 envelope with empty details;
- forbidden literal sweep found no private values in public responses or repo.

Tests:
- <command>: pass
```

Apply and close:

```bash
bd update rom-operator-bridge-o73 --append-notes "$(cat "$O73_PRIVATE_ROOT/evidence/o73-sanitized-summary.private.txt")"
bd close rom-operator-bridge-o73 --reason "Live RestoreSnapshot acceptance passed on snapstore-enabled worker"
```

## Blocked Or Failed Outcome

If acceptance cannot complete, do not close `o73`. Append a sanitized blocker:

```text
Live RestoreSnapshot acceptance still blocked on <component>.

Observed on 2026-06-25:
- handoff env present: yes;
- snapstore manifest lookup: <pass/fail>;
- snapstore-enabled worker readiness: <pass/fail>;
- bridge start/status/stop: <pass/fail/not reached>;
- public error sanitization: <pass/fail/not reached>.

Private values were not included. Raw logs remain only under the private o73
evidence directory.

Next unblock step:
- <single concrete action>
```

If the failure is bridge-owned, create a new narrow bead and mention it in the
o73 note.

## Cleanup Private Processes

Stop only processes started by this plan using private PID files:

```bash
stop_plan_process_group() {
  pid_file="$1"
  [ -f "$pid_file" ] || return 0
  pid="$(cat "$pid_file")"
  case "$pid" in
    ''|*[!0-9]*) echo 'invalid private pid file; inspect before cleanup' >&2; return 1 ;;
  esac
  pgid="$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ')"
  [ -n "$pgid" ] || { rm -f "$pid_file"; return 0; }
  if [ "$pgid" != "$pid" ]; then
    echo 'refusing process-group kill because pid is not its group leader' >&2
    return 1
  fi
  args="$(ps -o args= -p "$pid" 2>/dev/null || true)"
  case "$args" in
    *rom-operator-bridge*|*dh-workerd*|*snapstore-server*) ;;
    *) echo 'refusing to kill unexpected process from private pid file' >&2; return 1 ;;
  esac
  kill -- "-$pid" 2>/dev/null || true
  for _ in $(seq 1 40); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.25
  done
  rm -f "$pid_file"
}

for pid_file in \
  "$O73_PRIVATE_ROOT/runtime/bridge.pid" \
  "$O73_PRIVATE_ROOT/runtime/backend-unavailable-bridge.pid" \
  "$O73_PRIVATE_ROOT/runtime/dh-workerd.pid" \
  "$O73_PRIVATE_ROOT/runtime/snapstore-server.pid"
do
  stop_plan_process_group "$pid_file"
done
```

Do not kill a pre-existing worker or snapstore process unless it was explicitly
operator-approved and tracked by this run.

## Git And Beads Session Close

Follow the repository `AGENTS.md` close protocol. At minimum:

```bash
git status --short
# Stage the actual changed files intentionally. Include code/docs/tests if the
# acceptance run required fixes; include plan files only if they changed.
git add <changed files>
if ! git diff --cached --quiet; then
  git commit -m "<specific commit message>"
fi
git pull --rebase
bd dolt push
git push
git status --short --branch
```

If the executing agent made code changes, include those files in the commit and
use a code-oriented commit message. The final status must show the branch up to
date with origin.
