# Complete Live RestoreSnapshot Acceptance For o73

## Goal

Finish `rom-operator-bridge-o73` by running the bridge against an
operator-approved, snapstore-backed `BRIDGE_REAL_SNAPSHOT_REF` and proving the
real `RestoreSnapshot` start/status/stop lifecycle succeeds without leaking
private material.

This plan supersedes the pre-handoff blocker notes in
`.agents/plans/live-restore-snapshot-acceptance-o73/08-current-execution-state.md`.
The missing private handoff was later provided through the private channel. Do
not copy that private path into committed files or bead notes.

## Current Starting Point

Already true:

- `bp8` real backend lifecycle code is implemented.
- Mock UDS `RestoreSnapshot` lifecycle coverage exists and previously passed.
- `determinism-hypervisor` fixed snapstore-enabled `dh-workerd` startup in
  commit `8b59bbf`.
- The `dh-m9-ready-handoff` path now produced an operator-private handoff env.
- The handoff env contains:
  - `BRIDGE_REAL_SNAPSHOT_REF`;
  - `BRIDGE_WORKLOAD_IMAGE_REF`;
  - `BRIDGE_CAPTURE_SPEC_REF`;
  - snapstore data/config/UDS values;
  - `DH_M9_IMAGE_CACHE`;
  - no `BRIDGE_CREATE_VM_CONFIG_REF`.

Remaining work:

1. Materialize bridge-private env/start-request/forbidden-literals files.
2. Ensure snapstore is serving the private snapshot.
3. Ensure `dh-workerd` is snapstore-enabled and reachable at the endpoint the
   bridge will use.
4. Run bridge real-mode start/status/stop acceptance through
   `BRIDGE_REAL_SNAPSHOT_REF`.
5. Run forbidden-literal sweeps and a sanitized unavailable-path probe.
6. Update and close `rom-operator-bridge-o73` only with sanitized evidence.

## Privacy Rule

The plan files are public-safe. They must not contain the actual private
handoff path, snapshot ref, workload image ref, capture spec ref, private root,
operator credential, session secret, cookie, lease token, or worker/snapstore
log excerpts.

The executing agent should set this variable privately:

```bash
export O73_HANDOFF_ENV="<operator-private handoff env path>"
```

Do not print the value. Do not commit it.

## Plan Files

| File | Purpose |
|---|---|
| `00-overview.md` | Goal, starting state, privacy rule |
| `01-private-workspace-and-config.md` | Private workspace, bridge env, start body, forbidden literals |
| `02-snapstore-and-worker-readiness.md` | Serve snapstore and start/verify snapstore-enabled worker |
| `03-live-bridge-acceptance.md` | Start bridge, call start/status/stop, verify worker cleanup |
| `04-sanitization-and-probes.md` | Forbidden sweeps, sanitized summaries, unavailable-path probe |
| `05-tests-and-closeout.md` | Quality gates, bead update, push protocol |

