# Runbook

Date: 2026-06-23

## Service Commands

Format, build, and test the Rust service:

```sh
cargo fmt --manifest-path service/Cargo.toml -- --check
cargo build --manifest-path service/Cargo.toml
cargo test --manifest-path service/Cargo.toml --all-targets
```

Run the scaffolded synthetic service on the frozen deployment bind:

```sh
ROM_OPERATOR_BRIDGE_BACKEND=synthetic \
cargo run --manifest-path service/Cargo.toml
```

For local development on loopback, override only the bind address:

```sh
ROM_OPERATOR_BRIDGE_BIND_ADDR=127.0.0.1:7410 \
ROM_OPERATOR_BRIDGE_BACKEND=synthetic \
RUST_LOG=rom_operator_bridge_service=info \
cargo run --manifest-path service/Cargo.toml
```

Private operator config can come from environment variables or from an
uncommitted env file referenced by `ROM_OPERATOR_BRIDGE_CONFIG_FILE`. The file
must be mode `0600`, and any configured private root is created and enforced as
mode `0700` with private files written as mode `0600`.

Use placeholders only in shared docs:

```sh
ROM_OPERATOR_BRIDGE_CONFIG_FILE=<absolute-path-to-uncommitted-env-file>
ROM_OPERATOR_BRIDGE_PRIVATE_ROOT=<absolute-private-runtime-root>
ROM_OPERATOR_BRIDGE_STATIC_PUBLISH_ROOT=<absolute-static-publish-root>
ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<operator-credential-from-secret-source>
ROM_OPERATOR_BRIDGE_SESSION_SECRET=<session-secret-from-secret-source>
```

The private root must not be world-writable and must not be inside the static
publish root. Do not commit the env file, credentials, tokens, ROM paths, or
real private root paths.

Check liveness:

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

The scaffold exposes no private paths or runtime artifact references through
`GET /health`. Runtime auth, session start/stop, frame preview, padlog writing,
and WebSocket routes are later beads.
