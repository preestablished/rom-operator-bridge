use crate::{
    artifacts::{InputRejectionRow, PrivateArtifactStore},
    backend::{
        BackendError, BridgeBackend, FrameCounter, InputScheduleReceipt, InputScheduleRequest,
        RunStatus, SessionId, SessionState,
    },
    input::PadWord,
};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, fmt};
use thiserror::Error;

pub const DEFAULT_INPUT_LEAD_FRAMES: FrameCounter = 1;
pub const FRAME_STALE_REASON_CODE: &str = "frame_stale";
pub const PUBLIC_INPUT_REJECTION_MESSAGE: &str = "Input rejected.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserInputState {
    pub session_id: SessionId,
    pub run_id: String,
    pub client_seq: u64,
    pub occurred_at: String,
    pub pad_word: PadWord,
}

impl BrowserInputState {
    pub fn new(
        session_id: impl Into<SessionId>,
        run_id: impl Into<String>,
        client_seq: u64,
        occurred_at: impl Into<String>,
        pad_word: PadWord,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            run_id: run_id.into(),
            client_seq,
            occurred_at: occurred_at.into(),
            pad_word,
        }
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
}

impl InputScheduleOutcome {
    fn applied(input: &BrowserInputState, receipt: &InputScheduleReceipt) -> Self {
        Self {
            client_seq: input.client_seq,
            status: InputScheduleStatus::Applied,
            assigned_frame: Some(receipt.assigned_frame),
            pad_word: receipt.pad_word.raw(),
        }
    }

    fn queued(input: &BrowserInputState) -> Self {
        Self {
            client_seq: input.client_seq,
            status: InputScheduleStatus::Queued,
            assigned_frame: None,
            pad_word: input.pad_word.raw(),
        }
    }

    fn dropped(input: &BrowserInputState) -> Self {
        Self {
            client_seq: input.client_seq,
            status: InputScheduleStatus::Dropped,
            assigned_frame: None,
            pad_word: input.pad_word.raw(),
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
    fn frame_stale(input: &BrowserInputState) -> Self {
        Self {
            run_id: input.run_id.clone(),
            client_seq: input.client_seq,
            occurred_at: input.occurred_at.clone(),
            reason_code: FRAME_STALE_REASON_CODE.to_string(),
            public_message: PUBLIC_INPUT_REJECTION_MESSAGE.to_string(),
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
    #[error("frame counter overflow while scheduling from {current_frame}")]
    FrameCounterOverflow { current_frame: FrameCounter },
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
        write!(formatter, "{:?}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct InputScheduler {
    lead_frames: FrameCounter,
    pending: VecDeque<BrowserInputState>,
    applied_frames: Vec<AppliedInputFrame>,
    last_assigned_frame: Option<FrameCounter>,
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
        Self {
            lead_frames: lead_frames.max(1),
            pending: VecDeque::new(),
            applied_frames: Vec::new(),
            last_assigned_frame: None,
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
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

        if status.state == SessionState::Paused {
            let outcome = InputScheduleOutcome::queued(&input);
            self.pending.push_back(input);
            return Ok(outcome);
        }

        self.apply_with_status(backend, input, status, rejection_sink)
    }

    pub fn flush_pending(
        &mut self,
        backend: &dyn BridgeBackend,
        session_id: &str,
        rejection_sink: &mut dyn InputRejectionSink,
    ) -> Result<Vec<InputScheduleOutcome>, InputSchedulerError> {
        let status = backend.status(session_id.to_string())?;
        if status.state == SessionState::Paused {
            return Ok(self
                .pending
                .iter()
                .filter(|input| input.session_id == session_id)
                .map(InputScheduleOutcome::queued)
                .collect());
        }

        let mut outcomes = Vec::new();
        let mut remaining = VecDeque::new();

        while let Some(input) = self.pending.pop_front() {
            if input.session_id == session_id {
                outcomes.push(self.apply_with_status(
                    backend,
                    input,
                    status.clone(),
                    rejection_sink,
                )?);
            } else {
                remaining.push_back(input);
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
        rejection_sink: &mut dyn InputRejectionSink,
    ) -> Result<InputScheduleOutcome, InputSchedulerError> {
        if status.state != SessionState::Running {
            return Err(InputSchedulerError::SessionNotAcceptingInput {
                session_id: status.session_id,
                state: status.state.into(),
            });
        }

        let target_frame = self.next_target_frame(status.current_frame)?;
        let request = InputScheduleRequest {
            session_id: input.session_id.clone(),
            target_frame,
            pad_word: input.pad_word,
        };

        match backend.inject_input(request) {
            Ok(receipt) => self.record_applied(&input, receipt),
            Err(BackendError::FrameStale { .. }) => {
                self.retry_after_stale_frame(backend, input, rejection_sink)
            }
            Err(error) => Err(InputSchedulerError::Backend(error)),
        }
    }

    fn retry_after_stale_frame(
        &mut self,
        backend: &dyn BridgeBackend,
        input: BrowserInputState,
        rejection_sink: &mut dyn InputRejectionSink,
    ) -> Result<InputScheduleOutcome, InputSchedulerError> {
        let refreshed = backend.status(input.session_id.clone())?;
        if refreshed.state != SessionState::Running {
            return Err(InputSchedulerError::SessionNotAcceptingInput {
                session_id: refreshed.session_id,
                state: refreshed.state.into(),
            });
        }

        let retry_frame = self.next_target_frame(refreshed.current_frame)?;
        let retry = InputScheduleRequest {
            session_id: input.session_id.clone(),
            target_frame: retry_frame,
            pad_word: input.pad_word,
        };

        match backend.inject_input(retry) {
            Ok(receipt) => self.record_applied(&input, receipt),
            Err(BackendError::FrameStale { .. }) => {
                rejection_sink
                    .record_input_rejection(&InputRejectionRecord::frame_stale(&input))?;
                Ok(InputScheduleOutcome::dropped(&input))
            }
            Err(error) => Err(InputSchedulerError::Backend(error)),
        }
    }

    fn next_target_frame(
        &self,
        current_frame: FrameCounter,
    ) -> Result<FrameCounter, InputSchedulerError> {
        let future_frame = current_frame
            .checked_add(self.lead_frames)
            .ok_or(InputSchedulerError::FrameCounterOverflow { current_frame })?;

        match self.last_assigned_frame {
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
        input: &BrowserInputState,
        receipt: InputScheduleReceipt,
    ) -> Result<InputScheduleOutcome, InputSchedulerError> {
        if let Some(previous_frame) = self.last_assigned_frame
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

        if self
            .applied_frames
            .iter()
            .any(|applied| applied.frame == receipt.assigned_frame)
        {
            return Err(InputSchedulerError::DuplicateAppliedFrame {
                frame: receipt.assigned_frame,
            });
        }

        self.last_assigned_frame = Some(receipt.assigned_frame);
        self.applied_frames.push(AppliedInputFrame::new(
            receipt.assigned_frame,
            receipt.pad_word,
        ));

        Ok(InputScheduleOutcome::applied(input, &receipt))
    }
}
