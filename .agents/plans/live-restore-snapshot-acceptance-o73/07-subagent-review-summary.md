# Subagent Review Summary

Two subagents reviewed this plan before handoff.

## Runtime Correctness Review

Accepted changes:

- corrected `grpcurl` UDS syntax to use the host-validated
  `unix:///run/dh/grpc.sock` address form;
- added HTTP status capture and explicit `200` checks for start, session,
  run-status, and stop;
- fixed the sanitized `GET /api/session` summary shape to use top-level response
  fields;
- removed language implying `StartSessionResponse` includes `backend_mode`;
- added explicit RestoreSnapshot branch preflight booleans:
  `BRIDGE_REAL_SNAPSHOT_REF` present and `BRIDGE_CREATE_VM_CONFIG_REF` absent;
- clarified that `dump-manifest` proves manifest lookup, while the live bridge
  start proves restoreability.

## Ops And Security Review

Accepted changes:

- run snapstore, `dh-workerd`, and the bridge with private log redirection and
  PID files instead of foreground logs;
- avoid exporting or passing the session cookie on the command line by writing a
  private curl config file;
- use quiet forbidden-literal sweeps so secret match lines are never printed;
- require `bd update --append-notes` with a sanitized summary before close or
  blocked handoff;
- include staging, commit, pull/rebase, `bd dolt push`, `git push`, and final
  status verification in the close protocol;
- require non-interactive `sudo -n`, curl timeouts, and private status capture.

Rejected changes:

- None. The review comments were either applied directly or folded into a
  nearby section with equivalent behavior.

## Additional Main-Agent Hardening

After applying reviewer feedback, the bridge launch command was also tightened
to unset known bridge environment variables before setting
`ROM_OPERATOR_BRIDGE_CONFIG_FILE`. This prevents a stale inherited
`BRIDGE_CREATE_VM_CONFIG_REF` from overriding the private env file and silently
switching the run back to `CreateVm`.

Long-running service commands use `setsid` plus PID files so cleanup can signal
the process group rather than only the Cargo wrapper process.

During implementation, the local `grpcurl` binary still tried to dial
`/run/dh/grpc.sock` as TCP when passed as a positional path with `-unix` or
`-unix=true`. The plan now uses the verified `unix:///run/dh/grpc.sock` address
form for worker readiness and slot-count checks.
