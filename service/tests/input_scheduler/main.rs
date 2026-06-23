use rom_operator_bridge_service::{
    backend::{
        BackendCapabilities, BackendError, BackendMode, BackendResult, BackendSession,
        BridgeBackend, CaptureJob, CaptureRequest, FrameCounter, FramePreview,
        InputScheduleReceipt, InputScheduleRequest, RunBoundary, RunStatus, SessionId,
        SessionState, StartBackendSession, StopReason, StoppedSession,
    },
    input::{
        AppliedInputFrame, BrowserInputState, FRAME_STALE_REASON_CODE, InputRejectionRecord,
        InputRejectionSink, InputScheduleStatus, InputScheduler, InputSchedulerError,
        PUBLIC_INPUT_REJECTION_MESSAGE, PadButton, PadWord,
    },
};
use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};

const SESSION_ID: &str = "session-001";
const RUN_ID: &str = "run-001";
const OCCURRED_AT: &str = "2026-06-23T00:00:00Z";

#[test]
fn assigns_current_frame_plus_one_from_fake_frame_counter() {
    let backend = FakeBackend::new([(SessionState::Running, 41)]);
    let mut scheduler = InputScheduler::new();
    let mut rejections = RecordingRejectionSink::default();
    let input = input(1, PadWord::from_buttons([PadButton::A]));

    let outcome = scheduler
        .submit(&backend, input, &mut rejections)
        .expect("input schedules");

    assert_eq!(outcome.status, InputScheduleStatus::Applied);
    assert_eq!(outcome.assigned_frame, Some(42));
    assert_eq!(backend.request_frames(), [42]);
    assert_eq!(
        scheduler.applied_frames(),
        [AppliedInputFrame {
            frame: 42,
            pad_word: PadButton::A.mask()
        }]
    );
    assert!(rejections.records.is_empty());
}

#[test]
fn queues_paused_input_and_flushes_in_fifo_order_after_resume() {
    let backend = FakeBackend::new([
        (SessionState::Paused, 10),
        (SessionState::Paused, 10),
        (SessionState::Running, 20),
    ]);
    let mut scheduler = InputScheduler::new();
    let mut rejections = RecordingRejectionSink::default();

    let first = scheduler
        .submit(
            &backend,
            input(1, PadWord::from_buttons([PadButton::A])),
            &mut rejections,
        )
        .expect("first input queues");
    let second = scheduler
        .submit(
            &backend,
            input(2, PadWord::from_buttons([PadButton::B])),
            &mut rejections,
        )
        .expect("second input queues");

    assert_eq!(first.status, InputScheduleStatus::Queued);
    assert_eq!(second.status, InputScheduleStatus::Queued);
    assert_eq!(scheduler.pending_len(), 2);
    assert!(backend.request_frames().is_empty());

    let flushed = scheduler
        .flush_pending(&backend, SESSION_ID, &mut rejections)
        .expect("pending inputs flush");

    assert_eq!(
        flushed
            .iter()
            .map(|outcome| (outcome.client_seq, outcome.status, outcome.assigned_frame))
            .collect::<Vec<_>>(),
        vec![
            (1, InputScheduleStatus::Applied, Some(21)),
            (2, InputScheduleStatus::Applied, Some(22)),
        ]
    );
    assert_eq!(backend.request_frames(), [21, 22]);
    assert_eq!(scheduler.pending_len(), 0);
    assert!(rejections.records.is_empty());
}

#[test]
fn retries_once_when_backend_rejects_a_late_frame() {
    let backend = FakeBackend::new([(SessionState::Running, 5), (SessionState::Running, 8)]);
    backend.push_injection(Err(stale_frame(6, 7)));
    let mut scheduler = InputScheduler::new();
    let mut rejections = RecordingRejectionSink::default();

    let outcome = scheduler
        .submit(
            &backend,
            input(1, PadWord::from_buttons([PadButton::Start])),
            &mut rejections,
        )
        .expect("late input retries");

    assert_eq!(outcome.status, InputScheduleStatus::Applied);
    assert_eq!(outcome.assigned_frame, Some(9));
    assert_eq!(backend.request_frames(), [6, 9]);
    assert_eq!(
        scheduler.applied_frames(),
        [AppliedInputFrame {
            frame: 9,
            pad_word: PadButton::Start.mask()
        }]
    );
    assert!(rejections.records.is_empty());
}

#[test]
fn failed_late_retry_records_private_input_rejection() {
    let backend = FakeBackend::new([(SessionState::Running, 5), (SessionState::Running, 8)]);
    backend.push_injection(Err(stale_frame(6, 7)));
    backend.push_injection(Err(stale_frame(9, 10)));
    let mut scheduler = InputScheduler::new();
    let mut rejections = RecordingRejectionSink::default();

    let outcome = scheduler
        .submit(
            &backend,
            input(7, PadWord::from_buttons([PadButton::Select])),
            &mut rejections,
        )
        .expect("failed stale retry drops input");

    assert_eq!(outcome.status, InputScheduleStatus::Dropped);
    assert_eq!(outcome.assigned_frame, None);
    assert_eq!(backend.request_frames(), [6, 9]);
    assert!(scheduler.applied_frames().is_empty());
    assert_eq!(
        rejections.records,
        [InputRejectionRecord {
            run_id: RUN_ID.to_string(),
            client_seq: 7,
            occurred_at: OCCURRED_AT.to_string(),
            reason_code: FRAME_STALE_REASON_CODE.to_string(),
            public_message: PUBLIC_INPUT_REJECTION_MESSAGE.to_string(),
        }]
    );
}

#[test]
fn applies_one_pad_word_per_replay_frame_preserving_order() {
    let backend = FakeBackend::new([(SessionState::Running, 100)]);
    let mut scheduler = InputScheduler::new();
    let mut rejections = RecordingRejectionSink::default();

    for (client_seq, button) in [(1, PadButton::A), (2, PadButton::B), (3, PadButton::Start)] {
        scheduler
            .submit(
                &backend,
                input(client_seq, PadWord::from_buttons([button])),
                &mut rejections,
            )
            .expect("input schedules");
    }

    assert_eq!(backend.request_frames(), [101, 102, 103]);
    assert_eq!(
        scheduler.applied_frames(),
        [
            AppliedInputFrame {
                frame: 101,
                pad_word: PadButton::A.mask()
            },
            AppliedInputFrame {
                frame: 102,
                pad_word: PadButton::B.mask()
            },
            AppliedInputFrame {
                frame: 103,
                pad_word: PadButton::Start.mask()
            },
        ]
    );
    assert_eq!(
        scheduler
            .applied_frames()
            .iter()
            .map(|applied| applied.frame)
            .collect::<HashSet<_>>()
            .len(),
        scheduler.applied_frames().len()
    );
    assert!(rejections.records.is_empty());
}

#[test]
fn serializes_assigned_frames_without_narrowing_to_u32() {
    let current_frame = u32::MAX as FrameCounter;
    let backend = FakeBackend::new([(SessionState::Running, current_frame)]);
    let mut scheduler = InputScheduler::new();
    let mut rejections = RecordingRejectionSink::default();

    let outcome = scheduler
        .submit(
            &backend,
            input(1, PadWord::from_buttons([PadButton::X])),
            &mut rejections,
        )
        .expect("large frame schedules");

    let assigned_frame = current_frame + 1;
    assert_eq!(outcome.assigned_frame, Some(assigned_frame));

    let json =
        serde_json::to_value(&scheduler.applied_frames()[0]).expect("applied frame serializes");
    assert_eq!(json["frame"], serde_json::json!(assigned_frame));
    assert_eq!(json["pad_word"], serde_json::json!(PadButton::X.mask()));

    let decoded: AppliedInputFrame =
        serde_json::from_value(json).expect("applied frame deserializes");
    assert_eq!(decoded.frame, assigned_frame);
    assert!(assigned_frame > u32::MAX as u64);
}

fn input(client_seq: u64, pad_word: PadWord) -> BrowserInputState {
    BrowserInputState::new(SESSION_ID, RUN_ID, client_seq, OCCURRED_AT, pad_word)
}

fn stale_frame(requested_frame: FrameCounter, current_frame: FrameCounter) -> BackendError {
    BackendError::FrameStale {
        requested_frame,
        current_frame,
    }
}

#[derive(Debug, Default)]
struct RecordingRejectionSink {
    records: Vec<InputRejectionRecord>,
}

impl InputRejectionSink for RecordingRejectionSink {
    fn record_input_rejection(
        &mut self,
        rejection: &InputRejectionRecord,
    ) -> Result<(), InputSchedulerError> {
        self.records.push(rejection.clone());
        Ok(())
    }
}

#[derive(Debug)]
struct FakeBackend {
    statuses: Mutex<VecDeque<RunStatus>>,
    last_status: Mutex<RunStatus>,
    injections: Mutex<VecDeque<BackendResult<InputScheduleReceipt>>>,
    requests: Mutex<Vec<InputScheduleRequest>>,
}

impl FakeBackend {
    fn new(statuses: impl IntoIterator<Item = (SessionState, FrameCounter)>) -> Self {
        let mut statuses = statuses
            .into_iter()
            .map(|(state, current_frame)| status(state, current_frame))
            .collect::<VecDeque<_>>();
        let first_status = statuses
            .front()
            .cloned()
            .expect("fake backend needs at least one status");

        Self {
            last_status: Mutex::new(first_status),
            statuses: Mutex::new({
                let mut queue = VecDeque::new();
                queue.append(&mut statuses);
                queue
            }),
            injections: Mutex::new(VecDeque::new()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn push_injection(&self, result: BackendResult<InputScheduleReceipt>) {
        self.injections.lock().unwrap().push_back(result);
    }

    fn request_frames(&self) -> Vec<FrameCounter> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.target_frame)
            .collect()
    }
}

impl BridgeBackend for FakeBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Synthetic
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::synthetic_mvp()
    }

    fn start_session(&self, _request: StartBackendSession) -> BackendResult<BackendSession> {
        unimplemented!("not needed by input scheduler tests")
    }

    fn stop_session(
        &self,
        _session_id: SessionId,
        _reason: StopReason,
    ) -> BackendResult<StoppedSession> {
        unimplemented!("not needed by input scheduler tests")
    }

    fn status(&self, session_id: SessionId) -> BackendResult<RunStatus> {
        let mut statuses = self.statuses.lock().unwrap();
        let mut last_status = self.last_status.lock().unwrap();
        let next = statuses.pop_front().unwrap_or_else(|| last_status.clone());
        *last_status = RunStatus { session_id, ..next };
        Ok(last_status.clone())
    }

    fn pause(&self, _session_id: SessionId) -> BackendResult<RunBoundary> {
        unimplemented!("not needed by input scheduler tests")
    }

    fn resume(&self, _session_id: SessionId) -> BackendResult<RunBoundary> {
        unimplemented!("not needed by input scheduler tests")
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        self.requests.lock().unwrap().push(request.clone());

        match self.injections.lock().unwrap().pop_front() {
            Some(result) => result,
            None => Ok(InputScheduleReceipt {
                session_id: request.session_id,
                assigned_frame: request.target_frame,
                pad_word: request.pad_word,
            }),
        }
    }

    fn framebuffer(&self, _session_id: SessionId) -> BackendResult<FramePreview> {
        unimplemented!("not needed by input scheduler tests")
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        unimplemented!("not needed by input scheduler tests")
    }

    fn capture_job(&self, _job_id: String) -> BackendResult<CaptureJob> {
        unimplemented!("not needed by input scheduler tests")
    }
}

fn status(state: SessionState, current_frame: FrameCounter) -> RunStatus {
    RunStatus {
        session_id: SESSION_ID.to_string(),
        run_id: RUN_ID.to_string(),
        state,
        backend_mode: BackendMode::Synthetic,
        current_frame,
        capabilities: BackendCapabilities::synthetic_mvp(),
    }
}
