use crate::{
    artifacts::{
        CaptureSummary as PrivateCaptureSummary, PrivateArtifactStore, RecentCapturesFile,
    },
    auth::{
        AuthError, AuthState, RuntimeAuthContext, expired_session_cookie_header,
        session_cookie_header, validate_runtime_headers, validate_runtime_request,
    },
    backend::{
        BackendCapabilities, BackendError, BackendMode, BridgeBackend, CaptureJob,
        CaptureJobStatus, FramePreview, RealBackend, StopReason, SyntheticBackend,
    },
    config::{DeploymentProfile, ServiceConfig},
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
    body::{Body, to_bytes},
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri,
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE, HOST, PRAGMA, SET_COOKIE,
            VARY,
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
    fs, io,
    path::{Path as StdPath, PathBuf},
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
    runtime_session: Arc<Mutex<Option<ActiveRuntimeSession>>>,
    captures: CaptureState,
    labels: LabelState,
    validation: ValidationStatusState,
    frame_previews: FramePreviewState,
    ws_events: WsEventState,
    ws_input: WsInputState,
    play: crate::play::PlayController,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRuntimeSession {
    session_id: String,
    capabilities: BackendCapabilities,
}

impl AppState {
    pub fn from_config(config: ServiceConfig) -> Self {
        let backend: Arc<dyn BridgeBackend> = match config.backend_mode() {
            BackendMode::Synthetic => Arc::new(SyntheticBackend::with_private_config(
                config.private_config().clone(),
            )),
            BackendMode::Real => Arc::new(RealBackend::new(
                config.private_config().clone(),
                config
                    .private_config()
                    .real_runtime_config()
                    .expect("real backend config is validated before AppState construction")
                    .clone(),
            )),
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
            play: crate::play::PlayController::new(),
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
            play: crate::play::PlayController::new(),
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
            play: crate::play::PlayController::new(),
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
            source: CaptureSource::Synthetic,
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
            features: None,
            sanitized_provenance: synthetic_provenance(),
        };

        if input.observed_preview_frame < input.current_frame {
            job.status = CaptureStatus::Failed;
            job.error = Some(frame_stale_error());
        }

        inner.idempotency.insert(key, job_id.clone());
        let view = job.view();
        inner.jobs.insert(job_id, job);

        if view.status == CaptureStatus::Failed {
            return Ok(view);
        }
        Ok(view)
    }

    fn trigger_real_frame_stale(
        &self,
        input: RealFrameStaleInput,
    ) -> Result<CaptureJobView, CaptureTriggerError> {
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
        let job_id = format!("real-capture-job-stale-{:04}", inner.next_job_seq);
        let job = CaptureJobRecord {
            job_id: job_id.clone(),
            session_id: input.session_id.clone(),
            source: CaptureSource::Real,
            status: CaptureStatus::Failed,
            requested_frame: input.observed_preview_frame,
            scheduled_frame: input.current_frame,
            captured_frame: None,
            capture_id: None,
            labelable: false,
            has_preview: false,
            error: Some(frame_stale_error()),
            preview_png: Vec::new(),
            durable: false,
            features: None,
            sanitized_provenance: real_provenance(),
        };

        inner.idempotency.insert(key, job_id.clone());
        let view = job.view();
        inner.jobs.insert(job_id, job);
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

    fn real_job_context(&self, job_id: &str) -> Result<(String, u64, u64), BackendError> {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let job = inner
            .jobs
            .get(job_id)
            .ok_or(BackendError::BackendUnavailable)?;
        Ok((
            job.session_id.clone(),
            job.requested_frame,
            job.scheduled_frame,
        ))
    }

    fn local_real_frame_stale_job(&self, job_id: &str) -> Option<CaptureJobView> {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let job = inner.jobs.get(job_id)?;
        let is_local_stale_real_failure = job.source == CaptureSource::Real
            && job.status == CaptureStatus::Failed
            && job
                .error
                .as_ref()
                .is_some_and(|error| error.code == ErrorCode::FrameStale);
        is_local_stale_real_failure.then(|| job.view())
    }

    fn upsert_real_job(
        &self,
        config: &ServiceConfig,
        session_id: &str,
        backend_job: CaptureJob,
        requested_frame: u64,
        scheduled_frame: u64,
    ) -> Result<CaptureJobView, BackendError> {
        let status = capture_status_from_backend(backend_job.status);
        let capture_id = backend_job.capture_id.clone();
        let public = backend_job.public.clone();
        if status == CaptureStatus::Completed && (capture_id.is_none() || public.is_none()) {
            return Err(BackendError::BackendUnavailable);
        }
        let labelable =
            status == CaptureStatus::Completed && capture_id.is_some() && public.is_some();
        let recent = if labelable {
            let capture_id = capture_id
                .as_ref()
                .ok_or(BackendError::BackendUnavailable)?
                .clone();
            let inner = self.inner.lock().expect("capture mutex poisoned");
            let mut captures = Vec::with_capacity(inner.capture_order.len() + 1);
            if !inner.captures.contains_key(&capture_id) {
                captures.push(PrivateCaptureSummary::new(
                    capture_id.clone(),
                    "1970-01-01T00:00:00Z",
                    status.as_str(),
                    true,
                ));
            }
            captures.extend(
                inner
                    .capture_order
                    .iter()
                    .filter_map(|existing_capture_id| {
                        let record = inner.captures.get(existing_capture_id)?;
                        let job = inner.jobs.get(&record.job_id)?;
                        Some(PrivateCaptureSummary::new(
                            existing_capture_id.clone(),
                            "1970-01-01T00:00:00Z",
                            job.status.as_str(),
                            job.labelable,
                        ))
                    }),
            );
            Some(RecentCapturesFile::new(captures))
        } else {
            None
        };

        if let Some(recent) = &recent
            && !config.private_config().is_placeholder()
        {
            PrivateArtifactStore::new(config.private_config())
                .write_recent_captures(recent)
                .map_err(|_| BackendError::BackendUnavailable)?;
        }

        let mut inner = self.inner.lock().expect("capture mutex poisoned");
        let record = inner
            .jobs
            .entry(backend_job.job_id.clone())
            .or_insert_with(|| CaptureJobRecord {
                job_id: backend_job.job_id.clone(),
                session_id: session_id.to_string(),
                source: CaptureSource::Real,
                status,
                requested_frame,
                scheduled_frame,
                captured_frame: None,
                capture_id: None,
                labelable: false,
                has_preview: false,
                error: None,
                preview_png: Vec::new(),
                durable: false,
                features: None,
                sanitized_provenance: real_provenance(),
            });
        record.status = status;
        record.capture_id = capture_id.clone();
        record.captured_frame = public
            .as_ref()
            .filter(|_| status == CaptureStatus::Completed)
            .map(|public| public.frame_counter);
        record.labelable = labelable;
        record.has_preview = public.as_ref().is_some_and(|public| public.has_preview);
        record.durable = labelable;
        record.features = public
            .as_ref()
            .filter(|public| public.features_available)
            .map(|_| Vec::new());
        if let Some(public) = &public {
            record.sanitized_provenance = SanitizedProvenance {
                capture_source: public.capture_source.clone(),
                layout_hash: public.layout_hash.clone(),
                capture_spec_hash: public.capture_spec_hash.clone(),
                map_hash: public.map_hash.clone(),
            };
        }
        record.error = (status == CaptureStatus::Failed).then(|| ErrorObject {
            code: ErrorCode::CaptureFailed,
            message: "Capture failed.".to_string(),
            retryable: true,
            details: json!({}),
        });
        let view = record.view();
        if labelable {
            let capture_id = capture_id.expect("labelable capture has capture id");
            if !inner.captures.contains_key(&capture_id) {
                inner.capture_order.push_front(capture_id.clone());
                inner.captures.insert(
                    capture_id,
                    CaptureRecord {
                        job_id: backend_job.job_id,
                    },
                );
            }
        }
        Ok(view)
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
            job.features = synthetic_capture_features(&capture_id, job.scheduled_frame, labelable);
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

    fn features(&self, capture_id: &str) -> Option<CaptureFeaturesView> {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let record = inner.captures.get(capture_id)?;
        let job = inner.jobs.get(&record.job_id)?;
        Some(CaptureFeaturesView {
            capture_id: capture_id.to_string(),
            available: job.features.is_some(),
            features: job.features.clone().unwrap_or_default(),
        })
    }

    fn preview(&self, capture_id: &str) -> Option<Vec<u8>> {
        let inner = self.inner.lock().expect("capture mutex poisoned");
        let record = inner.captures.get(capture_id)?;
        let job = inner.jobs.get(&record.job_id)?;
        if !job.has_preview || job.source == CaptureSource::Real {
            return None;
        }
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
    source: CaptureSource,
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
    features: Option<Vec<CaptureFeatureValue>>,
    sanitized_provenance: SanitizedProvenance,
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
            has_preview: self.has_preview,
            preview_image_url: self
                .has_preview
                .then(|| format!("/api/capture/{capture_id}/preview")),
            privileged_features_available: self.features.is_some(),
            labels: Vec::new(),
            sanitized_provenance: self.sanitized_provenance.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureSource {
    Synthetic,
    Real,
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
    has_preview: bool,
    preview_image_url: Option<String>,
    privileged_features_available: bool,
    labels: Vec<String>,
    sanitized_provenance: SanitizedProvenance,
}

#[derive(Debug, Clone, PartialEq)]
struct CaptureFeaturesView {
    capture_id: String,
    available: bool,
    features: Vec<CaptureFeatureValue>,
}

#[derive(Debug, Clone, PartialEq)]
struct CaptureFeatureValue {
    name: String,
    value: f64,
}

#[derive(Debug, Clone)]
struct CaptureTriggerInput {
    session_id: String,
    idempotency_key: String,
    observed_preview_frame: u64,
    current_frame: u64,
    preview_png: Vec<u8>,
}

#[derive(Debug, Clone)]
struct RealFrameStaleInput {
    session_id: String,
    idempotency_key: String,
    observed_preview_frame: u64,
    current_frame: u64,
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

fn capture_status_from_backend(status: CaptureJobStatus) -> CaptureStatus {
    match status {
        CaptureJobStatus::Pending => CaptureStatus::Requested,
        CaptureJobStatus::Running => CaptureStatus::Capturing,
        CaptureJobStatus::Completed => CaptureStatus::Completed,
        CaptureJobStatus::Failed => CaptureStatus::Failed,
    }
}

fn frame_stale_error() -> ErrorObject {
    ErrorObject {
        code: ErrorCode::FrameStale,
        message: "Capture failed.".to_string(),
        retryable: true,
        details: json!({}),
    }
}

fn synthetic_capture_features(
    capture_id: &str,
    scheduled_frame: u64,
    labelable: bool,
) -> Option<Vec<CaptureFeatureValue>> {
    if !labelable {
        return None;
    }
    let capture_bucket = capture_id
        .rsplit_once('-')
        .and_then(|(_, suffix)| suffix.parse::<u64>().ok())
        .unwrap_or(0);
    Some(vec![
        CaptureFeatureValue {
            name: "screen.room_id".to_string(),
            value: (capture_bucket % 8) as f64,
        },
        CaptureFeatureValue {
            name: "player.health".to_string(),
            value: ((scheduled_frame % 10) as f64) / 10.0,
        },
        CaptureFeatureValue {
            name: "encounter.phase".to_string(),
            value: ((scheduled_frame / 2) % 4) as f64,
        },
    ])
}

fn synthetic_provenance() -> SanitizedProvenance {
    SanitizedProvenance {
        capture_source: "synthetic".to_string(),
        layout_hash: "sha256:synthetic-layout-v1".to_string(),
        capture_spec_hash: "sha256:synthetic-capture-v1".to_string(),
        map_hash: "sha256:synthetic-map-v1".to_string(),
    }
}

fn real_provenance() -> SanitizedProvenance {
    SanitizedProvenance {
        capture_source: "hypervisor".to_string(),
        layout_hash: "private-layout-hash".to_string(),
        capture_spec_hash: "private-capture-spec".to_string(),
        map_hash: "private-feature-map".to_string(),
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
        .route("/api/run/play", post(play_run).fallback(method_not_allowed))
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
            "/api/capture/{capture_id}/features",
            get(capture_features).fallback(method_not_allowed),
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
        .route(
            "/ws/frames",
            get(frames_ws_handshake).fallback(method_not_allowed),
        )
        .fallback(static_or_not_found)
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
    body: Body,
) -> Response {
    let auth_context =
        match validate_runtime_request(&headers, &uri, state.config.deployment_security()) {
            Ok(context) => context,
            Err(error) => return auth_error(error).into_response(),
        };

    let request = match parse_start_session_request(body).await {
        Ok(request) => request,
        Err(response) => return response,
    };

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

    let requested_capabilities = match requested_capabilities(&request.requested_capabilities) {
        Ok(capabilities) => capabilities,
        Err(response) => return response,
    };
    let granted_capabilities =
        grant_capabilities(state.backend.capabilities(), requested_capabilities);

    let operator_session = match state.auth.start_session(state.config.private_config()) {
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
            requested_capabilities: granted_capabilities,
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
        .expect("runtime session mutex poisoned") = Some(ActiveRuntimeSession {
        session_id,
        capabilities: granted_capabilities,
    });
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
        capabilities: granted_capabilities,
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie_header(
            &operator_session,
            auth_context.cookie_secure,
        ))
        .expect("session cookie contains only valid header characters"),
    );
    response
}

async fn parse_start_session_request(body: Body) -> Result<StartSessionRequest, Response> {
    let bytes = to_bytes(body, 16 * 1024).await.map_err(|_| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::BadRequest,
            "Invalid session start request.",
            false,
        )
        .into_response()
    })?;
    serde_json::from_slice::<StartSessionRequest>(&bytes).map_err(|_| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            ErrorCode::BadRequest,
            "Invalid session start request.",
            false,
        )
        .into_response()
    })
}

async fn session_status(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };

    let Some(active_session) = state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .clone()
    else {
        return auth_error(AuthError::MissingSession).into_response();
    };

    let session_id = active_session.session_id.clone();
    let status = match state.backend.status(session_id.clone()) {
        Ok(status) => status,
        Err(error) => {
            return backend_error_clearing_session(
                &state,
                &headers,
                &auth_context,
                &session_id,
                error,
            );
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn stop_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<StopSessionRequest>,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
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
        Err(error) => {
            return backend_error_clearing_session(
                &state,
                &headers,
                &auth_context,
                &request.session_id,
                error,
            );
        }
    };
    publish_stopped_event(&state, &stopped);

    clear_runtime_session_state(&state, &stopped.session_id);
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&expired_session_cookie_header(auth_context.cookie_secure))
            .expect("expired session cookie contains only valid header characters"),
    );
    response
}

async fn run_status(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let session_id = match active_session_id(&state) {
        Ok(session_id) => session_id,
        Err(response) => return response,
    };

    let status = match state.backend.status(session_id.clone()) {
        Ok(status) => status,
        Err(error) => {
            return backend_error_clearing_session(
                &state,
                &headers,
                &auth_context,
                &session_id,
                error,
            );
        }
    };
    if status.session_id != session_id {
        return auth_error(AuthError::BadSession).into_response();
    }
    let status = project_active_capture(&state, status);

    let mut response = Json(RunStatusResponse::from(status)).into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn validation_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if let Err(response) = active_session_id(&state) {
        return response;
    }

    let mut response = Json(ValidationStatusResponse::from(
        state.validation_status_snapshot(),
    ))
    .into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn frame_current(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn frame_current_image(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let auth_context =
        match authenticate_runtime_request_allowing_frame_hint(&state, &headers, &uri) {
            Ok(context) => context,
            Err(response) => return response,
        };
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
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
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
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
    if state.backend.mode() == BackendMode::Real {
        if !status.capabilities.capture {
            return backend_error(BackendError::BackendUnavailable).into_response();
        }
        if request.observed_preview_frame > status.current_frame {
            return bad_request("Preview frame unavailable.").into_response();
        }
        if request.observed_preview_frame < status.current_frame {
            let view = match state
                .captures
                .trigger_real_frame_stale(RealFrameStaleInput {
                    session_id: request.session_id.clone(),
                    idempotency_key: request.idempotency_key,
                    observed_preview_frame: request.observed_preview_frame,
                    current_frame: status.current_frame,
                }) {
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
            apply_runtime_headers(response.headers_mut(), &auth_context);
            return response;
        }
        let backend_job = match state
            .backend
            .trigger_capture(crate::backend::CaptureRequest {
                session_id: request.session_id.clone(),
                idempotency_key: request.idempotency_key,
            }) {
            Ok(job) => job,
            Err(error) => return backend_error(error).into_response(),
        };
        let view = match state.captures.upsert_real_job(
            &state.config,
            &request.session_id,
            backend_job,
            request.observed_preview_frame,
            status.current_frame,
        ) {
            Ok(view) => view,
            Err(error) => return backend_error(error).into_response(),
        };
        publish_capture_event(&state, &request.session_id, &view);

        let mut response = Json(CaptureTriggerResponse::from(view)).into_response();
        apply_runtime_headers(response.headers_mut(), &auth_context);
        return response;
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn capture_job_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Path(job_id): Path<String>,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if !is_contract_id(&job_id) {
        return bad_request("Invalid capture job.").into_response();
    }
    let view = if state.backend.mode() == BackendMode::Real {
        if let Some(view) = state.captures.local_real_frame_stale_job(&job_id) {
            publish_capture_event(&state, &view.session_id, &view);

            let mut response = Json(CaptureJobResponse::from(view)).into_response();
            apply_runtime_headers(response.headers_mut(), &auth_context);
            return response;
        }
        let (session_id, requested_frame, scheduled_frame) =
            match state.captures.real_job_context(&job_id) {
                Ok(context) => context,
                Err(error) => return backend_error(error).into_response(),
            };
        let backend_job = match state.backend.capture_job(job_id) {
            Ok(job) => job,
            Err(error) => return backend_error(error).into_response(),
        };
        match state.captures.upsert_real_job(
            &state.config,
            &session_id,
            backend_job,
            requested_frame,
            scheduled_frame,
        ) {
            Ok(view) => view,
            Err(error) => return backend_error(error).into_response(),
        }
    } else {
        match state.captures.job(&state.config, &job_id) {
            Ok(view) => view,
            Err(error) => return backend_error(error).into_response(),
        }
    };
    publish_capture_event(&state, &view.session_id, &view);

    let mut response = Json(CaptureJobResponse::from(view)).into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn capture_recent(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let auth_context = match authenticate_runtime_request_allowing_query(
        &state,
        &headers,
        &uri,
        is_capture_recent_query,
    ) {
        Ok(context) => context,
        Err(response) => return response,
    };
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn capture_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Path(capture_id): Path<String>,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn capture_features(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Path(capture_id): Path<String>,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if !active_session_capabilities(&state)
        .is_some_and(|capabilities| capabilities.privileged_features)
    {
        return AppError::new(
            StatusCode::FORBIDDEN,
            ErrorCode::AuthRejected,
            "Privileged feature access is not granted.",
            false,
        )
        .into_response();
    }
    if !is_contract_id(&capture_id) {
        return bad_request("Invalid capture id.").into_response();
    }
    let Some(view) = state.captures.features(&capture_id) else {
        return AppError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::BadRequest,
            "Capture not found.",
            false,
        )
        .into_response();
    };

    let mut response = Json(CaptureFeaturesResponse::from(view)).into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn capture_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Path(capture_id): Path<String>,
) -> Response {
    let auth_context =
        match authenticate_runtime_request_allowing_frame_hint(&state, &headers, &uri) {
            Ok(context) => context,
            Err(response) => return response,
        };
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
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
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn labels_snapshot(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };

    let mut response = Json(LabelsSnapshotResponse::from(state.labels.snapshot())).into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
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
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };

    let Some(active_session) = state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .clone()
    else {
        return auth_error(AuthError::MissingSession).into_response();
    };
    let session_id = active_session.session_id;

    let backend = state.backend.clone();
    let ws_input = state.ws_input.clone();
    let private_config = state.config.private_config().clone();
    let mut response = ws
        .on_upgrade(move |socket| {
            serve_input_socket(socket, backend, ws_input, session_id, private_config)
        })
        .into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn events_ws_handshake(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };

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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

/// `GET /ws/frames` — live frame stream for continuous play. Binary messages
/// `[u64 frame_counter LE][PNG bytes]`; the client renders only frames newer than
/// the last displayed one. A `watch` receiver means each connection always sees
/// the latest frame; a slow/late subscriber simply misses intermediate frames.
async fn frames_ws_handshake(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if let Err(response) = active_session_id(&state) {
        return response;
    }
    let frames = state.play.subscribe();
    let mut response = ws
        .on_upgrade(move |socket| serve_frames_socket(socket, frames))
        .into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

async fn serve_frames_socket(
    mut socket: WebSocket,
    mut frames: tokio::sync::watch::Receiver<crate::play::FrameSlot>,
) {
    // Send the latest frame immediately (if a run is already producing).
    let current = frames.borrow_and_update().clone();
    if let Some(frame) = current
        && socket
            .send(Message::Binary(frame.as_ref().clone().into()))
            .await
            .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            changed = frames.changed() => {
                if changed.is_err() {
                    break;
                }
                let current = frames.borrow_and_update().clone();
                if let Some(frame) = current
                    && socket
                        .send(Message::Binary(frame.as_ref().clone().into()))
                        .await
                        .is_err()
                {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    _ => {}
                }
            }
        }
    }
}

fn run_state_transition(
    state: AppState,
    headers: HeaderMap,
    uri: Uri,
    request: SessionOnlyRequest,
    transition: RunTransition,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if request.schema_version != RUNTIME_API_SCHEMA_VERSION {
        return bad_request("Unsupported schema version.").into_response();
    }
    if let Err(response) = ensure_active_session(&state, &request.session_id) {
        return response;
    }

    // Pause (and Resume, which single-steps) must first halt any continuous-play
    // loop so it stops issuing Runs before we change the run state. `stop` joins
    // the loop, briefly parking this async worker for its in-flight iteration
    // (~1-2 frames) — bounded, and consistent with the backend RPCs that already
    // block synchronously inside these handlers.
    state.play.stop(&request.session_id);

    if transition == RunTransition::Resume
        && flush_pending_input(&state, &request.session_id).is_err()
    {
        return backend_error(BackendError::BackendUnavailable).into_response();
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
        && state.backend.mode() == BackendMode::Synthetic
        && flush_pending_input(&state, &boundary.session_id).is_err()
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
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

/// `POST /api/run/play` — enter continuous play. Fire-and-forget: transition the
/// session to `Playing`, spawn the dedicated Play loop thread, and return
/// immediately (the loop streams frames over `/ws/frames`).
async fn play_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<SessionOnlyRequest>,
) -> Response {
    let auth_context = match authenticate_runtime_request(&state, &headers, &uri) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if request.schema_version != RUNTIME_API_SCHEMA_VERSION {
        return bad_request("Unsupported schema version.").into_response();
    }
    if let Err(response) = ensure_active_session(&state, &request.session_id) {
        return response;
    }

    let boundary = match state.backend.play_start(request.session_id.clone()) {
        Ok(boundary) => boundary,
        Err(error) => return backend_error(error).into_response(),
    };
    publish_run_boundary_event(&state, &boundary);

    // Stop any prior loop BEFORE spawning the new one so two loops never run
    // concurrently for the session. (`register` also stops, but only after the
    // new thread has already started calling `play_step`.)
    state.play.stop_any();

    // Dedicated OS thread: not the axum runtime (would block a worker for
    // minutes) and not the serialized worker thread (must service Stop/Status).
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let loop_state = state.clone();
    let loop_session = request.session_id.clone();
    let loop_stop = stop.clone();
    let join = std::thread::Builder::new()
        .name("play-loop".to_string())
        .spawn(move || play_loop(loop_state, loop_session, loop_stop))
        .expect("spawn play loop thread");
    state.play.register(request.session_id.clone(), stop, join);

    let mut response = Json(RunStateResponse {
        schema_version: RUNTIME_API_SCHEMA_VERSION,
        state: boundary.state,
        current_frame: boundary.current_frame,
    })
    .into_response();
    apply_runtime_headers(response.headers_mut(), &auth_context);
    response
}

/// The continuous-play loop body (runs on a dedicated thread). Per frame: flush
/// buffered input (scheduled for upcoming frames), advance exactly one frame, push
/// it to `/ws/frames`, and publish `run_updated`. Exits when the stop flag is set
/// (Pause/Stop) or on the first backend error (fault).
fn play_loop(
    state: AppState,
    session_id: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    let frames = state.play.frames_sender();
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        // Self-terminate when the operator's session TTL lapses mid-Play. A
        // passive `/ws/frames` viewer sends no authenticated request, so nothing
        // else detects expiry: without this the loop would pin a core forever and
        // keep streaming pixels to a now-unauthenticated client. Blank the last
        // frame and drop the handle on the way out.
        if !state.auth.active_session_live() {
            let _ = frames.send(None);
            state.play.deregister(&session_id);
            break;
        }
        // Inject inputs the client buffered for upcoming frames (best-effort:
        // a scheduling hiccup on one frame must not kill the loop).
        let _ = flush_pending_input(&state, &session_id);

        match state.backend.play_step(session_id.clone()) {
            Ok(step) => {
                let _ = frames.send(Some(crate::play::frame_message(
                    step.frame,
                    &step.png_bytes,
                )));
                publish_run_boundary_event(
                    &state,
                    &crate::backend::RunBoundary {
                        session_id: session_id.clone(),
                        state: crate::backend::SessionState::Playing,
                        current_frame: step.frame,
                        preview_stale: false,
                    },
                );
            }
            Err(_) => {
                // Fault or the session went away. Surface the terminal state (if
                // still resolvable) so the UI flips out of Playing.
                if let Ok(status) = state.backend.status(session_id.clone()) {
                    publish_run_boundary_event(
                        &state,
                        &crate::backend::RunBoundary {
                            session_id: session_id.clone(),
                            state: status.state,
                            current_frame: status.current_frame,
                            preview_stale: status.preview_stale,
                        },
                    );
                }
                // Clear the last frame so a late `/ws/frames` subscriber is not
                // handed a stale framebuffer, and drop the now-exited handle
                // (self-exit: deregister rather than stop_any to avoid self-join).
                let _ = frames.send(None);
                state.play.deregister(&session_id);
                break;
            }
        }
    }
}

fn flush_pending_input(
    state: &AppState,
    session_id: &str,
) -> Result<Vec<crate::input::InputScheduleOutcome>, crate::input::InputSchedulerError> {
    if state.config.private_config().is_placeholder() {
        let mut rejection_sink = crate::input::NoopInputRejectionSink;
        state
            .ws_input
            .flush_pending(state.backend.as_ref(), session_id, &mut rejection_sink)
    } else {
        let mut rejection_sink = PrivateArtifactStore::new(state.config.private_config());
        state
            .ws_input
            .flush_pending(state.backend.as_ref(), session_id, &mut rejection_sink)
    }
}

fn authenticate_runtime_request(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<RuntimeAuthContext, Response> {
    let auth_context =
        match validate_runtime_request(headers, uri, state.config.deployment_security()) {
            Ok(context) => context,
            Err(error) => return Err(auth_error(error).into_response()),
        };

    match state.auth.authenticate_headers(headers) {
        Ok(()) => Ok(auth_context),
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
) -> Result<RuntimeAuthContext, Response> {
    authenticate_runtime_request_allowing_query(state, headers, uri, is_frame_hint_query)
}

fn authenticate_runtime_request_allowing_query(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
    allowed_query: fn(&str) -> bool,
) -> Result<RuntimeAuthContext, Response> {
    if uri.query().is_some_and(allowed_query) {
        let auth_context =
            match validate_runtime_headers(headers, state.config.deployment_security()) {
                Ok(context) => context,
                Err(error) => return Err(auth_error(error).into_response()),
            };
        match state.auth.authenticate_headers(headers) {
            Ok(()) => Ok(auth_context),
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
    // Stop any continuous-play loop before tearing down the slot, so it does not
    // issue a Run against a destroyed lease (Stop / SessionReplaced / fault / TTL).
    state.play.stop_any();

    let active_session = state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .clone();
    let Some(active_session) = active_session else {
        return Ok(());
    };

    match state
        .backend
        .stop_session(active_session.session_id.clone(), reason)
    {
        Ok(stopped) => {
            publish_stopped_event(state, &stopped);
            clear_runtime_session_state(state, &stopped.session_id);
            Ok(())
        }
        Err(error) => {
            clear_runtime_session_state(state, &active_session.session_id);
            Err(error)
        }
    }
}

fn clear_runtime_session_state(state: &AppState, session_id: &str) -> bool {
    let cleared = {
        let mut active = state
            .runtime_session
            .lock()
            .expect("runtime session mutex poisoned");
        if active
            .as_ref()
            .is_some_and(|active| active.session_id == session_id)
        {
            *active = None;
            true
        } else {
            false
        }
    };
    if cleared {
        state.frame_previews.reset_session(session_id);
        state.captures.reset_session(session_id);
        state.labels.reset();
        state.validation.reset();
        state.ws_events.reset_session(session_id);
        state.ws_input.reset_session(session_id);
    }
    cleared
}

fn backend_error_clearing_session(
    state: &AppState,
    headers: &HeaderMap,
    auth_context: &RuntimeAuthContext,
    session_id: &str,
    error: BackendError,
) -> Response {
    if clear_runtime_session_state(state, session_id) {
        let _ = state.auth.clear_session_headers(headers);
        let mut response = backend_error(error).into_response();
        response.headers_mut().insert(
            SET_COOKIE,
            HeaderValue::from_str(&expired_session_cookie_header(auth_context.cookie_secure))
                .expect("expired session cookie contains only valid header characters"),
        );
        response
    } else {
        backend_error(error).into_response()
    }
}

fn publish_run_boundary_event(state: &AppState, boundary: &crate::backend::RunBoundary) {
    let sanitizer = state.config.private_config().public_sanitizer();
    let _ = state.ws_events.publish_boundary(
        boundary,
        state.backend.mode(),
        event_capabilities(state),
        state.captures.active_job_id(&boundary.session_id),
        &sanitizer,
    );
}

fn publish_stopped_event(state: &AppState, stopped: &crate::backend::StoppedSession) {
    let sanitizer = state.config.private_config().public_sanitizer();
    let _ = state.ws_events.publish_stopped(
        stopped,
        state.backend.mode(),
        event_capabilities(state),
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

fn requested_capabilities(requested: &[String]) -> Result<BackendCapabilities, Response> {
    let mut capabilities = BackendCapabilities {
        input: false,
        preview: false,
        capture: false,
        labels: false,
        privileged_features: false,
        validation_runner: false,
    };
    let mut seen = BTreeSet::new();
    for capability in requested {
        if !seen.insert(capability.as_str()) {
            return Err(bad_request("Invalid requested capabilities.").into_response());
        }
        match capability.as_str() {
            "input" => capabilities.input = true,
            "preview" => capabilities.preview = true,
            "capture" => capabilities.capture = true,
            "labels" => capabilities.labels = true,
            "privileged_features" => capabilities.privileged_features = true,
            "validation_runner" => capabilities.validation_runner = true,
            _ => return Err(bad_request("Invalid requested capabilities.").into_response()),
        }
    }
    Ok(capabilities)
}

fn grant_capabilities(
    supported: BackendCapabilities,
    requested: BackendCapabilities,
) -> BackendCapabilities {
    BackendCapabilities {
        input: supported.input && requested.input,
        preview: supported.preview && requested.preview,
        capture: supported.capture && requested.capture,
        labels: supported.labels && requested.labels,
        privileged_features: supported.privileged_features && requested.privileged_features,
        validation_runner: supported.validation_runner && requested.validation_runner,
    }
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
    if let Some(capabilities) = active_session_capabilities(state) {
        status.capabilities = capabilities;
    }
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
        .as_ref()
        .map(|session| session.session_id.clone())
        .ok_or_else(|| auth_error(AuthError::MissingSession).into_response())
}

fn active_session_capabilities(state: &AppState) -> Option<BackendCapabilities> {
    state
        .runtime_session
        .lock()
        .expect("runtime session mutex poisoned")
        .as_ref()
        .map(|session| session.capabilities)
}

fn event_capabilities(state: &AppState) -> BackendCapabilities {
    active_session_capabilities(state).unwrap_or_else(|| state.backend.capabilities())
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

async fn static_or_not_found(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return not_found().await.into_response();
    }
    if is_runtime_prefix(uri.path()) {
        return not_found().await.into_response();
    }

    let Some(root) = state.config.private_config().static_publish_root() else {
        return not_found().await.into_response();
    };
    let Some(profile) = state
        .config
        .deployment_security()
        .profile_for_host_header(headers.get(HOST).and_then(|value| value.to_str().ok()))
    else {
        return not_found().await.into_response();
    };

    match static_file_response(root, uri.path(), method == Method::HEAD, profile) {
        Ok(Some(response)) => response,
        Ok(None) => not_found().await.into_response(),
        Err(error) => error.into_response(),
    }
}

fn is_runtime_prefix(path: &str) -> bool {
    path == "/health"
        || path.starts_with("/health/")
        || path == "/api"
        || path.starts_with("/api/")
        || path == "/ws"
        || path.starts_with("/ws/")
}

fn static_file_response(
    root: &StdPath,
    request_path: &str,
    head_only: bool,
    profile: &DeploymentProfile,
) -> Result<Option<Response>, AppError> {
    let requested = static_relative_path(request_path)?;
    let path = match safe_static_file(root, &requested)? {
        Some(path) => Some(path),
        None if looks_like_asset_path(&requested) => None,
        None => safe_static_file(root, StdPath::new("index.html"))?,
    };
    let Some(path) = path else {
        return Ok(None);
    };
    let contents = fs::read(&path).map_err(|_| not_found_error())?;
    let body = if head_only {
        Body::empty()
    } else {
        Body::from(contents)
    };
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, static_content_type(&path))
        .body(body)
        .map_err(|_| not_found_error())?;
    apply_static_headers(response.headers_mut(), profile);
    Ok(Some(response))
}

fn static_relative_path(request_path: &str) -> Result<PathBuf, AppError> {
    if request_path.contains('%') || request_path.contains('\\') || request_path.contains('\0') {
        return Err(not_found_error());
    }

    let mut relative = PathBuf::new();
    for segment in request_path.trim_start_matches('/').split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == "."
            || segment == ".."
            || segment.starts_with('.')
            || segment.contains(':')
            || segment.ends_with(".map")
        {
            return Err(not_found_error());
        }
        relative.push(segment);
    }

    if relative.as_os_str().is_empty() {
        relative.push("index.html");
    }
    Ok(relative)
}

fn safe_static_file(root: &StdPath, relative_path: &StdPath) -> Result<Option<PathBuf>, AppError> {
    let mut current = root.to_path_buf();
    for component in relative_path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(not_found_error());
        };
        current.push(part);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(not_found_error()),
        };
        if metadata.file_type().is_symlink() {
            return Err(not_found_error());
        }
    }

    match fs::metadata(&current) {
        Ok(metadata) if metadata.is_file() => Ok(Some(current)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(not_found_error()),
    }
}

fn looks_like_asset_path(path: &StdPath) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains('.'))
}

fn static_content_type(path: &StdPath) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn not_found_error() -> AppError {
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
    pub has_preview: bool,
    pub preview_image_url: Option<String>,
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
            has_preview: view.has_preview,
            preview_image_url: view.preview_image_url,
            privileged_features_available: view.privileged_features_available,
            labels: view.labels,
            sanitized_provenance: view.sanitized_provenance,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaptureFeaturesResponse {
    pub schema_version: u16,
    pub capture_id: String,
    pub available: bool,
    pub features: Vec<CaptureFeatureResponse>,
}

impl From<CaptureFeaturesView> for CaptureFeaturesResponse {
    fn from(view: CaptureFeaturesView) -> Self {
        Self {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            capture_id: view.capture_id,
            available: view.available,
            features: view
                .features
                .into_iter()
                .map(CaptureFeatureResponse::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaptureFeatureResponse {
    pub name: String,
    pub value: f64,
}

impl From<CaptureFeatureValue> for CaptureFeatureResponse {
    fn from(feature: CaptureFeatureValue) -> Self {
        Self {
            name: feature.name,
            value: feature.value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SanitizedProvenance {
    pub capture_source: String,
    pub layout_hash: String,
    pub capture_spec_hash: String,
    pub map_hash: String,
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
            preview_stale: status.preview_stale,
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
    FrameUnavailable,
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

fn apply_static_headers(headers: &mut HeaderMap, profile: &DeploymentProfile) {
    apply_no_store_headers(headers);
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_str(&profile.static_csp())
            .expect("deployment profile CSP contains only header-safe values"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
}

fn apply_runtime_headers(headers: &mut HeaderMap, auth_context: &RuntimeAuthContext) {
    apply_no_store_headers(headers);
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, auth_context.origin.clone());
    headers.insert(VARY, HeaderValue::from_static("Origin"));
}

fn bad_request(message: &'static str) -> AppError {
    AppError::new(
        StatusCode::BAD_REQUEST,
        ErrorCode::BadRequest,
        message,
        false,
    )
}

fn backend_error(error: crate::backend::BackendError) -> AppError {
    match error {
        crate::backend::BackendError::CaptureInProgress => AppError::new(
            StatusCode::CONFLICT,
            ErrorCode::CaptureInProgress,
            "Capture already in progress.",
            false,
        ),
        crate::backend::BackendError::FrameUnavailable => AppError::new(
            StatusCode::NOT_FOUND,
            ErrorCode::FrameUnavailable,
            "Frame not available yet.",
            true,
        ),
        _ => AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::BackendUnavailable,
            "Backend unavailable.",
            true,
        ),
    }
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
        AuthError::PrivateConfig(_) => AppError::new(
            StatusCode::UNAUTHORIZED,
            ErrorCode::AuthRejected,
            "Authentication rejected.",
            false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::CaptureJobStatus;

    #[test]
    fn real_completed_job_without_public_projection_fails_closed() {
        let captures = CaptureState::new();
        let config = ServiceConfig::synthetic_for_addr("127.0.0.1:0".parse().unwrap());

        let error = captures
            .upsert_real_job(
                &config,
                "real-session-test",
                CaptureJob {
                    job_id: "real-capture-job-test".to_string(),
                    status: CaptureJobStatus::Completed,
                    capture_id: Some("real-capture-test".to_string()),
                    public: None,
                },
                12,
                12,
            )
            .expect_err("completed real jobs require public completion projection");

        assert!(matches!(error, BackendError::BackendUnavailable));
    }
}
