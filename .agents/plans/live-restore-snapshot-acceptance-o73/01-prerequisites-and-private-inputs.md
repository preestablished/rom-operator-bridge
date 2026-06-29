# Prerequisites And Private Inputs

## Host Assumptions

Run this on the expected project host where these checkouts live under
`/home/infra-admin/git/preestablished`:

- `rom-operator-bridge`
- `determinism-hypervisor`
- `snapshot-store`
- the reference workload checkout expected by the bridge

The user has already created `/run/dh` for the worker socket on this host:

```bash
sudo -n install -d -o infra-admin -g infra-admin -m 0755 /run/dh
```

If that directory is missing or has incompatible ownership, recreate it with the
same command before starting `dh-workerd`. If `sudo -n` fails, stop and get the
operator to prepare the directory; do not run an interactive sudo prompt from an
automation session.

## Private Material

Collect the following from the operator or existing private host setup. Keep all
values in a private directory outside committed paths:

- operator credential for `POST /api/session/start`;
- bridge session secret with enough entropy for signed cookies;
- private bridge root directory;
- workload image ref;
- capture spec ref;
- reference workload checkout path;
- 64-character hex `BRIDGE_REAL_SNAPSHOT_REF`;
- snapstore data root containing that snapshot ref, or the private import
  procedure that loads it before the run.

Use placeholder text in commands and notes. Do not paste real values into this
plan, bead notes, shell history intended for sharing, commits, PR descriptions,
or public logs.

## Private Workspace

Create a private evidence root outside the repository. Example shape:

```bash
set +x
umask 077
export O73_PRIVATE_ROOT="$HOME/.local/state/rom-operator-bridge/o73"
install -d -m 0700 "$O73_PRIVATE_ROOT"
install -d -m 0700 "$O73_PRIVATE_ROOT"/{bridge,snapstore,evidence,runtime}
```

Place private runtime files here:

- bridge env file, mode `0600`;
- snapstore config, mode `0600`;
- private request JSON files;
- raw API responses;
- raw `GetWorkerInfo` output;
- raw worker and snapstore logs.

Committed evidence should be limited to sanitized summaries.

Do not enable shell tracing while handling private values. Long-running process
logs must be redirected into `$O73_PRIVATE_ROOT/evidence/*.private.log`, not left
streaming in a shared terminal transcript.

## Bridge Env File Template

Create a private env file, for example:
`$O73_PRIVATE_ROOT/bridge/real-restore-snapshot.env`.

Use a local bridge bind port that does not conflict with snapstore defaults:

```dotenv
ROM_OPERATOR_BRIDGE_BACKEND=real
ROM_OPERATOR_BRIDGE_BIND_ADDR=127.0.0.1:7420
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT=<private bridge root>
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<private operator credential>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<private session secret>

BRIDGE_HYPERVISOR_ENDPOINT=<hypervisor endpoint, for example unix:///run/dh/grpc.sock on the expected host>
BRIDGE_WORKLOAD_IMAGE_REF=<private workload image ref>
BRIDGE_CAPTURE_SPEC_REF=<private capture spec ref>
BRIDGE_REFERENCE_WORKLOAD_CHECKOUT=<reference workload checkout path>
BRIDGE_REAL_SNAPSHOT_REF=<private 64 hex snapshot ref>
```

Do not set `BRIDGE_CREATE_VM_CONFIG_REF` for this acceptance run. The bead is
specifically about the RestoreSnapshot branch selected by
`BRIDGE_REAL_SNAPSHOT_REF`.

Before running the bridge, validate the file permissions:

```bash
stat -c '%a %n' "$O73_PRIVATE_ROOT/bridge/real-restore-snapshot.env"
```

The mode must be `600` or stricter.

Privately prove the RestoreSnapshot branch will be selected without printing
values. The launch command in `03-bridge-restore-snapshot-run.md` also unsets
known bridge env vars so the private config file, not a stale inherited shell
environment, controls the branch:

```bash
grep -q '^BRIDGE_REAL_SNAPSHOT_REF=.' "$O73_PRIVATE_ROOT/bridge/real-restore-snapshot.env"
! grep -q '^BRIDGE_CREATE_VM_CONFIG_REF=' "$O73_PRIVATE_ROOT/bridge/real-restore-snapshot.env"
```

Record only sanitized booleans in bead notes:

- `BRIDGE_REAL_SNAPSHOT_REF` configured: yes;
- `BRIDGE_CREATE_VM_CONFIG_REF` configured: no;
- snapstore manifest lookup succeeded: yes or no.

## Private Request Body Template

Create `$O73_PRIVATE_ROOT/evidence/start-request.json` with the live credential:

```json
{
  "schema_version": 1,
  "operator_credential": "<private operator credential>",
  "backend_mode": "real",
  "requested_capabilities": ["input", "preview", "capture"]
}
```

The stop body is generated after start because it needs the returned
`session_id`.
