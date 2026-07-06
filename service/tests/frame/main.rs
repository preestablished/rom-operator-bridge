use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE, COOKIE, ORIGIN, PRAGMA,
            SET_COOKIE, VARY,
        },
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::ALLOWED_ORIGIN,
    backend::{
        BackendCapabilities, BackendError, BackendMode, BackendResult, BackendSession,
        BridgeBackend, CaptureJob, CaptureJobStatus, CaptureRequest, FramePreview,
        InputScheduleReceipt, InputScheduleRequest, PlayStepOutcome, RunBoundary, RunStatus,
        SessionId, SessionState, StartBackendSession, StopReason, StoppedSession,
    },
    config::ServiceConfig,
    framebuffer::{SYNTHETIC_FRAME_HEIGHT, SYNTHETIC_FRAME_WIDTH, synthetic_frame_png},
    private_config::{ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
    sanitization::PublicSanitizer,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tower::ServiceExt;

const SECRET_LITERAL: &str = "private-secret-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const SESSION_ID: &str = "synthetic-session-frame";
const RUN_ID: &str = "synthetic-run-frame";
const JSON_SAFE_U64_MAX: u64 = 9_007_199_254_740_991;

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
        .with_forbidden_literal(SECRET_LITERAL)
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
    assert!(!image_text.contains(SECRET_LITERAL));
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
async fn frame_image_serves_the_preview_advertised_by_metadata() {
    let (_workspace, app, _private_root) =
        frame_app(FrameBackend::new(10, 9).with_preview_frames([9, 10]));
    let cookie = login_cookie(app.clone()).await;

    let metadata_response = app
        .clone()
        .oneshot(runtime_get("/api/frame/current", &cookie))
        .await
        .expect("frame metadata request runs");
    assert_eq!(metadata_response.status(), StatusCode::OK);
    let metadata_body = to_bytes(metadata_response.into_body(), 8192)
        .await
        .expect("metadata body reads");
    let metadata: Value = serde_json::from_slice(&metadata_body).expect("metadata json parses");
    let image_url = metadata["image_url"]
        .as_str()
        .expect("image_url is a string");

    let image_response = app
        .oneshot(runtime_get(image_url, &cookie))
        .await
        .expect("frame image request runs");
    assert_eq!(image_response.status(), StatusCode::OK);
    let bytes = to_bytes(image_response.into_body(), 512 * 1024)
        .await
        .expect("image body reads");

    assert_eq!(metadata["frame"], 9);
    assert_eq!(metadata["preview_hash"], sha256_ref(&bytes));
    assert_eq!(bytes.as_ref(), synthetic_frame_png(9).as_slice());
    assert_ne!(bytes.as_ref(), synthetic_frame_png(10).as_slice());
}

#[tokio::test]
async fn frame_metadata_reports_frame_unavailable_without_backend_outage_semantics() {
    let (_workspace, app, _private_root) =
        frame_app(FrameBackend::new(10, 9).with_frame_unavailable());
    let cookie = login_cookie(app.clone()).await;

    let response = app
        .oneshot(runtime_get("/api/frame/current", &cookie))
        .await
        .expect("frame metadata request runs");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("error body reads");
    let envelope: Value = serde_json::from_slice(&body).expect("error json parses");

    assert_matches_runtime_schema(&envelope);
    assert_eq!(envelope["error"]["code"], "frame_unavailable");
    assert_eq!(envelope["error"]["retryable"], true);
}

#[tokio::test]
async fn frame_routes_reject_backend_session_mismatches() {
    let (_workspace, status_app, status_private_root) =
        frame_app(FrameBackend::new(10, 10).with_status_session_id("other-session"));
    let status_cookie = login_cookie(status_app.clone()).await;

    let status_mismatch = status_app
        .oneshot(runtime_get("/api/frame/current", &status_cookie))
        .await
        .expect("frame metadata request runs");
    assert_eq!(status_mismatch.status(), StatusCode::UNAUTHORIZED);
    assert_session_inactive_safe_error(status_mismatch, &status_private_root).await;

    let (_workspace, preview_app, preview_private_root) =
        frame_app(FrameBackend::new(10, 10).with_preview_session_id("other-session"));
    let preview_cookie = login_cookie(preview_app.clone()).await;

    let metadata_preview_mismatch = preview_app
        .clone()
        .oneshot(runtime_get("/api/frame/current", &preview_cookie))
        .await
        .expect("frame metadata request runs");
    assert_eq!(metadata_preview_mismatch.status(), StatusCode::UNAUTHORIZED);
    assert_session_inactive_safe_error(metadata_preview_mismatch, &preview_private_root).await;

    let image_preview_mismatch = preview_app
        .oneshot(runtime_get(
            "/api/frame/current/image?frame=10",
            &preview_cookie,
        ))
        .await
        .expect("frame image request runs");
    assert_eq!(image_preview_mismatch.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(
        image_preview_mismatch
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    assert_session_inactive_safe_error(image_preview_mismatch, &preview_private_root).await;
}

#[tokio::test]
async fn frame_metadata_rejects_schema_unsafe_frame_counters() {
    let (_workspace, app, _private_root) = frame_app(FrameBackend::new(
        JSON_SAFE_U64_MAX + 1,
        JSON_SAFE_U64_MAX + 1,
    ));
    let cookie = login_cookie(app.clone()).await;

    let response = app
        .oneshot(runtime_get("/api/frame/current", &cookie))
        .await
        .expect("frame metadata request runs");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn frame_image_allows_only_numeric_frame_query_hint() {
    let (_workspace, app, private_root) = synthetic_frame_app();
    let cookie = login_cookie(app.clone()).await;

    for uri in [
        "/api/frame/current/image?next=private-secret-from-test-source",
        "/api/frame/current/image?frame=",
        "/api/frame/current/image?frame=1&next=2",
    ] {
        let response = app
            .clone()
            .oneshot(runtime_get(uri, &cookie))
            .await
            .expect("frame image request runs");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_auth_safe_error(response, &private_root).await;
    }
}

fn assert_no_store_headers(headers: &axum::http::HeaderMap) {
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
            .get("x-content-type-options")
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        headers
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some(ALLOWED_ORIGIN)
    );
    assert_eq!(
        headers.get(VARY).and_then(|value| value.to_str().ok()),
        Some("Origin")
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
        .with_forbidden_literal(SECRET_LITERAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&json)
        .expect("error is public-safe");
}

async fn assert_session_inactive_safe_error(
    response: axum::response::Response,
    private_root: &Path,
) {
    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("error body reads");
    let json: Value = serde_json::from_slice(&body).expect("error json parses");
    assert_eq!(json["error"]["code"], "session_inactive");
    PublicSanitizer::new()
        .with_private_root(private_root)
        .with_forbidden_literal(SECRET_LITERAL)
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
    preview_frames: Mutex<VecDeque<u64>>,
    status_session_id: String,
    preview_session_id: String,
    state: Mutex<SessionState>,
    frame_unavailable: bool,
}

impl FrameBackend {
    fn new(current_frame: u64, preview_frame: u64) -> Self {
        Self {
            current_frame,
            preview_frames: Mutex::new(VecDeque::from([preview_frame])),
            status_session_id: SESSION_ID.to_string(),
            preview_session_id: SESSION_ID.to_string(),
            state: Mutex::new(SessionState::Running),
            frame_unavailable: false,
        }
    }

    fn with_frame_unavailable(mut self) -> Self {
        self.frame_unavailable = true;
        self
    }

    fn with_status_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.status_session_id = session_id.into();
        self
    }

    fn with_preview_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.preview_session_id = session_id.into();
        self
    }

    fn with_preview_frames<const N: usize>(self, frames: [u64; N]) -> Self {
        *self.preview_frames.lock().expect("preview mutex poisoned") = VecDeque::from(frames);
        self
    }

    fn state(&self) -> SessionState {
        *self.state.lock().expect("state mutex poisoned")
    }

    fn preview_frame(&self) -> u64 {
        let mut frames = self.preview_frames.lock().expect("preview mutex poisoned");
        if frames.len() > 1 {
            frames.pop_front().expect("preview frame exists")
        } else {
            *frames.front().expect("preview frame exists")
        }
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

    fn status(&self, _session_id: SessionId) -> BackendResult<RunStatus> {
        let last_preview_frame = *self
            .preview_frames
            .lock()
            .expect("preview mutex poisoned")
            .front()
            .expect("preview frame exists");
        Ok(RunStatus {
            session_id: self.status_session_id.clone(),
            run_id: RUN_ID.to_string(),
            state: self.state(),
            backend_mode: self.mode(),
            current_frame: self.current_frame,
            capabilities: self.capabilities(),
            last_applied_input_frame: 0,
            last_preview_frame,
            preview_stale: last_preview_frame < self.current_frame,
            active_capture_job_id: None,
        })
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Paused,
            current_frame: self.current_frame,
            preview_stale: true,
        })
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Running,
            current_frame: self.current_frame,
            preview_stale: true,
        })
    }

    fn play_start(&self, _session_id: SessionId) -> BackendResult<RunBoundary> {
        unimplemented!("play mode not exercised by frame tests")
    }

    fn play_step(&self, _session_id: SessionId) -> BackendResult<PlayStepOutcome> {
        unimplemented!("play mode not exercised by frame tests")
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        Ok(InputScheduleReceipt {
            session_id: request.session_id,
            assigned_frame: request.target_frame,
            pad_word: request.pad_word,
        })
    }

    fn framebuffer(&self, _session_id: SessionId) -> BackendResult<FramePreview> {
        if self.frame_unavailable {
            return Err(BackendError::FrameUnavailable);
        }
        let frame = self.preview_frame();
        Ok(FramePreview {
            session_id: self.preview_session_id.clone(),
            frame,
            width: SYNTHETIC_FRAME_WIDTH,
            height: SYNTHETIC_FRAME_HEIGHT,
            png_bytes: synthetic_frame_png(frame),
        })
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id: "frame-capture-job".to_string(),
            status: CaptureJobStatus::Running,
            capture_id: None,
            public: None,
        })
    }

    fn capture_job(&self, job_id: String) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id,
            status: CaptureJobStatus::Running,
            capture_id: None,
            public: None,
        })
    }
}
