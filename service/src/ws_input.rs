use crate::{
    api::{ErrorCode, ErrorEnvelope, ErrorObject, RUNTIME_API_SCHEMA_VERSION},
    backend::BridgeBackend,
    input::{
        BrowserInputState, FRAME_STALE_REASON_CODE, InputScheduleOutcome, InputScheduleStatus,
        InputScheduler, InputSchedulerError, NoopInputRejectionSink,
        PUBLIC_INPUT_REJECTION_MESSAGE, PadButton, PadWord,
    },
};
use axum::extract::ws::{Message, WebSocket};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

const JSON_SAFE_U64_MAX: u64 = 9_007_199_254_740_991;
const TOP_LEVEL_FIELDS: &[&str] = &[
    "schema_version",
    "type",
    "session_id",
    "client_seq",
    "source_id",
    "server_seq",
    "payload",
];
const MAX_RETAINED_REPLIES_PER_SOURCE: usize = 1024;

#[derive(Debug, Clone, Default)]
pub struct WsInputState {
    inner: Arc<Mutex<WsInputInner>>,
}

impl WsInputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_session(&self, _session_id: &str) {
        let mut inner = self.inner.lock().expect("ws input mutex poisoned");
        inner.scheduler = InputScheduler::new();
        inner.server_seq = 0;
        inner.sources.clear();
    }

    pub fn flush_pending(
        &self,
        backend: &dyn BridgeBackend,
        session_id: &str,
    ) -> Result<Vec<InputScheduleOutcome>, InputSchedulerError> {
        let mut inner = self.inner.lock().expect("ws input mutex poisoned");
        let mut rejection_sink = NoopInputRejectionSink;
        inner
            .scheduler
            .flush_pending(backend, session_id, &mut rejection_sink)
    }

    fn handle_text(
        &self,
        backend: &dyn BridgeBackend,
        active_session_id: &str,
        text: &str,
    ) -> Option<String> {
        let incoming = IncomingEnvelope::parse(text)?;
        let mut inner = self.inner.lock().expect("ws input mutex poisoned");
        let key = SourceKey {
            session_id: incoming.session_id.clone(),
            source_id: incoming.source_id.clone(),
        };

        if let Some(reply) = inner
            .sources
            .get(&key)
            .and_then(|source| source.replies.get(&incoming.client_seq))
        {
            return Some(reply.clone());
        }

        if incoming.session_id != active_session_id {
            return Some(inner.reject_and_remember(
                key,
                incoming,
                ErrorCode::SessionInactive,
                "Session inactive.",
                false,
            ));
        }

        if let Some(error) = incoming.validation_error() {
            return Some(inner.reject_and_remember(
                key,
                incoming,
                error.code,
                error.message,
                error.retryable,
            ));
        }

        if inner
            .sources
            .get(&key)
            .and_then(|source| source.highest_client_seq)
            .is_some_and(|highest| incoming.client_seq <= highest)
        {
            return Some(inner.reject_and_remember(
                key,
                incoming,
                ErrorCode::BadRequest,
                PUBLIC_INPUT_REJECTION_MESSAGE,
                false,
            ));
        }

        let input = match incoming.browser_input() {
            Ok(input) => input,
            Err(error) => {
                return Some(inner.reject_and_remember(
                    key,
                    incoming,
                    error.code,
                    error.message,
                    error.retryable,
                ));
            }
        };

        let mut rejection_sink = NoopInputRejectionSink;
        let outcome = match inner.scheduler.submit(backend, input, &mut rejection_sink) {
            Ok(outcome) => outcome,
            Err(error) => {
                let error = WsInputError::from_scheduler(error);
                return Some(inner.reject_and_remember(
                    key,
                    incoming,
                    error.code,
                    error.message,
                    error.retryable,
                ));
            }
        };

        let reply = match outcome.status {
            InputScheduleStatus::Applied | InputScheduleStatus::Queued => {
                match inner.ack(&incoming, &outcome) {
                    Ok(reply) => reply,
                    Err(error) => {
                        inner.reject(&incoming, error.code, error.message, error.retryable)
                    }
                }
            }
            InputScheduleStatus::Dropped => {
                let error = WsInputError::from_dropped(&outcome);
                inner.reject(&incoming, error.code, error.message, error.retryable)
            }
        };
        inner.remember(key, incoming.client_seq, reply.clone());
        Some(reply)
    }
}

pub async fn serve_input_socket(
    mut socket: WebSocket,
    backend: Arc<dyn BridgeBackend>,
    input_state: WsInputState,
    active_session_id: String,
) {
    while let Some(message) = socket.recv().await {
        let Ok(message) = message else {
            break;
        };

        let reply = match message {
            Message::Text(text) => {
                input_state.handle_text(backend.as_ref(), &active_session_id, text.as_str())
            }
            Message::Close(_) => break,
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => None,
        };

        if let Some(reply) = reply
            && socket.send(Message::Text(reply.into())).await.is_err()
        {
            break;
        }
    }
}

#[derive(Debug, Default)]
struct WsInputInner {
    scheduler: InputScheduler,
    server_seq: u64,
    sources: BTreeMap<SourceKey, SourceState>,
}

impl WsInputInner {
    fn next_server_seq(&mut self) -> u64 {
        self.server_seq += 1;
        self.server_seq
    }

    fn remember(&mut self, key: SourceKey, client_seq: u64, reply: String) {
        let source = self.sources.entry(key).or_default();
        source.highest_client_seq = Some(
            source
                .highest_client_seq
                .map_or(client_seq, |highest| highest.max(client_seq)),
        );
        source.replies.insert(client_seq, reply);
        while source.replies.len() > MAX_RETAINED_REPLIES_PER_SOURCE {
            source.replies.pop_first();
        }
    }

    fn ack(
        &mut self,
        incoming: &IncomingEnvelope,
        outcome: &InputScheduleOutcome,
    ) -> Result<String, WsInputError> {
        let assigned_frame = outcome.assigned_frame.unwrap_or_default();
        if assigned_frame > JSON_SAFE_U64_MAX {
            return Err(WsInputError::bad_request());
        }

        let envelope = WsOutputEnvelope {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            message_type: "input_ack",
            session_id: incoming.session_id.clone(),
            client_seq: incoming.client_seq,
            source_id: incoming.source_id.clone(),
            server_seq: Some(self.next_server_seq()),
            payload: json!(InputAckPayload {
                client_event_id: incoming.client_event_id().to_string(),
                assigned_frame,
                pad_word: outcome.pad_word,
                status: outcome.status,
            }),
        };
        Ok(serde_json::to_string(&envelope).expect("ws input ack serializes"))
    }

    fn reject(
        &mut self,
        incoming: &IncomingEnvelope,
        code: ErrorCode,
        message: &'static str,
        retryable: bool,
    ) -> String {
        let envelope = WsOutputEnvelope {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            message_type: "input_reject",
            session_id: incoming.session_id.clone(),
            client_seq: incoming.client_seq,
            source_id: incoming.source_id.clone(),
            server_seq: Some(self.next_server_seq()),
            payload: json!(ErrorEnvelope {
                schema_version: RUNTIME_API_SCHEMA_VERSION,
                error: ErrorObject {
                    code,
                    message: message.to_string(),
                    retryable,
                    details: json!({}),
                },
            }),
        };
        serde_json::to_string(&envelope).expect("ws input reject serializes")
    }

    fn reject_and_remember(
        &mut self,
        key: SourceKey,
        incoming: IncomingEnvelope,
        code: ErrorCode,
        message: &'static str,
        retryable: bool,
    ) -> String {
        let reply = self.reject(&incoming, code, message, retryable);
        self.remember(key, incoming.client_seq, reply.clone());
        reply
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SourceKey {
    session_id: String,
    source_id: String,
}

#[derive(Debug, Clone, Default)]
struct SourceState {
    highest_client_seq: Option<u64>,
    replies: BTreeMap<u64, String>,
}

#[derive(Debug, Clone)]
struct RawIncomingEnvelope {
    schema_version: Option<u16>,
    message_type: Option<String>,
    session_id: Option<String>,
    client_seq: Option<u64>,
    source_id: Option<String>,
    server_seq: Option<Value>,
    payload: Option<Value>,
    has_unknown_fields: bool,
}

#[derive(Debug, Clone)]
struct IncomingEnvelope {
    schema_version: Option<u16>,
    message_type: Option<String>,
    session_id: String,
    client_seq: u64,
    source_id: String,
    server_seq: Option<Value>,
    payload: Option<Value>,
    has_unknown_fields: bool,
}

impl IncomingEnvelope {
    fn parse(text: &str) -> Option<Self> {
        let raw = RawIncomingEnvelope::parse(text)?;
        Some(Self {
            schema_version: raw.schema_version,
            message_type: raw.message_type,
            session_id: raw.session_id?,
            client_seq: raw.client_seq?,
            source_id: raw.source_id?,
            server_seq: raw.server_seq,
            payload: raw.payload,
            has_unknown_fields: raw.has_unknown_fields,
        })
    }

    fn validation_error(&self) -> Option<WsInputError> {
        if self.has_unknown_fields {
            return Some(WsInputError::bad_request());
        }
        if self.schema_version != Some(RUNTIME_API_SCHEMA_VERSION) {
            return Some(WsInputError::bad_request());
        }
        if self.message_type.as_deref() != Some("input_state") {
            return Some(WsInputError::bad_request());
        }
        if self
            .server_seq
            .as_ref()
            .is_none_or(|server_seq| !server_seq.is_null())
        {
            return Some(WsInputError::bad_request());
        }
        if !is_contract_id(&self.session_id)
            || self.source_id.is_empty()
            || self.source_id.len() > 64
            || self.client_seq > JSON_SAFE_U64_MAX
            || self.payload.is_none()
        {
            return Some(WsInputError::bad_request());
        }

        None
    }

    fn browser_input(&self) -> Result<BrowserInputState, WsInputError> {
        let payload: InputStatePayload =
            serde_json::from_value(self.payload.clone().ok_or_else(WsInputError::bad_request)?)
                .map_err(|_| WsInputError::bad_request())?;
        payload.validate()?;

        let mut seen = BTreeSet::new();
        let mut buttons = Vec::with_capacity(payload.buttons.len());
        for name in payload.buttons {
            if !seen.insert(name.clone()) {
                return Err(WsInputError::bad_request());
            }
            let Some(button) = PadButton::from_name(&name) else {
                return Err(WsInputError::bad_request());
            };
            buttons.push(button);
        }

        Ok(BrowserInputState::new(
            self.session_id.clone(),
            self.client_seq,
            "1970-01-01T00:00:00Z",
            PadWord::from_buttons(buttons),
        ))
    }

    fn client_event_id(&self) -> &str {
        self.payload
            .as_ref()
            .unwrap_or(&Value::Null)
            .get("client_event_id")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
}

impl RawIncomingEnvelope {
    fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let object = value.as_object()?;
        let has_unknown_fields = object
            .keys()
            .any(|key| !TOP_LEVEL_FIELDS.contains(&key.as_str()));

        Some(Self {
            schema_version: object
                .get("schema_version")
                .and_then(Value::as_u64)
                .and_then(|value| u16::try_from(value).ok()),
            message_type: object
                .get("type")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            session_id: object
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|session_id| is_contract_id(session_id))
                .map(ToOwned::to_owned),
            client_seq: object
                .get("client_seq")
                .and_then(Value::as_u64)
                .filter(|client_seq| *client_seq <= JSON_SAFE_U64_MAX),
            source_id: object
                .get("source_id")
                .and_then(Value::as_str)
                .filter(|source_id| !source_id.is_empty() && source_id.len() <= 64)
                .map(ToOwned::to_owned),
            server_seq: object.get("server_seq").cloned(),
            payload: object.get("payload").cloned(),
            has_unknown_fields,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputStatePayload {
    client_event_id: String,
    client_time_ms: f64,
    source: String,
    buttons: Vec<String>,
}

impl InputStatePayload {
    fn validate(&self) -> Result<(), WsInputError> {
        if self.client_event_id.is_empty()
            || !is_uuid(&self.client_event_id)
            || !self.client_time_ms.is_finite()
            || self.client_time_ms < 0.0
            || !matches!(self.source.as_str(), "keyboard" | "gamepad" | "combined")
        {
            return Err(WsInputError::bad_request());
        }

        Ok(())
    }
}

fn is_contract_id(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.len() <= 128
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn is_uuid(candidate: &str) -> bool {
    if candidate.len() != 36 {
        return false;
    }

    candidate.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

#[derive(Debug, Clone, Serialize)]
struct WsOutputEnvelope {
    schema_version: u16,
    #[serde(rename = "type")]
    message_type: &'static str,
    session_id: String,
    client_seq: u64,
    source_id: String,
    server_seq: Option<u64>,
    payload: Value,
}

#[derive(Debug, Clone, Serialize)]
struct InputAckPayload {
    client_event_id: String,
    assigned_frame: u64,
    pad_word: u16,
    status: InputScheduleStatus,
}

#[derive(Debug, Clone, Copy)]
struct WsInputError {
    code: ErrorCode,
    message: &'static str,
    retryable: bool,
}

impl WsInputError {
    fn bad_request() -> Self {
        Self {
            code: ErrorCode::BadRequest,
            message: PUBLIC_INPUT_REJECTION_MESSAGE,
            retryable: false,
        }
    }

    fn from_dropped(outcome: &InputScheduleOutcome) -> Self {
        match outcome
            .rejection
            .as_ref()
            .map(|rejection| rejection.reason_code.as_str())
        {
            Some(FRAME_STALE_REASON_CODE) => Self {
                code: ErrorCode::FrameStale,
                message: PUBLIC_INPUT_REJECTION_MESSAGE,
                retryable: true,
            },
            _ => Self::bad_request(),
        }
    }

    fn from_scheduler(error: InputSchedulerError) -> Self {
        match error {
            InputSchedulerError::Backend(crate::backend::BackendError::BackendUnavailable) => {
                Self {
                    code: ErrorCode::BackendUnavailable,
                    message: PUBLIC_INPUT_REJECTION_MESSAGE,
                    retryable: true,
                }
            }
            InputSchedulerError::Backend(crate::backend::BackendError::FrameStale { .. }) => Self {
                code: ErrorCode::FrameStale,
                message: PUBLIC_INPUT_REJECTION_MESSAGE,
                retryable: true,
            },
            _ => Self::bad_request(),
        }
    }
}
