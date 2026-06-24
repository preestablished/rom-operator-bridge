use crate::{
    auth::{ALLOWED_ORIGIN, AuthError, AuthState, session_cookie_header, validate_runtime_request},
    backend::{BackendMode, BridgeBackend, RealBackendUnavailable, SyntheticBackend},
    config::ServiceConfig,
    input::{PAD_LAYOUT_ID, PAD_LAYOUT_VERSION},
};
use axum::{
    Json, Router,
    extract::State,
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
}

impl AppState {
    pub fn from_config(config: ServiceConfig) -> Self {
        let backend: Arc<dyn BridgeBackend> = match config.backend_mode() {
            BackendMode::Synthetic => Arc::new(SyntheticBackend),
            BackendMode::Real => Arc::new(RealBackendUnavailable),
        };

        Self {
            config,
            backend,
            auth: AuthState::new(),
            runtime_session: Arc::new(Mutex::new(None)),
        }
    }

    pub fn synthetic_for_tests(config: ServiceConfig) -> Self {
        Self::synthetic_for_tests_with_auth(config, AuthState::new())
    }

    pub fn synthetic_for_tests_with_auth(config: ServiceConfig, auth: AuthState) -> Self {
        Self {
            config,
            backend: Arc::new(SyntheticBackend),
            auth,
            runtime_session: Arc::new(Mutex::new(None)),
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
            "/ws/input",
            get(input_ws_handshake).fallback(method_not_allowed),
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
    if let Err(error) = validate_runtime_request(&headers, &uri)
        .and_then(|()| state.auth.authenticate_headers(&headers))
    {
        return auth_error(error).into_response();
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

async fn input_ws_handshake(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(error) = validate_runtime_request(&headers, &uri)
        .and_then(|()| state.auth.authenticate_headers(&headers))
    {
        return auth_error(error).into_response();
    }

    let mut response = StatusCode::SWITCHING_PROTOCOLS.into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
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
        AuthError::MissingSession | AuthError::BadSession => AppError::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::SessionInactive,
            "Session inactive.",
            false,
        ),
    }
}
