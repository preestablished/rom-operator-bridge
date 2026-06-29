# Dependencies and Worker Client

## Cargo Dependencies

In `service/Cargo.toml`, add dependencies that match the versions resolved by
`determinism-hypervisor`. Confirm with `cargo metadata` before editing so the
bridge does not pull in a second incompatible tonic/prost major version:

```toml
dh-proto = { path = "../../determinism-hypervisor/crates/dh-proto" }
hyper-util = "0.1"
tonic = "0.12"
tower = { version = "0.5", features = ["util"] }
```

`tokio` already has `net`, `rt-multi-thread`, and `sync`, which are enough for
UDS transport and the command loop.

After editing dependencies, run:

```bash
cd service
cargo check
```

Expect `service/Cargo.lock` to change.

## Endpoint Support

Use the existing `HypervisorEndpoint` enum in `service/src/private_config.rs`:

- `unix:///run/dh/grpc.sock` maps to a UDS connector;
- `http://127.0.0.1:7400` maps to a normal tonic `Endpoint`.

Add public accessors needed by the backend:

```rust
impl HypervisorEndpoint {
    pub fn unix_path(&self) -> Option<&Path>;
    pub fn http_uri(&self) -> Option<&str>;
}
```

Do not expose those values in Debug output. Keep the existing redacted Debug
implementations.

## UDS Client Pattern

Model the UDS connector after
`determinism-hypervisor/crates/dh-worker/tests/m6_full_api_uds.rs`:

```rust
use dh_proto::v1::hypervisor_worker_client::HypervisorWorkerClient;
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tonic::transport::Endpoint;
use tower::service_fn;

Endpoint::try_from("http://[::]:0")?
    .connect_with_connector(service_fn(move |_uri: tonic::transport::Uri| {
        let path = uds_path.clone();
        async move {
            let stream = UnixStream::connect(path).await?;
            Ok::<_, std::io::Error>(TokioIo::new(stream))
        }
    }))
    .await
    .map(HypervisorWorkerClient::new)
```

For HTTP endpoints, use `Endpoint::from_shared(uri.to_owned())?.connect().await`.

## Command Loop Shape

Add a private helper in `service/src/backend.rs` or a new
`service/src/real_backend.rs` module. Prefer a module if the backend file starts
getting too large.

Suggested types:

```rust
struct RealWorkerThread {
    tx: std::sync::mpsc::Sender<RealWorkerCommand>,
}

enum RealWorkerCommand {
    Start {
        config: RealRuntimeConfig,
        requested: BackendCapabilities,
        reply: std::sync::mpsc::Sender<BackendResult<RealStartOutcome>>,
    },
    Stop {
        session_id: SessionId,
        lease: dh_proto::v1::Lease,
        reply: std::sync::mpsc::Sender<BackendResult<StoppedSession>>,
    },
    Pause { ... },
    Resume { ... },
    Status { ... },
}
```

The command-loop thread owns:

- a Tokio runtime;
- one `HypervisorWorkerClient`;
- reconnection logic when the first command cannot connect.

The loop must not log or return raw endpoint paths, lease tokens, snapshot refs,
or tonic status messages to public callers. Internally, use `tracing::debug!`
with redacted labels only.

## Failure Mapping

Map all worker connection failures, tonic statuses, missing response fields, and
invalid private refs to:

```rust
Err(BackendError::BackendUnavailable)
```

Do not add public details to `AppError`. The existing `backend_error` and
`start_session` failure paths already emit:

```json
{"code":"backend_unavailable","message":"Backend unavailable.","details":{}}
```
