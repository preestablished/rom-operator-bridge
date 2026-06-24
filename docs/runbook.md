# Runbook

Date: 2026-06-24

This runbook records the shared command contract for the bridge service and the
Phase-0-confirmed verifier workflow. Shared logs may include command templates,
exit status, and sanitized summaries only. For private real-host runs, share the
template with placeholders, not the instantiated command line. Do not paste
secret config values, private ROM paths, private bundle paths, validation report
contents, raw capture ids, decoded feature values, screenshots, or private
artifact refs into shared handoff text.

Use placeholders exactly as placeholders in shared docs:

```sh
ROM_OPERATOR_BRIDGE_CONFIG_FILE=<absolute-path-to-uncommitted-env-file>
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT=<absolute-private-runtime-root>
ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-publish-root>
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<operator-credential-from-secret-source>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<session-secret-from-secret-source>
```

Private operator config can come from environment variables or an uncommitted
env file referenced by `ROM_OPERATOR_BRIDGE_CONFIG_FILE`. The env file must be
mode `0600`; configured private roots are created and enforced as mode `0700`;
private files are written as mode `0600`. Do not commit the env file,
credentials, tokens, ROM paths, or real private root paths.

## Bridge Stack Commands

Run the frozen Phase 0 bridge-stack command family before handoff:

```sh
cargo fmt --manifest-path service/Cargo.toml -- --check
cargo test --manifest-path service/Cargo.toml --all-targets
npm --prefix ui ci
npm --prefix ui run typecheck
npm --prefix ui test -- --run
npm --prefix ui run build
scripts/quality-gate.sh
```

Ralph agents should use `scripts/quality-gate.sh` as the root VERIFY command.
The command is documented in `docs/quality-gate.md` and is the extension point
for later static redaction and synthetic integration gates.

Run a service-only compile smoke when iterating on Rust service changes:

```sh
cargo build --manifest-path service/Cargo.toml
```

Run the synthetic service using the configured deployment bind:

```sh
ROM_OPERATOR_BRIDGE_BACKEND=synthetic \
cargo run --manifest-path service/Cargo.toml
```

Check deployment-bind liveness:

```sh
curl -fsS http://10.0.0.106:7410/health
```

Run the synthetic service on loopback for local development:

```sh
ROM_OPERATOR_BRIDGE_BIND_ADDR=127.0.0.1:7410 \
ROM_OPERATOR_BRIDGE_BACKEND=synthetic \
RUST_LOG=rom_operator_bridge_service=info \
cargo run --manifest-path service/Cargo.toml
```

Check loopback development liveness:

```sh
curl -fsS http://127.0.0.1:7410/health
```

Expected shape:

```json
{
  "schema_version": 1,
  "ok": true,
  "service_version": "0.1.0",
  "backend_mode": "synthetic",
  "runtime_api": 1
}
```

`GET /health` must not expose private paths, credentials, runtime artifact refs,
or host-control details. Authenticated runtime requests must use the private
operator credential source; do not paste credential-bearing request bodies into
shared logs.

## Agent-Runnable Synthetic Checks

These checks use repository fixtures and synthetic paths only. They are safe for
agents to run and summarize by pass/fail count:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p refwork-script)
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p refwork-verify phase4 -- --nocapture)
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p xtask pad_layout)
(cd /home/infra-admin/git/preestablished/determinism-hypervisor && cargo test -p dh-worker inject_mapper)
(cd /home/infra-admin/git/preestablished/determinism-hypervisor && cargo test -p dh-devices frame_counter_write_logs_frame_mark)
(cd /home/infra-admin/git/preestablished/control-plane && cargo test -p determinism-proto --features scorer,inputsynth)
```

Phase 0 observed results:

```text
refwork-script: 12 passed
refwork-verify phase4 filter: 20 passed
xtask pad_layout filter: 4 passed
dh-worker inject_mapper filter: 4 passed
dh-devices frame_counter_write_logs_frame_mark filter: 1 passed
determinism-proto scorer,inputsynth: 19 passed
```

## Verifier Artifact Commands

The following command shapes are exact Phase-0-confirmed verifier commands. They
become operator-only commands whenever any placeholder points at a private ROM,
private capture bundle, private label file, private report path, or private
forbidden-literal file.

### Feature Map Validation

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-featuremap -- validate \
  <feature-map.yaml> \
  --scoring <scoring-program.yaml>)
```

### Layout

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-layout \
  --map <feature-map.yaml> \
  --out <layout.json> \
  --capture-spec-hash <blake3-or-ref> \
  --compiler-or-exporter-commit <commit>)
```

### Score Plan

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-score-plan \
  --captures <captures/index.jsonl> \
  --out <score-plan.json> \
  --first-boss <capture-id> \
  --goal-positive <capture-id> \
  --goal-negative <capture-id>)
```

### Trace

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- trace \
  --captures <captures/index.jsonl> \
  --map <feature-map.yaml> \
  --scoring <scoring-program.yaml> \
  --labels <private-bundle-dir>/labels/phase4-trace-labels.yaml \
  --out <trajectory.jsonl> \
  --report <trace-report.json>)
```

### Bundle Check

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-bundle-check \
  --bundle <private-bundle-dir> \
  --report <validation/phase4-bundle-check.json>)
```

### Checksum Manifest

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-checksum-manifest \
  --bundle <private-bundle-dir> \
  --out <validation/checksums.json>)
```

### Padlog Validation

The frozen padlog parser and writer live in
`/home/infra-admin/git/preestablished/reference-workload/crates/refwork-script/src/lib.rs`.
There is no standalone Phase-0-confirmed padlog validator CLI. The accepted
agent-runnable parser/writer validation is:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo test --locked -p refwork-script)
```

For private bundle context validation, `phase4-context-check` validates
`recent-input.padlog` when that private bundle file exists:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-context-check \
  --bundle <private-context-dir> \
  --report <validation/phase4-context-check.json>)
```

### Redaction Scan

Run before sharing public notes, static artifacts, or handoff text:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- redaction-scan \
  --input <public-note.md> \
  --report <validation/redaction-scan.json> \
  --forbid-file <private-forbid-literals.txt>)
```

The redaction scanner reports finding kind, line, and column only; it must not
echo matched private literals or source excerpts. Add operator-specific
forbidden literals with repeatable `--forbid` and `--forbid-file` arguments
before producing any public handoff.

## Private Real-Host Checks

Run these only on the operator-approved host with private ROM metadata, private
capture artifacts, private labels, and private forbidden-literal files available.
Keep command output and generated reports private. After redaction scanning and
operator approval, shared handoffs may include only aggregate status, counts,
pass/fail booleans, and approved labels; never share report bodies, capture ids,
decoded values, private artifact refs, private paths, or raw verifier/scorer
errors.

Private intake:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-private-intake \
  --private-root <private-root> \
  --operator-approved \
  --rom-dir <private-rom-dir>)
```

Private bundle check:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-bundle-check \
  --bundle <private-bundle-dir> \
  --report <validation/phase4-bundle-check.json>)
```

Private context check:

```sh
(cd /home/infra-admin/git/preestablished/reference-workload && cargo run --locked -p refwork-verify -- phase4-context-check \
  --bundle <private-context-dir> \
  --report <validation/phase4-context-check.json>)
```

Real acceptance remains blocked until the operator supplies private ROM
metadata, private capture artifacts, and private labels, and until the bridge can
write durable private capture payloads plus `captures/index.jsonl`.

## Deployment Checks

Future deployment checks once a service and route exist:

```sh
getent hosts rombridge.birb.homes
curl -I --resolve rombridge.birb.homes:443:10.0.0.106 https://rombridge.birb.homes/
curl -i -H 'Origin: https://example.invalid' https://rombridge.birb.homes/api/session
curl -i https://rombridge.birb.homes/api/session
curl -I https://rombridge.birb.homes/api/session
```

Expected deployment check results:

- hostname resolves to `10.0.0.106`;
- TLS is served for `rombridge.birb.homes`;
- unrelated origins are rejected;
- unauthenticated API requests are rejected without private details;
- runtime responses include `Cache-Control: no-store`.
