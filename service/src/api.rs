use crate::{
    artifacts::{
        CaptureSummary as PrivateCaptureSummary, PrivateArtifactStore, RecentCapturesFile,
    },
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
    labels::{
        LabelApplyOutcome, LabelApplyRequest, LabelConflict, LabelConflictKind, LabelSnapshot,
        LabelState, LabelStoreError, LabelUpdate,
    },
    sanitization::PublicSanitizer,
    validation_status::{
        PublicValidationStatus, ValidationRunUpdate, ValidationStatusError, ValidationStatusState,
    },
    ws_events::{WsEventState, serve_event_socket},
    ws_input::{WsInputState, serve_input_socket},
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State, ws::WebSocketUpgrade},
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
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

pub const RUNTIME_API_SCHEMA_VERSION: u16 = 1;
const JSON_SAFE_U64_MAX: u64 = 9_007_199_254_740_991;
const MAX_CACHED_FRAME_PREVIEWS: usize = 16;
const DEFAULT_CAPTURE_LIMIT: usize = 50;
const MAX_CAPTURE_LIMIT: usize = 200;

#[derive(Clone)]
pub struct AppState {
    config: ServiceConfig,
    backend: Arc<dyn BridgeBackend>,
    auth: AuthState,
    runtime_session: Arc<Mutex<Option<String>>>,
    captures: CaptureState,
    labels: LabelState,
    validation: ValidationStatusState,
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
            captures: CaptureState::new(),
            labels: LabelState::new(),
            validation: ValidationStatusState::new(),
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
            captures: CaptureState::new(),
            labels: LabelState::new(),
            validation: ValidationStatusState::new(),
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
            captures: CaptureState::new(),
            labels: LabelState::new(),
            validation: ValidationStatusState::new(),
            frame_previews: FramePreviewState::new(),
            ws_events: WsEventState::new(),
            ws_input: WsInputState::new(),
        }
    }

    pub fn validation_status_snapshot(&self) -> PublicValidationStatus {
        self.validation.snapshot()
    }

    pub fn record_validation_run(
        &self,
        update: ValidationRunUpdate,
    ) -> Result<PublicValidationStatus, ValidationStatusError> {
        let active_session_id =
            active_session_id(self).map_err(|_| ValidationStatusError::StaleSession)?;
        if !update.matches_session(&active_session_id) {
            return Err(ValidationStatusError::StaleSession);
        }
        let sanitizer = state_sanitizer(self);
        let public =
            self.validation
                .record_run(self.config.private_config(), &sanitizer, update)?;
        publish_validation_event(self, public.clone());
        Ok(public)
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

#[derive(Debug, Clone, Default)]
struct CaptureState {
    inner: Arc<Mutex<CaptureInner>>,
}

impl CaptureState {
    fn new() -> Self {
        Self::default()
    }

    fn reset_session(&self, session_id: &str) {
        let mut inner = self.inner.lock().expect("capture mutex poisoned");
        let removed_job_ids: Vec<String> = inner
            .jobs
            .iter()
            .filter(|(_, job)| job.session_id == session_id)
            .map(|(job_id, _)| job_id.clone())
            .collect();
        if removed_job_ids.is_empty() {
            return;
        }
        let removed_capture_ids: BTreeSet<String> = inner
            .captures
            .iter()
            .filter(|(_, record)| removed_job_ids.contains(&record.job_id))
            .map(|(capture_id, _)| capture_id.clone())
            .collect();
        inner
            .jobs
            .retain(|job_id, _| !removed_job_ids.contains(job_id));
        inner
            .captures
            .retain(|capture_id, _| !removed_capture_ids.contains(capture_id));
        inner
            .capture_order
            .retain(|capture_id| !removed_capture_ids.contains(capture_id));
        inner.idempotency.retain(|(stored_session_id, _), job_id| {
            stored_session_id != session_id || !removed_job_ids.contains(job_id)
        });
    }

    fn active_job_id(&self, session_id: &str) -> Option<String> {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        inner
            .jobs
            .values()
            .find(|job| job.session_id == session_id && job.status.is_active())
            .map(|job| job.job_id.clone())
    }

    fn is_labelable_capture(&self, session_id: &str, capture_id: &str) -> bool {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let Some(record) = inner.captures.get(capture_id) else {
            return false;
        };
        let Some(job) = inner.jobs.get(&record.job_id) else {
            return false;
        };
        job.session_id == session_id
            && job.capture_id.as_deref() == Some(capture_id)
            && job.status == CaptureStatus::Completed
            && job.labelable
    }

    fn trigger(&self, input: CaptureTriggerInput) -> Result<CaptureJobView, CaptureTriggerError> {
        let mut inner = self.inner.lock().expect("capture mutex poisoned");
        let key = (input.session_id.clone(), input.idempotency_key.clone());
        if let Some(job_id) = inner.idempotency.get(&key)
            && let Some(job) = inner.jobs.get(job_id)
        {
            return Ok(job.view());
        }
        if inner
            .jobs
            .values()
            .any(|job| job.session_id == input.session_id && job.status.is_active())
        {
            return Err(CaptureTriggerError::InProgress);
        }

        inner.next_job_seq += 1;
        let job_id = format!("synthetic-capture-job-{:04}", inner.next_job_seq);
        let scheduled_frame = input.observed_preview_frame.saturating_add(1);
        let mut job = CaptureJobRecord {
            job_id: job_id.clone(),
            session_id: input.session_id.clone(),
            status: CaptureStatus::Requested,
            requested_frame: input.observed_preview_frame,
            scheduled_frame,
            captured_frame: None,
            capture_id: None,
            labelable: false,
            has_preview: false,
            error: None,
            preview_png: input.preview_png,
            durable: false,
        };

        if input.observed_preview_frame < input.current_frame {
            job.status = CaptureStatus::Failed;
            job.error = Some(ErrorObject {
                code: ErrorCode::FrameStale,
                message: "Capture failed.".to_string(),
                retryable: true,
                details: json!({}),
            });
        }

        inner.idempotency.insert(key, job_id.clone());
        let view = job.view();
        inner.jobs.insert(job_id, job);

        if view.status == CaptureStatus::Failed {
            return Ok(view);
        }
        Ok(view)
    }

    fn job(&self, config: &ServiceConfig, job_id: &str) -> Result<CaptureJobView, BackendError> {
        let status = {
            let inner = self.inner.lock().expect("capture mutex poisoned");
            let Some(job) = inner.jobs.get(job_id) else {
                return Err(BackendError::BackendUnavailable);
            };
            job.status
        };

        match status {
            CaptureStatus::Requested => {
                let mut inner = self.inner.lock().expect("capture mutex poisoned");
                let Some(job) = inner.jobs.get_mut(job_id) else {
                    return Err(BackendError::BackendUnavailable);
                };
                job.status = CaptureStatus::Capturing;
                Ok(job.view())
            }
            CaptureStatus::Capturing => self.complete_capturing_job(config, job_id),
            CaptureStatus::Completed | CaptureStatus::Failed | CaptureStatus::NotLabelable => {
                let inner = self.inner.lock().expect("capture mutex poisoned");
                let Some(job) = inner.jobs.get(job_id) else {
                    return Err(BackendError::BackendUnavailable);
                };
                Ok(job.view())
            }
        }
    }

    fn complete_capturing_job(
        &self,
        config: &ServiceConfig,
        job_id: &str,
    ) -> Result<CaptureJobView, BackendError> {
        let (capture_id, status, labelable, recent) = {
            let inner = self.inner.lock().expect("capture mutex poisoned");
            let Some(job) = inner.jobs.get(job_id) else {
                return Err(BackendError::BackendUnavailable);
            };
            if job.status != CaptureStatus::Capturing {
                return Ok(job.view());
            }

            let capture_id = format!("synthetic-capture-{:04}", inner.next_capture_seq + 1);
            let status = if job.requested_frame == 0 {
                CaptureStatus::NotLabelable
            } else {
                CaptureStatus::Completed
            };
            let labelable = status == CaptureStatus::Completed;
            let mut captures = Vec::with_capacity(inner.capture_order.len() + 1);
            captures.push(PrivateCaptureSummary::new(
                capture_id.clone(),
                "1970-01-01T00:00:00Z",
                status.as_str(),
                labelable,
            ));
            captures.extend(inner.capture_order.iter().filter_map(|capture_id| {
                let record = inner.captures.get(capture_id)?;
                let job = inner.jobs.get(&record.job_id)?;
                Some(PrivateCaptureSummary::new(
                    capture_id.clone(),
                    "1970-01-01T00:00:00Z",
                    job.status.as_str(),
                    job.labelable,
                ))
            }));
            (
                capture_id,
                status,
                labelable,
                RecentCapturesFile::new(captures),
            )
        };

        if !config.private_config().is_placeholder() {
            PrivateArtifactStore::new(config.private_config())
                .write_recent_captures(&recent)
                .map_err(|_| BackendError::BackendUnavailable)?;
        }

        let mut inner = self.inner.lock().expect("capture mutex poisoned");
        let (view, completed_job_id) = {
            let Some(current_status) = inner.jobs.get(job_id).map(|job| job.status) else {
                return Err(BackendError::BackendUnavailable);
            };
            if current_status != CaptureStatus::Capturing {
                let job = inner
                    .jobs
                    .get(job_id)
                    .expect("job exists after status lookup");
                return Ok(job.view());
            }
            inner.next_capture_seq += 1;
            let job = inner
                .jobs
                .get_mut(job_id)
                .expect("job exists after status lookup");
            job.status = status;
            job.capture_id = Some(capture_id.clone());
            job.captured_frame = Some(job.scheduled_frame);
            job.labelable = labelable;
            job.has_preview = true;
            job.durable = true;
            (job.view(), job.job_id.clone())
        };
        inner.capture_order.push_front(capture_id.clone());
        inner.captures.insert(
            capture_id,
            CaptureRecord {
                job_id: completed_job_id,
            },
        );
        Ok(view)
    }

    fn recent(&self, offset: usize, limit: usize) -> CaptureRecentView {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let mut captures = Vec::new();
        let end = offset.saturating_add(limit);
        for capture_id in inner.capture_order.iter().skip(offset).take(limit) {
            let Some(record) = inner.captures.get(capture_id) else {
                continue;
            };
            let Some(job) = inner.jobs.get(&record.job_id) else {
                continue;
            };
            captures.push(job.summary());
        }

        CaptureRecentView {
            captures,
            next_cursor: if inner.capture_order.len() > end {
                Some(end.to_string())
            } else {
                None
            },
        }
    }

    fn detail(&self, capture_id: &str) -> Option<CaptureDetailView> {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let record = inner.captures.get(capture_id)?;
        let job = inner.jobs.get(&record.job_id)?;
        Some(job.detail())
    }

    fn preview(&self, capture_id: &str) -> Option<Vec<u8>> {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let record = inner.captures.get(capture_id)?;
        let job = inner.jobs.get(&record.job_id)?;
        Some(job.preview_png.clone())
    }
}

#[derive(Debug, Default)]
struct CaptureInner {
    next_job_seq: u64,
    next_capture_seq: u64,
    idempotency: BTreeMap<(String, String), String>,
    jobs: BTreeMap<String, CaptureJobRecord>,
    captures: BTreeMap<String, CaptureRecord>,
    capture_order: VecDeque<String>,
}

#[derive(Debug, Clone)]
struct CaptureJobRecord {
    job_id: String,
    session_id: String,
    status: CaptureStatus,
    requested_frame: u64,
    scheduled_frame: u64,
    captured_frame: Option<u64>,
    capture_id: Option<String>,
    labelable: bool,
    has_preview: bool,
    error: Option<ErrorObject>,
    preview_png: Vec<u8>,
    durable: bool,
}

impl CaptureJobRecord {
    fn view(&self) -> CaptureJobView {
        CaptureJobView {
            job_id: self.job_id.clone(),
            session_id: self.session_id.clone(),
            status: self.status,
            requested_frame: self.requested_frame,
            scheduled_frame: self.scheduled_frame,
            captured_frame: self.captured_frame,
            capture_id: self.capture_id.clone(),
            labelable: self.labelable,
            has_preview: self.has_preview,
            error: self.error.clone(),
        }
    }

    fn summary(&self) -> CaptureSummaryView {
        CaptureSummaryView {
            capture_id: self.capture_id.clone().unwrap_or_default(),
            frame: self.captured_frame.unwrap_or(self.scheduled_frame),
            status: self.status,
            labelable: self.labelable,
            has_preview: self.has_preview,
            labels: Vec::new(),
            created_at: "1970-01-01T00:00:00Z",
        }
    }

    fn detail(&self) -> CaptureDetailView {
        let capture_id = self.capture_id.clone().unwrap_or_default();
        CaptureDetailView {
            capture_id: capture_id.clone(),
            frame: self.captured_frame.unwrap_or(self.scheduled_frame),
            status: self.status,
            labelable: self.labelable,
            preview_image_url: format!("/api/capture/{capture_id}/preview"),
            privileged_features_available: false,
            labels: Vec::new(),
            sanitized_provenance: SanitizedProvenance {
                capture_source: "synthetic",
                layout_hash: "sha256:synthetic-layout-v1",
                capture_spec_hash: "sha256:synthetic-capture-v1",
                map_hash: "sha256:synthetic-map-v1",
            },
        }
    }
}

#[derive(Debug, Clone)]
struct CaptureRecord {
    job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureJobView {
    job_id: String,
    session_id: String,
    status: CaptureStatus,
    requested_frame: u64,
    scheduled_frame: u64,
    captured_frame: Option<u64>,
    capture_id: Option<String>,
    labelable: bool,
    has_preview: bool,
    error: Option<ErrorObject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureRecentView {
    captures: Vec<CaptureSummaryView>,
    next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureSummaryView {
    capture_id: String,
    frame: u64,
    status: CaptureStatus,
    labelable: bool,
    has_preview: bool,
    labels: Vec<String>,
    created_at: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureDetailView {
    capture_id: String,
    frame: u64,
    status: CaptureStatus,
    labelable: bool,
    preview_image_url: String,
    privileged_features_available: bool,
    labels: Vec<String>,
    sanitized_provenance: SanitizedProvenance,
}

#[derive(Debug, Clone)]
struct CaptureTriggerInput {
    session_id: String,
    idempotency_key: String,
    observed_preview_frame: u64,
    current_frame: u64,
    preview_png: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Requested,
    Capturing,
    Completed,
    Failed,
    NotLabelable,
}

impl CaptureStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Capturing => "capturing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::NotLabelable => "not_labelable",
        }
    }

    fn is_active(self) -> bool {
        matches!(self, Self::Requested | Self::Capturing)
    }
}

#[derive(Debug, Clone)]
enum CaptureTriggerError {
    InProgress,
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
            "/api/validation/status",
            get(validation_status).fallback(method_not_allowed),
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
            "/api/capture/trigger",
            post(capture_trigger).fallback(method_not_allowed),
        )
        .route(
            "/api/capture/jobs/{job_id}",
            get(capture_job_status).fallback(method_not_allowed),
        )
        .route(
            "/api/capture/recent",
            get(capture_recent).fallback(method_not_allowed),
        )
        .route(
            "/api/capture/{capture_id}",
            get(capture_detail).fallback(method_not_allowed),
        )
        .route(
            "/api/capture/{capture_id}/preview",
            get(capture_preview).fallback(method_not_allowed),
        )
        .route(
            "/api/labels",
            get(labels_snapshot)
                .post(labels_apply)
                .fallback(method_not_allowed),
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

async fn health(State(state): State<AppState>) -> Response {
    let mut response = Json(HealthResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        ok: true,
        service_version: state.config.service_version().to_string(),
        backend_mode: state.backend.mode(),
        runtime_api: RUNTIME_API_SCHEMA_VERSION,
    })
    .into_response();
    apply_no_store_headers(response.headers_mut());
    response
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
    state.captures.reset_session(&backend_session.session_id);
    state.labels.reset();
    state.validation.reset();
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
    state.captures.reset_session(&stopped.session_id);
    state.labels.reset();
    state.validation.reset();
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

    let status = match state.backend.status(session_id.clone()) {
        Ok(status) => status,
        Err(error) => return backend_error(error).into_response(),
    };
    if status.session_id != session_id {
        return auth_error(AuthError::BadSession).into_response();
    }
    let status = project_active_capture(&state, status);

    let mut response = Json(RunStatusResponse::from(status)).into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn validation_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    if let Err(response) = active_session_id(&state) {
        return response;
    }

    let mut response = Json(ValidationStatusResponse::from(
        state.validation_status_snapshot(),
    ))
    .into_response();
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

async fn capture_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<CaptureTriggerRequest>,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    if request.schema_version != RUNTIME_API_SCHEMA_VERSION {
        return bad_request("Unsupported schema version.").into_response();
    }
    if request.reason != CaptureReason::OperatorMark {
        return bad_request("Unsupported capture reason.").into_response();
    }
    if request.observed_preview_frame >= JSON_SAFE_U64_MAX {
        return bad_request("Preview frame unavailable.").into_response();
    }
    if !is_contract_id(&request.session_id) || !is_contract_uuid(&request.idempotency_key) {
        return bad_request("Invalid capture request.").into_response();
    }
    if let Err(response) = ensure_active_session(&state, &request.session_id) {
        return response;
    }

    let status = match state.backend.status(request.session_id.clone()) {
        Ok(status) => status,
        Err(error) => return backend_error(error).into_response(),
    };
    if status.session_id != request.session_id {
        return auth_error(AuthError::BadSession).into_response();
    }
    if status.current_frame > JSON_SAFE_U64_MAX {
        return bad_request("Preview frame unavailable.").into_response();
    }

    let preview_png = if request.observed_preview_frame < status.current_frame {
        Vec::new()
    } else {
        if request.observed_preview_frame > status.current_frame {
            return bad_request("Preview frame unavailable.").into_response();
        }
        let preview = match state
            .frame_previews
            .get(&request.session_id, request.observed_preview_frame)
        {
            Some(preview) => preview,
            None => {
                let preview = match state.backend.framebuffer(request.session_id.clone()) {
                    Ok(preview) => preview,
                    Err(error) => return backend_error(error).into_response(),
                };
                if let Err(response) = validate_frame_preview(&request.session_id, &preview) {
                    return response;
                }
                state.frame_previews.remember(&preview);
                preview
            }
        };
        if preview.frame != request.observed_preview_frame || preview.frame > JSON_SAFE_U64_MAX {
            return bad_request("Preview frame unavailable.").into_response();
        }
        preview.png_bytes
    };

    let input = CaptureTriggerInput {
        session_id: request.session_id.clone(),
        idempotency_key: request.idempotency_key,
        observed_preview_frame: request.observed_preview_frame,
        current_frame: status.current_frame,
        preview_png,
    };
    let view = match state.captures.trigger(input) {
        Ok(view) => view,
        Err(CaptureTriggerError::InProgress) => {
            return AppError::new(
                StatusCode::CONFLICT,
                ErrorCode::CaptureInProgress,
                "Capture already in progress.",
                false,
            )
            .into_response();
        }
    };
    publish_capture_event(&state, &request.session_id, &view);

    let mut response = Json(CaptureTriggerResponse::from(view)).into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn capture_job_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Path(job_id): Path<String>,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    if !is_contract_id(&job_id) {
        return bad_request("Invalid capture job.").into_response();
    }
    let view = match state.captures.job(&state.config, &job_id) {
        Ok(view) => view,
        Err(error) => return backend_error(error).into_response(),
    };
    publish_capture_event(&state, &view.session_id, &view);

    let mut response = Json(CaptureJobResponse::from(view)).into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn capture_recent(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    if let Err(response) =
        authenticate_runtime_request_allowing_query(&state, &headers, &uri, is_capture_recent_query)
    {
        return response;
    }
    let (offset, limit) = match capture_recent_window(&uri) {
        Ok(window) => window,
        Err(response) => return response,
    };
    let view = state.captures.recent(offset, limit);

    let mut response = Json(CaptureRecentResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        captures: view
            .captures
            .into_iter()
            .map(|summary| {
                let mut response = CaptureSummaryResponse::from(summary);
                response.labels = state.labels.label_names_for_capture(&response.capture_id);
                response
            })
            .collect(),
        next_cursor: view.next_cursor,
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn capture_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Path(capture_id): Path<String>,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    if !is_contract_id(&capture_id) {
        return bad_request("Invalid capture id.").into_response();
    }
    let Some(view) = state.captures.detail(&capture_id) else {
        return AppError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::BadRequest,
            "Capture not found.",
            false,
        )
        .into_response();
    };

    let mut detail = CaptureDetailResponse::from(view);
    detail.labels = state.labels.label_names_for_capture(&detail.capture_id);
    let mut response = Json(detail).into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn capture_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Path(capture_id): Path<String>,
) -> Response {
    if let Err(response) = authenticate_runtime_request_allowing_frame_hint(&state, &headers, &uri)
    {
        return response;
    }
    if let Err(response) = requested_frame_hint(&uri) {
        return response;
    }
    if !is_contract_id(&capture_id) {
        return bad_request("Invalid capture id.").into_response();
    }
    let Some(png) = state.captures.preview(&capture_id) else {
        return AppError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::BadRequest,
            "Capture not found.",
            false,
        )
        .into_response();
    };

    let mut response = Response::new(Body::from(png));
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("image/png"));
    response
}

async fn labels_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<LabelsRequest>,
) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }
    if request.schema_version != RUNTIME_API_SCHEMA_VERSION {
        return bad_request("Unsupported schema version.").into_response();
    }
    if !is_contract_id(&request.session_id) || !is_contract_uuid(&request.idempotency_key) {
        return bad_request("Invalid labels request.").into_response();
    }
    if request.updates.is_empty() && request.dedup_updates.is_empty() {
        return bad_request("Invalid labels request.").into_response();
    }
    if let Err(response) = ensure_active_session(&state, &request.session_id) {
        return response;
    }

    let store = (!state.config.private_config().is_placeholder())
        .then(|| PrivateArtifactStore::new(state.config.private_config()));
    let apply = LabelApplyRequest {
        session_id: request.session_id.clone(),
        idempotency_key: request.idempotency_key,
        updates: request.updates,
        dedup_updates: request.dedup_updates,
    };
    let outcome = match state.labels.apply(
        apply,
        |capture_id| {
            state
                .captures
                .is_labelable_capture(&request.session_id, capture_id)
        },
        store.as_ref(),
    ) {
        Ok(outcome) => outcome,
        Err(LabelStoreError::BackendUnavailable) => {
            return backend_error(BackendError::BackendUnavailable).into_response();
        }
        Err(LabelStoreError::Conflict(conflicts)) => LabelApplyOutcome {
            applied: false,
            label_revision: state.labels.snapshot().label_revision,
            conflicts,
        },
    };
    publish_label_event(&state, outcome.label_revision, outcome.applied);

    let mut response = Json(LabelsResponse::from(outcome)).into_response();
    apply_runtime_headers(response.headers_mut(), Some(ALLOWED_ORIGIN));
    response
}

async fn labels_snapshot(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    if let Err(response) = authenticate_runtime_request(&state, &headers, &uri) {
        return response;
    }

    let mut response = Json(LabelsSnapshotResponse::from(state.labels.snapshot())).into_response();
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
    let status = project_active_capture(&state, status);

    let ws_events = state.ws_events.clone();
    let sanitizer = state.config.private_config().public_sanitizer();
    let label_revision = state.labels.snapshot().label_revision;
    let validation_status = state.validation_status_snapshot();
    let mut response = ws
        .on_upgrade(move |socket| {
            serve_event_socket(
                socket,
                ws_events,
                sanitizer,
                status,
                label_revision,
                validation_status,
            )
        })
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
    authenticate_runtime_request_allowing_query(state, headers, uri, is_frame_hint_query)
}

fn authenticate_runtime_request_allowing_query(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    allowed_query: fn(&str) -> bool,
) -> Result<(), Response> {
    if uri.query().is_some_and(allowed_query) {
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

fn is_capture_recent_query(query: &str) -> bool {
    if query.is_empty() {
        return false;
    }
    let mut seen_cursor = false;
    let mut seen_limit = false;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            return false;
        };
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        match key {
            "cursor" if !seen_cursor => seen_cursor = true,
            "limit" if !seen_limit => seen_limit = true,
            _ => return false,
        }
    }
    seen_cursor || seen_limit
}

fn capture_recent_window(uri: &Uri) -> Result<(usize, usize), Response> {
    let Some(query) = uri.query() else {
        return Ok((0, DEFAULT_CAPTURE_LIMIT));
    };
    if !is_capture_recent_query(query) {
        return Err(bad_request("Invalid capture pagination.").into_response());
    }

    let mut offset = 0;
    let mut limit = DEFAULT_CAPTURE_LIMIT;
    for pair in query.split('&') {
        let (key, value) = pair
            .split_once('=')
            .expect("capture recent query pair was validated");
        let parsed = value
            .parse::<usize>()
            .ok()
            .filter(|value| *value as u128 <= JSON_SAFE_U64_MAX as u128)
            .ok_or_else(|| bad_request("Invalid capture pagination.").into_response())?;
        match key {
            "cursor" => offset = parsed,
            "limit" => limit = parsed.clamp(1, MAX_CAPTURE_LIMIT),
            _ => unreachable!("capture recent query key was validated"),
        }
    }
    Ok((offset, limit))
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

fn is_contract_id(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.len() <= 128
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
}

fn is_contract_uuid(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
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
    state.captures.reset_session(&stopped.session_id);
    state.labels.reset();
    state.validation.reset();
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
        state.captures.active_job_id(&boundary.session_id),
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

fn publish_capture_event(state: &AppState, session_id: &str, view: &CaptureJobView) {
    let sanitizer = state.config.private_config().public_sanitizer();
    let _ = state.ws_events.publish_capture(
        session_id,
        view.job_id.clone(),
        view.status.as_str(),
        view.capture_id.clone(),
        &sanitizer,
    );
}

fn publish_label_event(state: &AppState, label_revision: u64, applied: bool) {
    let Ok(session_id) = active_session_id(state) else {
        return;
    };
    let sanitizer = state_sanitizer(state);
    let _ = state
        .ws_events
        .publish_label(&session_id, label_revision, applied, &sanitizer);
}

fn publish_validation_event(state: &AppState, status: PublicValidationStatus) {
    let Ok(session_id) = active_session_id(state) else {
        return;
    };
    let sanitizer = state_sanitizer(state);
    let _ = state
        .ws_events
        .publish_validation(&session_id, status, &sanitizer);
}

fn state_sanitizer(state: &AppState) -> PublicSanitizer {
    state.config.private_config().public_sanitizer()
}

fn project_active_capture(
    state: &AppState,
    mut status: crate::backend::RunStatus,
) -> crate::backend::RunStatus {
    if status.active_capture_job_id.is_none() {
        status.active_capture_job_id = state.captures.active_job_id(&status.session_id);
    }
    status
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
pub struct ValidationStatusResponse {
    pub schema_version: u16,
    pub status: crate::validation_status::ValidationRunStatus,
    pub command_class: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub summary: String,
    pub issue_summaries: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTriggerRequest {
    pub schema_version: u16,
    pub session_id: String,
    pub idempotency_key: String,
    pub observed_preview_frame: u64,
    pub reason: CaptureReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureReason {
    OperatorMark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureTriggerResponse {
    pub schema_version: u16,
    pub job_id: String,
    pub status: CaptureStatus,
    pub requested_frame: u64,
    pub scheduled_frame: u64,
}

impl From<CaptureJobView> for CaptureTriggerResponse {
    fn from(view: CaptureJobView) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            job_id: view.job_id,
            status: view.status,
            requested_frame: view.requested_frame,
            scheduled_frame: view.scheduled_frame,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureJobResponse {
    pub schema_version: u16,
    pub job_id: String,
    pub status: CaptureStatus,
    pub requested_frame: u64,
    pub scheduled_frame: u64,
    pub captured_frame: Option<u64>,
    pub capture_id: Option<String>,
    pub labelable: bool,
    pub has_preview: bool,
    pub error: Option<ErrorObject>,
}

impl From<CaptureJobView> for CaptureJobResponse {
    fn from(view: CaptureJobView) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            job_id: view.job_id,
            status: view.status,
            requested_frame: view.requested_frame,
            scheduled_frame: view.scheduled_frame,
            captured_frame: view.captured_frame,
            capture_id: view.capture_id,
            labelable: view.labelable,
            has_preview: view.has_preview,
            error: view.error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureRecentResponse {
    pub schema_version: u16,
    pub captures: Vec<CaptureSummaryResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureSummaryResponse {
    pub capture_id: String,
    pub frame: u64,
    pub status: CaptureStatus,
    pub labelable: bool,
    pub has_preview: bool,
    pub labels: Vec<String>,
    pub created_at: &'static str,
}

impl From<CaptureSummaryView> for CaptureSummaryResponse {
    fn from(view: CaptureSummaryView) -> Self {
        Self {
            capture_id: view.capture_id,
            frame: view.frame,
            status: view.status,
            labelable: view.labelable,
            has_preview: view.has_preview,
            labels: view.labels,
            created_at: view.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureDetailResponse {
    pub schema_version: u16,
    pub capture_id: String,
    pub frame: u64,
    pub status: CaptureStatus,
    pub labelable: bool,
    pub preview_image_url: String,
    pub privileged_features_available: bool,
    pub labels: Vec<String>,
    pub sanitized_provenance: SanitizedProvenance,
}

impl From<CaptureDetailView> for CaptureDetailResponse {
    fn from(view: CaptureDetailView) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            capture_id: view.capture_id,
            frame: view.frame,
            status: view.status,
            labelable: view.labelable,
            preview_image_url: view.preview_image_url,
            privileged_features_available: view.privileged_features_available,
            labels: view.labels,
            sanitized_provenance: view.sanitized_provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SanitizedProvenance {
    pub capture_source: &'static str,
    pub layout_hash: &'static str,
    pub capture_spec_hash: &'static str,
    pub map_hash: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelsRequest {
    pub schema_version: u16,
    pub session_id: String,
    pub idempotency_key: String,
    pub updates: Vec<LabelUpdate>,
    #[serde(default)]
    pub dedup_updates: Vec<crate::labels::DedupUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LabelsResponse {
    pub schema_version: u16,
    pub applied: bool,
    pub label_revision: u64,
    pub conflicts: Vec<ErrorObject>,
}

impl From<LabelApplyOutcome> for LabelsResponse {
    fn from(outcome: LabelApplyOutcome) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            applied: outcome.applied,
            label_revision: outcome.label_revision,
            conflicts: outcome
                .conflicts
                .into_iter()
                .map(ErrorObject::from)
                .collect(),
        }
    }
}

impl From<LabelConflict> for ErrorObject {
    fn from(conflict: LabelConflict) -> Self {
        let code = match conflict.kind {
            LabelConflictKind::LabelConflict => ErrorCode::LabelConflict,
            LabelConflictKind::BadRequest => ErrorCode::BadRequest,
        };
        Self {
            code,
            message: conflict.message.to_string(),
            retryable: conflict.retryable,
            details: json!({}),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LabelsSnapshotResponse {
    pub schema_version: u16,
    pub label_revision: u64,
    pub target_labels: crate::labels::LabelTargetSnapshot,
    pub status_labels: Vec<crate::labels::StatusLabelSnapshot>,
    pub dedup_groups: Vec<crate::labels::DedupGroup>,
}

impl From<LabelSnapshot> for LabelsSnapshotResponse {
    fn from(snapshot: LabelSnapshot) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            label_revision: snapshot.label_revision,
            target_labels: snapshot.target_labels,
            status_labels: snapshot.status_labels,
            dedup_groups: snapshot.dedup_groups,
        }
    }
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

impl From<PublicValidationStatus> for ValidationStatusResponse {
    fn from(status: PublicValidationStatus) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            status: status.status,
            command_class: status.command_class,
            started_at: status.started_at,
            completed_at: status.completed_at,
            summary: status.summary,
            issue_summaries: status.issue_summaries,
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

fn apply_no_store_headers(headers: &mut HeaderMap) {
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
}

fn apply_runtime_headers(headers: &mut HeaderMap, origin: Option<&'static str>) {
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
