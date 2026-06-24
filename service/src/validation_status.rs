use crate::{
    artifacts::{PrivateArtifactStore, ValidationRunRow},
    private_config::BridgePrivateConfig,
    sanitization::PublicSanitizer,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use thiserror::Error;

const SUMMARY_MAX_LEN: usize = 240;
const MAX_ISSUE_SUMMARIES: usize = 8;

#[derive(Debug, Clone, Default)]
pub struct ValidationStatusState {
    inner: Arc<Mutex<PublicValidationStatus>>,
}

impl ValidationStatusState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> PublicValidationStatus {
        self.inner
            .lock()
            .expect("validation status mutex poisoned")
            .clone()
    }

    pub fn reset(&self) {
        *self.inner.lock().expect("validation status mutex poisoned") =
            PublicValidationStatus::default();
    }

    pub fn record_run(
        &self,
        private_config: &BridgePrivateConfig,
        sanitizer: &PublicSanitizer,
        update: ValidationRunUpdate,
    ) -> Result<PublicValidationStatus, ValidationStatusError> {
        let command_class = sanitize_command_class(&update.command_class, sanitizer);
        let started_at = sanitize_timestamp("started_at", &update.started_at, sanitizer)?;
        let completed_at = update
            .completed_at
            .as_deref()
            .map(|timestamp| sanitize_timestamp("completed_at", timestamp, sanitizer))
            .transpose()?;
        let summary = sanitize_summary(update.status, &update.summary, sanitizer);
        let issue_summaries = update
            .issue_summaries
            .iter()
            .take(MAX_ISSUE_SUMMARIES)
            .map(|issue| sanitize_issue_summary(*issue, sanitizer))
            .collect::<Vec<_>>();

        let public = PublicValidationStatus {
            status: update.status,
            command_class: Some(command_class.clone()),
            started_at: Some(started_at.clone()),
            completed_at,
            summary: summary.clone(),
            issue_summaries,
        };

        PrivateArtifactStore::new(private_config).append_validation_run(&ValidationRunRow::new(
            update.validation_id,
            started_at,
            command_class,
            update.status.as_str(),
            summary,
        ))?;

        *self.inner.lock().expect("validation status mutex poisoned") = public.clone();
        Ok(public)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicValidationStatus {
    pub status: ValidationRunStatus,
    pub command_class: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub summary: String,
    pub issue_summaries: Vec<String>,
}

impl Default for PublicValidationStatus {
    fn default() -> Self {
        Self {
            status: ValidationRunStatus::NotRun,
            command_class: None,
            started_at: None,
            completed_at: None,
            summary: String::new(),
            issue_summaries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationRunStatus {
    NotRun,
    Running,
    Passed,
    Failed,
}

impl ValidationRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotRun => "not_run",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }

    const fn fallback_summary(self) -> &'static str {
        match self {
            Self::NotRun => "",
            Self::Running => "Validation running.",
            Self::Passed => "Validation passed.",
            Self::Failed => "Validation failed.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationRunUpdate {
    pub session_id: Option<String>,
    pub validation_id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub command_class: String,
    pub status: ValidationRunStatus,
    pub summary: String,
    pub issue_summaries: Vec<PublicValidationIssue>,
}

impl ValidationRunUpdate {
    pub fn new(
        validation_id: impl Into<String>,
        started_at: impl Into<String>,
        command_class: impl Into<String>,
        status: ValidationRunStatus,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            session_id: None,
            validation_id: validation_id.into(),
            started_at: started_at.into(),
            completed_at: None,
            command_class: command_class.into(),
            status,
            summary: summary.into(),
            issue_summaries: Vec::new(),
        }
    }

    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn matches_session(&self, session_id: &str) -> bool {
        self.session_id.as_deref() == Some(session_id)
    }

    pub fn completed_at(mut self, completed_at: impl Into<String>) -> Self {
        self.completed_at = Some(completed_at.into());
        self
    }

    pub fn issue_summaries<I>(mut self, issue_summaries: I) -> Self
    where
        I: IntoIterator<Item = PublicValidationIssue>,
    {
        self.issue_summaries = issue_summaries.into_iter().collect();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicValidationIssue {
    MissingRequiredLabel,
    ConflictingLabels,
    GoalRouteMismatch,
    DedupArtifactMismatch,
    ReportRedacted,
}

impl PublicValidationIssue {
    const fn message(self) -> &'static str {
        match self {
            Self::MissingRequiredLabel => "Required validation label is missing.",
            Self::ConflictingLabels => "Validation labels conflict.",
            Self::GoalRouteMismatch => "Goal route validation failed.",
            Self::DedupArtifactMismatch => "Dedup artifact validation failed.",
            Self::ReportRedacted => "Validation failed; inspect the private server-side report.",
        }
    }
}

#[derive(Debug, Error)]
pub enum ValidationStatusError {
    #[error(transparent)]
    Artifact(#[from] crate::artifacts::ArtifactError),
    #[error("validation update does not match the active session")]
    StaleSession,
    #[error("validation timestamp field `{field}` is not RFC3339")]
    InvalidTimestamp { field: &'static str },
}

fn sanitize_command_class(candidate: &str, sanitizer: &PublicSanitizer) -> String {
    let public_class = match candidate.trim() {
        "phase4-score-plan" => "phase4_score_plan",
        "redaction-scan" => "redaction_scan",
        "bundle-check" | "phase4-bundle-check" => "bundle_check",
        "context-check" | "phase4-context-check" => "context_check",
        "checksums" => "checksums",
        "verifier" => "verifier",
        _ => "verifier",
    };
    if sanitizer.inspect_text(public_class).is_ok() {
        public_class.to_string()
    } else {
        "verifier".to_string()
    }
}

fn sanitize_timestamp(
    field: &'static str,
    candidate: &str,
    sanitizer: &PublicSanitizer,
) -> Result<String, ValidationStatusError> {
    if is_rfc3339_timestamp(candidate) && sanitizer.inspect_text(candidate).is_ok() {
        Ok(candidate.to_string())
    } else {
        Err(ValidationStatusError::InvalidTimestamp { field })
    }
}

fn sanitize_summary(
    status: ValidationRunStatus,
    candidate: &str,
    sanitizer: &PublicSanitizer,
) -> String {
    let fallback = status.fallback_summary();
    if candidate.trim() == fallback && sanitizer.inspect_text(candidate).is_ok() {
        return bounded_public_text(candidate, SUMMARY_MAX_LEN);
    }
    bounded_public_text(
        &sanitizer.sanitize_text(candidate, fallback),
        SUMMARY_MAX_LEN,
    )
}

fn sanitize_issue_summary(issue: PublicValidationIssue, sanitizer: &PublicSanitizer) -> String {
    let message = issue.message();
    bounded_public_text(
        &sanitizer.sanitize_text(message, "Validation issue redacted."),
        SUMMARY_MAX_LEN,
    )
}

fn bounded_public_text(candidate: &str, max_len: usize) -> String {
    candidate.chars().take(max_len).collect()
}

fn is_rfc3339_timestamp(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    if !is_date_time_prefix(bytes) {
        return false;
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while bytes.get(index).is_some_and(|byte| byte.is_ascii_digit()) {
            index += 1;
        }
        if index == fraction_start {
            return false;
        }
    }
    match bytes.get(index) {
        Some(b'Z') => index + 1 == bytes.len(),
        Some(b'+') | Some(b'-') => {
            bytes.len() == index + 6
                && bytes[index + 1].is_ascii_digit()
                && bytes[index + 2].is_ascii_digit()
                && bytes[index + 3] == b':'
                && bytes[index + 4].is_ascii_digit()
                && bytes[index + 5].is_ascii_digit()
        }
        _ => false,
    }
}

fn is_date_time_prefix(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        [
            y0,
            y1,
            y2,
            y3,
            b'-',
            m0,
            m1,
            b'-',
            d0,
            d1,
            b'T',
            h0,
            h1,
            b':',
            n0,
            n1,
            b':',
            s0,
            s1,
            ..
        ] if y0.is_ascii_digit()
            && y1.is_ascii_digit()
            && y2.is_ascii_digit()
            && y3.is_ascii_digit()
            && m0.is_ascii_digit()
            && m1.is_ascii_digit()
            && d0.is_ascii_digit()
            && d1.is_ascii_digit()
            && h0.is_ascii_digit()
            && h1.is_ascii_digit()
            && n0.is_ascii_digit()
            && n1.is_ascii_digit()
            && s0.is_ascii_digit()
            && s1.is_ascii_digit()
    )
}
