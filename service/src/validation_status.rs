use crate::{
    artifacts::{PrivateArtifactStore, ValidationRunRow},
    private_config::BridgePrivateConfig,
    sanitization::PublicSanitizer,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use thiserror::Error;

const SUMMARY_MAX_LEN: usize = 240;
const COMMAND_CLASS_MAX_LEN: usize = 64;
const MAX_ISSUE_SUMMARIES: usize = 8;
const FALLBACK_TIMESTAMP: &str = "1970-01-01T00:00:00Z";

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

    pub fn record_run(
        &self,
        private_config: &BridgePrivateConfig,
        sanitizer: &PublicSanitizer,
        update: ValidationRunUpdate,
    ) -> Result<PublicValidationStatus, ValidationStatusError> {
        let command_class = sanitize_command_class(&update.command_class, sanitizer);
        let started_at = sanitize_timestamp(&update.started_at, sanitizer);
        let completed_at = update
            .completed_at
            .as_deref()
            .map(|timestamp| sanitize_timestamp(timestamp, sanitizer));
        let summary = sanitize_summary(update.status, &update.summary, sanitizer);
        let issue_summaries = update
            .issue_summaries
            .iter()
            .filter(|issue| !issue.trim().is_empty())
            .take(MAX_ISSUE_SUMMARIES)
            .map(|issue| sanitize_issue_summary(issue, sanitizer))
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
    pub validation_id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub command_class: String,
    pub status: ValidationRunStatus,
    pub summary: String,
    pub issue_summaries: Vec<String>,
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
            validation_id: validation_id.into(),
            started_at: started_at.into(),
            completed_at: None,
            command_class: command_class.into(),
            status,
            summary: summary.into(),
            issue_summaries: Vec::new(),
        }
    }

    pub fn completed_at(mut self, completed_at: impl Into<String>) -> Self {
        self.completed_at = Some(completed_at.into());
        self
    }

    pub fn issue_summaries<I, S>(mut self, issue_summaries: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.issue_summaries = issue_summaries.into_iter().map(Into::into).collect();
        self
    }
}

#[derive(Debug, Error)]
pub enum ValidationStatusError {
    #[error(transparent)]
    Artifact(#[from] crate::artifacts::ArtifactError),
}

fn sanitize_command_class(candidate: &str, sanitizer: &PublicSanitizer) -> String {
    let normalized = normalize_command_class(candidate);
    if !normalized.is_empty() && sanitizer.inspect_text(&normalized).is_ok() {
        normalized
    } else {
        "verifier".to_string()
    }
}

fn normalize_command_class(candidate: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    for character in candidate.chars() {
        let next = if character.is_ascii_alphanumeric() {
            last_was_separator = false;
            character.to_ascii_lowercase()
        } else if matches!(character, '.' | ':' | '_' | '-') {
            if last_was_separator {
                continue;
            }
            last_was_separator = true;
            '_'
        } else {
            continue;
        };
        if normalized.len() >= COMMAND_CLASS_MAX_LEN {
            break;
        }
        normalized.push(next);
    }
    normalized.trim_matches('_').to_string()
}

fn sanitize_timestamp(candidate: &str, sanitizer: &PublicSanitizer) -> String {
    bounded_public_text(
        &sanitizer.sanitize_text(candidate, FALLBACK_TIMESTAMP),
        SUMMARY_MAX_LEN,
    )
}

fn sanitize_summary(
    status: ValidationRunStatus,
    candidate: &str,
    sanitizer: &PublicSanitizer,
) -> String {
    bounded_public_text(
        &sanitizer.sanitize_text(candidate, status.fallback_summary()),
        SUMMARY_MAX_LEN,
    )
}

fn sanitize_issue_summary(candidate: &str, sanitizer: &PublicSanitizer) -> String {
    bounded_public_text(
        &sanitizer.sanitize_text(candidate, "Validation issue redacted."),
        SUMMARY_MAX_LEN,
    )
}

fn bounded_public_text(candidate: &str, max_len: usize) -> String {
    candidate.chars().take(max_len).collect()
}
