use crate::{
    api::RUNTIME_API_SCHEMA_VERSION,
    backend::{BackendCapabilities, BackendMode, RunStatus, SessionState},
    sanitization::{PublicSanitizer, SanitizationError},
};
use axum::extract::ws::{Message, WebSocket};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const JSON_SAFE_U64_MAX: u64 = 9_007_199_254_740_991;
const SERVER_SOURCE_ID: &str = "server";

#[derive(Debug, Clone, Default)]
pub struct WsEventState {
    inner: Arc<Mutex<WsEventInner>>,
}

impl WsEventState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_session(&self, session_id: &str) {
        let mut inner = self.inner.lock().expect("ws event mutex poisoned");
        inner.session_id = Some(session_id.to_string());
        inner.server_seq = 0;
    }

    fn snapshot_messages(
        &self,
        status: &RunStatus,
        sanitizer: &PublicSanitizer,
    ) -> Result<Vec<String>, WsEventError> {
        ensure_json_safe(status.current_frame)?;

        let mut inner = self.inner.lock().expect("ws event mutex poisoned");
        let mut messages = Vec::with_capacity(5);
        messages.push(inner.message(
            &status.session_id,
            "session_updated",
            json!(SessionUpdatedPayload {
                state: status.state,
                backend_mode: status.backend_mode,
                current_frame: status.current_frame,
                capabilities: status.capabilities,
            }),
            sanitizer,
        )?);
        messages.push(inner.message(
            &status.session_id,
            "run_updated",
            json!(RunUpdatedPayload {
                state: status.state,
                current_frame: status.current_frame,
                preview_stale: status.last_preview_frame < status.current_frame,
                active_capture_job_id: status.active_capture_job_id.clone(),
            }),
            sanitizer,
        )?);
        if let Some(job_id) = &status.active_capture_job_id {
            messages.push(inner.message(
                &status.session_id,
                "capture_updated",
                json!(CaptureUpdatedPayload {
                    job_id: job_id.clone(),
                    status: "capturing",
                    capture_id: None,
                }),
                sanitizer,
            )?);
        }
        messages.push(inner.message(
            &status.session_id,
            "label_updated",
            json!(LabelUpdatedPayload {
                label_revision: 0,
                applied: false,
            }),
            sanitizer,
        )?);
        messages.push(inner.message(
            &status.session_id,
            "validation_updated",
            json!(ValidationUpdatedPayload {
                status: "not_run",
                summary: "",
            }),
            sanitizer,
        )?);

        Ok(messages)
    }
}

pub async fn serve_event_socket(
    mut socket: WebSocket,
    event_state: WsEventState,
    sanitizer: PublicSanitizer,
    status: RunStatus,
) {
    let Ok(messages) = event_state.snapshot_messages(&status, &sanitizer) else {
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    for message in messages {
        if socket.send(Message::Text(message.into())).await.is_err() {
            return;
        }
    }

    while let Some(message) = socket.recv().await {
        let Ok(message) = message else {
            break;
        };
        if matches!(message, Message::Close(_)) {
            break;
        }
    }
}

#[derive(Debug, Default)]
struct WsEventInner {
    session_id: Option<String>,
    server_seq: u64,
}

impl WsEventInner {
    fn next_server_seq(&mut self, session_id: &str) -> Result<u64, WsEventError> {
        if self.session_id.as_deref() != Some(session_id) {
            self.session_id = Some(session_id.to_string());
            self.server_seq = 0;
        }
        if self.server_seq >= JSON_SAFE_U64_MAX {
            return Err(WsEventError::JsonSafeNumber);
        }
        self.server_seq += 1;
        Ok(self.server_seq)
    }

    fn message(
        &mut self,
        session_id: &str,
        message_type: &'static str,
        payload: Value,
        sanitizer: &PublicSanitizer,
    ) -> Result<String, WsEventError> {
        let envelope = WsEventEnvelope {
            schema_version: RUNTIME_API_SCHEMA_VERSION,
            message_type,
            session_id: session_id.to_string(),
            client_seq: None,
            source_id: SERVER_SOURCE_ID,
            server_seq: self.next_server_seq(session_id)?,
            payload,
        };
        let value = serde_json::to_value(envelope).expect("ws event serializes");
        sanitizer.inspect_event(&value)?;
        Ok(serde_json::to_string(&value).expect("ws event value serializes"))
    }
}

fn ensure_json_safe(value: u64) -> Result<(), WsEventError> {
    if value <= JSON_SAFE_U64_MAX {
        Ok(())
    } else {
        Err(WsEventError::JsonSafeNumber)
    }
}

#[derive(Debug, Clone, Serialize)]
struct WsEventEnvelope {
    schema_version: u16,
    #[serde(rename = "type")]
    message_type: &'static str,
    session_id: String,
    client_seq: Option<u64>,
    source_id: &'static str,
    server_seq: u64,
    payload: Value,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct SessionUpdatedPayload {
    state: SessionState,
    backend_mode: BackendMode,
    current_frame: u64,
    capabilities: BackendCapabilities,
}

#[derive(Debug, Clone, Serialize)]
struct RunUpdatedPayload {
    state: SessionState,
    current_frame: u64,
    preview_stale: bool,
    active_capture_job_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CaptureUpdatedPayload {
    job_id: String,
    status: &'static str,
    capture_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct LabelUpdatedPayload {
    label_revision: u64,
    applied: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ValidationUpdatedPayload {
    status: &'static str,
    summary: &'static str,
}

#[derive(Debug)]
enum WsEventError {
    JsonSafeNumber,
    Sanitization,
}

impl From<SanitizationError> for WsEventError {
    fn from(_error: SanitizationError) -> Self {
        Self::Sanitization
    }
}
