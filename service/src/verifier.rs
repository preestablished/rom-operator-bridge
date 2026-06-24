use crate::{
    artifacts::ARTIFACT_SCHEMA_VERSION,
    labels::{
        ChangedOffsetRange, DedupGroup, DedupRelation, DedupStatus, LabelSnapshot, StatusLabelRole,
    },
    private_config::{BridgePrivateConfig, PrivateConfigError},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Component, Path, PathBuf},
};
use thiserror::Error;

const CAPTURES_INDEX_PATH: &str = "captures/index.jsonl";
const DEDUP_GROUPS_PATH: &str = "dedup-groups.jsonl";
const SCORE_PLAN_INPUT_PATH: &str = "validation/phase4-score-plan-input.json";
const SCORE_PLAN_OUTPUT_PATH: &str = "validation/score-plan.json";
const SCORE_PLAN_REPORT_PATH: &str = "validation/phase4-score-plan.json";

pub fn write_phase4_verifier_inputs(
    private_config: &BridgePrivateConfig,
    snapshot: &LabelSnapshot,
) -> Result<Phase4VerifierInputs, VerifierTransformError> {
    if private_config.is_placeholder() {
        return Err(VerifierTransformError::MissingPrivateConfig);
    }

    let targets = required_targets(snapshot)?;
    let dedup_groups = validate_dedup_groups(&snapshot.dedup_groups)?;
    let request = Phase4ScorePlanInputFile::new(snapshot.label_revision, &targets);
    let input_bytes = serde_json::to_vec_pretty(&request)?;

    let mut dedup_bytes = Vec::new();
    for group in dedup_groups {
        serde_json::to_writer(&mut dedup_bytes, &DedupArtifactRow::from(group))?;
        dedup_bytes.push(b'\n');
    }

    private_config.write_private_file_atomic(DEDUP_GROUPS_PATH, &dedup_bytes)?;
    private_config.write_private_file_atomic(SCORE_PLAN_INPUT_PATH, &input_bytes)?;

    Ok(Phase4VerifierInputs {
        score_plan: Phase4ScorePlanInvocation::new(targets),
        private_artifacts: Phase4PrivateArtifacts::default(),
    })
}

#[derive(Clone, PartialEq, Eq)]
pub struct Phase4VerifierInputs {
    pub score_plan: Phase4ScorePlanInvocation,
    pub private_artifacts: Phase4PrivateArtifacts,
}

impl fmt::Debug for Phase4VerifierInputs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Phase4VerifierInputs")
            .field("score_plan", &self.score_plan)
            .field("private_artifacts", &self.private_artifacts)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Phase4ScorePlanInvocation {
    pub command_class: &'static str,
    pub working_directory: PrivateVerifierPath,
    pub arguments: Vec<String>,
    pub first_boss: String,
    pub goal_positive: String,
    pub goal_negative: String,
}

impl Phase4ScorePlanInvocation {
    fn new(targets: RequiredTargets) -> Self {
        Self {
            command_class: "phase4-score-plan",
            working_directory: PrivateVerifierPath::new("."),
            arguments: vec![
                "--captures".to_string(),
                CAPTURES_INDEX_PATH.to_string(),
                "--out".to_string(),
                SCORE_PLAN_OUTPUT_PATH.to_string(),
                "--first-boss".to_string(),
                targets.first_boss.clone(),
                "--goal-positive".to_string(),
                targets.goal_positive.clone(),
                "--goal-negative".to_string(),
                targets.goal_negative.clone(),
            ],
            first_boss: targets.first_boss,
            goal_positive: targets.goal_positive,
            goal_negative: targets.goal_negative,
        }
    }

    pub fn resolved_arguments(&self, private_root: impl AsRef<Path>) -> Vec<String> {
        let private_root = private_root.as_ref();
        vec![
            self.command_class.to_string(),
            "--captures".to_string(),
            private_root.join(CAPTURES_INDEX_PATH).display().to_string(),
            "--out".to_string(),
            private_root
                .join(SCORE_PLAN_OUTPUT_PATH)
                .display()
                .to_string(),
            "--first-boss".to_string(),
            self.first_boss.clone(),
            "--goal-positive".to_string(),
            self.goal_positive.clone(),
            "--goal-negative".to_string(),
            self.goal_negative.clone(),
        ]
    }
}

impl fmt::Debug for Phase4ScorePlanInvocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Phase4ScorePlanInvocation")
            .field("command_class", &self.command_class)
            .field("working_directory", &self.working_directory)
            .field("argument_count", &self.arguments.len())
            .field("targets", &"[private capture ids]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Phase4PrivateArtifacts {
    pub score_plan_input: PrivateVerifierPath,
    pub score_plan_output: PrivateVerifierPath,
    pub score_plan_report: PrivateVerifierPath,
    pub captures_index: PrivateVerifierPath,
    pub dedup_groups: PrivateVerifierPath,
}

impl Default for Phase4PrivateArtifacts {
    fn default() -> Self {
        Self {
            score_plan_input: PrivateVerifierPath::new(SCORE_PLAN_INPUT_PATH),
            score_plan_output: PrivateVerifierPath::new(SCORE_PLAN_OUTPUT_PATH),
            score_plan_report: PrivateVerifierPath::new(SCORE_PLAN_REPORT_PATH),
            captures_index: PrivateVerifierPath::new(CAPTURES_INDEX_PATH),
            dedup_groups: PrivateVerifierPath::new(DEDUP_GROUPS_PATH),
        }
    }
}

impl fmt::Debug for Phase4PrivateArtifacts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Phase4PrivateArtifacts")
            .field("score_plan_input", &self.score_plan_input)
            .field("score_plan_output", &self.score_plan_output)
            .field("score_plan_report", &self.score_plan_report)
            .field("captures_index", &self.captures_index)
            .field("dedup_groups", &self.dedup_groups)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PrivateVerifierPath {
    relative_path: PathBuf,
}

impl PrivateVerifierPath {
    fn new(path: impl Into<PathBuf>) -> Self {
        let relative_path = path.into();
        debug_assert!(
            relative_path.is_relative()
                && !relative_path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
        );
        Self { relative_path }
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }
}

impl fmt::Debug for PrivateVerifierPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[private verifier path]")
    }
}

impl fmt::Display for PrivateVerifierPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[private verifier path]")
    }
}

#[derive(Error)]
pub enum VerifierTransformError {
    #[error("private verifier config is not configured")]
    MissingPrivateConfig,
    #[error("required verifier labels are missing")]
    MissingRequiredLabels(Vec<&'static str>),
    #[error("verifier labels conflict")]
    ConflictingLabels(Vec<VerifierLabelConflict>),
    #[error("dedup groups are invalid")]
    InvalidDedupGroups(Vec<VerifierLabelConflict>),
    #[error("private verifier artifact write failed")]
    PrivateWrite(#[from] PrivateConfigError),
    #[error("private verifier artifact serialization failed")]
    Json(#[from] serde_json::Error),
}

impl fmt::Debug for VerifierTransformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrivateConfig => formatter.write_str("MissingPrivateConfig"),
            Self::MissingRequiredLabels(labels) => formatter
                .debug_tuple("MissingRequiredLabels")
                .field(labels)
                .finish(),
            Self::ConflictingLabels(conflicts) => formatter
                .debug_struct("ConflictingLabels")
                .field("count", &conflicts.len())
                .finish(),
            Self::InvalidDedupGroups(conflicts) => formatter
                .debug_struct("InvalidDedupGroups")
                .field("count", &conflicts.len())
                .finish(),
            Self::PrivateWrite(_) => formatter.write_str("PrivateWrite([redacted])"),
            Self::Json(_) => formatter.write_str("Json([redacted])"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifierLabelConflict {
    pub code: &'static str,
    pub capture_id: String,
    pub message: &'static str,
}

impl VerifierLabelConflict {
    fn new(code: &'static str, capture_id: impl Into<String>, message: &'static str) -> Self {
        Self {
            code,
            capture_id: capture_id.into(),
            message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequiredTargets {
    first_boss: String,
    goal_positive: String,
    goal_negative: String,
}

fn required_targets(snapshot: &LabelSnapshot) -> Result<RequiredTargets, VerifierTransformError> {
    let mut missing = Vec::new();
    let first_boss = required_label(
        snapshot.target_labels.first_boss.clone(),
        "first_boss",
        &mut missing,
    );
    let goal_positive = required_label(
        snapshot.target_labels.goal_positive.clone(),
        "goal_positive",
        &mut missing,
    );
    let goal_negative = required_label(
        snapshot.target_labels.goal_negative.clone(),
        "goal_negative",
        &mut missing,
    );
    if !missing.is_empty() {
        return Err(VerifierTransformError::MissingRequiredLabels(missing));
    }

    let targets = RequiredTargets {
        first_boss: first_boss.expect("missing checked"),
        goal_positive: goal_positive.expect("missing checked"),
        goal_negative: goal_negative.expect("missing checked"),
    };
    validate_target_conflicts(snapshot, &targets)?;
    Ok(targets)
}

fn required_label(
    label: Option<String>,
    name: &'static str,
    missing: &mut Vec<&'static str>,
) -> Option<String> {
    if label.is_none() {
        missing.push(name);
    }
    label
}

fn validate_target_conflicts(
    snapshot: &LabelSnapshot,
    targets: &RequiredTargets,
) -> Result<(), VerifierTransformError> {
    let mut conflicts = Vec::new();
    let mut by_capture: BTreeMap<&str, Vec<&'static str>> = BTreeMap::new();
    by_capture
        .entry(&targets.first_boss)
        .or_default()
        .push("first_boss");
    by_capture
        .entry(&targets.goal_positive)
        .or_default()
        .push("goal_positive");
    by_capture
        .entry(&targets.goal_negative)
        .or_default()
        .push("goal_negative");
    for (capture_id, roles) in by_capture {
        if roles.len() > 1 {
            conflicts.push(VerifierLabelConflict::new(
                "target_role_conflict",
                capture_id,
                "Capture is assigned to multiple required verifier target roles.",
            ));
        }
    }

    let target_ids: BTreeSet<&str> = [
        targets.first_boss.as_str(),
        targets.goal_positive.as_str(),
        targets.goal_negative.as_str(),
    ]
    .into_iter()
    .collect();
    for status in &snapshot.status_labels {
        if target_ids.contains(status.capture_id.as_str()) {
            conflicts.push(VerifierLabelConflict::new(
                match status.status {
                    StatusLabelRole::NeedsReview => "target_needs_review_conflict",
                    StatusLabelRole::Rejected => "target_rejected_conflict",
                },
                &status.capture_id,
                "Verifier target capture also has a non-target status label.",
            ));
        }
    }

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(VerifierTransformError::ConflictingLabels(conflicts))
    }
}

fn validate_dedup_groups(groups: &[DedupGroup]) -> Result<&[DedupGroup], VerifierTransformError> {
    let mut conflicts = Vec::new();
    let mut group_ids = BTreeSet::new();
    let mut saw_same_canonical_state = false;
    let mut saw_distinct_stable_state = false;

    for group in groups {
        if !group_ids.insert(group.group_id.as_str()) {
            conflicts.push(VerifierLabelConflict::new(
                "dedup_duplicate_group",
                &group.group_id,
                "Dedup artifact contains the same group more than once.",
            ));
        }
        if matches!(group.status, Some(DedupStatus::Conflict)) {
            conflicts.push(VerifierLabelConflict::new(
                "dedup_conflict_status",
                &group.group_id,
                "Conflict dedup groups must not be emitted to verifier artifacts.",
            ));
        } else {
            match group.expected_relation {
                DedupRelation::SameCanonicalState => saw_same_canonical_state = true,
                DedupRelation::DistinctStableState => saw_distinct_stable_state = true,
            }
        }

        let mut capture_ids = BTreeSet::new();
        for capture_id in &group.capture_ids {
            if !capture_ids.insert(capture_id) {
                conflicts.push(VerifierLabelConflict::new(
                    "dedup_duplicate_capture",
                    capture_id,
                    "Dedup group contains the same capture more than once.",
                ));
            }
        }
        let mut changed_features = BTreeSet::new();
        for feature in &group.changed_features {
            if !changed_features.insert(feature) {
                conflicts.push(VerifierLabelConflict::new(
                    "dedup_duplicate_changed_feature",
                    &group.group_id,
                    "Dedup group contains the same changed feature more than once.",
                ));
            }
        }
        for range in &group.changed_offset_ranges {
            if range.len == 0 {
                conflicts.push(VerifierLabelConflict::new(
                    "dedup_empty_offset_range",
                    &group.group_id,
                    "Dedup group changed offset ranges must have non-zero length.",
                ));
            }
        }
        if group.capture_ids.len() < 2 {
            conflicts.push(VerifierLabelConflict::new(
                "dedup_too_small",
                &group.group_id,
                "Dedup group requires at least two captures.",
            ));
        }
        if group.changed_features.is_empty() && group.changed_offset_ranges.is_empty() {
            conflicts.push(VerifierLabelConflict::new(
                "dedup_missing_change",
                &group.group_id,
                "Dedup group requires changed features or changed offset ranges.",
            ));
        }
    }
    if !saw_same_canonical_state {
        conflicts.push(VerifierLabelConflict::new(
            "dedup_missing_same_canonical_state",
            "dedup-groups",
            "Dedup artifact requires at least one same_canonical_state group.",
        ));
    }
    if !saw_distinct_stable_state {
        conflicts.push(VerifierLabelConflict::new(
            "dedup_missing_distinct_stable_state",
            "dedup-groups",
            "Dedup artifact requires at least one distinct_stable_state group.",
        ));
    }

    if conflicts.is_empty() {
        Ok(groups)
    } else {
        Err(VerifierTransformError::InvalidDedupGroups(conflicts))
    }
}

#[derive(Debug, Serialize)]
struct DedupArtifactRow<'a> {
    schema_version: u16,
    group_id: &'a str,
    expected_relation: DedupRelation,
    capture_ids: &'a Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    changed_features: &'a Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    changed_offset_ranges: &'a Vec<ChangedOffsetRange>,
}

impl<'a> From<&'a DedupGroup> for DedupArtifactRow<'a> {
    fn from(group: &'a DedupGroup) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            group_id: &group.group_id,
            expected_relation: group.expected_relation,
            capture_ids: &group.capture_ids,
            changed_features: &group.changed_features,
            changed_offset_ranges: &group.changed_offset_ranges,
        }
    }
}

#[derive(Debug, Serialize)]
struct Phase4ScorePlanInputFile<'a> {
    schema_version: u16,
    kind: &'static str,
    command_class: &'static str,
    label_revision: u64,
    captures: &'static str,
    out: &'static str,
    report: &'static str,
    dedup_groups: &'static str,
    labels: Phase4ScorePlanLabels<'a>,
}

#[derive(Debug, Serialize)]
struct Phase4ScorePlanLabels<'a> {
    first_boss: &'a str,
    goal_positive: &'a str,
    goal_negative: &'a str,
}

impl<'a> Phase4ScorePlanInputFile<'a> {
    fn new(label_revision: u64, targets: &'a RequiredTargets) -> Self {
        Self {
            schema_version: ARTIFACT_SCHEMA_VERSION,
            kind: "phase4-score-plan-input",
            command_class: "phase4-score-plan",
            label_revision,
            captures: CAPTURES_INDEX_PATH,
            out: SCORE_PLAN_OUTPUT_PATH,
            report: SCORE_PLAN_REPORT_PATH,
            dedup_groups: DEDUP_GROUPS_PATH,
            labels: Phase4ScorePlanLabels {
                first_boss: &targets.first_boss,
                goal_positive: &targets.goal_positive,
                goal_negative: &targets.goal_negative,
            },
        }
    }
}
