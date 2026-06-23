use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{COOKIE, ORIGIN, SET_COOKIE, UPGRADE},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::{
        ALLOWED_ORIGIN, AuthState, MAX_FAILED_AUTH_ATTEMPTS, SESSION_COOKIE_NAME,
        SESSION_TTL_SECONDS,
    },
    config::{ENV_BACKEND_MODE, ServiceConfig},
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
};
use serde_json::{Value, json};
use std::path::PathBuf;
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";

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
async fn bad_credential_and_credential_in_query_are_rejected_without_leaks() {
    let (_workspace, app, private_root) = auth_app();

    let bad_response = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session",
            Body::from(start_session_body("wrong-credential")),
        ))
        .await
        .expect("bad credential request runs");
    assert_eq!(bad_response.status(), StatusCode::UNAUTHORIZED);
    assert_auth_safe_error(bad_response, "auth_rejected", &private_root).await;

    let query_response = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session?operator_credential=operator-credential-from-test-source",
            Body::from(start_session_body(GOOD_CREDENTIAL)),
        ))
        .await
        .expect("query credential request runs");
    assert_eq!(query_response.status(), StatusCode::BAD_REQUEST);
    assert_auth_safe_error(query_response, "auth_rejected", &private_root).await;
}

#[tokio::test]
async fn unrelated_absent_and_null_origins_are_rejected() {
    let (_workspace, app, private_root) = auth_app();

    for origin in [Some("https://example.invalid"), Some("null"), None] {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/api/session")
            .header("content-type", "application/json");
        if let Some(origin) = origin {
            builder = builder.header(ORIGIN, origin);
        }

        let response = app
            .clone()
            .oneshot(
                builder
                    .body(Body::from(start_session_body(GOOD_CREDENTIAL)))
                    .expect("request builds"),
            )
            .await
            .expect("origin request runs");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_auth_safe_error(response, "origin_rejected", &private_root).await;
    }
}

#[tokio::test]
async fn successful_login_sets_strict_cookie_and_allows_session_status() {
    let (_workspace, app, _private_root) = auth_app();

    let login_response = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session",
            Body::from(start_session_body(GOOD_CREDENTIAL)),
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
            "/api/session",
            Body::from(start_session_body(GOOD_CREDENTIAL)),
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
            "/api/session",
            Body::from(start_session_body(GOOD_CREDENTIAL)),
        ))
        .await
        .expect("first login runs");
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session",
            Body::from(start_session_body(GOOD_CREDENTIAL)),
        ))
        .await
        .expect("second login runs");
    assert_eq!(second.status(), StatusCode::CONFLICT);
    assert_auth_safe_error(second, "session_active_elsewhere", &private_root).await;
}

#[tokio::test]
async fn failed_auth_attempts_are_rate_limited() {
    let (_workspace, app, private_root) = auth_app();

    let mut last_response = None;
    for _ in 0..MAX_FAILED_AUTH_ATTEMPTS {
        last_response = Some(
            app.clone()
                .oneshot(runtime_request(
                    Method::POST,
                    "/api/session",
                    Body::from(start_session_body("wrong-credential")),
                ))
                .await
                .expect("bad credential request runs"),
        );
    }

    let response = last_response.expect("at least one response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_auth_safe_error(response, "auth_rejected", &private_root).await;
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
            "/api/session",
            Body::from(start_session_body_for_backend(GOOD_CREDENTIAL, "real")),
        ))
        .await
        .expect("first backend failure request runs");
    assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_auth_safe_error(first, "backend_unavailable", &private_root).await;

    let second = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session",
            Body::from(start_session_body_for_backend(GOOD_CREDENTIAL, "real")),
        ))
        .await
        .expect("second backend failure request runs");
    assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_auth_safe_error(second, "backend_unavailable", &private_root).await;
}

#[tokio::test]
async fn websocket_handshake_uses_same_origin_and_cookie_auth() {
    let (_workspace, app, _private_root) = auth_app();

    let missing_cookie = app
        .clone()
        .oneshot(
            runtime_request(Method::GET, "/ws/input", Body::empty())
                .with_header(UPGRADE, "websocket"),
        )
        .await
        .expect("ws missing cookie request runs");
    assert_eq!(missing_cookie.status(), StatusCode::UNAUTHORIZED);

    let login_response = app
        .clone()
        .oneshot(runtime_request(
            Method::POST,
            "/api/session",
            Body::from(start_session_body(GOOD_CREDENTIAL)),
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

    let accepted = app
        .oneshot(
            runtime_request(Method::GET, "/ws/input", Body::empty())
                .with_header(COOKIE, cookie)
                .with_header(UPGRADE, "websocket"),
        )
        .await
        .expect("ws accepted request runs");

    assert_eq!(accepted.status(), StatusCode::SWITCHING_PROTOCOLS);
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
        (
            ENV_OPERATOR_CREDENTIAL.to_string(),
            GOOD_CREDENTIAL.to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
    ])
    .expect("private config loads")
}

fn real_config(private_root: &std::path::Path) -> ServiceConfig {
    ServiceConfig::from_pairs([
        (ENV_BACKEND_MODE.to_string(), "real".to_string()),
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (
            ENV_OPERATOR_CREDENTIAL.to_string(),
            GOOD_CREDENTIAL.to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
    ])
    .expect("real private config loads")
}

fn runtime_request(method: Method, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
        .header("content-type", "application/json")
        .body(body)
        .expect("request builds")
}

fn start_session_body(credential: &str) -> String {
    start_session_body_for_backend(credential, "synthetic")
}

fn start_session_body_for_backend(credential: &str, backend_mode: &str) -> String {
    json!({
        "schema_version": 1,
        "operator_credential": credential,
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
    assert!(!body.contains(GOOD_CREDENTIAL));
    assert!(!body.contains(SESSION_SECRET));
    assert!(!body.contains(&private_root.display().to_string()));
}
