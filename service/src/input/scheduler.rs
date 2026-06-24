use crate::{
    artifacts::{InputRejectionRow, PrivateArtifactStore},
    backend::{
        BackendError, BridgeBackend, FrameCounter, InputScheduleReceipt, InputScheduleRequest,
        RunStatus, SessionId, SessionState,
    },
    input::PadWord,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
};
use thiserror::Error;

pub const DEFAULT_INPUT_LEAD_FRAMES: FrameCounter = 1;
pub const PENDING_INPUT_LIMIT_PER_SESSION: usize = 120;
pub const FRAME_STALE_REASON_CODE: &str = "frame_stale";
pub const QUEUE_FULL_REASON_CODE: &str = "queue_full";
pub const SESSION_REPLACED_REASON_CODE: &str = "session_replaced";
pub const PUBLIC_INPUT_REJECTION_MESSAGE: &str = "Input rejected.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserInputState {
    pub session_id: SessionId,
    pub client_seq: u64,
    pub source_id: String,
    pub occurred_at: String,
    pub pad_word: PadWord,
}

impl BrowserInputState {
    pub fn new(
        session_id: impl Into<SessionId>,
        client_seq: u64,
        occurred_at: impl Into<String>,
        pad_word: PadWord,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            client_seq,
            source_id: "input".to_string(),
            occurred_at: occurred_at.into(),
            pad_word,
        }
    }

    pub fn with_source_id(mut self, source_id: impl Into<String>) -> Self {
        self.source_id = source_id.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedInputFrame {
    pub frame: FrameCounter,
    pub pad_word: u16,
}

impl AppliedInputFrame {
    fn new(frame: FrameCounter, pad_word: PadWord) -> Self {
        Self {
            frame,
            pad_word: pad_word.raw(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputScheduleStatus {
    Applied,
    Queued,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputScheduleOutcome {
    pub client_seq: u64,
    pub status: InputScheduleStatus,
    pub assigned_frame: Option<FrameCounter>,
    pub pad_word: u16,
    pub rejection: Option<InputRejectionNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputRejectionNotice {
    pub reason_code: String,
    pub public_message: String,
}

impl InputScheduleOutcome {
    fn applied(input: &BrowserInputState, receipt: &InputScheduleReceipt) -> Self {
        Self {
            client_seq: input.client_seq,
            status: InputScheduleStatus::Applied,
            assigned_frame: Some(receipt.assigned_frame),
            pad_word: receipt.pad_word.raw(),
            rejection: None,
        }
    }

    fn queued(input: &BrowserInputState) -> Self {
        Self {
            client_seq: input.client_seq,
            status: InputScheduleStatus::Queued,
            assigned_frame: None,
            pad_word: input.pad_word.raw(),
            rejection: None,
        }
    }

    fn dropped(input: &BrowserInputState, rejection: InputRejectionNotice) -> Self {
        Self {
            client_seq: input.client_seq,
            status: InputScheduleStatus::Dropped,
            assigned_frame: None,
            pad_word: input.pad_word.raw(),
            rejection: Some(rejection),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRejectionRecord {
    pub run_id: String,
    pub client_seq: u64,
    pub occurred_at: String,
    pub reason_code: String,
    pub public_message: String,
}

impl InputRejectionRecord {
    fn new_for_input(input: &BrowserInputState, run_id: &str, reason_code: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            client_seq: input.client_seq,
            occurred_at: input.occurred_at.clone(),
            reason_code: reason_code.to_string(),
            public_message: PUBLIC_INPUT_REJECTION_MESSAGE.to_string(),
        }
    }

    fn notice(&self) -> InputRejectionNotice {
        InputRejectionNotice {
            reason_code: self.reason_code.clone(),
            public_message: self.public_message.clone(),
        }
    }
}

pub trait InputRejectionSink {
    fn record_input_rejection(
        &mut self,
        rejection: &InputRejectionRecord,
    ) -> Result<(), InputSchedulerError>;
}

#[derive(Debug, Default)]
pub struct NoopInputRejectionSink;

impl InputRejectionSink for NoopInputRejectionSink {
    fn record_input_rejection(
        &mut self,
        _rejection: &InputRejectionRecord,
    ) -> Result<(), InputSchedulerError> {
        Ok(())
    }
}

impl InputRejectionSink for PrivateArtifactStore<'_> {
    fn record_input_rejection(
        &mut self,
        rejection: &InputRejectionRecord,
    ) -> Result<(), InputSchedulerError> {
        self.append_input_rejection(
            &rejection.run_id,
            &InputRejectionRow::new(
                &rejection.run_id,
                rejection.client_seq,
                &rejection.occurred_at,
                &rejection.reason_code,
                &rejection.public_message,
            ),
        )
        .map_err(|error| InputSchedulerError::RejectionSink(error.to_string()))?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InputSchedulerError {
    #[error("backend input scheduling failed: {0}")]
    Backend(#[from] BackendError),
    #[error("input lead frames must be at least 1")]
    InvalidLeadFrames,
    #[error("frame counter overflow while scheduling from {current_frame}")]
    FrameCounterOverflow { current_frame: FrameCounter },
    #[error(
        "backend returned status for session {actual_session_id}, expected {expected_session_id}"
    )]
    BackendStatusSessionMismatch {
        expected_session_id: SessionId,
        actual_session_id: SessionId,
    },
    #[error("session {session_id} is not accepting input in state {state}")]
    SessionNotAcceptingInput {
        session_id: SessionId,
        state: SessionStateLabel,
    },
    #[error("backend assigned duplicate input frame {frame}")]
    DuplicateAppliedFrame { frame: FrameCounter },
    #[error("backend assigned input frame {frame} after {previous_frame}")]
    OutOfOrderAppliedFrame {
        frame: FrameCounter,
        previous_frame: FrameCounter,
    },
    #[error(
        "backend input receipt session mismatch: expected {expected_session_id}, got {actual_session_id}"
    )]
    ReceiptSessionMismatch {
        expected_session_id: SessionId,
        actual_session_id: SessionId,
    },
    #[error(
        "backend input receipt pad word mismatch: expected {expected_pad_word:#06x}, got {actual_pad_word:#06x}"
    )]
    ReceiptPadWordMismatch {
        expected_pad_word: u16,
        actual_pad_word: u16,
    },
    #[error("backend assigned input frame {assigned_frame} before requested target {target_frame}")]
    ReceiptFrameBeforeTarget {
        assigned_frame: FrameCounter,
        target_frame: FrameCounter,
    },
    #[error("input rejection sink failed: {0}")]
    RejectionSink(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStateLabel(SessionState);

impl From<SessionState> for SessionStateLabel {
    fn from(value: SessionState) -> Self {
        Self(value)
    }
}

impl fmt::Display for SessionStateLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.0 {
            SessionState::Idle => "idle",
            SessionState::Starting => "starting",
            SessionState::Running => "running",
            SessionState::Paused => "paused",
            SessionState::CapturePending => "capture_pending",
            SessionState::Stopping => "stopping",
            SessionState::Stopped => "stopped",
            SessionState::Faulted => "faulted",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct InputRunKey {
    session_id: SessionId,
    run_id: String,
}

impl InputRunKey {
    fn from_status(
        expected_session_id: &str,
        status: &RunStatus,
    ) -> Result<Self, InputSchedulerError> {
        if status.session_id != expected_session_id {
            return Err(InputSchedulerError::BackendStatusSessionMismatch {
                expected_session_id: expected_session_id.to_string(),
                actual_session_id: status.session_id.clone(),
            });
        }

        Ok(Self {
            session_id: status.session_id.clone(),
            run_id: status.run_id.clone(),
        })
    }
}

#[derive(Debug, Clone)]
struct QueuedInputState {
    key: InputRunKey,
    input: BrowserInputState,
}

#[derive(Debug, Clone, Default)]
struct InputRunState {
    applied_frames: Vec<AppliedInputFrame>,
    last_assigned_frame: Option<FrameCounter>,
}

#[derive(Debug, Clone)]
pub struct InputScheduler {
    lead_frames: FrameCounter,
    pending: VecDeque<QueuedInputState>,
    run_states: BTreeMap<InputRunKey, InputRunState>,
    applied_frames: Vec<AppliedInputFrame>,
}

impl Default for InputScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl InputScheduler {
    pub fn new() -> Self {
        Self::with_lead_frames(DEFAULT_INPUT_LEAD_FRAMES)
    }

    pub fn with_lead_frames(lead_frames: FrameCounter) -> Self {
        Self::try_with_lead_frames(lead_frames).expect("lead_frames must be at least 1")
    }

    pub fn try_with_lead_frames(lead_frames: FrameCounter) -> Result<Self, InputSchedulerError> {
        if lead_frames == 0 {
            return Err(InputSchedulerError::InvalidLeadFrames);
        }

        Ok(Self {
            lead_frames,
            pending: VecDeque::new(),
            run_states: BTreeMap::new(),
            applied_frames: Vec::new(),
        })
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn pending_len_for_session(&self, session_id: &str) -> usize {
        self.pending_for_session(session_id)
    }

    pub fn applied_frames(&self) -> &[AppliedInputFrame] {
        &self.applied_frames
    }

    pub fn submit(
        &mut self,
        backend: &dyn BridgeBackend,
        input: BrowserInputState,
        rejection_sink: &mut dyn InputRejectionSink,
    ) -> Result<InputScheduleOutcome, InputSchedulerError> {
        let status = backend.status(input.session_id.clone())?;
        let key = InputRunKey::from_status(&input.session_id, &status)?;

        if status.state == SessionState::Paused {
            if self.pending_for_session(&input.session_id) >= PENDING_INPUT_LIMIT_PER_SESSION {
                let rejection = InputRejectionRecord::new_for_input(
                    &input,
                    &key.run_id,
                    QUEUE_FULL_REASON_CODE,
                );
                rejection_sink.record_input_rejection(&rejection)?;
                return Ok(InputScheduleOutcome::dropped(&input, rejection.notice()));
            }

            let outcome = InputScheduleOutcome::queued(&input);
            self.pending.push_back(QueuedInputState { key, input });
            return Ok(outcome);
        }

        self.apply_with_status(backend, input, status, key, rejection_sink)
    }

    pub fn flush_pending(
        &mut self,
        backend: &dyn BridgeBackend,
        session_id: &str,
        rejection_sink: &mut dyn InputRejectionSink,
    ) -> Result<Vec<InputScheduleOutcome>, InputSchedulerError> {
        let status = backend.status(session_id.to_string())?;
        let key = InputRunKey::from_status(session_id, &status)?;
        if status.state == SessionState::Paused {
            return Ok(self
                .pending
                .iter()
                .filter(|queued| queued.key.session_id == session_id)
                .map(|queued| InputScheduleOutcome::queued(&queued.input))
                .collect());
        }

        let mut outcomes = Vec::new();
        let mut remaining = VecDeque::new();

        while let Some(queued) = self.pending.pop_front() {
            if queued.key.session_id == session_id {
                if queued.key != key {
                    let rejection = InputRejectionRecord::new_for_input(
                        &queued.input,
                        &queued.key.run_id,
                        SESSION_REPLACED_REASON_CODE,
                    );
                    rejection_sink.record_input_rejection(&rejection)?;
                    outcomes.push(InputScheduleOutcome::dropped(
                        &queued.input,
                        rejection.notice(),
                    ));
                    continue;
                }

                match self.apply_with_status(
                    backend,
                    queued.input.clone(),
                    status.clone(),
                    key.clone(),
                    rejection_sink,
                ) {
                    Ok(outcome) => outcomes.push(outcome),
                    Err(error) => {
                        remaining.push_back(queued);
                        remaining.append(&mut self.pending);
                        self.pending = remaining;
                        return Err(error);
                    }
                }
            } else {
                remaining.push_back(queued);
            }
        }

        self.pending = remaining;
        Ok(outcomes)
    }

    fn apply_with_status(
        &mut self,
        backend: &dyn BridgeBackend,
        input: BrowserInputState,
        status: RunStatus,
        key: InputRunKey,
        rejection_sink: &mut dyn InputRejectionSink,
    ) -> Result<InputScheduleOutcome, InputSchedulerError> {
        if status.state != SessionState::Running {
            return Err(InputSchedulerError::SessionNotAcceptingInput {
                session_id: status.session_id,
                state: status.state.into(),
            });
        }

        let target_frame = self.next_target_frame(&key, status.current_frame)?;
        let request = InputScheduleRequest {
            session_id: input.session_id.clone(),
            target_frame,
            pad_word: input.pad_word,
            client_seq: input.client_seq,
            source_id: input.source_id.clone(),
        };

        match backend.inject_input(request) {
            Ok(receipt) => self.record_applied(&key, &input, target_frame, receipt),
            Err(BackendError::FrameStale { .. }) => {
                self.retry_after_stale_frame(backend, input, key, rejection_sink)
            }
            Err(error) => Err(InputSchedulerError::Backend(error)),
        }
    }

    fn retry_after_stale_frame(
        &mut self,
        backend: &dyn BridgeBackend,
        input: BrowserInputState,
        original_key: InputRunKey,
        rejection_sink: &mut dyn InputRejectionSink,
    ) -> Result<InputScheduleOutcome, InputSchedulerError> {
        let refreshed = backend.status(input.session_id.clone())?;
        let refreshed_key = InputRunKey::from_status(&input.session_id, &refreshed)?;
        if refreshed_key != original_key {
            let rejection = InputRejectionRecord::new_for_input(
                &input,
                &original_key.run_id,
                SESSION_REPLACED_REASON_CODE,
            );
            rejection_sink.record_input_rejection(&rejection)?;
            return Ok(InputScheduleOutcome::dropped(&input, rejection.notice()));
        }

        if refreshed.state != SessionState::Running {
            return Err(InputSchedulerError::SessionNotAcceptingInput {
                session_id: refreshed.session_id,
                state: refreshed.state.into(),
            });
        }

        let retry_frame = self.next_target_frame(&refreshed_key, refreshed.current_frame)?;
        let retry = InputScheduleRequest {
            session_id: input.session_id.clone(),
            target_frame: retry_frame,
            pad_word: input.pad_word,
            client_seq: input.client_seq,
            source_id: input.source_id.clone(),
        };

        match backend.inject_input(retry) {
            Ok(receipt) => self.record_applied(&refreshed_key, &input, retry_frame, receipt),
            Err(BackendError::FrameStale { .. }) => {
                let rejection = InputRejectionRecord::new_for_input(
                    &input,
                    &refreshed_key.run_id,
                    FRAME_STALE_REASON_CODE,
                );
                rejection_sink.record_input_rejection(&rejection)?;
                Ok(InputScheduleOutcome::dropped(&input, rejection.notice()))
            }
            Err(error) => Err(InputSchedulerError::Backend(error)),
        }
    }

    fn next_target_frame(
        &self,
        key: &InputRunKey,
        current_frame: FrameCounter,
    ) -> Result<FrameCounter, InputSchedulerError> {
        let future_frame = current_frame
            .checked_add(self.lead_frames)
            .ok_or(InputSchedulerError::FrameCounterOverflow { current_frame })?;

        match self
            .run_states
            .get(key)
            .and_then(|state| state.last_assigned_frame)
        {
            Some(last_assigned) if last_assigned >= future_frame => last_assigned
                .checked_add(1)
                .ok_or(InputSchedulerError::FrameCounterOverflow {
                    current_frame: last_assigned,
                }),
            _ => Ok(future_frame),
        }
    }

    fn record_applied(
        &mut self,
        key: &InputRunKey,
        input: &BrowserInputState,
        target_frame: FrameCounter,
        receipt: InputScheduleReceipt,
    ) -> Result<InputScheduleOutcome, InputSchedulerError> {
        if receipt.session_id != input.session_id {
            return Err(InputSchedulerError::ReceiptSessionMismatch {
                expected_session_id: input.session_id.clone(),
                actual_session_id: receipt.session_id,
            });
        }

        if receipt.pad_word != input.pad_word {
            return Err(InputSchedulerError::ReceiptPadWordMismatch {
                expected_pad_word: input.pad_word.raw(),
                actual_pad_word: receipt.pad_word.raw(),
            });
        }

        if receipt.assigned_frame < target_frame {
            return Err(InputSchedulerError::ReceiptFrameBeforeTarget {
                assigned_frame: receipt.assigned_frame,
                target_frame,
            });
        }

        let run_state = self.run_states.entry(key.clone()).or_default();

        if let Some(previous_frame) = run_state.last_assigned_frame
            && receipt.assigned_frame <= previous_frame
        {
            return if receipt.assigned_frame == previous_frame {
                Err(InputSchedulerError::DuplicateAppliedFrame {
                    frame: receipt.assigned_frame,
                })
            } else {
                Err(InputSchedulerError::OutOfOrderAppliedFrame {
                    frame: receipt.assigned_frame,
                    previous_frame,
                })
            };
        }

        if run_state
            .applied_frames
            .iter()
            .any(|applied| applied.frame == receipt.assigned_frame)
        {
            return Err(InputSchedulerError::DuplicateAppliedFrame {
                frame: receipt.assigned_frame,
            });
        }

        let applied = AppliedInputFrame::new(receipt.assigned_frame, receipt.pad_word);
        run_state.last_assigned_frame = Some(receipt.assigned_frame);
        run_state.applied_frames.push(applied.clone());
        self.applied_frames.push(applied);

        Ok(InputScheduleOutcome::applied(input, &receipt))
    }

    fn pending_for_session(&self, session_id: &str) -> usize {
        self.pending
            .iter()
            .filter(|queued| queued.key.session_id == session_id)
            .count()
    }
}
