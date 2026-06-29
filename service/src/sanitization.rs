use serde_json::Value;
use std::path::Path;
use thiserror::Error;

const DEFAULT_SAFE_MESSAGE: &str = "Request could not be completed.";
const AUTH_REJECTION_MESSAGE: &str = "Authentication rejected.";
const INPUT_REJECTION_MESSAGE: &str = "Input rejected.";

#[derive(Debug, Clone)]
pub struct PublicSanitizer {
    private_roots: Vec<String>,
    forbidden_literals: Vec<String>,
}

impl Default for PublicSanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl PublicSanitizer {
    pub fn new() -> Self {
        Self {
            private_roots: Vec::new(),
            forbidden_literals: Vec::new(),
        }
    }

    pub fn with_private_root(mut self, root: impl AsRef<Path>) -> Self {
        if let Some(root) = normalize_root(root.as_ref()) {
            self.private_roots.push(root);
        }
        self
    }

    pub fn with_forbidden_literal(mut self, literal: impl Into<String>) -> Self {
        let literal = literal.into();
        if !literal.is_empty() {
            self.forbidden_literals.push(literal);
        }
        self
    }

    pub fn inspect_text(&self, text: &str) -> Result<(), SanitizationError> {
        if text.contains('\0') {
            return Err(SanitizationError::RawPayloadSnippet {
                pattern: "nul byte",
            });
        }

        for literal in &self.forbidden_literals {
            if text.contains(literal) {
                return Err(SanitizationError::ForbiddenLiteral);
            }
        }

        for root in &self.private_roots {
            if text.contains(root) {
                return Err(SanitizationError::ConfiguredPrivateRoot);
            }
        }

        if contains_private_path(text) {
            return Err(SanitizationError::PrivatePath);
        }

        let lowered = text.to_ascii_lowercase();
        if contains_any(&lowered, COMMAND_OUTPUT_PATTERNS) {
            return Err(SanitizationError::CommandOutput);
        }
        if contains_any(&lowered, RAW_PAYLOAD_PATTERNS) {
            return Err(SanitizationError::RawPayloadSnippet {
                pattern: "raw payload",
            });
        }
        if contains_any(&lowered, VALIDATION_REPORT_PATTERNS) {
            return Err(SanitizationError::ValidationReportExcerpt);
        }

        Ok(())
    }

    pub fn inspect_json(&self, value: &Value) -> Result<(), SanitizationError> {
        self.inspect_json_value(value)
    }

    pub fn inspect_event(&self, value: &Value) -> Result<(), SanitizationError> {
        self.inspect_json(value)
    }

    pub fn inspect_capture_metadata(&self, value: &Value) -> Result<(), SanitizationError> {
        self.inspect_json(value)
    }

    pub fn inspect_validation_summary(&self, value: &Value) -> Result<(), SanitizationError> {
        self.inspect_json(value)
    }

    pub fn sanitize_text(&self, candidate: &str, fallback: &str) -> String {
        let fallback = if self.inspect_text(fallback).is_ok() && !fallback.is_empty() {
            fallback
        } else {
            DEFAULT_SAFE_MESSAGE
        };

        if !candidate.is_empty() && self.inspect_text(candidate).is_ok() {
            candidate.to_string()
        } else {
            fallback.to_string()
        }
    }

    pub fn sanitize_error_message(&self, candidate: &str) -> String {
        self.sanitize_text(candidate, DEFAULT_SAFE_MESSAGE)
    }

    pub fn sanitize_auth_rejection_message(&self, candidate: &str) -> String {
        self.sanitize_text(candidate, AUTH_REJECTION_MESSAGE)
    }

    pub fn sanitize_input_rejection_message(&self, candidate: &str) -> String {
        self.sanitize_text(candidate, INPUT_REJECTION_MESSAGE)
    }

    pub fn empty_public_details(&self) -> Value {
        Value::Object(Default::default())
    }

    fn inspect_json_value(&self, value: &Value) -> Result<(), SanitizationError> {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
            Value::String(text) => self.inspect_text(text),
            Value::Array(items) => {
                for item in items {
                    self.inspect_json_value(item)?;
                }
                Ok(())
            }
            Value::Object(map) => {
                for (key, value) in map {
                    self.inspect_text(key)?;
                    inspect_public_key(key)?;
                    self.inspect_json_value(value)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SanitizationError {
    #[error("public surface contains a private path")]
    PrivatePath,
    #[error("public surface contains a configured private root")]
    ConfiguredPrivateRoot,
    #[error("public surface contains a configured forbidden literal")]
    ForbiddenLiteral,
    #[error("public surface contains command output")]
    CommandOutput,
    #[error("public surface contains raw payload data: {pattern}")]
    RawPayloadSnippet { pattern: &'static str },
    #[error("public surface contains a validation report excerpt")]
    ValidationReportExcerpt,
    #[error("public surface contains forbidden field `{field}`")]
    ForbiddenField { field: String },
}

fn normalize_root(path: &Path) -> Option<String> {
    let root = path.to_string_lossy();
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        None
    } else {
        Some(root.to_string())
    }
}

fn inspect_public_key(key: &str) -> Result<(), SanitizationError> {
    let canonical = canonical_field_name(key);
    if FORBIDDEN_FIELD_NAMES.contains(&canonical.as_str())
        || FORBIDDEN_FIELD_FRAGMENTS
            .iter()
            .any(|fragment| canonical.contains(fragment))
    {
        Err(SanitizationError::ForbiddenField {
            field: key.to_string(),
        })
    } else {
        Ok(())
    }
}

fn canonical_field_name(key: &str) -> String {
    key.chars()
        .filter(|ch| !matches!(ch, '_' | '-' | ' ' | '\t' | '\n' | '\r'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn contains_private_path(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    if contains_any(&lowered, PRIVATE_PATH_PATTERNS) {
        return true;
    }

    text.as_bytes()
        .windows(3)
        .any(|window| window[0].is_ascii_alphabetic() && window[1] == b':' && window[2] == b'\\')
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| text.contains(pattern))
}

const PRIVATE_PATH_PATTERNS: &[&str] = &[
    "/home/",
    "/users/",
    "/root/",
    "/private/",
    "/run/dh/",
    "/run/rom",
    "/run/secret",
    "/etc/",
    "/opt/",
    "/var/",
    "/tmp/",
    "/mnt/",
    "/srv/",
    "/dev/shm/",
];

const COMMAND_OUTPUT_PATTERNS: &[&str] = &[
    "stderr:",
    "stdout:",
    "stack backtrace",
    "rust_backtrace",
    "panicked at",
    "traceback (most recent call last)",
    "command failed",
    "exit status",
];

const RAW_PAYLOAD_PATTERNS: &[&str] = &[
    "feature_bytes",
    "decoded_values",
    "decoded_features",
    "raw_payload",
    "payload_snippet",
    "raw framebuffer",
    "rom bytes",
    "save ram",
    "worker lease token",
    "artifact ref",
];

const VALIDATION_REPORT_PATTERNS: &[&str] = &[
    "validation report",
    "phase4-bundle-check",
    "redaction-scan",
    "score-plan",
    "validation/redaction",
    "bundle-check",
];

const FORBIDDEN_FIELD_NAMES: &[&str] = &[
    "workerleasetoken",
    "privatepath",
    "privateroot",
    "artifactref",
    "featurebytes",
    "decodedvalues",
    "decodedfeatures",
    "rawpayload",
    "payloadsnippet",
    "validationreport",
    "stderr",
    "stdout",
    "commandoutput",
    "stacktrace",
    "rombytes",
    "saveram",
    "screenshot",
];

const FORBIDDEN_FIELD_FRAGMENTS: &[&str] = &[
    "privatepath",
    "privateroot",
    "workerlease",
    "artifactref",
    "featurebytes",
    "decodedvalue",
    "rawpayload",
    "payloadsnippet",
    "validationreport",
    "stacktrace",
    "stderr",
    "stdout",
    "commandoutput",
];
