use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{COOKIE, HOST, ORIGIN, SET_COOKIE},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::{ALLOWED_ORIGIN, AuthState, SESSION_COOKIE_NAME, SESSION_TTL_SECONDS},
    config::{ENV_BACKEND_MODE, ENV_DEPLOYMENT_PROFILES, ServiceConfig},
    private_config::{
        ENV_CAPTURE_SPEC_REF, ENV_PRIVATE_ROOT, ENV_REAL_SNAPSHOT_REF,
        ENV_REFERENCE_WORKLOAD_CHECKOUT, ENV_SESSION_SECRET, ENV_WORKLOAD_IMAGE_REF,
    },
};
use serde_json::{Value, json};
use std::path::PathBuf;
use tower::ServiceExt;

const QUERY_SECRET: &str = "private-secret-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const TAILSCALE_ORIGIN: &str = "http://tailrombridge.birb.homes";

#[tokio::test]
async fn missing_session_cookie_is_rejected_without_private_details() {
    let (_workspace, app, private_root) = auth_app();

    let response = app
        .oneshot(runtime_request(Method::GET, "/api/session", Body::empty()))
        .await
        .expect("request runs");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_auth_safe_error(response, "session_inactive", &private_root).await;
}

#[tokio::test]
async fn query_strings_are_rejected_without_leaks() {
    let (_workspace, app, private_root) = auth_app();

    let query_response = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start?next=private-secret-from-test-source",
            Body::from(start_session_body()),
        ))
        .await
        .expect("query request runs");
    assert_eq!(query_response.status(), StatusCode::BAD_REQUEST);
    assert_auth_safe_error(query_response, "auth_rejected", &private_root).await;
}

#[tokio::test]
async fn unrelated_absent_and_null_origins_are_rejected() {
    let (_workspace, app, private_root) = auth_app();

    for origin in [Some("https://example.invalid"), Some("null"), None] {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/session/start")
            .header("content-type", "application/json");
        if let Some(origin) = origin {
            builder = builder.header(ORIGIN, origin);
        }

        let response = app
            .clone()
            .oneshot(
                builder
                    .body(Body::from(start_session_body()))
                    .expect("request builds"),
            )
            .await
            .expect("origin request runs");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_auth_safe_error(response, "origin_rejected", &private_root).await;
    }
}

#[tokio::test]
async fn same_origin_get_without_origin_header_uses_host_profile() {
    let (_workspace, app, private_root) = auth_app();

    // Browsers omit the Origin header on same-origin GET requests; the Host
    // header must be enough to resolve the deployment profile.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/session")
                .header(HOST, host_for_origin(ALLOWED_ORIGIN))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request runs");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_auth_safe_error(response, "session_inactive", &private_root).await;

    // An unrecognized Host without an Origin header is still rejected.
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/session")
                .header(HOST, "unknown.example.invalid")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request runs");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_auth_safe_error(response, "origin_rejected", &private_root).await;
}

#[tokio::test]
async fn legacy_start_body_still_requires_valid_origin_first() {
    let (_workspace, app, private_root) = auth_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/session/start")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "schema_version": 1,
                        "operator_credential": "deprecated-secret",
                        "backend_mode": "synthetic",
                        "requested_capabilities": ["input"]
                    })
                    .to_string(),
                ))
                .expect("request builds"),
        )
        .await
        .expect("legacy body request runs");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_auth_safe_error(response, "origin_rejected", &private_root).await;
}

#[tokio::test]
async fn runtime_requests_reject_host_origin_profile_mismatches() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(tailscale_config(
        &private_root,
    )));

    let response = app
        .oneshot(
            runtime_request_with_origin(
                Method::POST,
                "/api/session/start",
                Body::from(start_session_body()),
                TAILSCALE_ORIGIN,
            )
            .with_header(HOST, "rombridge.birb.homes"),
        )
        .await
        .expect("host mismatch request runs");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_auth_safe_error(response, "origin_rejected", &private_root).await;
}

#[tokio::test]
async fn successful_login_sets_strict_cookie_and_allows_session_status() {
    let (_workspace, app, _private_root) = auth_app();

    let login_response = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
        ))
        .await
        .expect("login runs");

    assert_eq!(login_response.status(), StatusCode::OK);
    assert_eq!(
        login_response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some(ALLOWED_ORIGIN)
    );
    let set_cookie = login_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("session cookie is set")
        .to_string();
    assert!(set_cookie.starts_with(&format!("{SESSION_COOKIE_NAME}=v1.")));
    assert!(set_cookie.contains("Path=/"));
    assert!(set_cookie.contains(&format!("Max-Age={SESSION_TTL_SECONDS}")));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("Secure"));
    assert!(set_cookie.contains("SameSite=Strict"));

    let login_json = json_body(login_response).await;
    assert_eq!(login_json["schema_version"], 1);
    assert_eq!(login_json["session_id"], "synthetic-session-scaffold");
    assert_eq!(login_json["run_id"], "synthetic-run-scaffold");
    assert_eq!(login_json["pad_layout"]["layout_id"], "console16-12btn-v1");

    let cookie = set_cookie
        .split(';')
        .next()
        .expect("cookie pair exists")
        .to_string();
    let status_response = app
        .oneshot(
            runtime_request(Method::GET, "/api/session", Body::empty())
                .map(|body| body)
                .with_header(COOKIE, cookie),
        )
        .await
        .expect("status request runs");

    assert_eq!(status_response.status(), StatusCode::OK);
    let status_json = json_body(status_response).await;
    assert_eq!(status_json["active"], true);
    assert_eq!(status_json["state"], "running");
}

#[tokio::test]
async fn tailscale_http_origin_sets_non_secure_cookie_and_allows_session_status() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(tailscale_config(
        &private_root,
    )));

    let login_response = app
        .clone()
        .oneshot(runtime_request_with_origin(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
            TAILSCALE_ORIGIN,
        ))
        .await
        .expect("tailscale login runs");

    assert_eq!(login_response.status(), StatusCode::OK);
    assert_eq!(
        login_response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some(TAILSCALE_ORIGIN)
    );
    let set_cookie = login_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("session cookie is set")
        .to_string();
    assert!(set_cookie.starts_with(&format!("{SESSION_COOKIE_NAME}=v1.")));
    assert_cookie_attribute(&set_cookie, "HttpOnly");
    assert_cookie_attribute(&set_cookie, "SameSite=Strict");
    assert_no_cookie_attribute(&set_cookie, "Secure");

    let cookie = set_cookie
        .split(';')
        .next()
        .expect("cookie pair exists")
        .to_string();
    let status_response = app
        .oneshot(
            runtime_request_with_origin(
                Method::GET,
                "/api/session",
                Body::empty(),
                TAILSCALE_ORIGIN,
            )
            .with_header(COOKIE, cookie),
        )
        .await
        .expect("tailscale status request runs");

    assert_eq!(status_response.status(), StatusCode::OK);
    assert_eq!(
        status_response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some(TAILSCALE_ORIGIN)
    );
    let status_json = json_body(status_response).await;
    assert_eq!(status_json["active"], true);
}

#[tokio::test]
async fn multi_profile_https_origin_keeps_secure_cookie() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(tailscale_config(
        &private_root,
    )));

    let login_response = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
        ))
        .await
        .expect("https login runs");

    assert_eq!(login_response.status(), StatusCode::OK);
    assert_eq!(
        login_response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some(ALLOWED_ORIGIN)
    );
    let set_cookie = login_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("session cookie is set");
    assert_cookie_attribute(set_cookie, "Secure");
    assert_cookie_attribute(set_cookie, "SameSite=Strict");
}

#[tokio::test]
async fn expired_session_cookie_is_rejected() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let auth = AuthState::fixed_for_tests(1_000);
    let app = router(AppState::synthetic_for_tests_with_auth(
        config(&private_root),
        auth.clone(),
    ));

    let login_response = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
        ))
        .await
        .expect("login runs");
    let cookie = login_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("cookie pair")
        .to_string();

    auth.advance_for_tests(SESSION_TTL_SECONDS + 1);

    let expired_response = app
        .oneshot(
            runtime_request(Method::GET, "/api/session", Body::empty()).with_header(COOKIE, cookie),
        )
        .await
        .expect("expired request runs");

    assert_eq!(expired_response.status(), StatusCode::UNAUTHORIZED);
    assert_auth_safe_error(expired_response, "session_inactive", &private_root).await;
}

#[tokio::test]
async fn only_one_operator_session_can_be_active() {
    let (_workspace, app, private_root) = auth_app();

    let first = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
        ))
        .await
        .expect("first login runs");
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
        ))
        .await
        .expect("second login runs");
    assert_eq!(second.status(), StatusCode::CONFLICT);
    assert_auth_safe_error(second, "session_active_elsewhere", &private_root).await;
}

#[tokio::test]
async fn backend_start_failure_does_not_leave_session_locked() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::from_config(real_config(&private_root)));

    let first = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body_for_backend("real")),
        ))
        .await
        .expect("first backend failure request runs");
    assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_auth_safe_error(first, "backend_unavailable", &private_root).await;

    let second = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body_for_backend("real")),
        ))
        .await
        .expect("second backend failure request runs");
    assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_auth_safe_error(second, "backend_unavailable", &private_root).await;
}

#[tokio::test]
async fn websocket_handshake_uses_same_origin_and_cookie_auth() {
    let (_workspace, app, _private_root) = auth_app();

    let missing_cookie = raw_ws_response(app.clone(), ALLOWED_ORIGIN, None, true).await;
    assert!(missing_cookie.starts_with("HTTP/1.1 401 Unauthorized"));

    let login_response = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
        ))
        .await
        .expect("login runs");
    let cookie = login_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("cookie pair")
        .to_string();

    let non_upgrade = raw_ws_response(app.clone(), ALLOWED_ORIGIN, Some(&cookie), false).await;
    assert!(!non_upgrade.starts_with("HTTP/1.1 101"));

    let accepted = raw_ws_response(app, ALLOWED_ORIGIN, Some(&cookie), true).await;
    let accepted_lower = accepted.to_ascii_lowercase();

    assert!(accepted.starts_with("HTTP/1.1 101 Switching Protocols"));
    assert!(accepted_lower.contains("upgrade: websocket"));
    assert!(accepted_lower.contains("sec-websocket-accept:"));
}

#[tokio::test]
async fn websocket_handshake_accepts_tailscale_http_origin() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(tailscale_config(
        &private_root,
    )));

    let login_response = app
        .clone()
        .oneshot(runtime_request_with_origin(
            Method::POST,
            "/api/session/start",
            Body::from(start_session_body()),
            TAILSCALE_ORIGIN,
        ))
        .await
        .expect("tailscale login runs");
    let cookie = login_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("cookie pair")
        .to_string();

    let accepted = raw_ws_response(app, TAILSCALE_ORIGIN, Some(&cookie), true).await;
    assert!(accepted.starts_with("HTTP/1.1 101 Switching Protocols"));
}

trait RequestExt {
    fn with_header(
        self,
        name: impl axum::http::header::IntoHeaderName,
        value: impl AsRef<str>,
    ) -> Self;
}

impl RequestExt for Request<Body> {
    fn with_header(
        mut self,
        name: impl axum::http::header::IntoHeaderName,
        value: impl AsRef<str>,
    ) -> Self {
        self.headers_mut().insert(
            name,
            value.as_ref().parse().expect("test header value parses"),
        );
        self
    }
}

fn auth_app() -> (tempfile::TempDir, axum::Router, PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(config(&private_root)));
    (workspace, app, private_root)
}

fn config(private_root: &std::path::Path) -> ServiceConfig {
    ServiceConfig::from_pairs([
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
    ])
    .expect("private config loads")
}

fn tailscale_config(private_root: &std::path::Path) -> ServiceConfig {
    let mut pairs = private_pairs(private_root);
    pairs.extend([
        (
            ENV_DEPLOYMENT_PROFILES.to_string(),
            "https-origin,tailscale-http".to_string(),
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_HTTPS_ORIGIN_PUBLIC_ORIGIN".to_string(),
            ALLOWED_ORIGIN.to_string(),
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_PUBLIC_ORIGIN".to_string(),
            TAILSCALE_ORIGIN.to_string(),
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_ALLOWED_ORIGINS".to_string(),
            TAILSCALE_ORIGIN.to_string(),
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_COOKIE_SECURE".to_string(),
            "false".to_string(),
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_EXPOSURE_MODE".to_string(),
            "tailscale-http".to_string(),
        ),
    ]);
    ServiceConfig::from_pairs(pairs).expect("tailscale config loads")
}

fn private_pairs(private_root: &std::path::Path) -> Vec<(String, String)> {
    vec![
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
    ]
}

fn real_config(private_root: &std::path::Path) -> ServiceConfig {
    let reference_checkout = private_root
        .parent()
        .expect("private root has parent")
        .join("reference-workload");
    ServiceConfig::from_pairs([
        (ENV_BACKEND_MODE.to_string(), "real".to_string()),
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
        (
            ENV_WORKLOAD_IMAGE_REF.to_string(),
            "private-workload-image-ref-from-test".to_string(),
        ),
        (
            ENV_CAPTURE_SPEC_REF.to_string(),
            "private-capture-spec-ref-from-test".to_string(),
        ),
        (
            ENV_REFERENCE_WORKLOAD_CHECKOUT.to_string(),
            reference_checkout.display().to_string(),
        ),
        (
            ENV_REAL_SNAPSHOT_REF.to_string(),
            "private-snapshot-ref-from-test".to_string(),
        ),
    ])
    .expect("real private config loads")
}

fn runtime_request(method: Method, uri: &str, body: Body) -> Request<Body> {
    runtime_request_with_origin(method, uri, body, ALLOWED_ORIGIN)
}

fn runtime_request_with_origin(
    method: Method,
    uri: &str,
    body: Body,
    origin: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(HOST, host_for_origin(origin))
        .header(ORIGIN, origin)
        .header("content-type", "application/json")
        .body(body)
        .expect("request builds")
}

async fn raw_ws_response(
    app: axum::Router,
    origin: &str,
    cookie: Option<&str>,
    upgrade: bool,
) -> String {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener binds");
    let addr = listener.local_addr().expect("listener addr is available");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server runs");
    });

    let mut stream = TcpStream::connect(addr)
        .await
        .expect("test client connects");
    let mut request = format!(
        "GET /ws/input HTTP/1.1\r\nHost: {}\r\nOrigin: {origin}\r\n",
        host_for_origin(origin)
    );
    if upgrade {
        request.push_str(
            "Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
        );
    }
    if let Some(cookie) = cookie {
        request.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .expect("request writes");
    let mut buffer = [0_u8; 4096];
    let read = stream.read(&mut buffer).await.expect("response reads");
    server.abort();

    String::from_utf8_lossy(&buffer[..read]).into_owned()
}

fn host_for_origin(origin: &str) -> &str {
    origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
        .expect("test origin includes scheme")
}

fn assert_cookie_attribute(set_cookie: &str, attribute: &str) {
    assert!(
        cookie_has_attribute(set_cookie, attribute),
        "expected cookie attribute {attribute:?} in {set_cookie:?}",
    );
}

fn assert_no_cookie_attribute(set_cookie: &str, attribute: &str) {
    assert!(
        !cookie_has_attribute(set_cookie, attribute),
        "unexpected cookie attribute {attribute:?} in {set_cookie:?}",
    );
}

fn cookie_has_attribute(set_cookie: &str, attribute: &str) -> bool {
    set_cookie
        .split(';')
        .map(str::trim)
        .any(|candidate| candidate.eq_ignore_ascii_case(attribute))
}

fn start_session_body() -> String {
    start_session_body_for_backend("synthetic")
}

fn start_session_body_for_backend(backend_mode: &str) -> String {
    json!({
        "schema_version": 1,
        "backend_mode": backend_mode,
        "requested_capabilities": ["input"]
    })
    .to_string()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 4096)
        .await
        .expect("body reads");
    serde_json::from_slice(&bytes).expect("json parses")
}

async fn assert_auth_safe_error(
    response: axum::response::Response,
    expected_code: &str,
    private_root: &std::path::Path,
) {
    let bytes = to_bytes(response.into_body(), 4096)
        .await
        .expect("body reads");
    let body = String::from_utf8(bytes.to_vec()).expect("body is utf8");
    let json: Value = serde_json::from_str(&body).expect("error json parses");

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["error"]["code"], expected_code);
    assert_eq!(json["error"]["details"], json!({}));
    assert!(!body.contains(QUERY_SECRET));
    assert!(!body.contains(SESSION_SECRET));
    assert!(!body.contains(&private_root.display().to_string()));
}
