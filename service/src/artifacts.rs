use crate::{
    backend::BackendMode,
    private_config::{BridgePrivateConfig, PrivateConfigError},
};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const ARTIFACT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateArtifactStore<'a> {
    config: &'a BridgePrivateConfig,
}

impl<'a> PrivateArtifactStore<'a> {
    pub const fn new(config: &'a BridgePrivateConfig) -> Self {
        Self { config }
    }

    pub fn write_run_manifest(
        &self,
        manifest: &RunManifest,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(manifest.schema_version)?;
        let run_id = path_segment("run_id", &manifest.run_id)?;
        self.write_json_atomic(
            PathBuf::from("runs").join(run_id).join("run-manifest.json"),
            manifest,
        )
    }

    pub fn append_bridge_event(
        &self,
        run_id: &str,
        row: &BridgeEventRow,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(row.schema_version)?;
        let run_id = path_segment("run_id", run_id)?;
        ensure_matching_identifier("run_id", run_id, &row.run_id)?;
        self.append_jsonl(
            PathBuf::from("runs")
                .join(run_id)
                .join("bridge-events.jsonl"),
            row,
        )
    }

    pub fn append_input_rejection(
        &self,
        run_id: &str,
        row: &InputRejectionRow,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(row.schema_version)?;
        let run_id = path_segment("run_id", run_id)?;
        ensure_matching_identifier("run_id", run_id, &row.run_id)?;
        self.append_jsonl(
            PathBuf::from("runs")
                .join(run_id)
                .join("input-rejections.jsonl"),
            row,
        )
    }

    pub fn write_padlog(
        &self,
        run_id: &str,
        padlog_text: &str,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        let run_id = path_segment("run_id", run_id)?;
        self.config.write_private_file_atomic(
            PathBuf::from("runs").join(run_id).join("input.padlog"),
            padlog_text.as_bytes(),
        )?;
        Ok(PrivateArtifactRef::new(
            PathBuf::from("runs").join(run_id).join("input.padlog"),
        ))
    }

    pub fn append_padlog_event(
        &self,
        run_id: &str,
        row: &PadLogEventRow,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(row.schema_version)?;
        let run_id = path_segment("run_id", run_id)?;
        ensure_matching_identifier("run_id", run_id, &row.run_id)?;
        self.append_jsonl(
            PathBuf::from("runs")
                .join(run_id)
                .join("padlog-events.jsonl"),
            row,
        )
    }

    pub fn write_recent_captures(
        &self,
        recent: &RecentCapturesFile,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(recent.schema_version)?;
        for capture in &recent.captures {
            path_segment("capture_id", &capture.capture_id)?;
        }
        self.write_json_atomic(
            PathBuf::from("captures").join("recent-captures.json"),
            recent,
        )
    }

    pub fn write_label_draft(
        &self,
        draft: &LabelDraftFile,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(draft.schema_version)?;
        let capture_id = path_segment("capture_id", &draft.capture_id)?;
        self.write_json_atomic(
            PathBuf::from("captures")
                .join(capture_id)
                .join("label-draft.json"),
            draft,
        )
    }

    pub fn append_validation_run(
        &self,
        row: &ValidationRunRow,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(row.schema_version)?;
        path_segment("validation_id", &row.validation_id)?;
        self.append_jsonl(
            PathBuf::from("validation").join("validation-runs.jsonl"),
            row,
        )
    }

    fn write_json_atomic<T: Serialize>(
        &self,
        relative_path: PathBuf,
        value: &T,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        let bytes = serde_json::to_vec_pretty(value)?;
        self.config
            .write_private_file_atomic(&relative_path, &bytes)?;
        Ok(PrivateArtifactRef::new(relative_path))
    }

    fn append_jsonl<T: Serialize>(
        &self,
        relative_path: PathBuf,
        value: &T,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        self.config.append_private_file(&relative_path, &bytes)?;
        Ok(PrivateArtifactRef::new(relative_path))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PrivateArtifactRef {
    relative_path: PathBuf,
}

impl PrivateArtifactRef {
    fn new(relative_path: PathBuf) -> Self {
        Self { relative_path }
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }
}

impl fmt::Debug for PrivateArtifactRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrivateArtifactRef")
            .field("relative_path", &"[redacted]")
            .finish()
    }
}

impl fmt::Display for PrivateArtifactRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[private artifact]")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunManifest {
    pub schema_version: u16,
    pub run_id: String,
    pub created_at: String,
    pub backend_mode: BackendMode,
    pub runtime_api: u16,
}

impl RunManifest {
    pub fn new(
        run_id: impl Into<String>,
        created_at: impl Into<String>,
        backend_mode: BackendMode,
        runtime_api: u16,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            run_id: run_id.into(),
            created_at: created_at.into(),
            backend_mode,
            runtime_api,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeEventRow {
    pub schema_version: u16,
    pub run_id: String,
    pub server_seq: u64,
    pub occurred_at: String,
    pub event_type: String,
    pub message: String,
}

impl BridgeEventRow {
    pub fn new(
        run_id: impl Into<String>,
        server_seq: u64,
        occurred_at: impl Into<String>,
        event_type: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            run_id: run_id.into(),
            server_seq,
            occurred_at: occurred_at.into(),
            event_type: event_type.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputRejectionRow {
    pub schema_version: u16,
    pub run_id: String,
    pub client_seq: u64,
    pub occurred_at: String,
    pub reason_code: String,
    pub public_message: String,
}

impl InputRejectionRow {
    pub fn new(
        run_id: impl Into<String>,
        client_seq: u64,
        occurred_at: impl Into<String>,
        reason_code: impl Into<String>,
        public_message: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            run_id: run_id.into(),
            client_seq,
            occurred_at: occurred_at.into(),
            reason_code: reason_code.into(),
            public_message: public_message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadLogEventRow {
    pub schema_version: u16,
    pub run_id: String,
    pub frame_index: u64,
    pub assigned_frame: u64,
    pub pad_word: u16,
    pub client_seq: u64,
    pub source_id: String,
    pub status: String,
    pub message: String,
}

impl PadLogEventRow {
    pub fn new(
        run_id: impl Into<String>,
        frame_index: u64,
        assigned_frame: u64,
        pad_word: u16,
        client_seq: u64,
        source_id: impl Into<String>,
        status: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            run_id: run_id.into(),
            frame_index,
            assigned_frame,
            pad_word,
            client_seq,
            source_id: source_id.into(),
            status: status.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentCapturesFile {
    pub schema_version: u16,
    pub captures: Vec<CaptureSummary>,
}

impl RecentCapturesFile {
    pub fn new(captures: Vec<CaptureSummary>) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            captures,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSummary {
    pub capture_id: String,
    pub requested_at: String,
    pub status: String,
    pub labelable: bool,
}

impl CaptureSummary {
    pub fn new(
        capture_id: impl Into<String>,
        requested_at: impl Into<String>,
        status: impl Into<String>,
        labelable: bool,
    ) -> Self {
        Self {
            capture_id: capture_id.into(),
            requested_at: requested_at.into(),
            status: status.into(),
            labelable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelDraftFile {
    pub schema_version: u16,
    pub capture_id: String,
    pub updated_at: String,
    pub labels: Vec<LabelDraft>,
    pub private_note: Option<String>,
}

impl LabelDraftFile {
    pub fn new(
        capture_id: impl Into<String>,
        updated_at: impl Into<String>,
        labels: Vec<LabelDraft>,
        private_note: Option<String>,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            capture_id: capture_id.into(),
            updated_at: updated_at.into(),
            labels,
            private_note,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelDraft {
    pub label: String,
    pub selected: bool,
}

impl LabelDraft {
    pub fn new(label: impl Into<String>, selected: bool) -> Self {
        Self {
            label: label.into(),
            selected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationRunRow {
    pub schema_version: u16,
    pub validation_id: String,
    pub started_at: String,
    pub command_class: String,
    pub status: String,
    pub sanitized_summary: String,
}

impl ValidationRunRow {
    pub fn new(
        validation_id: impl Into<String>,
        started_at: impl Into<String>,
        command_class: impl Into<String>,
        status: impl Into<String>,
        sanitized_summary: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            validation_id: validation_id.into(),
            started_at: started_at.into(),
            command_class: command_class.into(),
            status: status.into(),
            sanitized_summary: sanitized_summary.into(),
        }
    }
}

fn ensure_schema_version(schema_version: u16) -> Result<(), ArtifactError> {
    if schema_version == ARTIFACT_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(ArtifactError::UnsupportedSchemaVersion { schema_version })
    }
}

fn path_segment<'a>(field: &'static str, value: &'a str) -> Result<&'a str, ArtifactError> {
    let valid = !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));

    if valid {
        Ok(value)
    } else {
        Err(ArtifactError::InvalidIdentifier {
            field,
            value: value.to_string(),
        })
    }
}

fn ensure_matching_identifier(
    field: &'static str,
    path_value: &str,
    row_value: &str,
) -> Result<(), ArtifactError> {
    path_segment(field, row_value)?;
    if path_value == row_value {
        Ok(())
    } else {
        Err(ArtifactError::MismatchedIdentifier {
            field,
            path_value: path_value.to_string(),
            row_value: row_value.to_string(),
        })
    }
}

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error(transparent)]
    PrivateConfig(#[from] PrivateConfigError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("artifact schema version {schema_version} is unsupported")]
    UnsupportedSchemaVersion { schema_version: u16 },
    #[error("artifact identifier `{field}` is invalid")]
    InvalidIdentifier { field: &'static str, value: String },
    #[error("artifact identifier `{field}` does not match its path")]
    MismatchedIdentifier {
        field: &'static str,
        path_value: String,
        row_value: String,
    },
}
