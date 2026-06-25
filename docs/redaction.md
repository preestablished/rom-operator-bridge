# Static Redaction Gate

`scripts/redaction-gate.sh` is the publish-blocking static scan for public
bridge material. It is intentionally agent-runnable and is called by
`scripts/quality-gate.sh`.

The gate scans:

- public docs: `README.md`, `docs/`, and `contracts/`;
- deployment templates and sanitized deployment handoff material under
  `deploy/`;
- UI shell/config: `ui/README.md`, `ui/index.html`, and `ui/public/`;
- built UI output: `ui/dist/`.

Before scanning, the gate runs `npm --prefix ui run build` so `ui/dist/` is
fresh. It then creates a temporary aggregate text input and invokes:

```sh
cargo run --locked -p refwork-verify -- redaction-scan \
  --input <aggregate-static-output> \
  --report <private-validation-dir>/redaction-scan.json \
  --forbid-file <default-canary-forbid-file> \
  --forbid-file <operator-private-forbid-file>
```

Set `BRIDGE_REFERENCE_WORKLOAD_CHECKOUT` or
`ROM_OPERATOR_BRIDGE_REFERENCE_WORKLOAD` when the reference workload checkout is
not at `/home/infra-admin/git/preestablished/reference-workload`.

Set `ROM_OPERATOR_BRIDGE_FORBID_FILE` for operator-specific private literals.
Set `ROM_OPERATOR_BRIDGE_REQUIRE_FORBID_FILE=1` for publish/deploy runs; this
fails the gate unless the private forbid file is present. If
`ROM_OPERATOR_BRIDGE_VALIDATION_DIR` is unset, reports go to
`$ROM_OPERATOR_BRIDGE_PRIVATE_ROOT/validation` when that root is set; otherwise
they are written under a temporary directory for the current run.

The wrapper also checks static-output classes not currently built into
`refwork-verify redaction-scan`: ROM/private corpus paths, credential-shaped
values, real capture IDs, screenshot or preview-cache payloads, source-map
private paths, validation excerpts, private network literals, and binary/blob
static assets, including symlinks in scanned public roots. Console output
remains sanitized; finding paths and JSON reports are private validation
artifacts. The gate uses private file permissions for generated reports.
