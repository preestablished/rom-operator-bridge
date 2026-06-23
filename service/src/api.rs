use crate::{
    backend::{BackendMode, BridgeBackend, RealBackendUnavailable, SyntheticBackend},
    config::ServiceConfig,
};
use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub const RUNTIME_API_SCHEMA_VERSION: u16 = 1;

#[derive(Clone)]
pub struct AppState {
    config: ServiceConfig,
    backend: Arc<dyn BridgeBackend>,
}

impl AppState {
    pub fn from_config(config: ServiceConfig) -> Self {
        let backend: Arc<dyn BridgeBackend> = match config.backend_mode() {
            BackendMode::Synthetic => Arc::new(SyntheticBackend),
            BackendMode::Real => Arc::new(RealBackendUnavailable),
        };

        Self { config, backend }
    }

    pub fn synthetic_for_tests(config: ServiceConfig) -> Self {
        Self {
            config,
            backend: Arc::new(SyntheticBackend),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .fallback(not_found)
        .with_state(state)
}

async fn health(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Json<HealthResponse> {
    Json(HealthResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        ok: true,
        service_version: state.config.service_version().to_string(),
        backend_mode: state.backend.mode(),
        runtime_api: RUNTIME_API_SCHEMA_VERSION,
    })
}

async fn not_found() -> AppError {
    AppError::new(
        StatusCode::NOT_FOUND,
        ErrorCode::BadRequest,
        "Route not found.",
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
        (self.status, Json(self.envelope)).into_response()
    }
}
