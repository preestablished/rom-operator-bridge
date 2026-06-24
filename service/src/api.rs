use crate::{
    auth::{
        ALLOWED_ORIGIN, AuthError, AuthState, expired_session_cookie_header, session_cookie_header,
        validate_runtime_request,
    },
    backend::{
        BackendError, BackendMode, BridgeBackend, RealBackendUnavailable, StopReason,
        SyntheticBackend,
    },
    config::ServiceConfig,
    input::{PAD_LAYOUT_ID, PAD_LAYOUT_VERSION},
    ws_events::{WsEventState, serve_event_socket},
    ws_input::{WsInputState, serve_input_socket},
};
use axum::{
    Json, Router,
    extract::{State, ws::WebSocketUpgrade},
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode, Uri,
        header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, PRAGMA, SET_COOKIE, VARY},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub const RUNTIME_API_SCHEMA_VERSION: u16 = 1;

#[derive(Clone)]
pub struct AppState {
    config: ServiceConfig,
    backend: Arc<dyn BridgeBackend>,
    auth: AuthState,
    runtime_session: Arc<Mutex<Option<String>>>,
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
            ws_events: WsEventState::new(),
            ws_input: WsInputState::new(),
        }
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

    *state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned") = None;
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
    *state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned") = None;
    state.ws_events.reset_session(&stopped.session_id);
    state.ws_input.reset_session(&stopped.session_id);
    Ok(())
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
