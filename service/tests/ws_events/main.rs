use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request,
        header::{COOKIE, ORIGIN, SET_COOKIE},
    },
};
use futures_util::StreamExt;
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
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
    sanitization::PublicSanitizer,
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderName, HeaderValue},
    },
};
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const SESSION_ID: &str = "synthetic-session-events";
const RUN_ID: &str = "synthetic-run-events";
const CAPTURE_JOB_ID: &str = "capture-job-events";

type TestSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::test]
async fn authenticated_event_connection_emits_ordered_sanitized_snapshot() {
    let (_workspace, app, private_root) = ws_app(EventBackend::new(
        SESSION_ID,
        SessionState::Running,
        Some(CAPTURE_JOB_ID.to_string()),
    ));
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let messages = read_events(&mut ws, 5).await;

    assert_eq!(
        event_types(&messages),
        [
            "session_updated",
            "run_updated",
            "capture_updated",
            "label_updated",
            "validation_updated",
        ]
    );
    assert_sanitized_ordered_events(&messages, &private_root);
    assert_eq!(messages[0]["payload"]["state"], "running");
    assert_eq!(messages[0]["payload"]["backend_mode"], "synthetic");
    assert_eq!(messages[1]["payload"]["preview_stale"], true);
    assert_eq!(
        messages[1]["payload"]["active_capture_job_id"],
        CAPTURE_JOB_ID
    );
    assert_eq!(messages[2]["payload"]["job_id"], CAPTURE_JOB_ID);
    assert_eq!(messages[2]["payload"]["status"], "capturing");
    assert_eq!(messages[2]["payload"]["capture_id"], Value::Null);
    assert_eq!(messages[3]["payload"]["label_revision"], 0);
    assert_eq!(messages[3]["payload"]["applied"], false);
    assert_eq!(messages[4]["payload"]["status"], "not_run");
    assert_eq!(messages[4]["payload"]["command_class"], Value::Null);
    assert_eq!(messages[4]["payload"]["started_at"], Value::Null);
    assert_eq!(messages[4]["payload"]["completed_at"], Value::Null);
    assert_eq!(messages[4]["payload"]["summary"], "");
    assert_eq!(messages[4]["payload"]["issue_summaries"], json!([]));

    let last_seq = messages
        .last()
        .and_then(|message| message["server_seq"].as_u64())
        .expect("last server_seq exists");
    let mut reconnect = server.connect(&cookie).await;
    let reconnect_first = read_events(&mut reconnect, 1).await;
    assert!(reconnect_first[0]["server_seq"].as_u64().unwrap() > last_seq);
}

#[tokio::test]
async fn event_websocket_handshake_includes_runtime_security_headers() {
    let (_workspace, app, _private_root) = ws_app(EventBackend::new(
        SESSION_ID,
        SessionState::Running,
        Some(CAPTURE_JOB_ID.to_string()),
    ));
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;

    let mut request = format!("ws://{}/ws/events", server.addr)
        .into_client_request()
        .expect("websocket request builds");
    request
        .headers_mut()
        .insert("Origin", HeaderValue::from_static(ALLOWED_ORIGIN));
    request.headers_mut().insert(
        HeaderName::from_static("cookie"),
        HeaderValue::from_str(&cookie).expect("cookie header parses"),
    );

    let (_socket, response) = connect_async(request).await.expect("websocket connects");
    let headers = response.headers();

    assert_eq!(
        headers
            .get(HeaderName::from_static("cache-control"))
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        headers
            .get(HeaderName::from_static("pragma"))
            .and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(
        headers
            .get(HeaderName::from_static("x-content-type-options"))
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        headers
            .get(HeaderName::from_static("access-control-allow-origin"))
            .and_then(|value| value.to_str().ok()),
        Some(ALLOWED_ORIGIN)
    );
    assert_eq!(
        headers
            .get(HeaderName::from_static("vary"))
            .and_then(|value| value.to_str().ok()),
        Some("Origin")
    );
}

#[tokio::test]
async fn event_connection_requires_authenticated_browser_origin() {
    let (_workspace, app, _private_root) = ws_app(EventBackend::new(
        SESSION_ID,
        SessionState::Running,
        Some(CAPTURE_JOB_ID.to_string()),
    ));
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;

    assert!(
        server
            .try_connect(None, Some(ALLOWED_ORIGIN))
            .await
            .is_err()
    );
    assert!(server.try_connect(Some(&cookie), None).await.is_err());
}

#[tokio::test]
async fn event_connection_rejects_backend_session_mismatch() {
    let (_workspace, app, _private_root) = ws_app(EventBackend::new(
        "different-session",
        SessionState::Running,
        None,
    ));
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;

    assert!(
        server
            .try_connect(Some(&cookie), Some(ALLOWED_ORIGIN))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn event_snapshot_omits_capture_event_when_no_capture_is_active() {
    let (_workspace, app, private_root) =
        ws_app(EventBackend::new(SESSION_ID, SessionState::Paused, None));
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let messages = read_events(&mut ws, 4).await;

    assert_eq!(
        event_types(&messages),
        [
            "session_updated",
            "run_updated",
            "label_updated",
            "validation_updated",
        ]
    );
    assert_sanitized_ordered_events(&messages, &private_root);
    assert_eq!(messages[0]["payload"]["state"], "paused");
    assert_eq!(messages[1]["payload"]["active_capture_job_id"], Value::Null);
}

#[tokio::test]
async fn event_stream_publishes_live_run_updates_after_pause() {
    let (_workspace, app, private_root) =
        ws_app(EventBackend::new(SESSION_ID, SessionState::Running, None));
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app.clone()).await;
    let mut ws = server.connect(&cookie).await;

    let snapshot = read_events(&mut ws, 4).await;
    assert_sanitized_ordered_events(&snapshot, &private_root);
    let last_snapshot_seq = snapshot
        .last()
        .and_then(|message| message["server_seq"].as_u64())
        .expect("snapshot server_seq exists");

    let pause = app
        .oneshot(runtime_json_request(
            Method::POST,
            "/api/run/pause",
            &cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID
            }),
        ))
        .await
        .expect("pause request runs");
    assert_eq!(pause.status(), 200);

    let updates = read_events(&mut ws, 2).await;

    assert_eq!(event_types(&updates), ["session_updated", "run_updated"]);
    assert_sanitized_ordered_events(&updates, &private_root);
    assert!(updates[0]["server_seq"].as_u64().unwrap() > last_snapshot_seq);
    assert_eq!(updates[0]["payload"]["state"], "paused");
    assert_eq!(updates[1]["payload"]["state"], "paused");
    assert_eq!(updates[1]["payload"]["preview_stale"], true);
}

struct WsServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl WsServer {
    async fn start(app: axum::Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let addr = listener.local_addr().expect("listener addr is available");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server runs");
        });

        Self { addr, handle }
    }

    async fn connect(&self, cookie: &str) -> TestSocket {
        self.try_connect(Some(cookie), Some(ALLOWED_ORIGIN))
            .await
            .expect("websocket connects")
    }

    async fn try_connect(
        &self,
        cookie: Option<&str>,
        origin: Option<&'static str>,
    ) -> Result<TestSocket, tokio_tungstenite::tungstenite::Error> {
        let mut request = format!("ws://{}/ws/events", self.addr)
            .into_client_request()
            .expect("websocket request builds");
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", HeaderValue::from_static(origin));
        }
        if let Some(cookie) = cookie {
            request.headers_mut().insert(
                HeaderName::from_static("cookie"),
                HeaderValue::from_str(cookie).expect("cookie header parses"),
            );
        }

        connect_async(request).await.map(|(socket, _)| socket)
    }
}

impl Drop for WsServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn read_events(ws: &mut TestSocket, count: usize) -> Vec<Value> {
    let mut messages = Vec::with_capacity(count);
    for _ in 0..count {
        let message = ws.next();
        let message = timeout(Duration::from_secs(2), message)
            .await
            .expect("event message is available before timeout")
            .expect("event message is available")
            .expect("event message succeeds");
        let Message::Text(text) = message else {
            panic!("expected text websocket response");
        };
        let json: Value = serde_json::from_str(&text).expect("event json parses");
        assert_matches_runtime_schema(&json);
        messages.push(json);
    }
    messages
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

fn assert_sanitized_ordered_events(messages: &[Value], private_root: &Path) {
    let sanitizer = PublicSanitizer::new()
        .with_private_root(private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET);
    let mut last_seq = 0;

    for message in messages {
        assert_eq!(message["schema_version"], 1);
        assert_eq!(message["session_id"], SESSION_ID);
        assert_eq!(message["client_seq"], Value::Null);
        assert_eq!(message["source_id"], "server");
        let server_seq = message["server_seq"]
            .as_u64()
            .expect("server_seq is numeric");
        assert!(
            server_seq > last_seq,
            "server_seq must increase: {server_seq} <= {last_seq}"
        );
        last_seq = server_seq;
        sanitizer
            .inspect_event(message)
            .expect("event is public-safe");
        let serialized = message.to_string();
        assert!(!serialized.contains(GOOD_CREDENTIAL));
        assert!(!serialized.contains(SESSION_SECRET));
        assert!(!serialized.contains(&private_root.display().to_string()));
        assert!(!serialized.contains("raw_payload"));
        assert!(!serialized.contains("private_path"));
    }
}

fn event_types(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message["type"].as_str().expect("event type is string"))
        .collect()
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
                        "requested_capabilities": ["input", "preview", "capture"]
                    })
                    .to_string(),
                ))
                .expect("login request builds"),
        )
        .await
        .expect("login request runs");
    assert_eq!(response.status(), 200);

    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("session cookie pair exists")
        .to_string();
    let _body = to_bytes(response.into_body(), 8192)
        .await
        .expect("login body reads");
    cookie
}

fn runtime_json_request(method: Method, uri: &str, cookie: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
        .header(COOKIE, cookie)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("runtime request builds")
}

fn ws_app(backend: EventBackend) -> (tempfile::TempDir, axum::Router, PathBuf) {
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

#[derive(Debug)]
struct EventBackend {
    status_session_id: String,
    state: Mutex<SessionState>,
    active_capture_job_id: Option<String>,
}

impl EventBackend {
    fn new(
        status_session_id: impl Into<String>,
        state: SessionState,
        active_capture_job_id: Option<String>,
    ) -> Self {
        Self {
            status_session_id: status_session_id.into(),
            state: Mutex::new(state),
            active_capture_job_id,
        }
    }

    fn state(&self) -> SessionState {
        *self.state.lock().expect("state mutex poisoned")
    }
}

impl BridgeBackend for EventBackend {
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
            current_frame: 18,
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
            final_frame: 18,
        })
    }

    fn status(&self, _session_id: SessionId) -> BackendResult<RunStatus> {
        Ok(RunStatus {
            session_id: self.status_session_id.clone(),
            run_id: RUN_ID.to_string(),
            state: self.state(),
            backend_mode: self.mode(),
            current_frame: 18,
            capabilities: self.capabilities(),
            last_applied_input_frame: 12,
            last_preview_frame: 17,
            active_capture_job_id: self.active_capture_job_id.clone(),
        })
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        *self.state.lock().expect("state mutex poisoned") = SessionState::Paused;
        Ok(RunBoundary {
            session_id,
            state: SessionState::Paused,
            current_frame: 18,
        })
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        *self.state.lock().expect("state mutex poisoned") = SessionState::Running;
        Ok(RunBoundary {
            session_id,
            state: SessionState::Running,
            current_frame: 18,
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
            frame: 18,
            width: 1,
            height: 1,
            png_bytes: Vec::new(),
        })
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id: CAPTURE_JOB_ID.to_string(),
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
