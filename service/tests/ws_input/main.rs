use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request,
        header::{COOKIE, ORIGIN, SET_COOKIE},
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
    input::{PAD_MASK, PadLog},
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
};
use serde_json::{Value, json};
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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
async fn input_websocket_handshake_includes_runtime_security_headers() {
    let (_workspace, app, _backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app).await;

    let mut request = format!("ws://{}/ws/input", server.addr)
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
    assert_runtime_security_headers(response.headers());
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
async fn resume_route_flushes_queued_paused_input() {
    let (_workspace, app, backend) = ws_app(SessionState::Running);
    let cookie = login_cookie(app.clone()).await;
    let pause = app
        .clone()
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

    let server = WsServer::start(app.clone()).await;
    let mut ws = server.connect(&cookie).await;
    let queued = send_input(&mut ws, input_message(1, "keyboard", &["A"])).await;
    assert_eq!(queued["type"], "input_ack");
    assert_eq!(queued["payload"]["status"], "queued");
    assert!(backend.injected_requests().is_empty());

    let resume = app
        .oneshot(runtime_json_request(
            Method::POST,
            "/api/run/resume",
            &cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID
            }),
        ))
        .await
        .expect("resume request runs");

    assert_eq!(resume.status(), 200);
    let injected = backend.injected_requests();
    assert_eq!(injected.len(), 1);
    assert_eq!(injected[0].session_id, SESSION_ID);
    assert_eq!(injected[0].pad_word.raw(), 1);
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

#[cfg(unix)]
#[tokio::test]
async fn synthetic_ws_input_writes_padlog_and_private_diagnostics() {
    let (_workspace, app, private_root) = synthetic_ws_app();
    let (cookie, session) = login_session(app.clone()).await;
    let session_id = session["session_id"]
        .as_str()
        .expect("session id is present");
    let run_id = session["run_id"].as_str().expect("run id is present");
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;
    let all_buttons = [
        "A", "B", "X", "Y", "L", "R", "Select", "Start", "Up", "Down", "Left", "Right",
    ];
    let expected_all_word = 0x0c3f_u16;
    let button_cases = [
        ("A", 0x0001_u16),
        ("B", 0x0002_u16),
        ("X", 0x0004_u16),
        ("Y", 0x0008_u16),
        ("L", 0x0010_u16),
        ("R", 0x0020_u16),
        ("Up", 0x0040_u16),
        ("Down", 0x0080_u16),
        ("Left", 0x0100_u16),
        ("Right", 0x0200_u16),
        ("Start", 0x0400_u16),
        ("Select", 0x0800_u16),
    ];

    let all_pressed = send_input(&mut ws, input_message(1, "keyboard", &all_buttons)).await;
    assert_eq!(all_pressed["type"], "input_ack");
    assert_eq!(all_pressed["session_id"], session_id);
    assert_eq!(all_pressed["payload"]["status"], "applied");
    assert_eq!(all_pressed["payload"]["pad_word"], expected_all_word);
    assert_ne!(expected_all_word, 0);
    assert_eq!(u64::from(expected_all_word) & u64::from(!PAD_MASK), 0);

    let duplicate_changed_payload = send_input(&mut ws, input_message(1, "keyboard", &["A"])).await;
    assert_eq!(duplicate_changed_payload, all_pressed);

    let mut expected_frames = vec![expected_all_word];
    for (offset, (button, expected_word)) in button_cases.into_iter().enumerate() {
        let ack = send_input(
            &mut ws,
            input_message(10 + offset as u64, "keyboard", &[button]),
        )
        .await;
        assert_eq!(ack["type"], "input_ack");
        assert_eq!(ack["payload"]["status"], "applied");
        assert_eq!(ack["payload"]["pad_word"], expected_word);
        assert_eq!(u64::from(expected_word) & u64::from(!PAD_MASK), 0);
        expected_frames.push(expected_word);
    }

    let mut zero_frames = Vec::new();
    for (client_seq, source_id) in [
        (100, "focus"),
        (101, "page-hidden"),
        (102, "reconnect"),
        (103, "gamepad-disconnect"),
    ] {
        let zero = send_input(&mut ws, input_message(client_seq, source_id, &[])).await;
        assert_eq!(zero["type"], "input_ack");
        assert_eq!(zero["source_id"], source_id);
        assert_eq!(zero["payload"]["status"], "applied");
        assert_eq!(zero["payload"]["pad_word"], 0);
        zero_frames.push(
            zero["payload"]["assigned_frame"]
                .as_u64()
                .expect("assigned frame is u64"),
        );
        expected_frames.push(0);
    }
    assert!(zero_frames.windows(2).all(|frames| frames[0] < frames[1]));

    let run_root = private_root.join("runs").join(run_id);
    let padlog_text = fs::read_to_string(run_root.join("input.padlog")).expect("padlog is written");
    assert!(padlog_text.starts_with("padlog v1\n"));
    assert!(padlog_text.ends_with("4x0000\n"));
    let parsed = PadLog::parse(&padlog_text).expect("padlog parses");
    assert_eq!(
        parsed
            .frames()
            .iter()
            .map(|word| word.raw())
            .collect::<Vec<_>>(),
        expected_frames
    );

    let event_lines = read_lines(&run_root.join("padlog-events.jsonl"));
    assert_eq!(
        event_lines.len(),
        expected_frames.len(),
        "duplicate client_seq must not append a second private event"
    );
    let events = event_lines
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).expect("event row parses"))
        .collect::<Vec<_>>();
    assert_eq!(events[0]["schema_version"], 1);
    assert_eq!(events[0]["run_id"], run_id);
    assert_eq!(events[0]["frame_index"], 0);
    assert_eq!(events[0]["client_seq"], 1);
    assert_eq!(events[0]["source_id"], "keyboard");
    assert_eq!(events[0]["pad_word"], expected_all_word);
    assert_eq!(events[0]["status"], "applied");
    assert_eq!(events[0]["message"], "input applied");

    for (event, (button, expected_word)) in events.iter().skip(1).zip(button_cases) {
        assert_eq!(event["source_id"], "keyboard");
        assert_eq!(event["pad_word"], expected_word);
        assert_eq!(
            event["pad_word"].as_u64().unwrap() & u64::from(!PAD_MASK),
            0
        );
        assert_eq!(
            event["client_seq"],
            10 + button_cases
                .iter()
                .position(|(name, _)| *name == button)
                .expect("button is in cases") as u64
        );
    }

    let zero_sources = ["focus", "page-hidden", "reconnect", "gamepad-disconnect"];
    let first_zero_event = 1 + button_cases.len();
    for ((event, source_id), acked_frame) in events
        .iter()
        .skip(first_zero_event)
        .zip(zero_sources)
        .zip(zero_frames)
    {
        assert_eq!(event["source_id"], source_id);
        assert_eq!(event["pad_word"], 0);
        assert_eq!(event["assigned_frame"], acked_frame);
        assert_eq!(
            event["pad_word"].as_u64().unwrap() & u64::from(!PAD_MASK),
            0
        );
    }

    let bridge_events = fs::read_to_string(run_root.join("bridge-events.jsonl"))
        .expect("bridge event log is written");
    assert!(bridge_events.contains("session_started"));
    assert!(!bridge_events.contains(&private_root.display().to_string()));
}

#[cfg(unix)]
#[tokio::test]
async fn synthetic_ws_input_artifact_append_failure_rejects_without_advancing_padlog() {
    let (_workspace, app, private_root) = synthetic_ws_app();
    let (cookie, session) = login_session(app.clone()).await;
    let run_id = session["run_id"].as_str().expect("run id is present");
    let run_root = private_root.join("runs").join(run_id);
    fs::create_dir(run_root.join("padlog-events.jsonl"))
        .expect("directory blocks padlog event append");
    let server = WsServer::start(app).await;
    let mut ws = server.connect(&cookie).await;

    let reject = send_input(&mut ws, input_message(1, "keyboard", &["A"])).await;

    assert_eq!(reject["type"], "input_reject");
    assert_eq!(reject["payload"]["error"]["code"], "backend_unavailable");
    assert_eq!(reject["payload"]["error"]["message"], "Input rejected.");
    let reject_text = reject.to_string();
    for private in [
        private_root.display().to_string(),
        GOOD_CREDENTIAL.to_string(),
        "input.padlog".to_string(),
        "padlog-events.jsonl".to_string(),
    ] {
        assert!(
            !reject_text.contains(&private),
            "public reject leaked {private}"
        );
    }

    let padlog_text = fs::read_to_string(run_root.join("input.padlog"))
        .expect("rollback padlog snapshot is written");
    assert_eq!(padlog_text, "padlog v1\n");
    assert!(
        PadLog::parse(&padlog_text)
            .expect("rollback padlog parses")
            .frames()
            .is_empty()
    );
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
            "source": payload_source(source_id),
            "buttons": buttons
        }
    })
}

async fn login_cookie(app: axum::Router) -> String {
    login_session(app).await.0
}

async fn login_session(app: axum::Router) -> (String, Value) {
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

    let cookie = response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("session cookie pair exists")
        .to_string();
    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("login body reads");
    let json = serde_json::from_slice(&body).expect("login response is json");

    (cookie, json)
}

fn payload_source(source_id: &str) -> &'static str {
    if source_id.contains("gamepad") {
        "gamepad"
    } else {
        "keyboard"
    }
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

fn assert_runtime_security_headers(headers: &tokio_tungstenite::tungstenite::http::HeaderMap) {
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

fn synthetic_ws_app() -> (tempfile::TempDir, axum::Router, PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::synthetic_for_tests(config(&private_root)));
    (workspace, app, private_root)
}

fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .expect("jsonl file reads")
        .lines()
        .map(ToOwned::to_owned)
        .collect()
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
    state: Mutex<SessionState>,
    injected: Mutex<Vec<InputScheduleRequest>>,
}

impl RecordingBackend {
    fn new(state: SessionState) -> Self {
        Self {
            state: Mutex::new(state),
            injected: Mutex::new(Vec::new()),
        }
    }

    fn state(&self) -> SessionState {
        *self.state.lock().expect("state mutex poisoned")
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
            state: self.state(),
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
            state: self.state(),
            backend_mode: self.mode(),
            current_frame: 0,
            capabilities: self.capabilities(),
            last_applied_input_frame: 0,
            last_preview_frame: 0,
            active_capture_job_id: None,
        })
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        *self.state.lock().expect("state mutex poisoned") = SessionState::Paused;
        Ok(RunBoundary {
            session_id,
            state: SessionState::Paused,
            current_frame: 0,
        })
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        *self.state.lock().expect("state mutex poisoned") = SessionState::Running;
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
