use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    future::Future,
    str::FromStr,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};
use thiserror::Error;

use dh_proto::v1 as dh;
use hyper_util::rt::TokioIo;
use prost::Message;
use tokio::net::UnixStream;
use tonic::{Code, transport::Endpoint};
use tower::service_fn;

use crate::{
    api::RUNTIME_API_SCHEMA_VERSION,
    artifacts::{
        BridgeEventRow, CaptureDeterminismClass, CaptureFeatureBytesIndex, CaptureFramebufferIndex,
        CaptureIndexRow, CaptureLifecycleRefs, CaptureManifest, CapturePayloadKind, PadLogEventRow,
        PrivateArtifactStore, RunManifest,
    },
    framebuffer::{
        RawFramebuffer, RawFramebufferFormat, SYNTHETIC_FRAME_HEIGHT, SYNTHETIC_FRAME_WIDTH,
        framebuffer_png, synthetic_frame_png,
    },
    input::{AppliedInputFrame, PadLog, PadWord},
    lease_store::{AllocationKind, LeaseIntent, LeaseRecord, LeaseStore},
    private_config::{BridgePrivateConfig, RealRuntimeConfig},
    real_capture::{ResolvedCaptureSpec, resolve_capture_spec},
};

const REAL_WORKER_RPC_TIMEOUT: Duration = Duration::from_secs(15);
const REAL_WORKER_REPLY_TIMEOUT: Duration = Duration::from_secs(20);

/// Streaming-play tuning. The worker holds the vCPU at frame boundaries while
/// the bridge is not reading, so a small channel is the pacing backstop; the
/// bridge's 60Hz-paced reads are the primary mechanism.
const PLAY_STREAM_CHANNEL_CAPACITY: usize = 2;
/// Effectively-unbounded streaming run budget. The worker's agenda
/// materialization/RSS fix (`c0337ab` or later) is a deployment prerequisite.
/// Keep a numeric `until` arm because the worker rejects a missing bound.
/// Segment reopening remains valid for early budget completion; DHILOG sealing
/// granularity is a separate concern from the resolved agenda OOM.
const PLAY_STREAM_ICOUNT_BUDGET: u64 = u64::MAX / 4;
const PLAY_STREAM_SEND_RETRY: Duration = Duration::from_millis(2);
/// After cancelling the stream, the worker parks the slot Paused at the next
/// frame boundary; poll introspection until it lands.
const PLAY_STREAM_STOP_POLL: Duration = Duration::from_millis(20);
const PLAY_STREAM_STOP_TIMEOUT: Duration = Duration::from_secs(5);

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
            privileged_features: true,
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

    pub const fn real_preview_mvp() -> Self {
        Self {
            input: false,
            preview: true,
            capture: false,
            labels: false,
            privileged_features: false,
            validation_runner: false,
        }
    }

    pub const fn real_input_preview_mvp() -> Self {
        Self {
            input: true,
            preview: true,
            capture: false,
            labels: false,
            privileged_features: false,
            validation_runner: false,
        }
    }

    pub const fn real_capture_mvp() -> Self {
        Self {
            input: true,
            preview: true,
            capture: true,
            labels: true,
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
    /// Continuous auto-advance: a bridge-side loop is running frames and
    /// streaming them over `/ws/frames`. Input is buffered on arrival and
    /// flushed by the loop between frames.
    Playing,
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
    pub preview_stale: bool,
    pub active_capture_job_id: Option<CaptureJobId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunBoundary {
    pub session_id: SessionId,
    pub state: SessionState,
    pub current_frame: FrameCounter,
    pub preview_stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputScheduleRequest {
    pub session_id: SessionId,
    pub target_frame: FrameCounter,
    pub pad_word: PadWord,
    pub client_seq: u64,
    pub source_id: String,
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

/// One continuous-play step: a single advanced frame plus its rendered image.
/// The bridge Play loop pushes this to `/ws/frames`. `frame` is the deterministic
/// pv-pad FRAME_COUNTER (monotonic within a run) used as the ordering key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayStepOutcome {
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
    pub public: Option<RealCapturePublicProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealCapturePublicProjection {
    pub frame_counter: FrameCounter,
    pub capture_source: String,
    pub layout_hash: String,
    pub capture_spec_hash: String,
    pub map_hash: String,
    pub has_preview: bool,
    pub features_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackendError {
    #[error("backend unavailable")]
    BackendUnavailable,
    #[error("frame not available from backend")]
    FrameUnavailable,
    #[error("capture already in progress")]
    CaptureInProgress,
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
    /// Enter continuous-play (`Playing`) from a resumable state. Does not itself
    /// advance frames — the API layer starts the Play loop, which calls
    /// [`play_step`](BridgeBackend::play_step) per frame.
    fn play_start(&self, session_id: SessionId) -> BackendResult<RunBoundary>;
    /// Advance exactly one frame while `Playing`, returning the rendered frame.
    fn play_step(&self, session_id: SessionId) -> BackendResult<PlayStepOutcome>;
    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt>;
    fn framebuffer(&self, session_id: SessionId) -> BackendResult<FramePreview>;
    fn trigger_capture(&self, request: CaptureRequest) -> BackendResult<CaptureJob>;
    fn capture_job(&self, job_id: CaptureJobId) -> BackendResult<CaptureJob>;
    /// Open a streaming-play session: one long worker run that emits a frame
    /// per FRAME_MARK, consumed by the Play loop at its own pace. Call after
    /// [`play_start`](BridgeBackend::play_start). Backends without a streaming
    /// path return `NotImplemented` and the Play loop falls back to per-frame
    /// [`play_step`](BridgeBackend::play_step).
    fn play_stream_start(
        &self,
        _session_id: SessionId,
    ) -> BackendResult<Box<dyn PlayStreamSession>> {
        Err(BackendError::NotImplemented {
            operation: "play_stream_start",
        })
    }
}

/// One event delivered by a [`PlayStreamSession`].
#[derive(Debug)]
pub enum PlayStreamEvent {
    /// A rendered frame, ready for `/ws/frames`.
    Frame(PlayStepOutcome),
    /// No frame arrived within the deadline. Transient: the caller re-checks
    /// its stop conditions and keeps pacing.
    TimedOut,
    /// The worker run ended (terminal stop, stream close, or fault). The
    /// session is no longer streaming; faults have already been recorded.
    Ended(PlayStreamEnd),
}

/// Aggregate-only classification and counters for a completed worker stream.
/// These values are safe to log: they contain no session, lease, endpoint,
/// snapshot, input-log, or framebuffer data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayStreamEnd {
    pub reason: PlayStreamEndReason,
    pub first_frame_icount: Option<u64>,
    pub last_frame_icount: Option<u64>,
    pub done_icount: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayStreamEndReason {
    BudgetReached,
    /// The stream ended without the budget terminal required by continuous
    /// Play. This includes transport EOF and non-budget `Done` reasons.
    UnexpectedEnd,
    Faulted,
}

impl PlayStreamEnd {
    const fn unexpected() -> Self {
        Self {
            reason: PlayStreamEndReason::UnexpectedEnd,
            first_frame_icount: None,
            last_frame_icount: None,
            done_icount: None,
        }
    }
}

/// A live streaming-play session. `next_frame` blocks up to the given timeout
/// for the next frame; `stop` cancels the stream and leaves the session Paused
/// at a frame boundary (≤1 frame of latency), returning the resulting boundary.
///
/// `input_in_flight` semantics while a stream is open: frame delivery never
/// sets it; `inject_input` still sets it for the duration of each InjectInputs
/// RPC (the worker applies the input at the next frame-hold), and
/// `play_stream_start` refuses to start while it is set.
pub trait PlayStreamSession: Send {
    fn next_frame(&mut self, timeout: Duration) -> BackendResult<PlayStreamEvent>;
    fn stop(&mut self) -> BackendResult<RunBoundary>;
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
            (session.run_id.clone(), faulted.status(self.mode()))
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

    fn start_session(&self, request: StartBackendSession) -> BackendResult<BackendSession> {
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
            applied_inputs: Vec::new(),
            capabilities: request.requested_capabilities,
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

        Ok(session.backend_session())
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

        Ok(session.status(self.mode()))
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
                preview_stale: session.last_preview_frame < session.current_frame,
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
            // Single-step: advance exactly one frame and land Paused, so the
            // operator can keep stepping (Resume is enabled from Paused).
            let next_frame = session.current_frame.saturating_add(1);
            let boundary = RunBoundary {
                session_id: session.session_id.clone(),
                state: SessionState::Paused,
                current_frame: next_frame,
                preview_stale: session.last_preview_frame < next_frame,
            };
            (session.run_id.clone(), boundary, true)
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
        session.state = SessionState::Paused;
        session.current_frame = boundary.current_frame;

        Ok(boundary)
    }

    fn play_start(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let session = inner
            .active
            .as_mut()
            .filter(|session| session.session_id == session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if !matches!(
            session.state,
            SessionState::Running | SessionState::Paused | SessionState::Playing
        ) {
            return Err(BackendError::BackendUnavailable);
        }
        session.state = SessionState::Playing;
        Ok(RunBoundary {
            session_id,
            state: SessionState::Playing,
            current_frame: session.current_frame,
            preview_stale: session.last_preview_frame < session.current_frame,
        })
    }

    /// One continuous-play step: advance exactly one frame and remain in
    /// `Playing`. Synthetic backend produces a deterministic frame.
    fn play_step(&self, session_id: SessionId) -> BackendResult<PlayStepOutcome> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let session = inner
            .active
            .as_mut()
            .filter(|session| session.session_id == session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if session.state != SessionState::Playing {
            return Err(BackendError::BackendUnavailable);
        }
        let frame = session.current_frame.saturating_add(1);
        session.current_frame = frame;
        session.last_preview_frame = frame;
        Ok(PlayStepOutcome {
            session_id,
            frame,
            width: SYNTHETIC_FRAME_WIDTH,
            height: SYNTHETIC_FRAME_HEIGHT,
            png_bytes: synthetic_frame_png(frame),
        })
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let (run_id, frame_index, previous_inputs, applied_inputs) = {
            let session = inner
                .active
                .as_ref()
                .filter(|session| session.session_id == request.session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            if !matches!(
                session.state,
                SessionState::Running | SessionState::Paused | SessionState::Playing
            ) {
                return Err(BackendError::BackendUnavailable);
            }
            let frame_index = session.applied_inputs.len() as u64;
            let previous_inputs = session.applied_inputs.clone();
            let mut applied_inputs = previous_inputs.clone();
            applied_inputs.push(AppliedInputFrame {
                frame: request.target_frame,
                pad_word: request.pad_word.raw(),
            });
            (
                session.run_id.clone(),
                frame_index,
                previous_inputs,
                applied_inputs,
            )
        };

        write_input_artifacts(
            &self.private_config,
            &run_id,
            &previous_inputs,
            &applied_inputs,
            PadLogEventRow::new(
                &run_id,
                frame_index,
                request.target_frame,
                request.pad_word.raw(),
                request.client_seq,
                &request.source_id,
                "applied",
                "input applied",
            ),
        )?;

        let session = inner
            .active
            .as_mut()
            .filter(|session| session.session_id == request.session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        session.last_applied_input_frame = request.target_frame;
        session.current_frame = session.current_frame.max(request.target_frame);
        session.applied_inputs = applied_inputs;
        Ok(InputScheduleReceipt {
            session_id: request.session_id,
            assigned_frame: request.target_frame,
            pad_word: request.pad_word,
        })
    }

    fn framebuffer(&self, session_id: SessionId) -> BackendResult<FramePreview> {
        let mut inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let session = inner
            .active
            .as_mut()
            .filter(|session| session.session_id == session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if !session.capabilities.preview {
            return Err(BackendError::BackendUnavailable);
        }
        let frame = session.current_frame;
        session.last_preview_frame = frame;

        Ok(FramePreview {
            session_id,
            frame,
            width: SYNTHETIC_FRAME_WIDTH,
            height: SYNTHETIC_FRAME_HEIGHT,
            png_bytes: synthetic_frame_png(frame),
        })
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id: "synthetic-capture-job-scaffold".to_string(),
            status: CaptureJobStatus::Pending,
            capture_id: None,
            public: None,
        })
    }

    fn capture_job(&self, job_id: CaptureJobId) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id,
            status: CaptureJobStatus::Pending,
            capture_id: None,
            public: None,
        })
    }

    fn play_stream_start(
        &self,
        session_id: SessionId,
    ) -> BackendResult<Box<dyn PlayStreamSession>> {
        let inner = self.inner.lock().expect("synthetic backend mutex poisoned");
        let session = inner
            .active
            .as_ref()
            .filter(|session| session.session_id == session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if session.state != SessionState::Playing {
            return Err(BackendError::BackendUnavailable);
        }
        Ok(Box::new(SyntheticPlayStreamSession {
            backend: self.clone(),
            session_id,
        }))
    }
}

/// Streaming adapter over the synthetic backend: one synthetic frame per
/// `next_frame` call, so the streaming Play loop is testable without a worker.
struct SyntheticPlayStreamSession {
    backend: SyntheticBackend,
    session_id: SessionId,
}

impl PlayStreamSession for SyntheticPlayStreamSession {
    fn next_frame(&mut self, _timeout: Duration) -> BackendResult<PlayStreamEvent> {
        match self.backend.play_step(self.session_id.clone()) {
            Ok(step) => Ok(PlayStreamEvent::Frame(step)),
            // The session left Playing (stop/fault raced the loop): the stream
            // is over rather than broken.
            Err(BackendError::BackendUnavailable) => {
                Ok(PlayStreamEvent::Ended(PlayStreamEnd::unexpected()))
            }
            Err(error) => Err(error),
        }
    }

    fn stop(&mut self) -> BackendResult<RunBoundary> {
        let mut inner = self
            .backend
            .inner
            .lock()
            .expect("synthetic backend mutex poisoned");
        let session = inner
            .active
            .as_mut()
            .filter(|session| session.session_id == self.session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if session.state == SessionState::Playing {
            session.state = SessionState::Paused;
        }
        Ok(RunBoundary {
            session_id: self.session_id.clone(),
            state: session.state,
            current_frame: session.current_frame,
            preview_stale: session.last_preview_frame < session.current_frame,
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

fn write_input_artifacts(
    private_config: &BridgePrivateConfig,
    run_id: &str,
    previous_inputs: &[AppliedInputFrame],
    applied_inputs: &[AppliedInputFrame],
    event: PadLogEventRow,
) -> BackendResult<()> {
    if private_config.is_placeholder() {
        return Ok(());
    }

    let store = PrivateArtifactStore::new(private_config);
    write_padlog_snapshot(&store, run_id, applied_inputs)?;
    if store.append_padlog_event(run_id, &event).is_err() {
        let _ = write_padlog_snapshot(&store, run_id, previous_inputs);
        return Err(BackendError::BackendUnavailable);
    }
    Ok(())
}

fn write_padlog_snapshot(
    store: &PrivateArtifactStore<'_>,
    run_id: &str,
    applied_inputs: &[AppliedInputFrame],
) -> BackendResult<()> {
    let padlog = PadLog::from_applied_frames(applied_inputs.iter().cloned())
        .map_err(|_| BackendError::BackendUnavailable)?;
    store
        .write_padlog(run_id, &padlog)
        .map(|_| ())
        .map_err(|_| BackendError::BackendUnavailable)
}

#[derive(Debug, Clone)]
struct SyntheticSession {
    session_id: SessionId,
    run_id: RunId,
    state: SessionState,
    current_frame: FrameCounter,
    last_preview_frame: FrameCounter,
    last_applied_input_frame: FrameCounter,
    applied_inputs: Vec<AppliedInputFrame>,
    capabilities: BackendCapabilities,
}

impl SyntheticSession {
    fn backend_session(&self) -> BackendSession {
        BackendSession {
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            state: self.state,
            current_frame: self.current_frame,
            capabilities: self.capabilities,
        }
    }

    fn status(&self, backend_mode: BackendMode) -> RunStatus {
        RunStatus {
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            state: self.state,
            backend_mode,
            current_frame: self.current_frame,
            capabilities: self.capabilities,
            last_applied_input_frame: self.last_applied_input_frame,
            last_preview_frame: self.last_preview_frame,
            preview_stale: self.last_preview_frame < self.current_frame,
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

#[derive(Clone)]
pub struct RealBackend {
    runtime_config: RealRuntimeConfig,
    private_config: BridgePrivateConfig,
    worker: RealWorkerThread,
    inner: Arc<Mutex<RealBackendInner>>,
    lifecycle: Arc<Mutex<()>>,
    reconcile_ready: Arc<Mutex<bool>>,
}

impl RealBackend {
    pub fn new(private_config: BridgePrivateConfig, runtime_config: RealRuntimeConfig) -> Self {
        let backend = Self {
            worker: RealWorkerThread::new(runtime_config.hypervisor_endpoint().clone()),
            runtime_config,
            private_config,
            inner: Arc::new(Mutex::new(RealBackendInner::default())),
            lifecycle: Arc::new(Mutex::new(())),
            reconcile_ready: Arc::new(Mutex::new(false)),
        };
        let _guard = backend
            .lifecycle
            .lock()
            .expect("real lifecycle mutex poisoned");
        backend.reconcile_leases();
        drop(_guard);
        backend
    }

    fn lease_store(&self) -> LeaseStore {
        LeaseStore::new(self.private_config.clone())
    }

    fn reconcile_leases(&self) {
        let store = self.lease_store();
        let pending = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            std::mem::take(&mut inner.pending_cleanup)
        };
        let mut pending_retained = Vec::new();
        for record in pending {
            if store.write_lease(&record).is_err() {
                match self.worker.stop(record.lease().unwrap_or(dh::Lease {
                    slot_id: 0,
                    token: Vec::new(),
                })) {
                    Ok(()) | Err(RealWorkerFailure::StaleLease) => {
                        let _ = store.remove_intent(&record.operation_id);
                    }
                    Err(_) => pending_retained.push(record),
                }
            }
        }
        if !pending_retained.is_empty() {
            let retained = pending_retained.len();
            self.inner
                .lock()
                .expect("real backend mutex poisoned")
                .pending_cleanup = pending_retained;
            *self
                .reconcile_ready
                .lock()
                .expect("reconcile mutex poisoned") = false;
            tracing::info!(
                found_leases = retained,
                found_intents = 0,
                destroyed = 0,
                stale_cleaned = 0,
                missing_cleaned = 0,
                dangling = 0,
                invalid = 0,
                retained,
                ready_for_real_sessions = false,
                "real lease reconciliation completed"
            );
            return;
        }
        let loaded = match store.load() {
            Ok(loaded) => loaded,
            Err(_) => {
                *self
                    .reconcile_ready
                    .lock()
                    .expect("reconcile mutex poisoned") = false;
                tracing::info!(
                    found_leases = 0,
                    found_intents = 0,
                    destroyed = 0,
                    stale_cleaned = 0,
                    missing_cleaned = 0,
                    dangling = 0,
                    invalid = 1,
                    retained = 1,
                    ready_for_real_sessions = false,
                    "real lease reconciliation completed"
                );
                return;
            }
        };
        let found_intents = loaded.intents.len();
        let found_leases = loaded.leases.len();
        let mut destroyed = 0usize;
        let mut stale_cleaned = 0usize;
        let mut missing_cleaned = 0usize;
        let mut retained = loaded.invalid;
        let mut clean_operations = Vec::new();
        if !loaded.leases.is_empty() {
            match self.worker.list_slots() {
                Ok(slots) => {
                    for record in &loaded.leases {
                        if !slots.contains(&record.slot_id) {
                            if store.remove_lease(&record.operation_id).is_ok() {
                                missing_cleaned += 1;
                                clean_operations.push(record.operation_id.clone());
                            } else {
                                retained += 1;
                            }
                            continue;
                        }
                        let Ok(lease) = record.lease() else {
                            retained += 1;
                            continue;
                        };
                        match self.destroy_recorded(&record.operation_id, lease) {
                            Ok(RecordedCleanup::Destroyed) => {
                                destroyed += 1;
                                clean_operations.push(record.operation_id.clone());
                            }
                            Ok(RecordedCleanup::Stale) => {
                                stale_cleaned += 1;
                                clean_operations.push(record.operation_id.clone());
                            }
                            Err(_) => retained += 1,
                        }
                    }
                }
                Err(_) => retained += loaded.leases.len(),
            }
        }
        for operation_id in &clean_operations {
            if store.remove_intent(operation_id).is_err() {
                retained += 1;
            } else if let Some(record) = loaded
                .leases
                .iter()
                .find(|record| &record.operation_id == operation_id)
            {
                let _ = self.append_real_event(
                    &record.run_id,
                    "session_stopped",
                    "real backend session stopped during lease reconciliation",
                );
            }
        }
        let dangling = loaded
            .intents
            .iter()
            .filter(|intent| {
                !clean_operations.contains(&intent.operation_id)
                    && !loaded
                        .leases
                        .iter()
                        .any(|lease| lease.operation_id == intent.operation_id)
            })
            .count();
        retained += dangling;
        let ready = loaded.invalid == 0 && retained == 0;
        *self
            .reconcile_ready
            .lock()
            .expect("reconcile mutex poisoned") = ready;
        tracing::info!(
            found_leases,
            found_intents,
            destroyed,
            stale_cleaned,
            missing_cleaned,
            dangling,
            invalid = loaded.invalid,
            retained,
            ready_for_real_sessions = ready,
            "real lease reconciliation completed"
        );
        if dangling > 0 {
            tracing::warn!(
                dangling,
                "unaccounted slot may exist; stop bridge, restart worker, verify full capacity, then use audited intent-clear tool before resuming"
            );
        }
    }

    fn destroy_recorded(
        &self,
        operation_id: &str,
        lease: dh::Lease,
    ) -> BackendResult<RecordedCleanup> {
        let result = match self.worker.stop(lease) {
            outcome @ (Ok(()) | Err(RealWorkerFailure::StaleLease)) => {
                let store = self.lease_store();
                store
                    .remove_lease(operation_id)
                    .map_err(|_| BackendError::BackendUnavailable)?;
                store
                    .remove_intent(operation_id)
                    .map_err(|_| BackendError::BackendUnavailable)?;
                Ok(match outcome {
                    Ok(()) => RecordedCleanup::Destroyed,
                    Err(RealWorkerFailure::StaleLease) => RecordedCleanup::Stale,
                    Err(_) => unreachable!("matched cleanup outcomes only"),
                })
            }
            Err(error) => Err(error.into()),
        };
        if result.is_err() {
            *self
                .reconcile_ready
                .lock()
                .expect("reconcile mutex poisoned") = false;
        }
        result
    }

    /// Mark the active session faulted (used by the play loop and run paths on a
    /// worker error). Clears any in-flight input.
    fn mark_faulted(&self, session: &RealSession) {
        {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if let Some(active) = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session.session_id)
            {
                active.state = SessionState::Faulted;
                active.input_in_flight = false;
            }
        }
        let _ = self.append_real_event(
            &session.run_id,
            "session_faulted",
            "real backend session faulted",
        );
    }

    fn capture_spec(&self) -> BackendResult<ResolvedCaptureSpec> {
        resolve_capture_spec(&self.runtime_config).map_err(|_| BackendError::BackendUnavailable)
    }

    fn start_command(&self) -> BackendResult<RealStartCommand> {
        let start_source = self.runtime_config.start_source();
        if let Some(snapshot_hash) = start_source
            .snapshot_hash()
            .map_err(|_| BackendError::BackendUnavailable)?
        {
            return Ok(RealStartCommand::RestoreSnapshot { snapshot_hash });
        }

        let config_path = start_source
            .create_vm_config_relative_path()
            .map_err(|_| BackendError::BackendUnavailable)?
            .ok_or(BackendError::BackendUnavailable)?;
        let bytes = self
            .private_config
            .read_private_file(config_path)
            .map_err(|_| BackendError::BackendUnavailable)?;
        let create_vm =
            parse_private_create_vm_config(&bytes).map_err(|_| BackendError::BackendUnavailable)?;
        Ok(RealStartCommand::CreateVm {
            config: create_vm.config,
            entropy_seed: create_vm.entropy_seed,
        })
    }

    fn write_real_manifest(&self, run_id: &str) -> BackendResult<()> {
        if self.private_config.is_placeholder() {
            return Ok(());
        }

        PrivateArtifactStore::new(&self.private_config)
            .write_run_manifest(&RunManifest::new(
                run_id,
                synthetic_timestamp(),
                BackendMode::Real,
                RUNTIME_API_SCHEMA_VERSION,
            ))
            .map(|_| ())
            .map_err(|_| BackendError::BackendUnavailable)
    }

    fn append_real_event(
        &self,
        run_id: &str,
        event_type: &str,
        message: &str,
    ) -> BackendResult<()> {
        if self.private_config.is_placeholder() {
            return Ok(());
        }

        let next_event_seq = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            inner.next_event_seq += 1;
            inner.next_event_seq
        };

        PrivateArtifactStore::new(&self.private_config)
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
            .map_err(|_| BackendError::BackendUnavailable)
    }

    fn append_cleanup_failed(&self, run_id: &str) {
        let _ = self.append_real_event(run_id, "cleanup_failed", "real backend cleanup failed");
    }

    fn quarantine_after_input_artifact_failure(&self, session: &RealSession) {
        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("real lifecycle mutex poisoned");
        {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if inner
                .active
                .as_ref()
                .is_some_and(|active| active.session_id == session.session_id)
            {
                inner.active = None;
            }
        }
        let _ = self.append_real_event(
            &session.run_id,
            "input_artifact_failed",
            "real backend input artifact persistence failed",
        );
        if self
            .destroy_recorded(&session.operation_id, session.lease.clone())
            .is_err()
        {
            self.append_cleanup_failed(&session.run_id);
        }
    }

    fn persist_real_capture(
        &self,
        session: &RealSession,
        job_id: &str,
        capture_id: &str,
        resolved: &ResolvedCaptureSpec,
        outcome: &RealCaptureOutcome,
    ) -> BackendResult<RealCapturePublicProjection> {
        let expected_len =
            usize::try_from(resolved.total_len).map_err(|_| BackendError::BackendUnavailable)?;
        if outcome.feature_bytes.len() != expected_len
            || outcome.fb_info.frame_counter != outcome.frame_counter
        {
            return Err(BackendError::BackendUnavailable);
        }
        let decoded = resolved
            .decode_values(&outcome.feature_bytes)
            .map_err(|_| BackendError::BackendUnavailable)?;
        let pixels = lz4_flex::decompress_size_prepended(&outcome.fb_lz4)
            .map_err(|_| BackendError::BackendUnavailable)?;
        let uncompressed_len =
            u64::try_from(pixels.len()).map_err(|_| BackendError::BackendUnavailable)?;
        let pixel_format = framebuffer_pixel_format_name(outcome.fb_info.format)?;
        validate_framebuffer_geometry(&outcome.fb_info, uncompressed_len)?;

        let store = PrivateArtifactStore::new(&self.private_config);
        let feature_payload = store
            .write_capture_payload(
                capture_id,
                CapturePayloadKind::FeatureBytes,
                "feature-bytes.bin",
                &outcome.feature_bytes,
            )
            .map_err(|_| BackendError::BackendUnavailable)?;
        let framebuffer_payload = store
            .write_capture_payload(
                capture_id,
                CapturePayloadKind::Framebuffer,
                "framebuffer.fb_lz4",
                &outcome.fb_lz4,
            )
            .map_err(|_| BackendError::BackendUnavailable)?;
        let manifest = CaptureManifest::new(
            capture_id,
            job_id,
            &session.run_id,
            u64::from(outcome.frame_counter),
            outcome.icount,
            outcome.vns,
            &resolved.layout_hash,
            &resolved.capture_spec_hash,
            &resolved.map_hash,
            "real_take_snapshot",
            capture_lifecycle_refs(outcome),
        );
        store
            .write_capture_manifest(capture_id, &manifest)
            .map_err(|_| BackendError::BackendUnavailable)?;
        let row = CaptureIndexRow {
            schema_version: crate::artifacts::ARTIFACT_SCHEMA_VERSION,
            capture_id: capture_id.to_string(),
            node_ref: format!("run:{}:capture:{}", session.run_id, capture_id),
            capture_source: "real_take_snapshot".to_string(),
            frame_counter: u64::from(outcome.frame_counter),
            layout_hash: resolved.layout_hash.clone(),
            feature_bytes: CaptureFeatureBytesIndex {
                artifact_ref: feature_payload.artifact_ref.artifact_ref(),
                len: feature_payload.len,
                blake3: feature_payload.blake3,
            },
            decoded_order: decoded.order,
            decoded_values: decoded.values,
            framebuffer: CaptureFramebufferIndex {
                artifact_ref: framebuffer_payload.artifact_ref.artifact_ref(),
                encoding: "fb_lz4".to_string(),
                width: outcome.fb_info.width,
                height: outcome.fb_info.height,
                stride: outcome.fb_info.stride,
                pixel_format: pixel_format.to_string(),
                uncompressed_len,
                blake3: framebuffer_payload.blake3,
            },
        };
        store
            .append_capture_index_row(&row)
            .map_err(|_| BackendError::BackendUnavailable)?;
        Ok(RealCapturePublicProjection {
            frame_counter: u64::from(outcome.frame_counter),
            capture_source: "hypervisor".to_string(),
            layout_hash: resolved.layout_hash.clone(),
            capture_spec_hash: public_capture_spec_hash(&resolved.capture_spec_hash),
            map_hash: resolved.map_hash.clone(),
            has_preview: false,
            features_available: false,
        })
    }

    fn clear_starting(&self) {
        self.inner
            .lock()
            .expect("real backend mutex poisoned")
            .starting = false;
    }
}

impl BridgeBackend for RealBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Real
    }

    fn capabilities(&self) -> BackendCapabilities {
        if self.capture_spec().is_ok() {
            BackendCapabilities::real_capture_mvp()
        } else {
            BackendCapabilities::real_input_preview_mvp()
        }
    }

    fn start_session(&self, request: StartBackendSession) -> BackendResult<BackendSession> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("real lifecycle mutex poisoned");
        if !*self
            .reconcile_ready
            .lock()
            .expect("reconcile mutex poisoned")
            && self
                .inner
                .lock()
                .expect("real backend mutex poisoned")
                .active
                .is_none()
        {
            self.reconcile_leases();
        }
        if !*self
            .reconcile_ready
            .lock()
            .expect("reconcile mutex poisoned")
        {
            return Err(BackendError::BackendUnavailable);
        }
        let (sequence, session_id, run_id) = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if inner.active.is_some() || inner.starting {
                return Err(BackendError::BackendUnavailable);
            }
            inner.starting = true;
            let sequence = inner.next_sequence;
            (sequence, real_session_id(sequence), real_run_id(sequence))
        };

        let start_command = match self.start_command() {
            Ok(command) => command,
            Err(error) => {
                self.clear_starting();
                return Err(error);
            }
        };
        let (allocation_kind, source) = match &start_command {
            RealStartCommand::RestoreSnapshot { snapshot_hash } => (
                AllocationKind::RestoreSnapshot,
                snapshot_hash
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            ),
            RealStartCommand::CreateVm { .. } => {
                (AllocationKind::CreateVm, "create_vm_config".to_string())
            }
        };
        let intent = LeaseIntent::new(session_id.clone(), run_id.clone(), source, allocation_kind);
        let store = self.lease_store();
        if store.write_intent(&intent).is_err() {
            self.clear_starting();
            return Err(BackendError::BackendUnavailable);
        }
        let outcome = match self.worker.start(start_command) {
            Ok(outcome) => outcome,
            Err(error) => {
                if matches!(error, RealWorkerFailure::AllocationRejected) {
                    if store.remove_intent(&intent.operation_id).is_err() {
                        *self
                            .reconcile_ready
                            .lock()
                            .expect("reconcile mutex poisoned") = false;
                    }
                } else {
                    *self
                        .reconcile_ready
                        .lock()
                        .expect("reconcile mutex poisoned") = false;
                }
                self.clear_starting();
                return Err(error.into());
            }
        };
        let lease_record = intent.promote(&outcome.lease);
        if store.write_lease(&lease_record).is_err() && store.write_lease(&lease_record).is_err() {
            match self.worker.stop(outcome.lease.clone()) {
                Ok(()) | Err(RealWorkerFailure::StaleLease) => {
                    if store.remove_intent(&intent.operation_id).is_err() {
                        *self
                            .reconcile_ready
                            .lock()
                            .expect("reconcile mutex poisoned") = false;
                    }
                }
                Err(_) => {
                    self.inner
                        .lock()
                        .expect("real backend mutex poisoned")
                        .pending_cleanup
                        .push(lease_record);
                    *self
                        .reconcile_ready
                        .lock()
                        .expect("reconcile mutex poisoned") = false;
                }
            }
            self.clear_starting();
            return Err(BackendError::BackendUnavailable);
        }
        if store.remove_intent(&intent.operation_id).is_err() {
            // A duplicate intent is harmless because the token-bearing record is authoritative.
            *self
                .reconcile_ready
                .lock()
                .expect("reconcile mutex poisoned") = false;
        }

        if let Err(error) = self.write_real_manifest(&run_id).and_then(|_| {
            self.append_real_event(&run_id, "session_started", "real backend session started")
        }) {
            let _ = self.destroy_recorded(&lease_record.operation_id, outcome.lease.clone());
            self.append_cleanup_failed(&run_id);
            self.clear_starting();
            return Err(error);
        }

        let session = RealSession {
            operation_id: intent.operation_id,
            session_id,
            run_id,
            lease: outcome.lease,
            state: outcome.state,
            current_frame: outcome.current_frame,
            current_icount: outcome.current_icount,
            last_preview_frame: 0,
            preview_stale: outcome.current_frame > 0,
            last_applied_input_frame: 0,
            frame_base_known: true,
            input_in_flight: false,
            active_capture_job_id: None,
            applied_inputs: Vec::new(),
            capabilities: request.requested_capabilities,
        };
        let backend_session = session.backend_session();
        let mut inner = self.inner.lock().expect("real backend mutex poisoned");
        inner.next_sequence = sequence.saturating_add(1);
        inner.active = Some(session);
        inner.starting = false;
        Ok(backend_session)
    }

    fn stop_session(
        &self,
        session_id: SessionId,
        _reason: StopReason,
    ) -> BackendResult<StoppedSession> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("real lifecycle mutex poisoned");
        let session = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            let Some(session) = inner
                .active
                .take()
                .filter(|session| session.session_id == session_id)
            else {
                return Err(BackendError::BackendUnavailable);
            };
            session
        };

        let stopped = StoppedSession {
            session_id: session.session_id.clone(),
            state: SessionState::Stopped,
            final_frame: session.current_frame,
        };
        match self.destroy_recorded(&session.operation_id, session.lease) {
            Ok(_) => {
                self.append_real_event(
                    &session.run_id,
                    "session_stopped",
                    "real backend session stopped",
                )?;
                Ok(stopped)
            }
            Err(error) => {
                self.append_cleanup_failed(&session.run_id);
                Err(error)
            }
        }
    }

    fn status(&self, session_id: SessionId) -> BackendResult<RunStatus> {
        let session = {
            let inner = self.inner.lock().expect("real backend mutex poisoned");
            inner
                .active
                .as_ref()
                .filter(|session| session.session_id == session_id)
                .cloned()
                .ok_or(BackendError::BackendUnavailable)?
        };

        let slot = match self.worker.status(session.lease.slot_id) {
            Ok(slot) => slot,
            Err(_) => {
                let status = {
                    let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                    let active = inner
                        .active
                        .as_mut()
                        .filter(|active| active.session_id == session.session_id)
                        .ok_or(BackendError::BackendUnavailable)?;
                    active.state = SessionState::Faulted;
                    active.input_in_flight = false;
                    active.status(self.mode())
                };
                let _ = self.append_real_event(
                    &session.run_id,
                    "session_faulted",
                    "real backend status check failed",
                );
                return Ok(status);
            }
        };

        let mut inner = self.inner.lock().expect("real backend mutex poisoned");
        let active = inner
            .active
            .as_mut()
            .filter(|active| active.session_id == session.session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        active.current_icount = slot.icount;
        Ok(active.status(self.mode()))
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        let session = {
            let inner = self.inner.lock().expect("real backend mutex poisoned");
            inner
                .active
                .as_ref()
                .filter(|session| session.session_id == session_id)
                .cloned()
                .ok_or(BackendError::BackendUnavailable)?
        };
        if session.state == SessionState::Faulted || session.input_in_flight {
            return Err(BackendError::BackendUnavailable);
        }
        // Already paused (e.g. a streaming Play teardown parked the slot at a
        // frame boundary before this handler ran): pausing is idempotent, and
        // dispatching a real worker Pause here could land on a Running slot
        // mid-race and quantize to the next epoch (~1s of extra play).
        if session.state == SessionState::Paused {
            let inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_ref()
                .filter(|active| active.session_id == session.session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            return Ok(active.boundary());
        }

        let outcome = match self.worker.pause(session.lease.clone()) {
            Ok(outcome) => outcome,
            // Coming from Playing the loop already left the slot PAUSED_S; a Pause
            // RPC against an already-paused slot is a benign precondition failure.
            Err(RealWorkerFailure::FailedPrecondition)
                if matches!(session.state, SessionState::Paused | SessionState::Playing) =>
            {
                RealPauseOutcome {
                    current_icount: session.current_icount,
                }
            }
            Err(error) => return Err(error.into()),
        };

        let should_append = session.state != SessionState::Paused;
        {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session.session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            active.state = SessionState::Paused;
            active.current_icount = outcome.current_icount;
        }
        if should_append {
            self.append_real_event(
                &session.run_id,
                "session_paused",
                "real backend session paused",
            )?;
        }

        let inner = self.inner.lock().expect("real backend mutex poisoned");
        let active = inner
            .active
            .as_ref()
            .filter(|active| active.session_id == session.session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        Ok(active.boundary())
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        let session = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_mut()
                .filter(|session| session.session_id == session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            if active.state != SessionState::Paused || active.input_in_flight {
                return Err(BackendError::BackendUnavailable);
            }
            active.state = SessionState::Running;
            active.clone()
        };

        let mut outcome = match self.worker.resume(session.lease.clone()) {
            Ok(outcome) => outcome,
            Err(error) => {
                {
                    let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                    if let Some(active) = inner
                        .active
                        .as_mut()
                        .filter(|active| active.session_id == session.session_id)
                    {
                        active.state = SessionState::Faulted;
                        active.input_in_flight = false;
                    }
                }
                let _ = self.append_real_event(
                    &session.run_id,
                    "session_faulted",
                    "real backend session faulted",
                );
                return Err(error.into());
            }
        };
        if outcome.faulted {
            {
                let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                if let Some(active) = inner
                    .active
                    .as_mut()
                    .filter(|active| active.session_id == session.session_id)
                {
                    active.state = SessionState::Faulted;
                    active.current_icount = outcome.current_icount;
                    active.input_in_flight = false;
                }
            }
            let _ = self.append_real_event(
                &session.run_id,
                "session_faulted",
                "real backend session faulted",
            );
            return Err(BackendError::BackendUnavailable);
        }

        // A plain frame-budget Run commonly omits fb_info. Resolve the
        // authoritative post-run frame from GetFramebuffer rather than
        // returning the pre-run cached counter or inferring from frames_elapsed.
        if outcome.current_frame.is_none() {
            match self.worker.frame_counter(session.lease.clone()) {
                Ok(counter) => {
                    outcome.current_frame = Some(counter.frame);
                    outcome.current_icount = counter.icount;
                }
                Err(error) => {
                    self.mark_faulted(&session);
                    return Err(error.into());
                }
            }
        }

        {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session.session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            active.state = SessionState::Paused;
            active.current_icount = outcome.current_icount;
            active.input_in_flight = false;
            active.current_frame = outcome
                .current_frame
                .expect("resume resolves an authoritative frame above");
            active.frame_base_known = true;
            active.preview_stale = active.last_preview_frame < active.current_frame;
        }
        self.append_real_event(
            &session.run_id,
            "session_resumed",
            "real backend session resumed",
        )?;

        let inner = self.inner.lock().expect("real backend mutex poisoned");
        let active = inner
            .active
            .as_ref()
            .filter(|active| active.session_id == session.session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        Ok(active.boundary())
    }

    fn play_start(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        let mut inner = self.inner.lock().expect("real backend mutex poisoned");
        let active = inner
            .active
            .as_mut()
            .filter(|active| active.session_id == session_id)
            .ok_or(BackendError::BackendUnavailable)?;
        if !matches!(
            active.state,
            SessionState::Running | SessionState::Paused | SessionState::Playing
        ) || active.input_in_flight
        {
            return Err(BackendError::BackendUnavailable);
        }
        active.state = SessionState::Playing;
        Ok(active.boundary())
    }

    fn play_step(&self, session_id: SessionId) -> BackendResult<PlayStepOutcome> {
        // Precondition: Playing and no input in flight (the loop flushes input
        // between frames, so input_in_flight is clear when it calls us).
        let session = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            if active.state != SessionState::Playing || active.input_in_flight {
                return Err(BackendError::BackendUnavailable);
            }
            active.clone()
        };

        // Advance exactly one frame and capture its framebuffer in a single
        // `Run{frame_budget=1, capture{framebuffer}}` RPC (one worker
        // round-trip per frame). The `GetFramebuffer`-based `resume` path
        // stays for non-Play uses (frame_counter, preview).
        let outcome = match self.worker.run_one_frame_captured(session.lease.clone()) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.mark_faulted(&session);
                return Err(error.into());
            }
        };
        if outcome.faulted {
            self.mark_faulted(&session);
            return Err(BackendError::BackendUnavailable);
        }

        let decoded = decode_captured_framebuffer(&outcome.fb_lz4, &outcome.fb_info)
            .inspect_err(|_| self.mark_faulted(&session))?;

        {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if let Some(active) = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session.session_id)
            {
                active.current_icount = outcome.current_icount;
                active.current_frame = decoded.frame;
                active.last_preview_frame = decoded.frame;
                active.frame_base_known = true;
                active.preview_stale = false;
            }
        }

        Ok(PlayStepOutcome {
            session_id,
            frame: decoded.frame,
            width: decoded.width,
            height: decoded.height,
            png_bytes: decoded.png_bytes,
        })
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        if request.target_frame >= u64::from(u32::MAX) {
            return Err(BackendError::BackendUnavailable);
        }
        let target_frame =
            u32::try_from(request.target_frame).map_err(|_| BackendError::BackendUnavailable)?;

        let mut session = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_mut()
                .filter(|session| session.session_id == request.session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            if !active.capabilities.input
                || !matches!(active.state, SessionState::Paused | SessionState::Playing)
                || active.input_in_flight
            {
                return Err(BackendError::BackendUnavailable);
            }
            active.input_in_flight = true;
            active.clone()
        };

        if !session.frame_base_known {
            match self.worker.frame_counter(session.lease.clone()) {
                Ok(outcome) => {
                    let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                    let active = inner
                        .active
                        .as_mut()
                        .filter(|active| active.session_id == session.session_id)
                        .filter(|active| active.input_in_flight)
                        .ok_or(BackendError::BackendUnavailable)?;
                    active.current_frame = outcome.frame;
                    active.current_icount = outcome.icount;
                    active.frame_base_known = true;
                    active.preview_stale = active.last_preview_frame < active.current_frame;
                    session = active.clone();
                }
                Err(error) => {
                    let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                    if let Some(active) = inner
                        .active
                        .as_mut()
                        .filter(|active| active.session_id == session.session_id)
                    {
                        active.input_in_flight = false;
                        active.frame_base_known = false;
                    }
                    return Err(error.into());
                }
            }
        }

        if request.target_frame <= session.current_frame {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if let Some(active) = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session.session_id)
            {
                active.input_in_flight = false;
            }
            return Err(BackendError::FrameStale {
                requested_frame: request.target_frame,
                current_frame: session.current_frame,
            });
        }

        let outcome =
            match self
                .worker
                .inject_input(session.lease.clone(), target_frame, request.pad_word)
            {
                Ok(outcome) => outcome,
                Err(RealWorkerFailure::FrameStale) => {
                    let current_frame = match self.worker.frame_counter(session.lease.clone()) {
                        Ok(outcome) => {
                            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                            if let Some(active) = inner
                                .active
                                .as_mut()
                                .filter(|active| active.session_id == session.session_id)
                            {
                                active.current_frame = outcome.frame;
                                active.current_icount = outcome.icount;
                                active.frame_base_known = true;
                                active.preview_stale =
                                    active.last_preview_frame < active.current_frame;
                                active.input_in_flight = false;
                            }
                            outcome.frame
                        }
                        Err(error) => {
                            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                            if let Some(active) = inner
                                .active
                                .as_mut()
                                .filter(|active| active.session_id == session.session_id)
                            {
                                active.frame_base_known = false;
                                active.input_in_flight = false;
                            }
                            return Err(error.into());
                        }
                    };
                    if current_frame < request.target_frame {
                        return Err(BackendError::BackendUnavailable);
                    }
                    return Err(BackendError::FrameStale {
                        requested_frame: request.target_frame,
                        current_frame,
                    });
                }
                Err(error) => {
                    let mut inner = self.inner.lock().expect("real backend mutex poisoned");
                    if let Some(active) = inner
                        .active
                        .as_mut()
                        .filter(|active| active.session_id == session.session_id)
                    {
                        active.input_in_flight = false;
                    }
                    return Err(error.into());
                }
            };

        if outcome.scheduled != 1 {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if let Some(active) = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session.session_id)
            {
                active.input_in_flight = false;
            }
            return Err(BackendError::BackendUnavailable);
        }

        let (run_id, frame_index, previous_inputs, applied_inputs, cleanup_session) = {
            let inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_ref()
                .filter(|active| active.session_id == session.session_id)
                .filter(|active| active.input_in_flight)
                .ok_or(BackendError::BackendUnavailable)?;
            let frame_index = active.applied_inputs.len() as u64;
            let previous_inputs = active.applied_inputs.clone();
            let mut applied_inputs = previous_inputs.clone();
            applied_inputs.push(AppliedInputFrame {
                frame: request.target_frame,
                pad_word: request.pad_word.raw(),
            });
            (
                active.run_id.clone(),
                frame_index,
                previous_inputs,
                applied_inputs,
                active.clone(),
            )
        };

        if let Err(error) = write_input_artifacts(
            &self.private_config,
            &run_id,
            &previous_inputs,
            &applied_inputs,
            PadLogEventRow::new(
                &run_id,
                frame_index,
                request.target_frame,
                request.pad_word.raw(),
                request.client_seq,
                &request.source_id,
                "applied",
                "input applied",
            ),
        ) {
            self.quarantine_after_input_artifact_failure(&cleanup_session);
            return Err(error);
        }

        let mut inner = self.inner.lock().expect("real backend mutex poisoned");
        let active = inner
            .active
            .as_mut()
            .filter(|active| active.session_id == session.session_id)
            .filter(|active| active.input_in_flight)
            .ok_or(BackendError::BackendUnavailable)?;
        active.input_in_flight = false;
        active.last_applied_input_frame = request.target_frame;
        active.applied_inputs = applied_inputs;
        Ok(InputScheduleReceipt {
            session_id: request.session_id,
            assigned_frame: request.target_frame,
            pad_word: request.pad_word,
        })
    }

    fn framebuffer(&self, session_id: SessionId) -> BackendResult<FramePreview> {
        let session = {
            let inner = self.inner.lock().expect("real backend mutex poisoned");
            inner
                .active
                .as_ref()
                .filter(|session| session.session_id == session_id)
                .cloned()
                .ok_or(BackendError::BackendUnavailable)?
        };
        if !session.capabilities.preview || session.state != SessionState::Paused {
            return Err(BackendError::BackendUnavailable);
        }

        // The worker reports FailedPrecondition when the guest has no
        // conformant framebuffer to read (e.g. pre-workload snapshots);
        // surface that as frame-unavailable rather than a backend outage.
        let outcome = self
            .worker
            .framebuffer(session.lease.clone())
            .map_err(|failure| match failure {
                RealWorkerFailure::FailedPrecondition => BackendError::FrameUnavailable,
                other => BackendError::from(other),
            })?;
        let mut inner = self.inner.lock().expect("real backend mutex poisoned");
        let active = inner
            .active
            .as_mut()
            .filter(|active| active.session_id == session.session_id)
            .filter(|active| active.capabilities.preview && active.state == SessionState::Paused)
            .ok_or(BackendError::BackendUnavailable)?;
        active.last_preview_frame = outcome.frame;
        active.current_frame = active.current_frame.max(outcome.frame);
        active.current_icount = outcome.icount;
        active.frame_base_known = true;
        active.preview_stale = active.last_preview_frame < active.current_frame;

        Ok(FramePreview {
            session_id,
            frame: outcome.frame,
            width: outcome.width,
            height: outcome.height,
            png_bytes: outcome.png_bytes,
        })
    }

    fn trigger_capture(&self, request: CaptureRequest) -> BackendResult<CaptureJob> {
        let key = (request.session_id.clone(), request.idempotency_key.clone());
        {
            let inner = self.inner.lock().expect("real backend mutex poisoned");
            if let Some(job_id) = inner.capture_idempotency.get(&key) {
                let job = inner
                    .capture_jobs
                    .get(job_id)
                    .ok_or(BackendError::BackendUnavailable)?;
                return Ok(job.capture_job());
            }
        }

        let resolved = self.capture_spec()?;
        let (session, job_id, capture_id) = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if let Some(job_id) = inner.capture_idempotency.get(&key) {
                let job = inner
                    .capture_jobs
                    .get(job_id)
                    .ok_or(BackendError::BackendUnavailable)?;
                return Ok(job.capture_job());
            }

            let (job_id, capture_id) =
                real_capture_ids(&request.session_id, &request.idempotency_key);
            let active = inner
                .active
                .as_mut()
                .filter(|session| session.session_id == request.session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            if active.active_capture_job_id.is_some() {
                return Err(BackendError::CaptureInProgress);
            }
            if !active.capabilities.capture
                || active.state != SessionState::Paused
                || active.input_in_flight
            {
                return Err(BackendError::BackendUnavailable);
            }
            active.active_capture_job_id = Some(job_id.clone());
            let session = active.clone();
            inner.capture_idempotency.insert(key, job_id.clone());
            inner.capture_jobs.insert(
                job_id.clone(),
                RealCaptureJobState {
                    job_id: job_id.clone(),
                    capture_id: capture_id.clone(),
                    status: CaptureJobStatus::Running,
                    public: None,
                },
            );
            (session, job_id, capture_id)
        };

        let capture_result = self
            .worker
            .capture(session.lease.clone(), resolved.spec.clone())
            .map_err(BackendError::from)
            .and_then(|outcome| {
                let public =
                    self.persist_real_capture(&session, &job_id, &capture_id, &resolved, &outcome)?;
                Ok((outcome, public))
            });

        let completed = match capture_result {
            Ok((outcome, public)) => Some((outcome, public)),
            Err(_) => None,
        };
        let terminal_status = if completed.is_some() {
            CaptureJobStatus::Completed
        } else {
            CaptureJobStatus::Failed
        };

        let job = {
            let mut inner = self.inner.lock().expect("real backend mutex poisoned");
            if let Some(active) = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == session.session_id)
            {
                if active.active_capture_job_id.as_deref() == Some(&job_id) {
                    active.active_capture_job_id = None;
                }
                if let Some((outcome, _public)) = &completed {
                    active.current_frame = u64::from(outcome.frame_counter);
                    active.current_icount = outcome.icount;
                    active.frame_base_known = true;
                    active.preview_stale = active.last_preview_frame < active.current_frame;
                }
            }
            let job = inner
                .capture_jobs
                .get_mut(&job_id)
                .ok_or(BackendError::BackendUnavailable)?;
            job.status = terminal_status;
            job.public = completed.map(|(_outcome, public)| public);
            job.capture_job()
        };

        let event = if terminal_status == CaptureJobStatus::Completed {
            ("capture_completed", "real backend capture completed")
        } else {
            ("capture_failed", "real backend capture failed")
        };
        let _ = self.append_real_event(&session.run_id, event.0, event.1);
        Ok(job)
    }

    fn capture_job(&self, job_id: CaptureJobId) -> BackendResult<CaptureJob> {
        let inner = self.inner.lock().expect("real backend mutex poisoned");
        inner
            .capture_jobs
            .get(&job_id)
            .map(RealCaptureJobState::capture_job)
            .ok_or(BackendError::BackendUnavailable)
    }

    fn play_stream_start(
        &self,
        session_id: SessionId,
    ) -> BackendResult<Box<dyn PlayStreamSession>> {
        let session = {
            let inner = self.inner.lock().expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_ref()
                .filter(|active| active.session_id == session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            // Precondition mirrors play_step: Playing, and no input RPC racing
            // the stream open (a fallback stop/inject/restart cycle owns the
            // slot while input_in_flight is set).
            if active.state != SessionState::Playing || active.input_in_flight {
                return Err(BackendError::BackendUnavailable);
            }
            active.clone()
        };

        let stream = match self.worker.play_stream_open(session.lease.clone()) {
            Ok(stream) => stream,
            Err(error) => {
                self.mark_faulted(&session);
                return Err(error.into());
            }
        };
        Ok(Box::new(RealPlayStreamSession {
            backend: self.clone(),
            session,
            stream,
            terminal: None,
        }))
    }
}

/// A live worker frame-capture stream feeding the Play loop. Frames arrive on
/// a bounded channel from the worker thread's streaming task; the command lane
/// stays free for Status/Stop/InjectInputs while this is open.
struct RealPlayStreamSession {
    backend: RealBackend,
    session: RealSession,
    stream: RealPlayStream,
    terminal: Option<PlayStreamEnd>,
}

impl PlayStreamSession for RealPlayStreamSession {
    fn next_frame(&mut self, timeout: Duration) -> BackendResult<PlayStreamEvent> {
        if let Some(end) = self.terminal {
            return Ok(PlayStreamEvent::Ended(end));
        }
        match self.stream.frames.recv_timeout(timeout) {
            Ok(RealStreamEvent::Frame(frame)) => {
                let fb_info = frame.fb_info.ok_or_else(|| {
                    self.backend.mark_faulted(&self.session);
                    BackendError::BackendUnavailable
                })?;
                let decoded = decode_captured_framebuffer(&frame.fb_lz4, &fb_info)
                    .inspect_err(|_| self.backend.mark_faulted(&self.session))?;
                {
                    let mut inner = self
                        .backend
                        .inner
                        .lock()
                        .expect("real backend mutex poisoned");
                    if let Some(active) = inner
                        .active
                        .as_mut()
                        .filter(|active| active.session_id == self.session.session_id)
                    {
                        active.current_icount = frame.icount;
                        active.current_frame = decoded.frame;
                        active.last_preview_frame = decoded.frame;
                        active.frame_base_known = true;
                        active.preview_stale = false;
                    }
                }
                Ok(PlayStreamEvent::Frame(PlayStepOutcome {
                    session_id: self.session.session_id.clone(),
                    frame: decoded.frame,
                    width: decoded.width,
                    height: decoded.height,
                    png_bytes: decoded.png_bytes,
                }))
            }
            Ok(RealStreamEvent::Ended(end)) => {
                self.terminal = Some(end);
                if matches!(
                    end.reason,
                    PlayStreamEndReason::UnexpectedEnd | PlayStreamEndReason::Faulted
                ) {
                    self.backend.mark_faulted(&self.session);
                }
                Ok(PlayStreamEvent::Ended(end))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(PlayStreamEvent::TimedOut),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The streaming task died without a terminal event.
                let end = PlayStreamEnd {
                    reason: PlayStreamEndReason::Faulted,
                    first_frame_icount: None,
                    last_frame_icount: None,
                    done_icount: None,
                };
                self.terminal = Some(end);
                self.backend.mark_faulted(&self.session);
                Ok(PlayStreamEvent::Ended(end))
            }
        }
    }

    fn stop(&mut self) -> BackendResult<RunBoundary> {
        let _ = self.stream.cancel.send(true);
        // Drain anything already delivered so a buffered terminal fault is not
        // lost; the cancel above stops further production.
        while let Ok(event) = self.stream.frames.try_recv() {
            if let RealStreamEvent::Ended(PlayStreamEnd {
                reason: PlayStreamEndReason::Faulted,
                ..
            }) = event
            {
                self.backend.mark_faulted(&self.session);
            }
        }

        // The worker parks the slot Paused at the next frame boundary once the
        // cancel lands; introspection fails FailedPrecondition until then.
        let deadline = std::time::Instant::now() + PLAY_STREAM_STOP_TIMEOUT;
        let outcome = loop {
            match self
                .backend
                .worker
                .frame_counter(self.session.lease.clone())
            {
                Ok(outcome) => break outcome,
                Err(RealWorkerFailure::FailedPrecondition)
                    if std::time::Instant::now() < deadline =>
                {
                    thread::sleep(PLAY_STREAM_STOP_POLL);
                }
                Err(error) => {
                    self.backend.mark_faulted(&self.session);
                    return Err(error.into());
                }
            }
        };

        let (boundary, transitioned) = {
            let mut inner = self
                .backend
                .inner
                .lock()
                .expect("real backend mutex poisoned");
            let active = inner
                .active
                .as_mut()
                .filter(|active| active.session_id == self.session.session_id)
                .ok_or(BackendError::BackendUnavailable)?;
            let transitioned = active.state == SessionState::Playing;
            if transitioned {
                active.state = SessionState::Paused;
            }
            active.current_frame = outcome.frame;
            active.current_icount = outcome.icount;
            active.frame_base_known = true;
            active.preview_stale = active.last_preview_frame < active.current_frame;
            (active.boundary(), transitioned)
        };
        if transitioned {
            let _ = self.backend.append_real_event(
                &self.session.run_id,
                "session_paused",
                "real backend session paused",
            );
        }
        Ok(boundary)
    }
}

#[derive(Debug, Default)]
struct RealBackendInner {
    active: Option<RealSession>,
    next_sequence: u64,
    next_event_seq: u64,
    capture_jobs: BTreeMap<CaptureJobId, RealCaptureJobState>,
    capture_idempotency: BTreeMap<(SessionId, String), CaptureJobId>,
    starting: bool,
    pending_cleanup: Vec<LeaseRecord>,
}

#[derive(Debug, Clone)]
struct RealCaptureJobState {
    job_id: CaptureJobId,
    capture_id: String,
    status: CaptureJobStatus,
    public: Option<RealCapturePublicProjection>,
}

impl RealCaptureJobState {
    fn capture_job(&self) -> CaptureJob {
        CaptureJob {
            job_id: self.job_id.clone(),
            status: self.status,
            capture_id: (self.status == CaptureJobStatus::Completed)
                .then(|| self.capture_id.clone()),
            public: (self.status == CaptureJobStatus::Completed)
                .then(|| self.public.clone())
                .flatten(),
        }
    }
}

#[derive(Debug, Clone)]
struct RealSession {
    operation_id: String,
    session_id: SessionId,
    run_id: RunId,
    lease: dh::Lease,
    state: SessionState,
    current_frame: FrameCounter,
    current_icount: u64,
    last_preview_frame: FrameCounter,
    preview_stale: bool,
    last_applied_input_frame: FrameCounter,
    frame_base_known: bool,
    input_in_flight: bool,
    active_capture_job_id: Option<CaptureJobId>,
    applied_inputs: Vec<AppliedInputFrame>,
    capabilities: BackendCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordedCleanup {
    Destroyed,
    Stale,
}

impl RealSession {
    fn backend_session(&self) -> BackendSession {
        BackendSession {
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            state: self.state,
            current_frame: self.current_frame,
            capabilities: self.capabilities,
        }
    }

    fn status(&self, backend_mode: BackendMode) -> RunStatus {
        RunStatus {
            session_id: self.session_id.clone(),
            run_id: self.run_id.clone(),
            state: self.state,
            backend_mode,
            current_frame: self.current_frame,
            capabilities: self.capabilities,
            last_applied_input_frame: self.last_applied_input_frame,
            last_preview_frame: self.last_preview_frame,
            preview_stale: self.preview_stale,
            active_capture_job_id: self.active_capture_job_id.clone(),
        }
    }

    fn boundary(&self) -> RunBoundary {
        RunBoundary {
            session_id: self.session_id.clone(),
            state: self.state,
            current_frame: self.current_frame,
            preview_stale: self.preview_stale,
        }
    }
}

#[derive(Clone)]
struct RealWorkerThread {
    tx: mpsc::Sender<RealWorkerCommand>,
}

impl RealWorkerThread {
    fn new(endpoint: crate::private_config::HypervisorEndpoint) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("real-backend-worker".to_string())
            .spawn(move || run_real_worker_thread(endpoint, rx))
            .expect("real backend worker thread starts");
        Self { tx }
    }

    fn start(&self, command: RealStartCommand) -> RealWorkerResult<RealStartOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::Start { command, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn stop(&self, lease: dh::Lease) -> RealWorkerResult<()> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::Stop { lease, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn list_slots(&self) -> RealWorkerResult<Vec<u64>> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::ListSlots { reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn pause(&self, lease: dh::Lease) -> RealWorkerResult<RealPauseOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::Pause { lease, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn resume(&self, lease: dh::Lease) -> RealWorkerResult<RealRunOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::Resume { lease, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn status(&self, slot_id: u64) -> RealWorkerResult<RealSlotStatus> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::Status { slot_id, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn framebuffer(&self, lease: dh::Lease) -> RealWorkerResult<RealFramebufferOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::Framebuffer { lease, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn frame_counter(&self, lease: dh::Lease) -> RealWorkerResult<RealFrameCounterOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::FrameCounter { lease, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn inject_input(
        &self,
        lease: dh::Lease,
        target_frame: u32,
        pad_word: PadWord,
    ) -> RealWorkerResult<RealInjectInputOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::InjectInput {
                lease,
                target_frame,
                pad_word,
                reply,
            })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn capture(
        &self,
        lease: dh::Lease,
        capture_spec: dh::CaptureSpec,
    ) -> RealWorkerResult<RealCaptureOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::Capture {
                lease,
                capture_spec,
                reply,
            })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn run_one_frame_captured(&self, lease: dh::Lease) -> RealWorkerResult<RealCapturedRunOutcome> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::RunFrameCaptured { lease, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }

    fn play_stream_open(&self, lease: dh::Lease) -> RealWorkerResult<RealPlayStream> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(RealWorkerCommand::PlayStreamOpen { lease, reply })
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?;
        recv_worker_reply(rx)
    }
}

type RealWorkerResult<T> = Result<T, RealWorkerFailure>;
type WorkerClient = dh::hypervisor_worker_client::HypervisorWorkerClient<tonic::transport::Channel>;

fn recv_worker_reply<T>(rx: mpsc::Receiver<RealWorkerResult<T>>) -> RealWorkerResult<T> {
    rx.recv_timeout(REAL_WORKER_REPLY_TIMEOUT)
        .map_err(|_| RealWorkerFailure::BackendUnavailable)?
}

enum RealWorkerCommand {
    Start {
        command: RealStartCommand,
        reply: mpsc::Sender<RealWorkerResult<RealStartOutcome>>,
    },
    Stop {
        lease: dh::Lease,
        reply: mpsc::Sender<RealWorkerResult<()>>,
    },
    ListSlots {
        reply: mpsc::Sender<RealWorkerResult<Vec<u64>>>,
    },
    Pause {
        lease: dh::Lease,
        reply: mpsc::Sender<RealWorkerResult<RealPauseOutcome>>,
    },
    Resume {
        lease: dh::Lease,
        reply: mpsc::Sender<RealWorkerResult<RealRunOutcome>>,
    },
    Status {
        slot_id: u64,
        reply: mpsc::Sender<RealWorkerResult<RealSlotStatus>>,
    },
    Framebuffer {
        lease: dh::Lease,
        reply: mpsc::Sender<RealWorkerResult<RealFramebufferOutcome>>,
    },
    FrameCounter {
        lease: dh::Lease,
        reply: mpsc::Sender<RealWorkerResult<RealFrameCounterOutcome>>,
    },
    InjectInput {
        lease: dh::Lease,
        target_frame: u32,
        pad_word: PadWord,
        reply: mpsc::Sender<RealWorkerResult<RealInjectInputOutcome>>,
    },
    Capture {
        lease: dh::Lease,
        capture_spec: dh::CaptureSpec,
        reply: mpsc::Sender<RealWorkerResult<RealCaptureOutcome>>,
    },
    /// One frame advanced and captured in a single Run RPC (Play fast path).
    RunFrameCaptured {
        lease: dh::Lease,
        reply: mpsc::Sender<RealWorkerResult<RealCapturedRunOutcome>>,
    },
    /// Open a `RunWithFrameCapture` stream. The stream gets its own lane — a
    /// task on the worker runtime forwards frames into the returned channel —
    /// so this command loop stays responsive for every other RPC while a
    /// stream is open.
    PlayStreamOpen {
        lease: dh::Lease,
        reply: mpsc::Sender<RealWorkerResult<RealPlayStream>>,
    },
}

#[derive(Debug)]
enum RealWorkerFailure {
    BackendUnavailable,
    FailedPrecondition,
    FrameStale,
    StaleLease,
    AllocationRejected,
}

impl From<RealWorkerFailure> for BackendError {
    fn from(failure: RealWorkerFailure) -> Self {
        match failure {
            RealWorkerFailure::BackendUnavailable
            | RealWorkerFailure::FailedPrecondition
            | RealWorkerFailure::FrameStale
            | RealWorkerFailure::StaleLease
            | RealWorkerFailure::AllocationRejected => BackendError::BackendUnavailable,
        }
    }
}

enum RealStartCommand {
    RestoreSnapshot {
        snapshot_hash: [u8; 32],
    },
    CreateVm {
        config: dh::MachineConfig,
        entropy_seed: Vec<u8>,
    },
}

struct RealStartOutcome {
    lease: dh::Lease,
    state: SessionState,
    current_frame: FrameCounter,
    current_icount: u64,
}

struct RealPauseOutcome {
    current_icount: u64,
}

struct RealRunOutcome {
    faulted: bool,
    current_icount: u64,
    current_frame: Option<FrameCounter>,
}

struct RealSlotStatus {
    icount: u64,
}

struct RealFramebufferOutcome {
    frame: FrameCounter,
    icount: u64,
    width: u32,
    height: u32,
    png_bytes: Vec<u8>,
}

struct RealFrameCounterOutcome {
    frame: FrameCounter,
    icount: u64,
}

struct RealInjectInputOutcome {
    scheduled: u32,
}

/// Outcome of a single `Run{frame_budget=1, capture{framebuffer}}`.
struct RealCapturedRunOutcome {
    faulted: bool,
    current_icount: u64,
    fb_lz4: Vec<u8>,
    fb_info: dh::FbInfo,
}

/// One event forwarded from the worker's frame-capture stream.
enum RealStreamEvent {
    Frame(dh::CapturedFrame),
    Ended(PlayStreamEnd),
}

/// The bridge-side end of an open frame-capture stream: a bounded frame
/// channel (the backstop pacing mechanism) plus an out-of-band cancel that
/// bypasses the command lane entirely.
struct RealPlayStream {
    frames: mpsc::Receiver<RealStreamEvent>,
    cancel: tokio::sync::watch::Sender<bool>,
}

struct RealCaptureOutcome {
    snapshot_hash: Option<Vec<u8>>,
    input_log_id: Vec<u8>,
    state_hash: Option<Vec<u8>>,
    machine_config_hash: Vec<u8>,
    determinism_class: Option<dh::DeterminismClass>,
    feature_bytes: Vec<u8>,
    fb_lz4: Vec<u8>,
    fb_info: dh::FbInfo,
    frame_counter: u32,
    icount: u64,
    vns: u64,
}

#[derive(Default)]
struct SlotWatchCache {
    slots: BTreeMap<u64, dh::SlotInfo>,
    lagged: bool,
}

struct RealWorkerState {
    endpoint: crate::private_config::HypervisorEndpoint,
    client: Option<WorkerClient>,
    slot_cache: Arc<Mutex<SlotWatchCache>>,
}

fn run_real_worker_thread(
    endpoint: crate::private_config::HypervisorEndpoint,
    rx: mpsc::Receiver<RealWorkerCommand>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(1)
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            while let Ok(command) = rx.recv() {
                reply_unavailable(command);
            }
            return;
        }
    };

    let mut state = RealWorkerState {
        endpoint,
        client: None,
        slot_cache: Arc::new(Mutex::new(SlotWatchCache::default())),
    };
    while let Ok(command) = rx.recv() {
        match command {
            RealWorkerCommand::Start { command, reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.start(command)));
            }
            RealWorkerCommand::Stop { lease, reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.stop(lease)));
            }
            RealWorkerCommand::ListSlots { reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.list_slots()));
            }
            RealWorkerCommand::Pause { lease, reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.pause(lease)));
            }
            RealWorkerCommand::Resume { lease, reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.resume(lease)));
            }
            RealWorkerCommand::Status { slot_id, reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.status(slot_id)));
            }
            RealWorkerCommand::Framebuffer { lease, reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.framebuffer(lease)));
            }
            RealWorkerCommand::FrameCounter { lease, reply } => {
                let _ = reply.send(run_worker_future(&runtime, state.frame_counter(lease)));
            }
            RealWorkerCommand::InjectInput {
                lease,
                target_frame,
                pad_word,
                reply,
            } => {
                let _ = reply.send(run_worker_future(
                    &runtime,
                    state.inject_input(lease, target_frame, pad_word),
                ));
            }
            RealWorkerCommand::Capture {
                lease,
                capture_spec,
                reply,
            } => {
                let _ = reply.send(run_worker_future(
                    &runtime,
                    state.capture(lease, capture_spec),
                ));
            }
            RealWorkerCommand::RunFrameCaptured { lease, reply } => {
                let _ = reply.send(run_worker_future(
                    &runtime,
                    state.run_one_frame_captured(lease),
                ));
            }
            RealWorkerCommand::PlayStreamOpen { lease, reply } => {
                // Only the stream *open* runs on the command lane (bounded by
                // the RPC timeout); consumption moves to its own task so the
                // loop keeps servicing commands for the whole play session.
                let result = run_worker_future(&runtime, state.play_stream_open(lease));
                let _ = reply.send(result.map(|stream| {
                    let (frames_tx, frames_rx) = mpsc::sync_channel(PLAY_STREAM_CHANNEL_CAPACITY);
                    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
                    runtime.spawn(consume_play_stream(stream, frames_tx, cancel_rx));
                    RealPlayStream {
                        frames: frames_rx,
                        cancel: cancel_tx,
                    }
                }));
            }
        }
    }
}

/// Forward worker frame-capture events into the bounded bridge channel until
/// the stream ends or the bridge cancels. Dropping the stream on exit cancels
/// the RPC, which makes the worker park the slot Paused at the next frame
/// boundary.
async fn consume_play_stream(
    mut stream: tonic::Streaming<dh::FrameCaptureEvent>,
    frames: mpsc::SyncSender<RealStreamEvent>,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) {
    let mut first_frame_icount = None;
    let mut last_frame_icount = None;
    loop {
        let message = tokio::select! {
            _ = cancel.changed() => return,
            message = stream.message() => message,
        };
        let event = match message {
            Ok(Some(event)) => event,
            Ok(None) => {
                forward_stream_event(
                    &frames,
                    RealStreamEvent::Ended(PlayStreamEnd {
                        reason: PlayStreamEndReason::UnexpectedEnd,
                        first_frame_icount,
                        last_frame_icount,
                        done_icount: None,
                    }),
                    &cancel,
                )
                .await;
                return;
            }
            Err(status) => {
                log_worker_rpc_failure("RunWithFrameCapture", &status);
                forward_stream_event(
                    &frames,
                    RealStreamEvent::Ended(PlayStreamEnd {
                        reason: PlayStreamEndReason::Faulted,
                        first_frame_icount,
                        last_frame_icount,
                        done_icount: None,
                    }),
                    &cancel,
                )
                .await;
                return;
            }
        };
        match event.msg {
            Some(dh::frame_capture_event::Msg::Frame(frame)) => {
                first_frame_icount.get_or_insert(frame.icount);
                last_frame_icount = Some(frame.icount);
                if !forward_stream_event(&frames, RealStreamEvent::Frame(frame), &cancel).await {
                    return;
                }
            }
            Some(dh::frame_capture_event::Msg::Done(done)) => {
                let reason = dh::StopReason::try_from(done.reason)
                    .unwrap_or(dh::StopReason::StopUnspecified);
                let reason = match reason {
                    dh::StopReason::BudgetReached => PlayStreamEndReason::BudgetReached,
                    dh::StopReason::Faulted => PlayStreamEndReason::Faulted,
                    _ => PlayStreamEndReason::UnexpectedEnd,
                };
                forward_stream_event(
                    &frames,
                    RealStreamEvent::Ended(PlayStreamEnd {
                        reason,
                        first_frame_icount,
                        last_frame_icount,
                        done_icount: Some(done.icount),
                    }),
                    &cancel,
                )
                .await;
                return;
            }
            None => {}
        }
    }
}

/// Push one event into the bounded channel without blocking the (single)
/// runtime worker thread: retry-with-sleep keeps the timer/IO driver live, and
/// a full channel is exactly the backpressure that holds the worker's vCPU at
/// a frame boundary. Returns false when delivery is impossible (cancelled or
/// receiver dropped).
async fn forward_stream_event(
    frames: &mpsc::SyncSender<RealStreamEvent>,
    mut event: RealStreamEvent,
    cancel: &tokio::sync::watch::Receiver<bool>,
) -> bool {
    loop {
        if *cancel.borrow() {
            return false;
        }
        match frames.try_send(event) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Full(back)) => {
                event = back;
                tokio::time::sleep(PLAY_STREAM_SEND_RETRY).await;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
        }
    }
}

fn run_worker_future<T>(
    runtime: &tokio::runtime::Runtime,
    future: impl Future<Output = RealWorkerResult<T>>,
) -> RealWorkerResult<T> {
    runtime.block_on(async {
        tokio::time::timeout(REAL_WORKER_RPC_TIMEOUT, future)
            .await
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?
    })
}

fn reply_unavailable(command: RealWorkerCommand) {
    match command {
        RealWorkerCommand::Start { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::Stop { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::ListSlots { reply } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::Pause { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::Resume { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::Status { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::Framebuffer { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::FrameCounter { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::InjectInput { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::Capture { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::RunFrameCaptured { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
        RealWorkerCommand::PlayStreamOpen { reply, .. } => {
            let _ = reply.send(Err(RealWorkerFailure::BackendUnavailable));
        }
    }
}

impl RealWorkerState {
    async fn ensure_client(&mut self) -> RealWorkerResult<()> {
        if self.client.is_some() {
            return Ok(());
        }
        let client = connect_real_worker(&self.endpoint).await?;
        spawn_slot_watch(client.clone(), Arc::clone(&self.slot_cache));
        self.client = Some(client);
        Ok(())
    }

    async fn client(&mut self) -> RealWorkerResult<&mut WorkerClient> {
        self.ensure_client().await?;
        self.client
            .as_mut()
            .ok_or(RealWorkerFailure::BackendUnavailable)
    }

    async fn start(&mut self, command: RealStartCommand) -> RealWorkerResult<RealStartOutcome> {
        match command {
            RealStartCommand::RestoreSnapshot { snapshot_hash } => {
                let response = self
                    .client()
                    .await?
                    .restore_snapshot(dh::RestoreSnapshotRequest {
                        snapshot: Some(dh::SnapshotRef {
                            hash: snapshot_hash.to_vec(),
                        }),
                        entropy_seed: Vec::new(),
                        baseline: None,
                    })
                    .await
                    .map_err(|status| {
                        log_worker_rpc_failure("RestoreSnapshot", &status);
                        allocation_worker_failure_from_status(status)
                    })?
                    .into_inner();
                let lease = response
                    .lease
                    .ok_or(RealWorkerFailure::BackendUnavailable)?;
                Ok(RealStartOutcome {
                    lease,
                    state: SessionState::Paused,
                    current_frame: u64::from(response.frame_counter),
                    current_icount: 0,
                })
            }
            RealStartCommand::CreateVm {
                config,
                entropy_seed,
            } => {
                let response = self
                    .client()
                    .await?
                    .create_vm(dh::CreateVmRequest {
                        config: Some(config),
                        entropy_seed,
                    })
                    .await
                    .map_err(|status| {
                        log_worker_rpc_failure("CreateVm", &status);
                        allocation_worker_failure_from_status(status)
                    })?
                    .into_inner();
                let lease = response
                    .lease
                    .ok_or(RealWorkerFailure::BackendUnavailable)?;
                Ok(RealStartOutcome {
                    lease,
                    state: SessionState::Paused,
                    current_frame: 0,
                    current_icount: response.icount,
                })
            }
        }
    }

    async fn stop(&mut self, lease: dh::Lease) -> RealWorkerResult<()> {
        self.client()
            .await?
            .destroy_vm(dh::DestroyVmRequest { lease: Some(lease) })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("DestroyVm", &status);
                destroy_worker_failure_from_status(status)
            })?;
        Ok(())
    }

    async fn list_slots(&mut self) -> RealWorkerResult<Vec<u64>> {
        Ok(self
            .client()
            .await?
            .list_slots(dh::ListSlotsRequest {})
            .await
            .map_err(|status| {
                log_worker_rpc_failure("ListSlots", &status);
                RealWorkerFailure::BackendUnavailable
            })?
            .into_inner()
            .slots
            .into_iter()
            .filter(|slot| slot.state != dh::SlotState::Empty as i32)
            .map(|slot| slot.slot_id)
            .collect())
    }

    async fn pause(&mut self, lease: dh::Lease) -> RealWorkerResult<RealPauseOutcome> {
        let response = self
            .client()
            .await?
            .pause(dh::PauseRequest { lease: Some(lease) })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("Pause", &status);
                worker_failure_from_status(status)
            })?
            .into_inner();
        Ok(RealPauseOutcome {
            current_icount: response.icount,
        })
    }

    async fn resume(&mut self, lease: dh::Lease) -> RealWorkerResult<RealRunOutcome> {
        let response = self
            .client()
            .await?
            .run(dh::RunRequest {
                lease: Some(lease),
                hard_icount_cap: 0,
                capture: None,
                until: Some(dh::run_request::Until::FrameBudget(1)),
            })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("Run", &status);
                RealWorkerFailure::BackendUnavailable
            })?
            .into_inner();
        let reason =
            dh::StopReason::try_from(response.reason).unwrap_or(dh::StopReason::StopUnspecified);
        Ok(RealRunOutcome {
            faulted: reason == dh::StopReason::Faulted,
            current_icount: response.icount,
            current_frame: response
                .fb_info
                .as_ref()
                .map(|info| u64::from(info.frame_counter)),
        })
    }

    async fn status(&mut self, slot_id: u64) -> RealWorkerResult<RealSlotStatus> {
        let response = self
            .client()
            .await?
            .list_slots(dh::ListSlotsRequest {})
            .await
            .map_err(|status| {
                log_worker_rpc_failure("ListSlots", &status);
                RealWorkerFailure::BackendUnavailable
            })?
            .into_inner();
        let slot = response
            .slots
            .into_iter()
            .find(|slot| slot.slot_id == slot_id)
            .ok_or_else(|| {
                tracing::warn!(slot_id, "hypervisor slot for active session not found");
                RealWorkerFailure::BackendUnavailable
            })?;
        let state = dh::SlotState::try_from(slot.state).unwrap_or(dh::SlotState::SlotUnspecified);
        match state {
            dh::SlotState::PausedS | dh::SlotState::Running => Ok(RealSlotStatus {
                icount: slot.icount,
            }),
            dh::SlotState::SlotUnspecified
            | dh::SlotState::Empty
            | dh::SlotState::Frozen
            | dh::SlotState::FaultedS => {
                tracing::warn!(slot_id, state = ?state, "hypervisor slot in unusable state");
                Err(RealWorkerFailure::BackendUnavailable)
            }
        }
    }

    async fn framebuffer(&mut self, lease: dh::Lease) -> RealWorkerResult<RealFramebufferOutcome> {
        let response = self
            .client()
            .await?
            .get_framebuffer(dh::GetFramebufferRequest { lease: Some(lease) })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("GetFramebuffer", &status);
                worker_failure_from_status(status)
            })?
            .into_inner();
        let format = match dh::PixelFormat::try_from(response.format).map_err(|_| {
            tracing::warn!(format = response.format, "framebuffer pixel format unknown");
            RealWorkerFailure::BackendUnavailable
        })? {
            dh::PixelFormat::Xrgb8888 => RawFramebufferFormat::Xrgb8888,
            dh::PixelFormat::PfUnspecified | dh::PixelFormat::Rgb565 => {
                tracing::warn!(
                    format = response.format,
                    "framebuffer pixel format unsupported"
                );
                return Err(RealWorkerFailure::BackendUnavailable);
            }
        };
        let png_bytes = framebuffer_png(RawFramebuffer {
            width: response.width,
            height: response.height,
            stride: response.stride,
            format,
            pixels: &response.pixels,
        })
        .map_err(|_| {
            tracing::warn!(
                width = response.width,
                height = response.height,
                stride = response.stride,
                "framebuffer png encode failed"
            );
            RealWorkerFailure::BackendUnavailable
        })?;
        Ok(RealFramebufferOutcome {
            frame: u64::from(response.frame_counter),
            icount: response.icount,
            width: response.width,
            height: response.height,
            png_bytes,
        })
    }

    async fn frame_counter(
        &mut self,
        lease: dh::Lease,
    ) -> RealWorkerResult<RealFrameCounterOutcome> {
        let response = self
            .client()
            .await?
            .get_framebuffer(dh::GetFramebufferRequest { lease: Some(lease) })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("GetFramebuffer", &status);
                // Preserve FailedPrecondition: the streaming-stop poll retries
                // on it while the worker parks the slot at a frame boundary.
                worker_failure_from_status(status)
            })?
            .into_inner();
        Ok(RealFrameCounterOutcome {
            frame: u64::from(response.frame_counter),
            icount: response.icount,
        })
    }

    async fn inject_input(
        &mut self,
        lease: dh::Lease,
        target_frame: u32,
        pad_word: PadWord,
    ) -> RealWorkerResult<RealInjectInputOutcome> {
        let response = self
            .client()
            .await?
            .inject_inputs(dh::InjectInputsRequest {
                lease: Some(lease),
                events: vec![dh::ScheduledEvent {
                    at: Some(dh::scheduled_event::At::AtFrame(target_frame)),
                    event: Some(dh::scheduled_event::Event::PadSet(dh::PadSet {
                        port: 0,
                        buttons: u32::from(pad_word.raw()),
                    })),
                }],
            })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("InjectInputs", &status);
                input_worker_failure_from_status(status)
            })?
            .into_inner();
        Ok(RealInjectInputOutcome {
            scheduled: response.scheduled,
        })
    }

    async fn run_one_frame_captured(
        &mut self,
        lease: dh::Lease,
    ) -> RealWorkerResult<RealCapturedRunOutcome> {
        let response = self
            .client()
            .await?
            .run(dh::RunRequest {
                lease: Some(lease),
                hard_icount_cap: 0,
                capture: Some(dh::CaptureSpec {
                    ranges: Vec::new(),
                    framebuffer: true,
                }),
                until: Some(dh::run_request::Until::FrameBudget(1)),
            })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("Run", &status);
                RealWorkerFailure::BackendUnavailable
            })?
            .into_inner();
        let reason =
            dh::StopReason::try_from(response.reason).unwrap_or(dh::StopReason::StopUnspecified);
        if reason == dh::StopReason::Faulted {
            return Ok(RealCapturedRunOutcome {
                faulted: true,
                current_icount: response.icount,
                fb_lz4: Vec::new(),
                fb_info: dh::FbInfo::default(),
            });
        }
        let fb_info = response.fb_info.ok_or_else(|| {
            tracing::warn!("captured run response missing fb_info");
            RealWorkerFailure::BackendUnavailable
        })?;
        if response.fb_lz4.is_empty() {
            tracing::warn!("captured run response missing framebuffer payload");
            return Err(RealWorkerFailure::BackendUnavailable);
        }
        Ok(RealCapturedRunOutcome {
            faulted: false,
            current_icount: response.icount,
            fb_lz4: response.fb_lz4,
            fb_info,
        })
    }

    async fn play_stream_open(
        &mut self,
        lease: dh::Lease,
    ) -> RealWorkerResult<tonic::Streaming<dh::FrameCaptureEvent>> {
        let response = self
            .client()
            .await?
            .run_with_frame_capture(dh::RunWithFrameCaptureRequest {
                lease: Some(lease),
                until: Some(dh::run_with_frame_capture_request::Until::IcountBudget(
                    PLAY_STREAM_ICOUNT_BUDGET,
                )),
                hard_icount_cap: 0,
            })
            .await
            .map_err(|status| {
                log_worker_rpc_failure("RunWithFrameCapture", &status);
                worker_failure_from_status(status)
            })?;
        Ok(response.into_inner())
    }

    async fn capture(
        &mut self,
        lease: dh::Lease,
        capture_spec: dh::CaptureSpec,
    ) -> RealWorkerResult<RealCaptureOutcome> {
        let response = self
            .client()
            .await?
            .take_snapshot(dh::TakeSnapshotRequest {
                lease: Some(lease),
                seal_input_log: Some(true),
                capture: Some(capture_spec),
            })
            .await
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?
            .into_inner();
        let fb_info = response
            .fb_info
            .ok_or(RealWorkerFailure::BackendUnavailable)?;
        if response.feature_bytes.is_empty() || response.fb_lz4.is_empty() {
            return Err(RealWorkerFailure::BackendUnavailable);
        }
        Ok(RealCaptureOutcome {
            snapshot_hash: response.snapshot.map(|snapshot| snapshot.hash),
            input_log_id: response.input_log_id,
            state_hash: response.state_hash.map(|state_hash| state_hash.hash),
            machine_config_hash: response.machine_config_hash,
            determinism_class: response.determinism_class,
            feature_bytes: response.feature_bytes,
            fb_lz4: response.fb_lz4,
            fb_info,
            frame_counter: response.frame_counter,
            icount: response.icount,
            vns: response.vns,
        })
    }
}

async fn connect_real_worker(
    endpoint: &crate::private_config::HypervisorEndpoint,
) -> RealWorkerResult<WorkerClient> {
    if let Some(path) = endpoint.unix_path() {
        let uds_path = path.to_path_buf();
        return Endpoint::try_from("http://[::]:0")
            .map_err(|_| RealWorkerFailure::BackendUnavailable)?
            .connect_timeout(REAL_WORKER_RPC_TIMEOUT)
            .timeout(REAL_WORKER_RPC_TIMEOUT)
            .connect_with_connector(service_fn(move |_uri: tonic::transport::Uri| {
                let path = uds_path.clone();
                async move {
                    let stream = UnixStream::connect(path).await?;
                    Ok::<_, std::io::Error>(TokioIo::new(stream))
                }
            }))
            .await
            .map(dh::hypervisor_worker_client::HypervisorWorkerClient::new)
            .map_err(|_| RealWorkerFailure::BackendUnavailable);
    }

    let uri = endpoint
        .http_uri()
        .ok_or(RealWorkerFailure::BackendUnavailable)?;
    Endpoint::from_shared(uri.to_owned())
        .map_err(|_| RealWorkerFailure::BackendUnavailable)?
        .connect_timeout(REAL_WORKER_RPC_TIMEOUT)
        .timeout(REAL_WORKER_RPC_TIMEOUT)
        .connect()
        .await
        .map(dh::hypervisor_worker_client::HypervisorWorkerClient::new)
        .map_err(|_| RealWorkerFailure::BackendUnavailable)
}

fn spawn_slot_watch(mut client: WorkerClient, slot_cache: Arc<Mutex<SlotWatchCache>>) {
    tokio::spawn(async move {
        let stream = client.watch_slots(dh::WatchSlotsRequest {}).await;
        let Ok(response) = stream else {
            slot_cache
                .lock()
                .expect("slot watch cache mutex poisoned")
                .lagged = true;
            return;
        };
        let mut stream = response.into_inner();
        loop {
            match stream.message().await {
                Ok(Some(event)) => {
                    if let Some(slot) = event.slot {
                        slot_cache
                            .lock()
                            .expect("slot watch cache mutex poisoned")
                            .slots
                            .insert(slot.slot_id, slot);
                    }
                }
                Ok(None) => {
                    slot_cache
                        .lock()
                        .expect("slot watch cache mutex poisoned")
                        .lagged = true;
                    return;
                }
                Err(_) => {
                    slot_cache
                        .lock()
                        .expect("slot watch cache mutex poisoned")
                        .lagged = true;
                    return;
                }
            }
        }
    });
}

fn worker_failure_from_status(status: tonic::Status) -> RealWorkerFailure {
    if status.code() == Code::FailedPrecondition {
        RealWorkerFailure::FailedPrecondition
    } else {
        RealWorkerFailure::BackendUnavailable
    }
}

fn destroy_worker_failure_from_status(status: tonic::Status) -> RealWorkerFailure {
    if status.code() == Code::FailedPrecondition {
        if let Ok(detail) = dh::ErrorDetail::decode(status.details())
            && (detail.code == "stale_lease" || detail.code == "no_such_slot")
        {
            return RealWorkerFailure::StaleLease;
        }
        RealWorkerFailure::FailedPrecondition
    } else {
        RealWorkerFailure::BackendUnavailable
    }
}

fn allocation_worker_failure_from_status(status: tonic::Status) -> RealWorkerFailure {
    match status.code() {
        Code::InvalidArgument | Code::ResourceExhausted => RealWorkerFailure::AllocationRejected,
        _ => RealWorkerFailure::BackendUnavailable,
    }
}

fn input_worker_failure_from_status(status: tonic::Status) -> RealWorkerFailure {
    match status.code() {
        Code::InvalidArgument => RealWorkerFailure::FrameStale,
        Code::FailedPrecondition => RealWorkerFailure::FailedPrecondition,
        _ => RealWorkerFailure::BackendUnavailable,
    }
}

fn log_worker_rpc_failure(rpc: &'static str, status: &tonic::Status) {
    tracing::warn!(
        rpc,
        code = %status.code(),
        "hypervisor rpc failed"
    );
}

fn framebuffer_pixel_format_name(format: i32) -> BackendResult<&'static str> {
    match dh::PixelFormat::try_from(format).map_err(|_| BackendError::BackendUnavailable)? {
        dh::PixelFormat::Xrgb8888 => Ok("xrgb8888"),
        dh::PixelFormat::PfUnspecified | dh::PixelFormat::Rgb565 => {
            Err(BackendError::BackendUnavailable)
        }
    }
}

struct DecodedFramebuffer {
    frame: FrameCounter,
    width: u32,
    height: u32,
    png_bytes: Vec<u8>,
}

/// Decode a worker `fb_lz4` framebuffer (lz4 block with prepended size, the
/// same format on Run captures and TakeSnapshot captures) into the `/ws/frames`
/// PNG. Geometry is validated first so unexpected dimensions surface as a
/// clean error rather than a panic.
fn decode_captured_framebuffer(
    fb_lz4: &[u8],
    fb_info: &dh::FbInfo,
) -> BackendResult<DecodedFramebuffer> {
    let pixels = lz4_flex::decompress_size_prepended(fb_lz4).map_err(|_| {
        tracing::warn!("captured framebuffer lz4 decompress failed");
        BackendError::BackendUnavailable
    })?;
    validate_framebuffer_geometry(fb_info, pixels.len() as u64)?;
    let png_bytes = framebuffer_png(RawFramebuffer {
        width: fb_info.width,
        height: fb_info.height,
        stride: fb_info.stride,
        format: RawFramebufferFormat::Xrgb8888,
        pixels: &pixels,
    })
    .map_err(|_| {
        tracing::warn!(
            width = fb_info.width,
            height = fb_info.height,
            stride = fb_info.stride,
            "captured framebuffer png encode failed"
        );
        BackendError::BackendUnavailable
    })?;
    Ok(DecodedFramebuffer {
        frame: u64::from(fb_info.frame_counter),
        width: fb_info.width,
        height: fb_info.height,
        png_bytes,
    })
}

fn validate_framebuffer_geometry(fb_info: &dh::FbInfo, uncompressed_len: u64) -> BackendResult<()> {
    let bytes_per_pixel = match dh::PixelFormat::try_from(fb_info.format)
        .map_err(|_| BackendError::BackendUnavailable)?
    {
        dh::PixelFormat::Xrgb8888 => 4u64,
        dh::PixelFormat::PfUnspecified | dh::PixelFormat::Rgb565 => {
            return Err(BackendError::BackendUnavailable);
        }
    };
    let row_width = u64::from(fb_info.width)
        .checked_mul(bytes_per_pixel)
        .ok_or(BackendError::BackendUnavailable)?;
    let stride = u64::from(fb_info.stride);
    let expected_len = stride
        .checked_mul(u64::from(fb_info.height))
        .ok_or(BackendError::BackendUnavailable)?;
    if fb_info.width == 0
        || fb_info.height == 0
        || fb_info.stride == 0
        || stride < row_width
        || expected_len != uncompressed_len
    {
        return Err(BackendError::BackendUnavailable);
    }
    Ok(())
}

fn capture_lifecycle_refs(outcome: &RealCaptureOutcome) -> CaptureLifecycleRefs {
    CaptureLifecycleRefs {
        snapshot_ref: outcome.snapshot_hash.as_deref().map(hex_ref),
        input_log_id: nonempty_hex_ref(&outcome.input_log_id),
        state_hash: outcome.state_hash.as_deref().map(hex_ref),
        machine_config_hash: nonempty_hex_ref(&outcome.machine_config_hash),
        determinism_class: outcome.determinism_class.as_ref().map(|class| {
            CaptureDeterminismClass {
                cpu_model: class.cpu_model.clone(),
                microcode: class.microcode.clone(),
                host_kernel: class.host_kernel.clone(),
                vmm_version: class.vmm_version.clone(),
            }
        }),
    }
}

fn public_capture_spec_hash(capture_spec_hash: &str) -> String {
    if capture_spec_hash.starts_with("blake3:") {
        capture_spec_hash.to_string()
    } else {
        "private-capture-spec".to_string()
    }
}

fn nonempty_hex_ref(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty()).then(|| hex_ref(bytes))
}

fn hex_ref(bytes: &[u8]) -> String {
    let mut output = String::with_capacity("blake3:".len() + bytes.len() * 2);
    output.push_str("blake3:");
    for byte in bytes {
        output.push(hex_char_private(byte >> 4));
        output.push(hex_char_private(byte & 0x0f));
    }
    output
}

fn hex_char_private(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => '0',
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateCreateVmFile {
    schema_version: u16,
    machine_config: PrivateMachineConfig,
    entropy_seed: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateMachineConfig {
    version: u32,
    mem_bytes: u64,
    vcpus: u32,
    clock_num: u32,
    clock_den: u32,
    base_image_hash: String,
    boot: PrivateBootSpec,
    epoch_len: u64,
    hash_epochs: String,
    skid_margin: u32,
    cpuid_table: Vec<PrivateCpuidLeaf>,
    device_set: Vec<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateBootSpec {
    elf: Option<PrivateElfBoot>,
    bzimage: Option<PrivateBzImageBoot>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateElfBoot {
    kernel_hash: String,
    cmdline: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateBzImageBoot {
    kernel_hash: String,
    initramfs_hash: String,
    cmdline: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateCpuidLeaf {
    function: u32,
    index: u32,
    flags: u32,
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
}

struct PrivateCreateVmConfig {
    config: dh::MachineConfig,
    entropy_seed: Vec<u8>,
}

#[derive(Debug)]
struct PrivateCreateVmConfigError;

fn parse_private_create_vm_config(
    bytes: &[u8],
) -> Result<PrivateCreateVmConfig, PrivateCreateVmConfigError> {
    let file: PrivateCreateVmFile =
        serde_json::from_slice(bytes).map_err(|_| PrivateCreateVmConfigError)?;
    if file.schema_version != 1 {
        return Err(PrivateCreateVmConfigError);
    }
    let entropy_seed = parse_hex32_private(&file.entropy_seed)?.to_vec();
    let config = private_machine_config_to_proto(file.machine_config)?;
    Ok(PrivateCreateVmConfig {
        config,
        entropy_seed,
    })
}

fn private_machine_config_to_proto(
    config: PrivateMachineConfig,
) -> Result<dh::MachineConfig, PrivateCreateVmConfigError> {
    if config.version != 1
        || config.mem_bytes == 0
        || config.mem_bytes % (2 * 1024 * 1024) != 0
        || config.vcpus != 1
        || config.clock_den == 0
    {
        return Err(PrivateCreateVmConfigError);
    }

    let mut cpuid_table = config
        .cpuid_table
        .into_iter()
        .map(|leaf| {
            (
                (leaf.function, leaf.index),
                dh::CpuidLeaf {
                    function: leaf.function,
                    index: leaf.index,
                    flags: leaf.flags,
                    eax: leaf.eax,
                    ebx: leaf.ebx,
                    ecx: leaf.ecx,
                    edx: leaf.edx,
                },
            )
        })
        .collect::<Vec<_>>();
    cpuid_table.sort_by_key(|(key, _)| *key);
    for pair in cpuid_table.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(PrivateCreateVmConfigError);
        }
    }

    let mut device_set = config.device_set;
    if device_set.iter().any(|device| *device > u16::MAX as u32) {
        return Err(PrivateCreateVmConfigError);
    }
    device_set.sort_unstable();
    for pair in device_set.windows(2) {
        if pair[0] == pair[1] {
            return Err(PrivateCreateVmConfigError);
        }
    }

    let hash_epochs = match config.hash_epochs.as_str() {
        "epochs_on" => dh::HashEpochs::EpochsOn,
        "final_only" => dh::HashEpochs::FinalOnly,
        _ => return Err(PrivateCreateVmConfigError),
    };

    Ok(dh::MachineConfig {
        version: config.version,
        mem_bytes: config.mem_bytes,
        vcpus: config.vcpus,
        clock_num: config.clock_num,
        clock_den: config.clock_den,
        base_image_hash: parse_hex32_private(&config.base_image_hash)?.to_vec(),
        boot: Some(private_boot_to_proto(config.boot)?),
        epoch_len: config.epoch_len,
        hash_epochs: hash_epochs as i32,
        skid_margin: config.skid_margin,
        cpuid_table: cpuid_table.into_iter().map(|(_, leaf)| leaf).collect(),
        device_set,
    })
}

fn private_boot_to_proto(
    boot: PrivateBootSpec,
) -> Result<dh::BootSpec, PrivateCreateVmConfigError> {
    match (boot.elf, boot.bzimage) {
        (Some(elf), None) => Ok(dh::BootSpec {
            kind: Some(dh::boot_spec::Kind::Elf(dh::ElfBoot {
                kernel_hash: parse_hex32_private(&elf.kernel_hash)?.to_vec(),
                cmdline: elf.cmdline.into_bytes(),
            })),
        }),
        (None, Some(bzimage)) => Ok(dh::BootSpec {
            kind: Some(dh::boot_spec::Kind::Bzimage(dh::BzImageBoot {
                kernel_hash: parse_hex32_private(&bzimage.kernel_hash)?.to_vec(),
                initramfs_hash: parse_hex32_private(&bzimage.initramfs_hash)?.to_vec(),
                cmdline: bzimage.cmdline.into_bytes(),
            })),
        }),
        _ => Err(PrivateCreateVmConfigError),
    }
}

fn parse_hex32_private(value: &str) -> Result<[u8; 32], PrivateCreateVmConfigError> {
    let value = value.trim();
    if value.len() != 64 {
        return Err(PrivateCreateVmConfigError);
    }

    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble_private(chunk[0]).ok_or(PrivateCreateVmConfigError)?;
        let low = hex_nibble_private(chunk[1]).ok_or(PrivateCreateVmConfigError)?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_nibble_private(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn real_session_id(sequence: u64) -> String {
    format!("real-session-{sequence:04}")
}

fn real_run_id(sequence: u64) -> String {
    format!("real-run-{sequence:04}")
}

fn real_capture_ids(session_id: &str, idempotency_key: &str) -> (String, String) {
    let key = idempotency_key.replace('-', "");
    (
        format!("real-capture-job-{session_id}-{key}"),
        format!("real-capture-{session_id}-{key}"),
    )
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

    fn play_start(&self, _session_id: SessionId) -> BackendResult<RunBoundary> {
        Err(BackendError::BackendUnavailable)
    }

    fn play_step(&self, _session_id: SessionId) -> BackendResult<PlayStepOutcome> {
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
