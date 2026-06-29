use axum::{
    body::{Body, to_bytes},
    http::{
        HeaderMap, HeaderName, Request, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, PRAGMA},
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
    private_config::{
        ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET, ENV_STATIC_PUBLISH_ROOT,
    },
};
use serde_json::Value;
use std::{fs, net::SocketAddr, path::Path};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::ServiceExt;

#[cfg(unix)]
use std::os::unix::fs::symlink;

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
    assert_no_store_headers(response.headers());

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
    assert_no_store_headers(&headers);
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
    assert_no_store_headers(response.headers());

    let body = to_bytes(response.into_body(), 4096)
        .await
        .expect("body reads");
    let json: Value = serde_json::from_slice(&body).expect("error response is json");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["error"]["code"], "bad_request");
    assert_eq!(json["error"]["message"], "Method not allowed.");
    assert_matches_runtime_schema(&json);
}

#[tokio::test]
async fn configured_static_root_serves_ui_shell_with_security_headers() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let static_root = workspace.path().join("static-publish");
    write_static_file(
        &static_root.join("index.html"),
        "<main>ROM operator shell</main>",
    );
    write_static_file(
        &static_root.join("runtime-config.json"),
        r#"{"schema_version":1}"#,
    );

    let app = router(AppState::synthetic_for_tests(config_with_static_root(
        &private_root,
        &static_root,
    )));

    let index = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("static index request succeeds");

    assert_eq!(index.status(), StatusCode::OK);
    assert_static_headers(index.headers());
    assert_eq!(
        index
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8")
    );
    let body = body_string(index).await;
    assert!(body.contains("ROM operator shell"));
    assert!(!body.contains(&static_root.display().to_string()));

    let runtime_config = app
        .oneshot(
            Request::builder()
                .uri("/runtime-config.json")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("runtime config request succeeds");

    assert_eq!(runtime_config.status(), StatusCode::OK);
    assert_static_headers(runtime_config.headers());
    assert_eq!(
        runtime_config
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json; charset=utf-8")
    );
}

#[tokio::test]
async fn static_spa_fallback_does_not_shadow_runtime_routes() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let static_root = workspace.path().join("static-publish");
    write_static_file(&static_root.join("index.html"), "<main>SPA fallback</main>");

    let app = router(AppState::synthetic_for_tests(config_with_static_root(
        &private_root,
        &static_root,
    )));

    let fallback = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/operator/session")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("spa fallback request succeeds");
    assert_eq!(fallback.status(), StatusCode::OK);
    assert!(body_string(fallback).await.contains("SPA fallback"));

    let missing_api = app
        .oneshot(
            Request::builder()
                .uri("/api/missing")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("missing api request succeeds");
    assert_eq!(missing_api.status(), StatusCode::NOT_FOUND);
    assert_no_store_headers(missing_api.headers());
    let body = body_string(missing_api).await;
    assert!(body.contains("Route not found."));
    assert!(!body.contains("SPA fallback"));
}

#[tokio::test]
async fn static_serving_rejects_unsafe_paths_and_source_maps() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let static_root = workspace.path().join("static-publish");
    write_static_file(&static_root.join("index.html"), "<main>safe shell</main>");
    write_static_file(
        &static_root.join("assets").join("app.js"),
        "console.log('ok');",
    );
    write_static_file(
        &static_root.join("assets").join("app.js.map"),
        r#"{"sources":["private"]}"#,
    );
    write_static_file(&static_root.join(".hidden"), "private");

    let app = router(AppState::synthetic_for_tests(config_with_static_root(
        &private_root,
        &static_root,
    )));

    for uri in [
        "/../private",
        "/%2e%2e/private",
        "/.hidden",
        "/assets/app.js.map",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("unsafe static request succeeds");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "uri={uri}");
        assert!(
            !body_string(response)
                .await
                .contains(&static_root.display().to_string())
        );
    }

    let asset = app
        .oneshot(
            Request::builder()
                .uri("/assets/app.js")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("safe asset request succeeds");
    assert_eq!(asset.status(), StatusCode::OK);
}

#[cfg(unix)]
#[tokio::test]
async fn static_serving_rejects_symlinked_files() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let static_root = workspace.path().join("static-publish");
    let secret_file = workspace.path().join("not-public.txt");
    fs::write(&secret_file, "private static leak").expect("secret fixture writes");
    write_static_file(&static_root.join("index.html"), "<main>safe shell</main>");
    symlink(&secret_file, static_root.join("leak.txt")).expect("symlink creates");

    let app = router(AppState::synthetic_for_tests(config_with_static_root(
        &private_root,
        &static_root,
    )));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/leak.txt")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("symlink request succeeds");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = body_string(response).await;
    assert!(!body.contains("private static leak"));
    assert!(!body.contains(&secret_file.display().to_string()));
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
    let synthetic = SyntheticBackend::new();
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
            client_seq: 1,
            source_id: "keyboard".to_string(),
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

async fn body_string(response: axum::response::Response) -> String {
    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("body reads");
    String::from_utf8(body.to_vec()).expect("body is utf8")
}

fn write_static_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("static parent creates");
    }
    fs::write(path, contents).expect("static file writes");
}

fn config_with_static_root(private_root: &Path, static_root: &Path) -> ServiceConfig {
    ServiceConfig::from_pairs([
        (ENV_BIND_ADDR.to_string(), "127.0.0.1:0".to_string()),
        (ENV_BACKEND_MODE.to_string(), "synthetic".to_string()),
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (
            ENV_STATIC_PUBLISH_ROOT.to_string(),
            static_root.display().to_string(),
        ),
        (
            ENV_OPERATOR_CREDENTIAL.to_string(),
            "operator-credential-from-test-source".to_string(),
        ),
        (
            ENV_SESSION_SECRET.to_string(),
            "session-secret-from-test-source-32-bytes".to_string(),
        ),
    ])
    .expect("static root config loads")
}

fn assert_static_headers(headers: &HeaderMap) {
    assert_no_store_headers(headers);
    assert_eq!(
        headers
            .get(HeaderName::from_static("content-security-policy"))
            .and_then(|value| value.to_str().ok()),
        Some(
            "default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
        )
    );
    assert_eq!(
        headers
            .get(HeaderName::from_static("referrer-policy"))
            .and_then(|value| value.to_str().ok()),
        Some("no-referrer")
    );
    assert_eq!(
        headers
            .get(HeaderName::from_static("x-frame-options"))
            .and_then(|value| value.to_str().ok()),
        Some("DENY")
    );
}

fn assert_no_store_headers(headers: &HeaderMap) {
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
