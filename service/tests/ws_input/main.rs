use axum::{
    body::Body,
    http::{
        Method, Request,
        header::{ORIGIN, SET_COOKIE},
    },
};
use futures_util::{SinkExt, StreamExt};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::ALLOWED_ORIGIN,
    backend::{
        BackendCapabilities, BackendMode, BackendResult, BackendSession, BridgeBackend, CaptureJob,
        CaptureRequest, FramePreview, InputScheduleReceipt, InputScheduleRequest, RunBoundary,
        RunStatus, SessionId, SessionState, StartBackendSession, StopReason, StoppedSession,
    },
    config::ServiceConfig,
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, time::timeout};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const SESSION_ID: &str = "synthetic-session-scaffold";
const RUN_ID: &str = "synthetic-run-scaffold";

#[tokio::test]
async fn duplicate_client_seq_returns_original_ack_and_applies_once() {
    let (_workspace, app, backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let first = send_input(&mut ws, input_message(1, "keyboard", &["A"])).await;
    let duplicate_changed_payload = send_input(&mut ws, input_message(1, "keyboard", &["B"])).await;

    assert_eq!(first, duplicate_changed_payload);
    assert_eq!(first["type"], "input_ack");
    assert_eq!(first["client_seq"], 1);
    assert_eq!(first["payload"]["status"], "applied");
    assert_eq!(first["payload"]["assigned_frame"], 1);
    assert_eq!(first["payload"]["pad_word"], 1);
    assert_eq!(backend.injected_requests().len(), 1);
}

#[tokio::test]
async fn client_seq_must_be_monotonic_per_source() {
    let (_workspace, app, backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let first_keyboard = send_input(&mut ws, input_message(1, "keyboard", &["A"])).await;
    let gamepad_same_seq = send_input(&mut ws, input_message(1, "gamepad", &["B"])).await;
    let later_keyboard = send_input(&mut ws, input_message(3, "keyboard", &["X"])).await;
    let stale_keyboard = send_input(&mut ws, input_message(2, "keyboard", &["Y"])).await;

    assert_eq!(first_keyboard["type"], "input_ack");
    assert_eq!(gamepad_same_seq["type"], "input_ack");
    assert_eq!(later_keyboard["type"], "input_ack");
    assert_eq!(stale_keyboard["type"], "input_reject");
    assert_eq!(stale_keyboard["payload"]["error"]["code"], "bad_request");
    assert_eq!(backend.injected_requests().len(), 3);
}

#[tokio::test]
async fn queue_overflow_returns_sanitized_input_reject() {
    let (_workspace, app, backend) = ws_app(SessionState::Paused);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    for client_seq in 0..120 {
        let queued = send_input(&mut ws, input_message(client_seq, "keyboard", &["A"])).await;
        assert_eq!(queued["type"], "input_ack");
        assert_eq!(queued["payload"]["status"], "queued");
    }

    let overflow = send_input(&mut ws, input_message(121, "keyboard", &["B"])).await;

    assert_eq!(overflow["type"], "input_reject");
    assert_eq!(overflow["client_seq"], 121);
    assert_eq!(overflow["payload"]["schema_version"], 1);
    assert_eq!(overflow["payload"]["error"]["code"], "bad_request");
    assert_eq!(overflow["payload"]["error"]["message"], "Input rejected.");
    assert_eq!(overflow["payload"]["error"]["details"], json!({}));
    assert!(!overflow.to_string().contains(GOOD_CREDENTIAL));
    assert!(backend.injected_requests().is_empty());
}

#[tokio::test]
async fn schema_version_mismatch_returns_input_reject() {
    let (_workspace, app, _backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;
    let mut message = input_message(1, "keyboard", &["A"]);
    message["schema_version"] = json!(2);

    let reject = send_input(&mut ws, message).await;

    assert_eq!(reject["type"], "input_reject");
    assert_eq!(reject["payload"]["error"]["code"], "bad_request");
    assert_eq!(reject["payload"]["error"]["message"], "Input rejected.");
}

#[tokio::test]
async fn schema_invalid_but_replyable_messages_return_input_reject() {
    let (_workspace, app, backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let mut missing_server_seq = input_message(1, "keyboard", &["A"]);
    missing_server_seq
        .as_object_mut()
        .expect("input message is object")
        .remove("server_seq");
    let missing_server_seq_reject = send_input(&mut ws, missing_server_seq).await;
    assert_eq!(missing_server_seq_reject["type"], "input_reject");

    let mut unknown_top_level = input_message(2, "keyboard", &["A"]);
    unknown_top_level["private_path"] = json!("/home/private/rom");
    let unknown_top_level_reject = send_input(&mut ws, unknown_top_level).await;
    assert_eq!(unknown_top_level_reject["type"], "input_reject");

    let mut unknown_payload = input_message(3, "keyboard", &["A"]);
    unknown_payload["payload"]["debug"] = json!("stderr: should not leak");
    let unknown_payload_reject = send_input(&mut ws, unknown_payload).await;
    assert_eq!(unknown_payload_reject["type"], "input_reject");

    let mut invalid_uuid = input_message(4, "keyboard", &["A"]);
    invalid_uuid["payload"]["client_event_id"] = json!("not-a-uuid");
    let invalid_uuid_reject = send_input(&mut ws, invalid_uuid).await;
    assert_eq!(invalid_uuid_reject["type"], "input_reject");

    assert!(backend.injected_requests().is_empty());
}

#[tokio::test]
async fn invalid_echo_fields_are_not_scheduled_or_echoed() {
    let (_workspace, app, backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let mut invalid_session_id = input_message(1, "keyboard", &["A"]);
    invalid_session_id["session_id"] = json!("bad/session");
    send_input_expect_no_reply(&mut ws, invalid_session_id).await;

    let mut too_large_client_seq = input_message(1, "keyboard", &["A"]);
    too_large_client_seq["client_seq"] = json!(9_007_199_254_740_992_u64);
    send_input_expect_no_reply(&mut ws, too_large_client_seq).await;

    assert!(backend.injected_requests().is_empty());
}

#[tokio::test]
async fn invalid_buttons_return_sanitized_input_reject() {
    let (_workspace, app, backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let reject = send_input(
        &mut ws,
        json!({
            "schema_version": 1,
            "type": "input_state",
            "session_id": SESSION_ID,
            "client_seq": 1,
            "source_id": "keyboard",
            "server_seq": null,
            "payload": {
                "client_event_id": "00000000-0000-0000-0000-000000000001",
                "client_time_ms": 1,
                "source": "keyboard",
                "buttons": ["A", "NotAButton"]
            }
        }),
    )
    .await;

    assert_eq!(reject["type"], "input_reject");
    assert_eq!(reject["payload"]["error"]["code"], "bad_request");
    assert_eq!(reject["payload"]["error"]["message"], "Input rejected.");
    assert_eq!(reject["payload"]["error"]["details"], json!({}));
    assert!(backend.injected_requests().is_empty());
}

#[tokio::test]
async fn reconnect_zero_input_is_acknowledged() {
    let (_workspace, app, backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;

    {
        let mut ws = server.connect(&cookie).await;
        let pressed = send_input(&mut ws, input_message(1, "keyboard", &["A"])).await;
        assert_eq!(pressed["type"], "input_ack");
    }

    let mut reconnected = server.connect(&cookie).await;
    let zero = send_input(&mut reconnected, input_message(2, "keyboard", &[])).await;

    assert_eq!(zero["type"], "input_ack");
    assert_eq!(zero["payload"]["status"], "applied");
    assert_eq!(zero["payload"]["pad_word"], 0);
    assert_eq!(backend.injected_requests().len(), 2);
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

    async fn connect(
        &self,
        cookie: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let mut request = format!("ws://{}/ws/input", self.addr)
            .into_client_request()
            .expect("websocket request builds");
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_static(ALLOWED_ORIGIN));
        request.headers_mut().insert(
            "Cookie",
            HeaderValue::from_str(cookie).expect("cookie header parses"),
        );

        connect_async(request).await.expect("websocket connects").0
    }
}

impl Drop for WsServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn send_input(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    message: Value,
) -> Value {
    ws.send(Message::Text(message.to_string().into()))
        .await
        .expect("websocket send succeeds");
    let response = ws
        .next()
        .await
        .expect("response is available")
        .expect("response succeeds");
    let Message::Text(text) = response else {
        panic!("expected text websocket response");
    };

    let json: Value = serde_json::from_str(&text).expect("response json parses");
    assert_matches_runtime_schema(&json);
    json
}

async fn send_input_expect_no_reply(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    message: Value,
) {
    ws.send(Message::Text(message.to_string().into()))
        .await
        .expect("websocket send succeeds");
    let response = timeout(Duration::from_millis(100), ws.next()).await;
    assert!(response.is_err(), "invalid echo fields must not be echoed");
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

fn input_message(client_seq: u64, source_id: &str, buttons: &[&str]) -> Value {
    json!({
        "schema_version": 1,
        "type": "input_state",
        "session_id": SESSION_ID,
        "client_seq": client_seq,
        "source_id": source_id,
        "server_seq": null,
        "payload": {
            "client_event_id": format!("00000000-0000-0000-0000-{client_seq:012}"),
            "client_time_ms": client_seq,
            "source": "keyboard",
            "buttons": buttons
        }
    })
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
                        "requested_capabilities": ["input"]
                    })
                    .to_string(),
                ))
                .expect("login request builds"),
        )
        .await
        .expect("login request runs");
    assert_eq!(response.status(), 200);

    response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("session cookie pair exists")
        .to_string()
}

fn ws_app(state: SessionState) -> (tempfile::TempDir, axum::Router, Arc<RecordingBackend>) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let backend = Arc::new(RecordingBackend::new(state));
    let app = router(AppState::for_tests_with_backend(
        config(&private_root),
        rom_operator_bridge_service::auth::AuthState::new(),
        backend.clone(),
    ));
    (workspace, app, backend)
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
struct RecordingBackend {
    state: SessionState,
    injected: Mutex<Vec<InputScheduleRequest>>,
}

impl RecordingBackend {
    fn new(state: SessionState) -> Self {
        Self {
            state,
            injected: Mutex::new(Vec::new()),
        }
    }

    fn injected_requests(&self) -> Vec<InputScheduleRequest> {
        self.injected
            .lock()
            .expect("injected mutex poisoned")
            .clone()
    }
}

impl BridgeBackend for RecordingBackend {
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
            state: self.state,
            current_frame: 0,
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
            final_frame: 0,
        })
    }

    fn status(&self, session_id: SessionId) -> BackendResult<RunStatus> {
        Ok(RunStatus {
            session_id,
            run_id: RUN_ID.to_string(),
            state: self.state,
            backend_mode: self.mode(),
            current_frame: 0,
            capabilities: self.capabilities(),
        })
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Paused,
            current_frame: 0,
        })
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Running,
            current_frame: 0,
        })
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        self.injected
            .lock()
            .expect("injected mutex poisoned")
            .push(request.clone());
        Ok(InputScheduleReceipt {
            session_id: request.session_id,
            assigned_frame: request.target_frame,
            pad_word: request.pad_word,
        })
    }

    fn framebuffer(&self, session_id: SessionId) -> BackendResult<FramePreview> {
        Ok(FramePreview {
            session_id,
            frame: 0,
            width: 1,
            height: 1,
            png_bytes: Vec::new(),
        })
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id: "synthetic-capture-job-scaffold".to_string(),
            status: rom_operator_bridge_service::backend::CaptureJobStatus::Pending,
            capture_id: None,
        })
    }

    fn capture_job(&self, job_id: String) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id,
            status: rom_operator_bridge_service::backend::CaptureJobStatus::Pending,
            capture_id: None,
        })
    }
}
