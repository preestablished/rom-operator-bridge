use crate::{
    backend::BackendMode,
    input::{PAD_MASK, PadLog},
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
        padlog: &PadLog,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        let run_id = path_segment("run_id", run_id)?;
        let relative_path = PathBuf::from("runs").join(run_id).join("input.padlog");
        let padlog_text = padlog.write_canonical();
        self.config
            .write_private_file_atomic(&relative_path, padlog_text.as_bytes())?;
        Ok(PrivateArtifactRef::new(relative_path))
    }

    pub fn append_padlog_event(
        &self,
        run_id: &str,
        row: &PadLogEventRow,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(row.schema_version)?;
        ensure_pad_word(row.pad_word)?;
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

    pub fn write_capture_payload(
        &self,
        capture_id: &str,
        kind: CapturePayloadKind,
        payload_name: &str,
        bytes: &[u8],
    ) -> Result<CapturePayloadArtifact, ArtifactError> {
        let capture_id = path_segment("capture_id", capture_id)?;
        let payload_name = path_segment("payload_name", payload_name)?;
        let relative_path = kind
            .relative_dir()
            .join(format!("{capture_id}-{payload_name}"));
        if self
            .config
            .private_root()
            .is_some_and(|root| root.join(&relative_path).exists())
        {
            return Err(ArtifactError::ExistingCapturePayload);
        }
        self.config
            .write_private_file_atomic(&relative_path, bytes)?;
        Ok(CapturePayloadArtifact {
            artifact_ref: PrivateArtifactRef::new(relative_path),
            len: u64::try_from(bytes.len()).map_err(|_| ArtifactError::PayloadTooLarge)?,
            blake3: blake3_ref(bytes),
        })
    }

    pub fn write_capture_manifest(
        &self,
        capture_id: &str,
        manifest: &CaptureManifest,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(manifest.schema_version)?;
        let capture_id = path_segment("capture_id", capture_id)?;
        ensure_matching_identifier("capture_id", capture_id, &manifest.capture_id)?;
        self.write_json_atomic(
            PathBuf::from("captures")
                .join(capture_id)
                .join("capture-manifest.json"),
            manifest,
        )
    }

    pub fn append_capture_index_row(
        &self,
        row: &CaptureIndexRow,
    ) -> Result<PrivateArtifactRef, ArtifactError> {
        ensure_schema_version(row.schema_version)?;
        path_segment("capture_id", &row.capture_id)?;
        ensure_nonempty("node_ref", &row.node_ref)?;
        ensure_nonempty("capture_source", &row.capture_source)?;
        ensure_hash_ref("layout_hash", &row.layout_hash)?;
        ensure_nonempty("feature_bytes.ref", &row.feature_bytes.artifact_ref)?;
        ensure_hash_ref("feature_bytes.blake3", &row.feature_bytes.blake3)?;
        ensure_nonempty("framebuffer.ref", &row.framebuffer.artifact_ref)?;
        ensure_hash_ref("framebuffer.blake3", &row.framebuffer.blake3)?;
        ensure_nonempty("framebuffer.encoding", &row.framebuffer.encoding)?;
        ensure_nonempty("framebuffer.pixel_format", &row.framebuffer.pixel_format)?;
        if row.feature_bytes.len == 0
            || row.framebuffer.width == 0
            || row.framebuffer.height == 0
            || row.framebuffer.stride == 0
            || row.framebuffer.uncompressed_len == 0
            || row.decoded_order.is_empty()
            || row.decoded_order.len() != row.decoded_values.len()
        {
            return Err(ArtifactError::InvalidCaptureIndexRow);
        }
        self.append_jsonl(PathBuf::from("captures").join("index.jsonl"), row)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePayloadKind {
    FeatureBytes,
    Framebuffer,
}

impl CapturePayloadKind {
    fn relative_dir(self) -> PathBuf {
        match self {
            Self::FeatureBytes => PathBuf::from("artifacts").join("feature-bytes"),
            Self::Framebuffer => PathBuf::from("artifacts").join("framebuffer"),
        }
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

    pub fn artifact_ref(&self) -> String {
        format!("artifact:{}", self.relative_path.to_string_lossy())
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

#[derive(Clone, PartialEq, Eq)]
pub struct CapturePayloadArtifact {
    pub artifact_ref: PrivateArtifactRef,
    pub len: u64,
    pub blake3: String,
}

impl fmt::Debug for CapturePayloadArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturePayloadArtifact")
            .field("artifact_ref", &self.artifact_ref)
            .field("len", &self.len)
            .field("blake3", &self.blake3)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureIndexRow {
    pub schema_version: u16,
    pub capture_id: String,
    pub node_ref: String,
    pub capture_source: String,
    pub frame_counter: u64,
    pub layout_hash: String,
    pub feature_bytes: CaptureFeatureBytesIndex,
    pub decoded_order: Vec<String>,
    pub decoded_values: Vec<i64>,
    pub framebuffer: CaptureFramebufferIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureFeatureBytesIndex {
    #[serde(rename = "ref")]
    pub artifact_ref: String,
    pub len: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureFramebufferIndex {
    #[serde(rename = "ref")]
    pub artifact_ref: String,
    pub encoding: String,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: String,
    pub uncompressed_len: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureManifest {
    pub schema_version: u16,
    pub capture_id: String,
    pub job_id: String,
    pub run_id: String,
    pub frame_counter: u64,
    pub icount: u64,
    pub vns: u64,
    pub layout_hash: String,
    pub capture_spec_hash: String,
    pub map_hash: String,
    pub capture_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_log_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_config_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub determinism_class: Option<CaptureDeterminismClass>,
}

impl CaptureManifest {
    pub fn new(
        capture_id: impl Into<String>,
        job_id: impl Into<String>,
        run_id: impl Into<String>,
        frame_counter: u64,
        icount: u64,
        vns: u64,
        layout_hash: impl Into<String>,
        capture_spec_hash: impl Into<String>,
        map_hash: impl Into<String>,
        capture_source: impl Into<String>,
        lifecycle: CaptureLifecycleRefs,
    ) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            capture_id: capture_id.into(),
            job_id: job_id.into(),
            run_id: run_id.into(),
            frame_counter,
            icount,
            vns,
            layout_hash: layout_hash.into(),
            capture_spec_hash: capture_spec_hash.into(),
            map_hash: map_hash.into(),
            capture_source: capture_source.into(),
            snapshot_ref: lifecycle.snapshot_ref,
            input_log_id: lifecycle.input_log_id,
            state_hash: lifecycle.state_hash,
            machine_config_hash: lifecycle.machine_config_hash,
            determinism_class: lifecycle.determinism_class,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CaptureLifecycleRefs {
    pub snapshot_ref: Option<String>,
    pub input_log_id: Option<String>,
    pub state_hash: Option<String>,
    pub machine_config_hash: Option<String>,
    pub determinism_class: Option<CaptureDeterminismClass>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureDeterminismClass {
    pub cpu_model: String,
    pub microcode: String,
    pub host_kernel: String,
    pub vmm_version: String,
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

fn ensure_nonempty(field: &'static str, value: &str) -> Result<(), ArtifactError> {
    if value.trim().is_empty() {
        Err(ArtifactError::InvalidCaptureIndexField { field })
    } else {
        Ok(())
    }
}

fn ensure_hash_ref(field: &'static str, value: &str) -> Result<(), ArtifactError> {
    if let Some(hex) = value.strip_prefix("blake3:")
        && hex.len() == 64
        && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Ok(());
    }
    Err(ArtifactError::InvalidCaptureIndexField { field })
}

fn ensure_pad_word(pad_word: u16) -> Result<(), ArtifactError> {
    let reserved = pad_word & !PAD_MASK;
    if reserved == 0 {
        Ok(())
    } else {
        Err(ArtifactError::InvalidPadWord { pad_word, reserved })
    }
}

fn blake3_ref(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
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
    #[error("artifact pad word {pad_word:#06x} sets reserved bits {reserved:#06x}")]
    InvalidPadWord { pad_word: u16, reserved: u16 },
    #[error("capture payload already exists")]
    ExistingCapturePayload,
    #[error("capture payload is too large")]
    PayloadTooLarge,
    #[error("capture index field `{field}` is invalid")]
    InvalidCaptureIndexField { field: &'static str },
    #[error("capture index row is invalid")]
    InvalidCaptureIndexRow,
}
