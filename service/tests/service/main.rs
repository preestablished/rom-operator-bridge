use axum::{
    body::{Body, to_bytes},
    http::{
        HeaderMap, HeaderName, Request, StatusCode,
        header::{CACHE_CONTROL, PRAGMA},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    backend::{
        BackendCapabilities, BackendError, BackendMode, BridgeBackend, InputScheduleRequest,
        RealBackendUnavailable, StartBackendSession, SyntheticBackend,
    },
    config::{ConfigError, DEFAULT_BIND_ADDR, ENV_BACKEND_MODE, ENV_BIND_ADDR, ServiceConfig},
    input::{PadButton, PadWord},
};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::ServiceExt;

#[tokio::test]
async fn health_route_returns_schema_v1_without_private_paths() {
    let app = router(AppState::synthetic_for_tests(
        ServiceConfig::synthetic_for_addr("127.0.0.1:0".parse().expect("test address parses")),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("health request succeeds");

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), 4096)
        .await
        .expect("body reads");
    let json: Value = serde_json::from_slice(&body).expect("health response is json");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["ok"], true);
    assert_eq!(json["service_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["backend_mode"], "synthetic");
    assert_eq!(json["runtime_api"], 1);
    assert_matches_runtime_schema(&json);

    let body = String::from_utf8(body.to_vec()).expect("body is utf8");
    assert!(!body.contains("/home/"));
    assert!(!body.contains("/run/"));
    assert!(!body.contains("rom-operator-bridge.env"));
    assert!(!body.contains(DEFAULT_BIND_ADDR));
}

#[tokio::test]
async fn unimplemented_api_route_uses_common_error_envelope() {
    let app = router(AppState::synthetic_for_tests(
        ServiceConfig::synthetic_for_addr("127.0.0.1:0".parse().expect("test address parses")),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/missing")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("missing route request succeeds");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let headers = response.headers().clone();

    let body = to_bytes(response.into_body(), 4096)
        .await
        .expect("body reads");
    let json: Value = serde_json::from_slice(&body).expect("error response is json");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["error"]["code"], "bad_request");
    assert_eq!(json["error"]["retryable"], false);
    assert_eq!(json["error"]["details"], serde_json::json!({}));
    assert_matches_runtime_schema(&json);
    assert_runtime_error_headers(&headers);
}

#[tokio::test]
async fn unsupported_method_uses_common_error_envelope() {
    let app = router(AppState::synthetic_for_tests(
        ServiceConfig::synthetic_for_addr("127.0.0.1:0".parse().expect("test address parses")),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/health")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("method mismatch request succeeds");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_runtime_error_headers(response.headers());

    let body = to_bytes(response.into_body(), 4096)
        .await
        .expect("body reads");
    let json: Value = serde_json::from_slice(&body).expect("error response is json");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["error"]["code"], "bad_request");
    assert_eq!(json["error"]["message"], "Method not allowed.");
    assert_matches_runtime_schema(&json);
}

#[test]
fn config_loads_defaults_and_overrides_from_pairs() {
    let default_config = ServiceConfig::from_pairs([] as [(&str, &str); 0]).expect("defaults load");
    assert_eq!(
        default_config.bind_addr(),
        DEFAULT_BIND_ADDR
            .parse::<SocketAddr>()
            .expect("default parses")
    );
    assert_eq!(default_config.backend_mode(), BackendMode::Synthetic);

    let overridden = ServiceConfig::from_pairs([
        (ENV_BIND_ADDR, "127.0.0.1:0"),
        (ENV_BACKEND_MODE, "synthetic"),
    ])
    .expect("overrides load");
    assert_eq!(
        overridden.bind_addr(),
        "127.0.0.1:0"
            .parse::<SocketAddr>()
            .expect("override parses")
    );
    assert_eq!(overridden.backend_mode(), BackendMode::Synthetic);
}

#[test]
fn config_rejects_invalid_overrides() {
    assert_eq!(
        ServiceConfig::from_pairs([(ENV_BIND_ADDR, "not-a-socket")]),
        Err(ConfigError::InvalidBindAddr { env: ENV_BIND_ADDR })
    );
    assert_eq!(
        ServiceConfig::from_pairs([(ENV_BACKEND_MODE, "control-plane")]),
        Err(ConfigError::InvalidBackendMode {
            env: ENV_BACKEND_MODE
        })
    );
}

#[test]
fn backend_trait_surface_wires_synthetic_and_real_modes() {
    let synthetic = SyntheticBackend;
    let requested_capabilities = BackendCapabilities::synthetic_mvp();
    let session = synthetic
        .start_session(StartBackendSession {
            requested_capabilities,
        })
        .expect("synthetic session starts");

    assert_eq!(
        session.state,
        rom_operator_bridge_service::backend::SessionState::Running
    );
    assert_eq!(session.capabilities, requested_capabilities);

    let pad_word = PadWord::from_buttons([PadButton::A]);
    let receipt = synthetic
        .inject_input(InputScheduleRequest {
            session_id: session.session_id.clone(),
            target_frame: 1,
            pad_word,
        })
        .expect("synthetic input schedules");

    assert_eq!(receipt.session_id, session.session_id);
    assert_eq!(receipt.assigned_frame, 1);
    assert_eq!(receipt.pad_word, pad_word);

    let real = RealBackendUnavailable;
    assert_eq!(
        real.start_session(StartBackendSession {
            requested_capabilities,
        }),
        Err(BackendError::BackendUnavailable)
    );
}

#[tokio::test]
async fn service_serves_health_over_local_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local listener binds");
    let addr = listener.local_addr().expect("local addr exists");
    let app = router(AppState::synthetic_for_tests(
        ServiceConfig::synthetic_for_addr(addr),
    ));

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server runs");
    });

    let mut stream = TcpStream::connect(addr).await.expect("client connects");
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("request writes");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("response reads");

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains(r#""schema_version":1"#));
    assert!(response.contains(r#""backend_mode":"synthetic""#));

    server.abort();
}

fn assert_matches_runtime_schema(json: &Value) {
    let schema: Value =
        serde_json::from_str(include_str!("../../../contracts/runtime-api.schema.json"))
            .expect("runtime schema parses");
    let validator = jsonschema::validator_for(&schema).expect("runtime schema compiles");
    validator.validate(json).unwrap_or_else(|error| {
        panic!("runtime schema validation failed: {error}");
    });
}

fn assert_runtime_error_headers(headers: &HeaderMap) {
    assert_eq!(
        headers
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        headers.get(PRAGMA).and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(
        headers
            .get(HeaderName::from_static("x-content-type-options"))
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
}
