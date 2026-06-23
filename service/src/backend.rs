use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

use crate::input::PadWord;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

#[derive(Debug, Default)]
pub struct SyntheticBackend;

impl BridgeBackend for SyntheticBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Synthetic
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::synthetic_mvp()
    }

    fn start_session(&self, _request: StartBackendSession) -> BackendResult<BackendSession> {
        Ok(BackendSession {
            session_id: "synthetic-session-scaffold".to_string(),
            run_id: "synthetic-run-scaffold".to_string(),
            state: SessionState::Running,
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
            run_id: "synthetic-run-scaffold".to_string(),
            state: SessionState::Running,
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
