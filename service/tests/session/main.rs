use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{COOKIE, ORIGIN, SET_COOKIE},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::{ALLOWED_ORIGIN, AuthState, SESSION_TTL_SECONDS},
    backend::{BridgeBackend, SessionState, SyntheticBackend},
    config::ServiceConfig,
    private_config::{ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::Arc};
use tower::ServiceExt;

const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";

#[tokio::test]
async fn synthetic_session_lifecycle_reports_states_and_clears_auth_lock() {
    let (_workspace, app, private_root) = session_app();

    let (start, cookie) = start_session(app.clone()).await;
    assert_eq!(start["state"], "running");
    assert_eq!(start["current_frame"], 0);
    assert_matches_runtime_schema(&start);

    let running = request_json(
        app.clone(),
        runtime_request(Method::GET, "/api/run/status", Body::empty()).with_header(COOKIE, &cookie),
    )
    .await;
    assert_eq!(running["state"], "running");
    assert_eq!(running["current_frame"], 1);
    assert_eq!(running["preview_stale"], true);
    assert_matches_runtime_schema(&running);

    let paused = request_json(
        app.clone(),
        runtime_request(
            Method::POST,
            "/api/run/pause",
            Body::from(session_only_body(start["session_id"].as_str().unwrap())),
        )
        .with_header(COOKIE, &cookie),
    )
    .await;
    assert_eq!(paused["state"], "paused");
    assert_matches_runtime_schema(&paused);

    let paused_status = request_json(
        app.clone(),
        runtime_request(Method::GET, "/api/run/status", Body::empty()).with_header(COOKIE, &cookie),
    )
    .await;
    assert_eq!(paused_status["state"], "paused");
    assert_eq!(paused_status["current_frame"], paused["current_frame"]);

    let resumed = request_json(
        app.clone(),
        runtime_request(
            Method::POST,
            "/api/run/resume",
            Body::from(session_only_body(start["session_id"].as_str().unwrap())),
        )
        .with_header(COOKIE, &cookie),
    )
    .await;
    assert_eq!(resumed["state"], "running");
    assert_matches_runtime_schema(&resumed);

    let stopped_response = app
        .clone()
        .oneshot(
            runtime_request(
                Method::POST,
                "/api/session/stop",
                Body::from(stop_session_body(start["session_id"].as_str().unwrap())),
            )
            .with_header(COOKIE, &cookie),
        )
        .await
        .expect("stop request runs");
    assert_eq!(stopped_response.status(), StatusCode::OK);
    assert!(
        stopped_response
            .headers()
            .get(SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|cookie| cookie.contains("Max-Age=0"))
    );
    let stopped = json_body(stopped_response).await;
    assert_eq!(stopped["state"], "stopped");
    assert_matches_runtime_schema(&stopped);

    let (_second_start, _second_cookie) = start_session(app.clone()).await;

    let body = stopped.to_string();
    assert!(!body.contains(&private_root.display().to_string()));
}

#[tokio::test]
async fn synthetic_session_writes_manifest_and_private_event_rows_under_private_root() {
    let (_workspace, app, private_root) = session_app();
    let (start, cookie) = start_session(app.clone()).await;
    let run_id = start["run_id"].as_str().expect("run id is string");
    let session_id = start["session_id"].as_str().expect("session id is string");

    let _ = request_json(
        app.clone(),
        runtime_request(
            Method::POST,
            "/api/run/pause",
            Body::from(session_only_body(session_id)),
        )
        .with_header(COOKIE, &cookie),
    )
    .await;
    let _ = request_json(
        app.clone(),
        runtime_request(
            Method::POST,
            "/api/run/resume",
            Body::from(session_only_body(session_id)),
        )
        .with_header(COOKIE, &cookie),
    )
    .await;
    let _ = request_json(
        app,
        runtime_request(
            Method::POST,
            "/api/session/stop",
            Body::from(stop_session_body(session_id)),
        )
        .with_header(COOKIE, &cookie),
    )
    .await;

    let manifest_path = private_root
        .join("runs")
        .join(run_id)
        .join("run-manifest.json");
    let events_path = private_root
        .join("runs")
        .join(run_id)
        .join("bridge-events.jsonl");
    assert!(manifest_path.starts_with(&private_root));
    assert!(events_path.starts_with(&private_root));
    assert!(manifest_path.is_file());
    assert!(events_path.is_file());

    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).expect("manifest reads"))
            .expect("manifest parses");
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["run_id"], run_id);
    assert_eq!(manifest["backend_mode"], "synthetic");

    let events = fs::read_to_string(events_path).expect("events read");
    let event_types = events
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("event parses"))
        .map(|event| event["event_type"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        event_types,
        vec![
            "session_started",
            "session_paused",
            "session_resumed",
            "session_stopped"
        ]
    );
    assert!(!events.contains(&private_root.display().to_string()));
}

#[tokio::test]
async fn synthetic_backend_can_report_faulted_status() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let backend = Arc::new(SyntheticBackend::with_private_config(
        config(&private_root).private_config().clone(),
    ));
    let app = router(AppState::for_tests_with_backend(
        config(&private_root),
        AuthState::new(),
        backend.clone(),
    ));
    let (_start, cookie) = start_session(app.clone()).await;
    backend
        .fault_active_session_for_tests()
        .expect("synthetic session faults");

    let faulted = request_json(
        app,
        runtime_request(Method::GET, "/api/run/status", Body::empty()).with_header(COOKIE, cookie),
    )
    .await;

    assert_eq!(faulted["state"], "faulted");
    assert_matches_runtime_schema(&faulted);
}

#[tokio::test]
async fn faulted_synthetic_session_rejects_pause_resume_and_remains_faulted() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let backend = Arc::new(SyntheticBackend::with_private_config(
        config(&private_root).private_config().clone(),
    ));
    let app = router(AppState::for_tests_with_backend(
        config(&private_root),
        AuthState::new(),
        backend.clone(),
    ));
    let (start, cookie) = start_session(app.clone()).await;
    let session_id = start["session_id"].as_str().expect("session id is string");
    backend
        .fault_active_session_for_tests()
        .expect("synthetic session faults");

    for path in ["/api/run/pause", "/api/run/resume"] {
        let response = app
            .clone()
            .oneshot(
                runtime_request(
                    Method::POST,
                    path,
                    Body::from(session_only_body(session_id)),
                )
                .with_header(COOKIE, &cookie),
            )
            .await
            .expect("state transition request runs");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let status = request_json(
            app.clone(),
            runtime_request(Method::GET, "/api/run/status", Body::empty())
                .with_header(COOKIE, &cookie),
        )
        .await;
        assert_eq!(status["state"], "faulted");
    }
}

#[tokio::test]
async fn runtime_session_requests_reject_unknown_fields() {
    let (_workspace, app, _private_root) = session_app();
    let (start, cookie) = start_session(app.clone()).await;
    let session_id = start["session_id"].as_str().expect("session id is string");

    let response = app
        .oneshot(
            runtime_request(
                Method::POST,
                "/api/run/pause",
                Body::from(
                    json!({
                        "schema_version": 1,
                        "session_id": session_id,
                        "private_root": "/tmp/should-not-be-accepted"
                    })
                    .to_string(),
                ),
            )
            .with_header(COOKIE, &cookie),
        )
        .await
        .expect("pause request runs");

    assert!(matches!(
        response.status(),
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ));
}

#[tokio::test]
async fn expired_auth_stops_stale_synthetic_run_before_replacement() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let auth = AuthState::fixed_for_tests(1_000);
    let app = router(AppState::synthetic_for_tests_with_auth(
        config(&private_root),
        auth.clone(),
    ));

    let (first, _first_cookie) = start_session(app.clone()).await;
    let first_run_id = first["run_id"].as_str().expect("run id is string");
    auth.advance_for_tests(SESSION_TTL_SECONDS + 1);

    let (second, _second_cookie) = start_session(app).await;

    assert_ne!(second["run_id"], first["run_id"]);
    let events_path = private_root
        .join("runs")
        .join(first_run_id)
        .join("bridge-events.jsonl");
    let events = fs::read_to_string(events_path).expect("events read");
    let event_types = events
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("event parses"))
        .map(|event| event["event_type"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(event_types, vec!["session_started", "session_stopped"]);
}

#[test]
fn synthetic_backend_trait_tracks_pause_resume_stop_state() {
    let backend = SyntheticBackend::new();
    let session = backend
        .start_session(rom_operator_bridge_service::backend::StartBackendSession {
            requested_capabilities: backend.capabilities(),
        })
        .expect("session starts");
    assert_eq!(session.state, SessionState::Running);

    let paused = backend
        .pause(session.session_id.clone())
        .expect("session pauses");
    assert_eq!(paused.state, SessionState::Paused);
    let status = backend
        .status(session.session_id.clone())
        .expect("status reports paused");
    assert_eq!(status.state, SessionState::Paused);

    let resumed = backend
        .resume(session.session_id.clone())
        .expect("session resumes");
    assert_eq!(resumed.state, SessionState::Running);

    let stopped = backend
        .stop_session(
            session.session_id.clone(),
            rom_operator_bridge_service::backend::StopReason::OperatorStop,
        )
        .expect("session stops");
    assert_eq!(stopped.state, SessionState::Stopped);
    assert!(backend.status(session.session_id).is_err());
}

#[test]
fn synthetic_backend_artifact_failure_does_not_activate_session() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let service_config = config(&private_root);
    fs::set_permissions(&private_root, fs::Permissions::from_mode(0o500))
        .expect("private root permissions update");
    let backend = SyntheticBackend::with_private_config(service_config.private_config().clone());

    let start = backend.start_session(rom_operator_bridge_service::backend::StartBackendSession {
        requested_capabilities: backend.capabilities(),
    });

    fs::set_permissions(&private_root, fs::Permissions::from_mode(0o700))
        .expect("private root permissions restore");
    assert!(start.is_err());
    assert!(
        backend
            .status("synthetic-session-scaffold".to_string())
            .is_err()
    );
    let session = backend
        .start_session(rom_operator_bridge_service::backend::StartBackendSession {
            requested_capabilities: backend.capabilities(),
        })
        .expect("session starts after permissions are restored");
    assert_eq!(session.session_id, "synthetic-session-scaffold");
}

fn session_app() -> (tempfile::TempDir, axum::Router, std::path::PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(config(&private_root)));
    (workspace, app, private_root)
}

fn config(private_root: &Path) -> ServiceConfig {
    ServiceConfig::from_pairs([
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
    ])
    .expect("private config loads")
}

async fn start_session(app: axum::Router) -> (Value, String) {
    let response = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(
                json!({
                    "schema_version": 1,
                    "backend_mode": "synthetic",
                    "requested_capabilities": ["input"]
                })
                .to_string(),
            ),
        ))
        .await
        .expect("start session request runs");
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("session cookie pair")
        .to_string();
    (json_body(response).await, cookie)
}

async fn request_json(app: axum::Router, request: Request<Body>) -> Value {
    let response = app.oneshot(request).await.expect("request runs");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 8192)
        .await
        .expect("body reads");
    serde_json::from_slice(&bytes).expect("json parses")
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

fn session_only_body(session_id: &str) -> String {
    json!({
        "schema_version": 1,
        "session_id": session_id
    })
    .to_string()
}

fn stop_session_body(session_id: &str) -> String {
    json!({
        "schema_version": 1,
        "session_id": session_id,
        "reason": "operator_stop"
    })
    .to_string()
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
