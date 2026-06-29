# rom-operator-bridge-o73 Plan: Live RestoreSnapshot Acceptance

## Goal

Implement bead `rom-operator-bridge-o73` by running the real bridge against a
snapstore-enabled `dh-workerd` and proving that the RestoreSnapshot path works
end to end.

This bead is acceptance work, not a feature-design task. The bridge already has
mock UDS RestoreSnapshot coverage. The remaining gap is a live run on the
expected host with:

- a running `snapstore-server`;
- `dh-workerd` started with snapstore enabled, not `--no-snapstore`;
- `BRIDGE_REAL_SNAPSHOT_REF` set to an operator-approved private snapshot ref;
- bridge real mode start/status/stop verified through the public API;
- worker slot/lease cleanup verified after stop;
- public responses checked for sanitized `backend_unavailable` behavior.

## Reading Order

Use these files in order:

1. `01-prerequisites-and-private-inputs.md`
2. `02-snapstore-and-worker-readiness.md`
3. `03-bridge-restore-snapshot-run.md`
4. `04-evidence-sanitization-and-failure-handling.md`
5. `05-tests-and-quality-gates.md`
6. `06-acceptance-and-beads-handoff.md`
7. `07-subagent-review-summary.md`
8. `08-current-execution-state.md`

## Non-Goals

Do not commit the private snapshot ref, operator credential, session secret,
lease token, worker socket path variants, snapstore data root, raw worker logs,
or raw API responses from the live host.

Do not change `determinism-hypervisor` or `snapshot-store` as part of this bead
unless the live acceptance run exposes a bridge defect that cannot be diagnosed
without a minimal local compatibility note. If the blocker is in another
component, update the bead with a sanitized blocker and file or link the external
follow-up.

Do not close `rom-operator-bridge-o73` on mock tests alone. The close condition
is a live RestoreSnapshot start/status/stop run against snapstore-enabled
`dh-workerd`.

## Expected Result

When complete, the bead notes should contain a sanitized summary with:

- date and branch/commit tested;
- snapstore transport class used, such as UDS or loopback TCP, without private
  paths;
- worker endpoint class used, such as `/run/dh/grpc.sock`, if already public in
  docs;
- start response returned HTTP 200 and state `paused` or `running`;
- `GET /api/session` reported an active session;
- `GET /api/run/status` reported a real session with an allowed state;
- stop returned `state: stopped`;
- worker slot availability returned to the pre-start count;
- public error probes returned sanitized envelopes and did not leak private
  values.
