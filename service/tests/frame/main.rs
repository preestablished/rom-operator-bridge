use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, ORIGIN, SET_COOKIE},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::ALLOWED_ORIGIN,
    backend::{
        BackendCapabilities, BackendMode, BackendResult, BackendSession, BridgeBackend, CaptureJob,
        CaptureJobStatus, CaptureRequest, FramePreview, InputScheduleReceipt, InputScheduleRequest,
        RunBoundary, RunStatus, SessionId, SessionState, StartBackendSession, StopReason,
        StoppedSession,
    },
    config::ServiceConfig,
    framebuffer::{SYNTHETIC_FRAME_HEIGHT, SYNTHETIC_FRAME_WIDTH, synthetic_frame_png},
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
    sanitization::PublicSanitizer,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const SESSION_ID: &str = "synthetic-session-frame";
const RUN_ID: &str = "synthetic-run-frame";

#[tokio::test]
async fn current_frame_metadata_and_image_are_schema_safe_and_no_store() {
    let (_workspace, app, private_root) = synthetic_frame_app();
    let cookie = login_cookie(app.clone()).await;

    let response = app
        .clone()
        .oneshot(runtime_get("/api/frame/current", &cookie))
        .await
        .expect("frame metadata request runs");
    assert_eq!(response.status(), StatusCode::OK);
    assert_no_store_headers(response.headers());

    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("metadata body reads");
    let metadata: Value = serde_json::from_slice(&body).expect("metadata json parses");
    assert_matches_runtime_schema(&metadata);
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&metadata)
        .expect("metadata is public-safe");

    assert_eq!(metadata["width"], SYNTHETIC_FRAME_WIDTH);
    assert_eq!(metadata["height"], SYNTHETIC_FRAME_HEIGHT);
    assert_eq!(metadata["format"], "image/png");
    assert_eq!(metadata["stale"], false);
    assert_eq!(metadata["captured_at"], "1970-01-01T00:00:00Z");

    let image_url = metadata["image_url"]
        .as_str()
        .expect("image_url is a string");
    assert!(image_url.starts_with("/api/frame/current/image?frame="));
    let image = app
        .oneshot(runtime_get(image_url, &cookie))
        .await
        .expect("frame image request runs");
    assert_eq!(image.status(), StatusCode::OK);
    assert_no_store_headers(image.headers());
    assert_eq!(
        image
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );

    let bytes = to_bytes(image.into_body(), 512 * 1024)
        .await
        .expect("image body reads");
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(metadata["preview_hash"], sha256_ref(&bytes));
    let image_text = String::from_utf8_lossy(&bytes);
    assert!(!image_text.contains(GOOD_CREDENTIAL));
    assert!(!image_text.contains(SESSION_SECRET));
    assert!(!image_text.contains(&private_root.display().to_string()));
}

#[tokio::test]
async fn frame_metadata_marks_preview_stale_when_backend_frame_lags() {
    let (_workspace, app, _private_root) = frame_app(FrameBackend::new(10, 9));
    let cookie = login_cookie(app.clone()).await;

    let response = app
        .oneshot(runtime_get("/api/frame/current", &cookie))
        .await
        .expect("frame metadata request runs");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("metadata body reads");
    let metadata: Value = serde_json::from_slice(&body).expect("metadata json parses");

    assert_matches_runtime_schema(&metadata);
    assert_eq!(metadata["frame"], 9);
    assert_eq!(metadata["stale"], true);
}

#[tokio::test]
async fn frame_image_allows_only_numeric_frame_query_hint() {
    let (_workspace, app, private_root) = synthetic_frame_app();
    let cookie = login_cookie(app.clone()).await;

    let response = app
        .oneshot(runtime_get(
            "/api/frame/current/image?next=operator-credential-from-test-source",
            &cookie,
        ))
        .await
        .expect("frame image request runs");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_auth_safe_error(response, &private_root).await;
}

fn assert_no_store_headers(headers: &axum::http::HeaderMap) {
    assert_eq!(
        headers
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        headers
            .get("x-content-type-options")
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
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

async fn assert_auth_safe_error(response: axum::response::Response, private_root: &Path) {
    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("error body reads");
    let json: Value = serde_json::from_slice(&body).expect("error json parses");
    assert_eq!(json["error"]["code"], "auth_rejected");
    PublicSanitizer::new()
        .with_private_root(private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&json)
        .expect("error is public-safe");
}

async fn login_cookie(app: axum::Router) -> String {
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/session/start")
                .header(ORIGIN, ALLOWED_ORIGIN)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "schema_version": 1,
                        "operator_credential": GOOD_CREDENTIAL,
                        "backend_mode": "synthetic",
                        "requested_capabilities": ["preview"]
                    })
                    .to_string(),
                ))
                .expect("login request builds"),
        )
        .await
        .expect("login request runs");
    assert_eq!(response.status(), StatusCode::OK);
    response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("session cookie pair exists")
        .to_string()
}

fn runtime_get(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
        .header(COOKIE, cookie)
        .body(Body::empty())
        .expect("runtime request builds")
}

fn synthetic_frame_app() -> (tempfile::TempDir, axum::Router, PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(config(&private_root)));
    (workspace, app, private_root)
}

fn frame_app(backend: FrameBackend) -> (tempfile::TempDir, axum::Router, PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::for_tests_with_backend(
        config(&private_root),
        rom_operator_bridge_service::auth::AuthState::new(),
        std::sync::Arc::new(backend),
    ));
    (workspace, app, private_root)
}

fn config(private_root: &Path) -> ServiceConfig {
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

fn sha256_ref(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::from("sha256:");
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[derive(Debug)]
struct FrameBackend {
    current_frame: u64,
    preview_frame: u64,
    state: Mutex<SessionState>,
}

impl FrameBackend {
    fn new(current_frame: u64, preview_frame: u64) -> Self {
        Self {
            current_frame,
            preview_frame,
            state: Mutex::new(SessionState::Running),
        }
    }

    fn state(&self) -> SessionState {
        *self.state.lock().expect("state mutex poisoned")
    }
}

impl BridgeBackend for FrameBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Synthetic
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::synthetic_mvp()
    }

    fn start_session(&self, _request: StartBackendSession) -> BackendResult<BackendSession> {
        Ok(BackendSession {
            session_id: SESSION_ID.to_string(),
            run_id: RUN_ID.to_string(),
            state: self.state(),
            current_frame: self.current_frame,
            capabilities: self.capabilities(),
        })
    }

    fn stop_session(
        &self,
        session_id: SessionId,
        _reason: StopReason,
    ) -> BackendResult<StoppedSession> {
        Ok(StoppedSession {
            session_id,
            state: SessionState::Stopped,
            final_frame: self.current_frame,
        })
    }

    fn status(&self, session_id: SessionId) -> BackendResult<RunStatus> {
        Ok(RunStatus {
            session_id,
            run_id: RUN_ID.to_string(),
            state: self.state(),
            backend_mode: self.mode(),
            current_frame: self.current_frame,
            capabilities: self.capabilities(),
            last_applied_input_frame: 0,
            last_preview_frame: self.preview_frame,
            active_capture_job_id: None,
        })
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Paused,
            current_frame: self.current_frame,
        })
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Running,
            current_frame: self.current_frame,
        })
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        Ok(InputScheduleReceipt {
            session_id: request.session_id,
            assigned_frame: request.target_frame,
            pad_word: request.pad_word,
        })
    }

    fn framebuffer(&self, session_id: SessionId) -> BackendResult<FramePreview> {
        Ok(FramePreview {
            session_id,
            frame: self.preview_frame,
            width: SYNTHETIC_FRAME_WIDTH,
            height: SYNTHETIC_FRAME_HEIGHT,
            png_bytes: synthetic_frame_png(self.preview_frame),
        })
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id: "frame-capture-job".to_string(),
            status: CaptureJobStatus::Running,
            capture_id: None,
        })
    }

    fn capture_job(&self, job_id: String) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id,
            status: CaptureJobStatus::Running,
            capture_id: None,
        })
    }
}
