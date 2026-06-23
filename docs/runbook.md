# Runbook

Date: 2026-06-23

## Service Commands

Format and test the Rust service:

```sh
cargo fmt --manifest-path service/Cargo.toml -- --check
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
cargo run --manifest-path service/Cargo.toml
```

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
