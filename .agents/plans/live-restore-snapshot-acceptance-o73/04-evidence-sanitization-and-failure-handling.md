# Evidence Sanitization And Failure Handling

## Sanitized Evidence

Keep raw evidence private. For committed notes or bead updates, reduce the
private files to a small sanitized summary:

```bash
jq '{
  schema_version,
  session_id_present: (.session_id != null),
  run_id_present: (.run_id != null),
  state,
  current_frame,
  capabilities
}' "$O73_PRIVATE_ROOT/evidence/start.body.private.json"

jq '{
  schema_version,
  active,
  session_id_present: (.session_id != null),
  run_id_present: (.run_id != null),
  state,
  current_frame,
  backend_mode
}' "$O73_PRIVATE_ROOT/evidence/session.body.private.json"

jq '{
  schema_version,
  backend_mode,
  state,
  current_frame,
  preview_stale,
  capabilities
}' "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json"

jq '{
  schema_version,
  session_id_present: (.session_id != null),
  state,
  final_frame
}' "$O73_PRIVATE_ROOT/evidence/stop.body.private.json"
```

If any jq selector does not match the current response shape, inspect the raw
private JSON and adjust the summary manually. Do not paste raw private JSON into
the bead.

## Forbidden Literal Sweep

Build a private forbidden-literals file with one value per line:

- private bridge root path;
- snapstore data root path;
- snapstore UDS path;
- operator credential;
- session secret;
- workload image ref;
- capture spec ref;
- reference workload checkout path if private;
- `BRIDGE_REAL_SNAPSHOT_REF`;
- extracted session cookie value;
- any raw worker error text that included private paths or refs.

Run literal sweeps against public response bodies and any planned committed
notes. Never run a form that prints matching lines. Example:

```bash
if rg -q -F -f "$O73_PRIVATE_ROOT/evidence/forbidden-literals.private.txt" \
    "$O73_PRIVATE_ROOT/evidence/start.body.private.json" \
    "$O73_PRIVATE_ROOT/evidence/session.body.private.json" \
    "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json" \
    "$O73_PRIVATE_ROOT/evidence/stop.body.private.json"; then
  rg -l -F -f "$O73_PRIVATE_ROOT/evidence/forbidden-literals.private.txt" \
    "$O73_PRIVATE_ROOT/evidence/start.body.private.json" \
    "$O73_PRIVATE_ROOT/evidence/session.body.private.json" \
    "$O73_PRIVATE_ROOT/evidence/run-status.body.private.json" \
    "$O73_PRIVATE_ROOT/evidence/stop.body.private.json" \
    > "$O73_PRIVATE_ROOT/evidence/forbidden-response-files.private.txt"
  echo "forbidden literals found; see private file list" >&2
  exit 1
fi
```

This command must exit successfully. If it fails, inspect the private file list,
fix the bridge redaction defect or avoid publishing that evidence, and do not
close the bead until public API responses are sanitized.

Before committing, also sweep the repository for private values:

```bash
if rg -q -F -f "$O73_PRIVATE_ROOT/evidence/forbidden-literals.private.txt" .; then
  rg -l -F -f "$O73_PRIVATE_ROOT/evidence/forbidden-literals.private.txt" . \
    > "$O73_PRIVATE_ROOT/evidence/forbidden-repo-files.private.txt"
  echo "forbidden literals found in repository; see private file list" >&2
  exit 1
fi
```

This must find no matches in committed files.

## Sanitized Backend Unavailable Probe

After the successful run, intentionally exercise one unavailable path without
using real private endpoint values in the request. Required probe:

1. Stop the bridge.
2. Copy the private env file to a temporary private env file with `cp -f`.
3. Change only `BRIDGE_HYPERVISOR_ENDPOINT` to a dummy non-existent UDS under
   `$O73_PRIVATE_ROOT/runtime`.
4. Restart the bridge with logs redirected privately.
5. Call `/api/session/start` with the private start body.

Use the same environment-isolated bridge launch pattern from
`03-bridge-restore-snapshot-run.md` for the probe.

Expected HTTP status is `503`, and the response body is:

```json
{
  "schema_version": 1,
  "error": {
    "code": "backend_unavailable",
    "message": "Backend unavailable.",
    "retryable": true,
    "details": {}
  }
}
```

Verify that no private endpoint path, snapshot ref, image ref, capture ref,
credential, lease token, snapstore error, or worker error text appears in the
response.

## Failure Classification

Use these outcomes:

- Snapstore cannot start or `dump-manifest` cannot find the snapshot: update
  o73 as blocked on private snapstore content or snapstore service readiness.
- `dh-workerd` only runs with `--no-snapstore`: update o73 as blocked on a
  snapstore-enabled worker.
- `GetWorkerInfo` cannot reach `/run/dh/grpc.sock`: update o73 as blocked on
  worker UDS readiness or permissions.
- Bridge start returns sanitized `backend_unavailable` while snapstore and
  worker are healthy: inspect private bridge logs and file a bridge bug bead if
  the cause is bridge-owned.
- Bridge start returns an unsanitized public error: fix the redaction bug before
  rerunning acceptance.
- Stop fails or worker slot count does not return to pre-start value: treat as a
  cleanup defect, file a focused follow-up, and do not close o73.
- All start/status/stop checks pass and sanitization passes: close o73.
