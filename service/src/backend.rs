use serde::{Deserialize, Serialize};
use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};
use thiserror::Error;

use crate::{
    api::RUNTIME_API_SCHEMA_VERSION,
    artifacts::{BridgeEventRow, PrivateArtifactStore, RunManifest},
    input::PadWord,
    private_config::BridgePrivateConfig,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendMode {
    Synthetic,
    Real,
}

impl BackendMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Synthetic => "synthetic",
            Self::Real => "real",
        }
    }
}

impl FromStr for BackendMode {
    type Err = BackendModeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "synthetic" => Ok(Self::Synthetic),
            "real" => Ok(Self::Real),
            _ => Err(BackendModeParseError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendModeParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BackendCapabilities {
    pub input: bool,
    pub preview: bool,
    pub capture: bool,
    pub labels: bool,
    pub privileged_features: bool,
    pub validation_runner: bool,
}

impl BackendCapabilities {
    pub const fn synthetic_mvp() -> Self {
        Self {
            input: true,
            preview: true,
            capture: true,
            labels: true,
            privileged_features: false,
            validation_runner: false,
        }
    }

    pub const fn unavailable_real() -> Self {
        Self {
            input: false,
            preview: false,
            capture: false,
            labels: false,
            privileged_features: false,
            validation_runner: false,
        }
    }
}

pub type BackendResult<T> = Result<T, BackendError>;
pub type SessionId = String;
pub type RunId = String;
pub type CaptureJobId = String;
pub type FrameCounter = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Starting,
    Running,
    Paused,
    CapturePending,
    Stopping,
    Stopped,
    Faulted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    OperatorStop,
    FaultCleanup,
    SessionReplaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartBackendSession {
    pub requested_capabilities: BackendCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSession {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub state: SessionState,
    pub current_frame: FrameCounter,
    pub capabilities: BackendCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoppedSession {
    pub session_id: SessionId,
    pub state: SessionState,
    pub final_frame: FrameCounter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStatus {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub state: SessionState,
    pub backend_mode: BackendMode,
    pub current_frame: FrameCounter,
    pub capabilities: BackendCapabilities,
    pub last_applied_input_frame: FrameCounter,
    pub last_preview_frame: FrameCounter,
    pub active_capture_job_id: Option<CaptureJobId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunBoundary {
    pub session_id: SessionId,
    pub state: SessionState,
    pub current_frame: FrameCounter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputScheduleRequest {
    pub session_id: SessionId,
    pub target_frame: FrameCounter,
    pub pad_word: PadWord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputScheduleReceipt {
    pub session_id: SessionId,
    pub assigned_frame: FrameCounter,
    pub pad_word: PadWord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePreview {
    pub session_id: SessionId,
    pub frame: FrameCounter,
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub session_id: SessionId,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureJob {
    pub job_id: CaptureJobId,
    pub status: CaptureJobStatus,
    pub capture_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackendError {
    #[error("backend unavailable")]
    BackendUnavailable,
    #[error("input target frame is stale")]
    FrameStale {
        requested_frame: FrameCounter,
        current_frame: FrameCounter,
    },
    #[error("{operation} is not implemented in the service scaffold")]
    NotImplemented { operation: &'static str },
}

pub trait BridgeBackend: Send + Sync {
    fn mode(&self) -> BackendMode;
    fn capabilities(&self) -> BackendCapabilities;

    fn start_session(&self, request: StartBackendSession) -> BackendResult<BackendSession>;
    fn stop_session(
        &self,
        session_id: SessionId,
        reason: StopReason,
    ) -> BackendResult<StoppedSession>;
    fn status(&self, session_id: SessionId) -> BackendResult<RunStatus>;
    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary>;
    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary>;
    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt>;
    fn framebuffer(&self, session_id: SessionId) -> BackendResult<FramePreview>;
    fn trigger_capture(&self, request: CaptureRequest) -> BackendResult<CaptureJob>;
    fn capture_job(&self, job_id: CaptureJobId) -> BackendResult<CaptureJob>;
}

#[derive(Debug, Clone)]
pub struct SyntheticBackend {
    inner: Arc<Mutex<SyntheticBackendInner>>,
    private_config: BridgePrivateConfig,
}

impl Default for SyntheticBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticBackend {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SyntheticBackendInner::default())),
            private_config: BridgePrivateConfig::placeholder(),
        }
    }

    pub fn with_private_config(private_config: BridgePrivateConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SyntheticBackendInner::default())),
            private_config,
        }
    }

    pub fn fault_active_session_for_tests(&self) -> BackendResult<RunStatus> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let (run_id, status) = {
            let session = inner
                .active
                .as_ref()
                .ok_or(BackendError::BackendUnavailable)?;
            let mut faulted = session.clone();
            faulted.state = SessionState::Faulted;
            (
                session.run_id.clone(),
                faulted.status(self.mode(), self.capabilities()),
            )
        };
        inner.append_event_for_run(
            &self.private_config,
            &run_id,
            "session_faulted",
            "session faulted",
        )?;
        inner
            .active
            .as_mut()
            .ok_or(BackendError::BackendUnavailable)?
            .state = SessionState::Faulted;
        Ok(status)
    }
}

impl BridgeBackend for SyntheticBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Synthetic
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::synthetic_mvp()
    }

    fn start_session(&self, _request: StartBackendSession) -> BackendResult<BackendSession> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        if inner.active.is_some() {
            return Err(BackendError::BackendUnavailable);
        }
        let sequence = inner.next_sequence;
        let session_id = synthetic_session_id(sequence);
        let run_id = synthetic_run_id(sequence);
        let session = SyntheticSession {
            session_id,
            run_id: run_id.clone(),
            state: SessionState::Running,
            current_frame: 0,
            last_preview_frame: 0,
            last_applied_input_frame: 0,
        };
        inner.write_manifest(&self.private_config, &run_id)?;
        inner.append_event_for_run(
            &self.private_config,
            &run_id,
            "session_started",
            "session started",
        )?;
        inner.next_sequence += 1;
        inner.active = Some(session.clone());

        Ok(session.backend_session(self.capabilities()))
    }

    fn stop_session(
        &self,
        session_id: SessionId,
        _reason: StopReason,
    ) -> BackendResult<StoppedSession> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let (run_id, stopped) = {
            let session = inner
                .active
                .as_ref()
                .filter(|session| session.session_id == session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            (
                session.run_id.clone(),
                StoppedSession {
                    session_id: session.session_id.clone(),
                    state: SessionState::Stopped,
                    final_frame: session.current_frame,
                },
            )
        };
        inner.append_event_for_run(
            &self.private_config,
            &run_id,
            "session_stopped",
            "session stopped",
        )?;
        inner.active = None;

        Ok(stopped)
    }

    fn status(&self, session_id: SessionId) -> BackendResult<RunStatus> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let session = inner
            .active
            .as_mut()
            .filter(|session| session.session_id == session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if session.state == SessionState::Running {
            session.current_frame = session.current_frame.saturating_add(1);
        }

        Ok(session.status(self.mode(), self.capabilities()))
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let (run_id, boundary, should_append_event) = {
            let session = inner
                .active
                .as_ref()
                .filter(|session| session.session_id == session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            if session.state == SessionState::Faulted {
                return Err(BackendError::BackendUnavailable);
            }
            let boundary = RunBoundary {
                session_id: session.session_id.clone(),
                state: SessionState::Paused,
                current_frame: session.current_frame,
            };
            (
                session.run_id.clone(),
                boundary,
                session.state != SessionState::Paused,
            )
        };
        if should_append_event {
            inner.append_event_for_run(
                &self.private_config,
                &run_id,
                "session_paused",
                "session paused",
            )?;
        }
        let session = inner
            .active
            .as_mut()
            .ok_or(BackendError::BackendUnavailable)?;
        session.state = SessionState::Paused;

        Ok(boundary)
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let (run_id, boundary, should_append_event) = {
            let session = inner
                .active
                .as_ref()
                .filter(|session| session.session_id == session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            if session.state == SessionState::Faulted {
                return Err(BackendError::BackendUnavailable);
            }
            let next_frame = if session.state == SessionState::Paused {
                session.current_frame.saturating_add(1)
            } else {
                session.current_frame
            };
            let boundary = RunBoundary {
                session_id: session.session_id.clone(),
                state: SessionState::Running,
                current_frame: next_frame,
            };
            (
                session.run_id.clone(),
                boundary,
                session.state != SessionState::Running,
            )
        };
        if should_append_event {
            inner.append_event_for_run(
                &self.private_config,
                &run_id,
                "session_resumed",
                "session resumed",
            )?;
        }
        let session = inner
            .active
            .as_mut()
            .ok_or(BackendError::BackendUnavailable)?;
        session.state = SessionState::Running;
        session.current_frame = boundary.current_frame;

        Ok(boundary)
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let session = inner
            .active
            .as_mut()
            .filter(|session| session.session_id == request.session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if session.state != SessionState::Running {
            return Err(BackendError::BackendUnavailable);
        }
        session.last_applied_input_frame = request.target_frame;
        session.current_frame = session.current_frame.max(request.target_frame);
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
            status: CaptureJobStatus::Pending,
            capture_id: None,
        })
    }

    fn capture_job(&self, job_id: CaptureJobId) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id,
            status: CaptureJobStatus::Pending,
            capture_id: None,
        })
    }
}

#[derive(Debug, Default)]
struct SyntheticBackendInner {
    active: Option<SyntheticSession>,
    next_sequence: u64,
    next_event_seq: u64,
}

impl SyntheticBackendInner {
    fn write_manifest(
        &self,
        private_config: &BridgePrivateConfig,
        run_id: &str,
    ) -> BackendResult<()> {
        if private_config.is_placeholder() {
            return Ok(());
        }

        PrivateArtifactStore::new(private_config)
            .write_run_manifest(&RunManifest::new(
                run_id,
                synthetic_timestamp(),
                BackendMode::Synthetic,
                RUNTIME_API_SCHEMA_VERSION,
            ))
            .map(|_| ())
            .map_err(|_| BackendError::BackendUnavailable)
    }

    fn append_event_for_run(
        &mut self,
        private_config: &BridgePrivateConfig,
        run_id: &str,
        event_type: &str,
        message: &str,
    ) -> BackendResult<()> {
        if private_config.is_placeholder() {
            return Ok(());
        }
        let next_event_seq = self.next_event_seq + 1;

        PrivateArtifactStore::new(private_config)
            .append_bridge_event(
                run_id,
                &BridgeEventRow::new(
                    run_id,
                    next_event_seq,
                    synthetic_timestamp(),
                    event_type,
                    message,
                ),
            )
            .map(|_| ())
            .map_err(|_| BackendError::BackendUnavailable)?;
        self.next_event_seq = next_event_seq;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct SyntheticSession {
    session_id: SessionId,
    run_id: RunId,
    state: SessionState,
    current_frame: FrameCounter,
    last_preview_frame: FrameCounter,
    last_applied_input_frame: FrameCounter,
}

impl SyntheticSession {
    fn backend_session(&self, capabilities: BackendCapabilities) -> BackendSession {
        BackendSession {
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            state: self.state,
            current_frame: self.current_frame,
            capabilities,
        }
    }

    fn status(&self, backend_mode: BackendMode, capabilities: BackendCapabilities) -> RunStatus {
        RunStatus {
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            state: self.state,
            backend_mode,
            current_frame: self.current_frame,
            capabilities,
            last_applied_input_frame: self.last_applied_input_frame,
            last_preview_frame: self.last_preview_frame,
            active_capture_job_id: None,
        }
    }
}

fn synthetic_session_id(sequence: u64) -> String {
    if sequence == 0 {
        "synthetic-session-scaffold".to_string()
    } else {
        format!("synthetic-session-{sequence:04}")
    }
}

fn synthetic_run_id(sequence: u64) -> String {
    if sequence == 0 {
        "synthetic-run-scaffold".to_string()
    } else {
        format!("synthetic-run-{sequence:04}")
    }
}

fn synthetic_timestamp() -> &'static str {
    "1970-01-01T00:00:00Z"
}

#[derive(Debug, Default)]
pub struct RealBackendUnavailable;

impl BridgeBackend for RealBackendUnavailable {
    fn mode(&self) -> BackendMode {
        BackendMode::Real
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::unavailable_real()
    }

    fn start_session(&self, _request: StartBackendSession) -> BackendResult<BackendSession> {
        Err(BackendError::BackendUnavailable)
    }

    fn stop_session(
        &self,
        _session_id: SessionId,
        _reason: StopReason,
    ) -> BackendResult<StoppedSession> {
        Err(BackendError::BackendUnavailable)
    }

    fn status(&self, _session_id: SessionId) -> BackendResult<RunStatus> {
        Err(BackendError::BackendUnavailable)
    }

    fn pause(&self, _session_id: SessionId) -> BackendResult<RunBoundary> {
        Err(BackendError::BackendUnavailable)
    }

    fn resume(&self, _session_id: SessionId) -> BackendResult<RunBoundary> {
        Err(BackendError::BackendUnavailable)
    }

    fn inject_input(&self, _request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        Err(BackendError::BackendUnavailable)
    }

    fn framebuffer(&self, _session_id: SessionId) -> BackendResult<FramePreview> {
        Err(BackendError::BackendUnavailable)
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        Err(BackendError::BackendUnavailable)
    }

    fn capture_job(&self, _job_id: CaptureJobId) -> BackendResult<CaptureJob> {
        Err(BackendError::BackendUnavailable)
    }
}
