use crate::{
    auth::{
        ALLOWED_ORIGIN, AuthError, AuthState, expired_session_cookie_header, session_cookie_header,
        validate_origin, validate_runtime_request,
    },
    backend::{
        BackendError, BackendMode, BridgeBackend, FramePreview, RealBackendUnavailable, StopReason,
        SyntheticBackend,
    },
    config::ServiceConfig,
    input::{PAD_LAYOUT_ID, PAD_LAYOUT_VERSION},
    ws_events::{WsEventState, serve_event_socket},
    ws_input::{WsInputState, serve_input_socket},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{State, ws::WebSocketUpgrade},
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode, Uri,
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE, PRAGMA, SET_COOKIE, VARY,
        },
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

pub const RUNTIME_API_SCHEMA_VERSION: u16 = 1;
const JSON_SAFE_U64_MAX: u64 = 9_007_199_254_740_991;
const MAX_CACHED_FRAME_PREVIEWS: usize = 16;

#[derive(Clone)]
pub struct AppState {
    config: ServiceConfig,
    backend: Arc<dyn BridgeBackend>,
    auth: AuthState,
    runtime_session: Arc<Mutex<Option<String>>>,
    frame_previews: FramePreviewState,
    ws_events: WsEventState,
    ws_input: WsInputState,
}

impl AppState {
    pub fn from_config(config: ServiceConfig) -> Self {
        let backend: Arc<dyn BridgeBackend> = match config.backend_mode() {
            BackendMode::Synthetic => Arc::new(SyntheticBackend::with_private_config(
                config.private_config().clone(),
            )),
            BackendMode::Real => Arc::new(RealBackendUnavailable),
        };

        Self {
            config,
            backend,
            auth: AuthState::new(),
            runtime_session: Arc::new(Mutex::new(None)),
            frame_previews: FramePreviewState::new(),
            ws_events: WsEventState::new(),
            ws_input: WsInputState::new(),
        }
    }

    pub fn synthetic_for_tests(config: ServiceConfig) -> Self {
        Self::synthetic_for_tests_with_auth(config, AuthState::new())
    }

    pub fn synthetic_for_tests_with_auth(config: ServiceConfig, auth: AuthState) -> Self {
        let private_config = config.private_config().clone();
        Self {
            config,
            backend: Arc::new(SyntheticBackend::with_private_config(private_config)),
            auth,
            runtime_session: Arc::new(Mutex::new(None)),
            frame_previews: FramePreviewState::new(),
            ws_events: WsEventState::new(),
            ws_input: WsInputState::new(),
        }
    }

    pub fn for_tests_with_backend(
        config: ServiceConfig,
        auth: AuthState,
        backend: Arc<dyn BridgeBackend>,
    ) -> Self {
        Self {
            config,
            backend,
            auth,
            runtime_session: Arc::new(Mutex::new(None)),
            frame_previews: FramePreviewState::new(),
            ws_events: WsEventState::new(),
            ws_input: WsInputState::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct FramePreviewState {
    inner: Arc<Mutex<VecDeque<FramePreview>>>,
}

impl FramePreviewState {
    fn new() -> Self {
        Self::default()
    }

    fn reset_session(&self, _session_id: &str) {
        self.inner
            .lock()
            .expect("frame preview mutex poisoned")
            .clear();
    }

    fn remember(&self, preview: &FramePreview) {
        let mut previews = self.inner.lock().expect("frame preview mutex poisoned");
        previews.retain(|cached| {
            cached.session_id != preview.session_id || cached.frame != preview.frame
        });
        previews.push_back(preview.clone());
        while previews.len() > MAX_CACHED_FRAME_PREVIEWS {
            previews.pop_front();
        }
    }

    fn get(&self, session_id: &str, frame: u64) -> Option<FramePreview> {
        self.inner
            .lock()
            .expect("frame preview mutex poisoned")
            .iter()
            .find(|preview| preview.session_id == session_id && preview.frame == frame)
            .cloned()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health).fallback(method_not_allowed))
        .route(
            "/api/session",
            get(session_status).fallback(method_not_allowed),
        )
        .route(
            "/api/session/start",
            post(start_session).fallback(method_not_allowed),
        )
        .route(
            "/api/session/stop",
            post(stop_session).fallback(method_not_allowed),
        )
        .route(
            "/api/run/status",
            get(run_status).fallback(method_not_allowed),
        )
        .route(
            "/api/run/pause",
            post(pause_run).fallback(method_not_allowed),
        )
        .route(
            "/api/run/resume",
            post(resume_run).fallback(method_not_allowed),
        )
        .route(
            "/api/frame/current",
            get(frame_current).fallback(method_not_allowed),
        )
        .route(
            "/api/frame/current/image",
            get(frame_current_image).fallback(method_not_allowed),
        )
        .route(
            "/ws/input",
            get(input_ws_handshake).fallback(method_not_allowed),
        )
        .route(
            "/ws/events",
            get(events_ws_handshake).fallback(method_not_allowed),
        )
        .fallback(not_found)
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        ok: true,
        service_version: state.config.service_version().to_string(),
        backend_mode: state.backend.mode(),
        runtime_api: RUNTIME_API_SCHEMA_VERSION,
    })
}

async fn start_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<StartSessionRequest>,
) -> Response {
    if let Err(error) = validate_runtime_request(&headers, &uri) {
        return auth_error(error).into_response();
    }

    if request.schema_version != RUNTIME_API_SCHEMA_VERSION {
        return AppError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::BadRequest,
            "Unsupported schema version.",
            false,
        )
        .into_response();
    }

    if request.backend_mode != state.config.backend_mode() {
        return AppError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::BadRequest,
            "Requested backend mode is not available.",
            false,
        )
        .into_response();
    }

    let operator_session = match state
        .auth
        .login(state.config.private_config(), &request.operator_credential)
    {
        Ok(session) => session,
        Err(error) => return auth_error(error).into_response(),
    };

    if let Err(error) = cleanup_runtime_session(&state, StopReason::SessionReplaced) {
        state.auth.clear_session_token(&operator_session.token);
        return backend_error(error).into_response();
    }

    let backend_session = match state
        .backend
        .start_session(crate::backend::StartBackendSession {
            requested_capabilities: state.backend.capabilities(),
        }) {
        Ok(session) => session,
        Err(_) => {
            state.auth.clear_session_token(&operator_session.token);
            return AppError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::BackendUnavailable,
                "Backend unavailable.",
                true,
            )
            .into_response();
        }
    };

    let session_id = backend_session.session_id.clone();
    *state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned") = Some(session_id);
    state
        .frame_previews
        .reset_session(&backend_session.session_id);
    state.ws_events.reset_session(&backend_session.session_id);
    state.ws_input.reset_session(&backend_session.session_id);

    let mut response = Json(StartSessionResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        session_id: backend_session.session_id,
        run_id: backend_session.run_id,
        state: backend_session.state,
        current_frame: backend_session.current_frame,
        pad_layout: PadLayoutResponse {
            layout_id: PAD_LAYOUT_ID,
            layout_version: PAD_LAYOUT_VERSION,
        },
        capabilities: backend_session.capabilities,
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie_header(&operator_session))
            .expect("session cookie contains only valid header characters"),
    );
    response
}

async fn session_status(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }

    let Some(session_id) = state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .clone()
    else {
        return auth_error(AuthError::MissingSession).into_response();
    };

    let status = match state.backend.status(session_id) {
        Ok(status) => status,
        Err(_) => {
            return AppError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::BackendUnavailable,
                "Backend unavailable.",
                true,
            )
            .into_response();
        }
    };

    let mut response = Json(SessionResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        active: true,
        session_id: status.session_id,
        run_id: status.run_id,
        state: status.state,
        current_frame: status.current_frame,
        backend_mode: status.backend_mode,
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn stop_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<StopSessionRequest>,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    if request.schema_version != RUNTIME_API_SCHEMA_VERSION {
        return bad_request("Unsupported schema version.").into_response();
    }
    if let Err(response) = ensure_active_session(&state, &request.session_id) {
        return response;
    }

    let stopped = match state
        .backend
        .stop_session(request.session_id.clone(), request.reason)
    {
        Ok(stopped) => stopped,
        Err(error) => return backend_error(error).into_response(),
    };
    publish_stopped_event(&state, &stopped);

    *state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned") = None;
    state.frame_previews.reset_session(&stopped.session_id);
    state.ws_events.reset_session(&stopped.session_id);
    state.ws_input.reset_session(&stopped.session_id);
    if let Err(error) = state.auth.clear_session_headers(&headers) {
        return auth_error(error).into_response();
    }

    let mut response = Json(StopSessionResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        session_id: stopped.session_id,
        state: stopped.state,
        final_frame: stopped.final_frame,
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&expired_session_cookie_header())
            .expect("expired session cookie contains only valid header characters"),
    );
    response
}

async fn run_status(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    let session_id = match active_session_id(&state) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };

    let status = match state.backend.status(session_id) {
        Ok(status) => status,
        Err(error) => return backend_error(error).into_response(),
    };

    let mut response = Json(RunStatusResponse::from(status)).into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn frame_current(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    let session_id = match active_session_id(&state) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };

    let status = match state.backend.status(session_id.clone()) {
        Ok(status) => status,
        Err(error) => return backend_error(error).into_response(),
    };
    if status.session_id != session_id {
        return auth_error(AuthError::BadSession).into_response();
    }
    let preview = match state.backend.framebuffer(session_id.clone()) {
        Ok(preview) => preview,
        Err(error) => return backend_error(error).into_response(),
    };
    if let Err(response) = validate_frame_preview(&session_id, &preview) {
        return response;
    }
    state.frame_previews.remember(&preview);

    let mut response = Json(FrameCurrentResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        frame: preview.frame,
        captured_at: "1970-01-01T00:00:00Z",
        stale: preview.frame < status.current_frame,
        width: preview.width,
        height: preview.height,
        format: "image/png",
        image_url: format!("/api/frame/current/image?frame={}", preview.frame),
        preview_hash: sha256_ref(&preview.png_bytes),
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn frame_current_image(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = authenticate_runtime_request_allowing_frame_hint(&state, &headers, &uri)
    {
        return response;
    }
    let requested_frame = match requested_frame_hint(&uri) {
        Ok(requested_frame) => requested_frame,
        Err(response) => return response,
    };
    let session_id = match active_session_id(&state) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let preview = if let Some(frame) = requested_frame {
        match state.frame_previews.get(&session_id, frame) {
            Some(preview) => preview,
            None => {
                let preview = match state.backend.framebuffer(session_id.clone()) {
                    Ok(preview) => preview,
                    Err(error) => return backend_error(error).into_response(),
                };
                if let Err(response) = validate_frame_preview(&session_id, &preview) {
                    return response;
                }
                if preview.frame != frame {
                    return bad_request("Preview frame unavailable.").into_response();
                }
                state.frame_previews.remember(&preview);
                preview
            }
        }
    } else {
        let preview = match state.backend.framebuffer(session_id.clone()) {
            Ok(preview) => preview,
            Err(error) => return backend_error(error).into_response(),
        };
        if let Err(response) = validate_frame_preview(&session_id, &preview) {
            return response;
        }
        state.frame_previews.remember(&preview);
        preview
    };

    let mut response = Response::new(Body::from(preview.png_bytes));
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("image/png"));
    response
}

async fn pause_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<SessionOnlyRequest>,
) -> Response {
    run_state_transition(state, headers, uri, request, RunTransition::Pause)
}

async fn resume_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<SessionOnlyRequest>,
) -> Response {
    run_state_transition(state, headers, uri, request, RunTransition::Resume)
}

async fn input_ws_handshake(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }

    let Some(session_id) = state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .clone()
    else {
        return auth_error(AuthError::MissingSession).into_response();
    };

    let backend = state.backend.clone();
    let ws_input = state.ws_input.clone();
    let mut response = ws
        .on_upgrade(move |socket| serve_input_socket(socket, backend, ws_input, session_id))
        .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn events_ws_handshake(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }

    let session_id = match active_session_id(&state) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };
    let status = match state.backend.status(session_id.clone()) {
        Ok(status) => status,
        Err(error) => return backend_error(error).into_response(),
    };
    if status.session_id != session_id {
        return auth_error(AuthError::BadSession).into_response();
    }

    let ws_events = state.ws_events.clone();
    let sanitizer = state.config.private_config().public_sanitizer();
    let mut response = ws
        .on_upgrade(move |socket| serve_event_socket(socket, ws_events, sanitizer, status))
        .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

fn run_state_transition(
    state: AppState,
    headers: HeaderMap,
    uri: Uri,
    request: SessionOnlyRequest,
    transition: RunTransition,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    if request.schema_version != RUNTIME_API_SCHEMA_VERSION {
        return bad_request("Unsupported schema version.").into_response();
    }
    if let Err(response) = ensure_active_session(&state, &request.session_id) {
        return response;
    }

    let boundary = match transition {
        RunTransition::Pause => state.backend.pause(request.session_id),
        RunTransition::Resume => state.backend.resume(request.session_id),
    };
    let boundary = match boundary {
        Ok(boundary) => boundary,
        Err(error) => return backend_error(error).into_response(),
    };
    if transition == RunTransition::Resume
        && state
            .ws_input
            .flush_pending(state.backend.as_ref(), &boundary.session_id)
            .is_err()
    {
        return backend_error(BackendError::BackendUnavailable).into_response();
    }
    publish_run_boundary_event(&state, &boundary);

    let mut response = Json(RunStateResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        state: boundary.state,
        current_frame: boundary.current_frame,
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

fn authenticate_runtime_request(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<(), Response> {
    if let Err(error) = validate_runtime_request(headers, uri) {
        return Err(auth_error(error).into_response());
    }

    match state.auth.authenticate_headers(headers) {
        Ok(()) => Ok(()),
        Err(AuthError::ExpiredSession) => {
            if let Err(error) = cleanup_runtime_session(state, StopReason::SessionReplaced) {
                return Err(backend_error(error).into_response());
            }
            Err(auth_error(AuthError::ExpiredSession).into_response())
        }
        Err(error) => Err(auth_error(error).into_response()),
    }
}

fn authenticate_runtime_request_allowing_frame_hint(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<(), Response> {
    if uri.query().is_some_and(is_frame_hint_query) {
        if let Err(error) = validate_origin(headers) {
            return Err(auth_error(error).into_response());
        }
        match state.auth.authenticate_headers(headers) {
            Ok(()) => Ok(()),
            Err(AuthError::ExpiredSession) => {
                if let Err(error) = cleanup_runtime_session(state, StopReason::SessionReplaced) {
                    return Err(backend_error(error).into_response());
                }
                Err(auth_error(AuthError::ExpiredSession).into_response())
            }
            Err(error) => Err(auth_error(error).into_response()),
        }
    } else {
        authenticate_runtime_request(state, headers, uri)
    }
}

fn is_frame_hint_query(query: &str) -> bool {
    let Some(frame) = query.strip_prefix("frame=") else {
        return false;
    };
    !frame.is_empty() && frame.bytes().all(|byte| byte.is_ascii_digit())
}

fn requested_frame_hint(uri: &Uri) -> Result<Option<u64>, Response> {
    let Some(query) = uri.query().filter(|query| is_frame_hint_query(query)) else {
        return Ok(None);
    };
    let frame = query
        .strip_prefix("frame=")
        .expect("frame hint query was validated")
        .parse::<u64>()
        .ok()
        .filter(|frame| *frame <= JSON_SAFE_U64_MAX)
        .ok_or_else(|| bad_request("Preview frame unavailable.").into_response())?;
    Ok(Some(frame))
}

fn validate_frame_preview(session_id: &str, preview: &FramePreview) -> Result<(), Response> {
    if preview.session_id != session_id {
        return Err(auth_error(AuthError::BadSession).into_response());
    }
    if preview.frame > JSON_SAFE_U64_MAX {
        return Err(bad_request("Preview frame unavailable.").into_response());
    }
    Ok(())
}

fn cleanup_runtime_session(state: &AppState, reason: StopReason) -> Result<(), BackendError> {
    let session_id = state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .clone();
    let Some(session_id) = session_id else {
        return Ok(());
    };

    let stopped = state.backend.stop_session(session_id, reason)?;
    publish_stopped_event(state, &stopped);
    *state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned") = None;
    state.frame_previews.reset_session(&stopped.session_id);
    state.ws_events.reset_session(&stopped.session_id);
    state.ws_input.reset_session(&stopped.session_id);
    Ok(())
}

fn publish_run_boundary_event(state: &AppState, boundary: &crate::backend::RunBoundary) {
    let sanitizer = state.config.private_config().public_sanitizer();
    let _ = state.ws_events.publish_boundary(
        boundary,
        state.backend.mode(),
        state.backend.capabilities(),
        &sanitizer,
    );
}

fn publish_stopped_event(state: &AppState, stopped: &crate::backend::StoppedSession) {
    let sanitizer = state.config.private_config().public_sanitizer();
    let _ = state.ws_events.publish_stopped(
        stopped,
        state.backend.mode(),
        state.backend.capabilities(),
        &sanitizer,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunTransition {
    Pause,
    Resume,
}

fn active_session_id(state: &AppState) -> Result<String, Response> {
    state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .clone()
        .ok_or_else(|| auth_error(AuthError::MissingSession).into_response())
}

fn ensure_active_session(state: &AppState, session_id: &str) -> Result<(), Response> {
    let active = active_session_id(state)?;
    if active == session_id {
        Ok(())
    } else {
        Err(auth_error(AuthError::BadSession).into_response())
    }
}

async fn not_found() -> AppError {
    AppError::new(
        StatusCode::NOT_FOUND,
        ErrorCode::BadRequest,
        "Route not found.",
        false,
    )
}

async fn method_not_allowed() -> AppError {
    AppError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        ErrorCode::BadRequest,
        "Method not allowed.",
        false,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HealthResponse {
    pub schema_version: u16,
    pub ok: bool,
    pub service_version: String,
    pub backend_mode: BackendMode,
    pub runtime_api: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartSessionRequest {
    pub schema_version: u16,
    pub operator_credential: String,
    pub backend_mode: BackendMode,
    pub requested_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartSessionResponse {
    pub schema_version: u16,
    pub session_id: String,
    pub run_id: String,
    pub state: crate::backend::SessionState,
    pub current_frame: u64,
    pub pad_layout: PadLayoutResponse,
    pub capabilities: crate::backend::BackendCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopSessionRequest {
    pub schema_version: u16,
    pub session_id: String,
    pub reason: crate::backend::StopReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopSessionResponse {
    pub schema_version: u16,
    pub session_id: String,
    pub state: crate::backend::SessionState,
    pub final_frame: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOnlyRequest {
    pub schema_version: u16,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunStateResponse {
    pub schema_version: u16,
    pub state: crate::backend::SessionState,
    pub current_frame: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunStatusResponse {
    pub schema_version: u16,
    pub session_id: String,
    pub run_id: String,
    pub state: crate::backend::SessionState,
    pub backend_mode: BackendMode,
    pub current_frame: u64,
    pub last_applied_input_frame: u64,
    pub last_preview_frame: u64,
    pub preview_stale: bool,
    pub active_capture_job_id: Option<String>,
    pub capabilities: crate::backend::BackendCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrameCurrentResponse {
    pub schema_version: u16,
    pub frame: u64,
    pub captured_at: &'static str,
    pub stale: bool,
    pub width: u32,
    pub height: u32,
    pub format: &'static str,
    pub image_url: String,
    pub preview_hash: String,
}

impl From<crate::backend::RunStatus> for RunStatusResponse {
    fn from(status: crate::backend::RunStatus) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            session_id: status.session_id,
            run_id: status.run_id,
            state: status.state,
            backend_mode: status.backend_mode,
            current_frame: status.current_frame,
            last_applied_input_frame: status.last_applied_input_frame,
            last_preview_frame: status.last_preview_frame,
            preview_stale: status.last_preview_frame < status.current_frame,
            active_capture_job_id: status.active_capture_job_id,
            capabilities: status.capabilities,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PadLayoutResponse {
    pub layout_id: &'static str,
    pub layout_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionResponse {
    pub schema_version: u16,
    pub active: bool,
    pub session_id: String,
    pub run_id: String,
    pub state: crate::backend::SessionState,
    pub current_frame: u64,
    pub backend_mode: BackendMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorEnvelope {
    pub schema_version: u16,
    pub error: ErrorObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorObject {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub details: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    AuthRejected,
    OriginRejected,
    SessionInactive,
    SessionActiveElsewhere,
    BackendUnavailable,
    FrameStale,
    CaptureInProgress,
    CaptureFailed,
    LabelConflict,
    ValidationFailed,
    BadRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    status: StatusCode,
    envelope: ErrorEnvelope,
}

impl AppError {
    pub fn new(
        status: StatusCode,
        code: ErrorCode,
        message: &'static str,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            envelope: ErrorEnvelope {
                schema_version: RUNTIME_API_SCHEMA_VERSION,
                error: ErrorObject {
                    code,
                    message: message.to_string(),
                    retryable,
                    details: json!({}),
                },
            },
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.envelope)).into_response();
        apply_no_store_headers(response.headers_mut());
        response
    }
}

fn apply_no_store_headers(headers: &mut axum::http::HeaderMap) {
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
}

fn apply_runtime_headers(headers: &mut axum::http::HeaderMap, origin: Option<&'static str>) {
    apply_no_store_headers(headers);
    if let Some(origin) = origin {
        headers.insert(
            ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static(origin),
        );
        headers.insert(VARY, HeaderValue::from_static("Origin"));
    }
}

fn bad_request(message: &'static str) -> AppError {
    AppError::new(
        StatusCode::BAD_REQUEST,
        ErrorCode::BadRequest,
        message,
        false,
    )
}

fn backend_error(_error: crate::backend::BackendError) -> AppError {
    AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::BackendUnavailable,
        "Backend unavailable.",
        true,
    )
}

fn sha256_ref(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity("sha256:".len() + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        output.push(hex_nibble(byte >> 4));
        output.push(hex_nibble(byte & 0x0f));
    }
    output
}

fn hex_nibble(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + value - 10) as char,
        _ => unreachable!("nibble is masked to four bits"),
    }
}

fn auth_error(error: AuthError) -> AppError {
    match error {
        AuthError::MissingOrigin | AuthError::OriginRejected => AppError::new(
            StatusCode::FORBIDDEN,
            ErrorCode::OriginRejected,
            "Origin rejected.",
            false,
        ),
        AuthError::CredentialInUrl => AppError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::AuthRejected,
            "Authentication rejected.",
            false,
        ),
        AuthError::BadCredential | AuthError::PrivateConfig(_) => AppError::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::AuthRejected,
            "Authentication rejected.",
            false,
        ),
        AuthError::RateLimited => AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::AuthRejected,
            "Authentication rejected.",
            true,
        ),
        AuthError::SessionActiveElsewhere => AppError::new(
            StatusCode::CONFLICT,
            ErrorCode::SessionActiveElsewhere,
            "Session active elsewhere.",
            false,
        ),
        AuthError::MissingSession | AuthError::ExpiredSession | AuthError::BadSession => {
            AppError::new(
                StatusCode::UNAUTHORIZED,
                ErrorCode::SessionInactive,
                "Session inactive.",
                false,
            )
        }
    }
}
