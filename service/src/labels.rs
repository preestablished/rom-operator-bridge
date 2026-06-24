use crate::artifacts::{LabelDraft, LabelDraftFile, PrivateArtifactStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

const UPDATED_AT: &str = "1970-01-01T00:00:00Z";
const JSON_SAFE_U64_MAX: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Default)]
pub struct LabelState {
    inner: Arc<Mutex<LabelInner>>,
}

impl LabelState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&self) {
        *self.inner.lock().expect("label mutex poisoned") = LabelInner::default();
    }

    pub fn apply<F>(
        &self,
        request: LabelApplyRequest,
        mut can_label_capture: F,
        store: Option<&PrivateArtifactStore<'_>>,
    ) -> Result<LabelApplyOutcome, LabelStoreError>
    where
        F: FnMut(&str) -> bool,
    {
        let key = (request.session_id.clone(), request.idempotency_key.clone());
        let mut inner = self.inner.lock().expect("label mutex poisoned");
        if let Some(outcome) = inner.idempotency.get(&key) {
            return Ok(outcome.clone());
        }

        let mut conflicts = Vec::new();
        for update in &request.updates {
            if !is_contract_id(&update.capture_id) || !can_label_capture(&update.capture_id) {
                conflicts.push(LabelConflict::label(
                    "Capture is not labelable in the active run.",
                    false,
                ));
            }
            if let Some(note) = &update.note
                && !valid_note(note)
            {
                conflicts.push(LabelConflict::bad_request("Label note is invalid.", false));
            }
        }
        for update in &request.dedup_updates {
            if let Some(group) = update.group() {
                for capture_id in &group.capture_ids {
                    if !can_label_capture(capture_id) {
                        conflicts.push(LabelConflict::label(
                            "Dedup captures must be labelable in the active run.",
                            false,
                        ));
                    }
                }
            }
        }
        if !conflicts.is_empty() {
            let outcome = LabelApplyOutcome {
                applied: false,
                label_revision: inner.label_revision,
                conflicts,
            };
            inner.idempotency.insert(key, outcome.clone());
            return Ok(outcome);
        }

        let mut next = inner.clone();
        let mut changed_captures = BTreeSet::new();
        let mut conflicts = Vec::new();

        for update in &request.updates {
            match update.op {
                LabelOp::Upsert => match next.upsert(update) {
                    Ok(changed) => changed_captures.extend(changed),
                    Err(conflict) => conflicts.push(conflict),
                },
                LabelOp::Delete => {
                    changed_captures.extend(next.delete(update));
                }
            }
        }
        for update in &request.dedup_updates {
            match update.op {
                DedupOp::Upsert => {
                    let Some(group) = update.group() else {
                        conflicts.push(LabelConflict::bad_request(
                            "Dedup update is invalid.",
                            false,
                        ));
                        continue;
                    };
                    match validate_dedup_group(&next, &group) {
                        Ok(()) => {
                            changed_captures.extend(group.capture_ids.iter().cloned());
                            next.dedup_groups.insert(group.group_id.clone(), group);
                        }
                        Err(LabelStoreError::Conflict(next_conflicts)) => {
                            conflicts.extend(next_conflicts)
                        }
                        Err(LabelStoreError::BackendUnavailable) => {
                            return Err(LabelStoreError::BackendUnavailable);
                        }
                    }
                }
                DedupOp::Delete => {
                    if !is_contract_id(&update.group_id) {
                        conflicts.push(LabelConflict::bad_request(
                            "Dedup group id is invalid.",
                            false,
                        ));
                        continue;
                    }
                    if let Some(group) = next.dedup_groups.remove(&update.group_id) {
                        changed_captures.extend(group.capture_ids);
                    }
                }
            }
        }

        if !conflicts.is_empty() {
            let outcome = LabelApplyOutcome {
                applied: false,
                label_revision: inner.label_revision,
                conflicts,
            };
            inner.idempotency.insert(key, outcome.clone());
            return Ok(outcome);
        }

        if changed_captures.is_empty() {
            let outcome = LabelApplyOutcome {
                applied: false,
                label_revision: inner.label_revision,
                conflicts: Vec::new(),
            };
            inner.idempotency.insert(key, outcome.clone());
            return Ok(outcome);
        }

        if let Some(store) = store {
            let rollback_drafts: BTreeMap<String, LabelDraftFile> = changed_captures
                .iter()
                .map(|capture_id| (capture_id.clone(), inner.draft_for(capture_id)))
                .collect();
            let mut written_captures = Vec::new();
            for capture_id in &changed_captures {
                if store
                    .write_label_draft(&next.draft_for(capture_id))
                    .is_err()
                {
                    for written_capture_id in written_captures {
                        if let Some(draft) = rollback_drafts.get(&written_capture_id) {
                            let _ = store.write_label_draft(draft);
                        }
                    }
                    return Err(LabelStoreError::BackendUnavailable);
                }
                written_captures.push(capture_id.clone());
            }
        }

        next.label_revision = inner.label_revision.saturating_add(1);
        let outcome = LabelApplyOutcome {
            applied: true,
            label_revision: next.label_revision,
            conflicts: Vec::new(),
        };
        next.idempotency.insert(key, outcome.clone());
        *inner = next;
        Ok(outcome)
    }

    pub fn snapshot(&self) -> LabelSnapshot {
        self.inner.lock().expect("label mutex poisoned").snapshot()
    }

    pub fn label_names_for_capture(&self, capture_id: &str) -> Vec<String> {
        self.inner
            .lock()
            .expect("label mutex poisoned")
            .roles_for_capture(capture_id)
            .into_iter()
            .map(|role| role.as_str().to_string())
            .collect()
    }

    pub fn upsert_dedup_group(&self, group: DedupGroup) -> Result<u64, LabelStoreError> {
        let mut inner = self.inner.lock().expect("label mutex poisoned");
        validate_dedup_group(&inner, &group)?;
        inner.dedup_groups.insert(group.group_id.clone(), group);
        inner.label_revision = inner.label_revision.saturating_add(1);
        Ok(inner.label_revision)
    }

    pub fn delete_dedup_group(&self, group_id: &str) -> Result<u64, LabelStoreError> {
        if !is_contract_id(group_id) {
            return Err(LabelStoreError::Conflict(vec![LabelConflict::bad_request(
                "Dedup group id is invalid.",
                false,
            )]));
        }
        let mut inner = self.inner.lock().expect("label mutex poisoned");
        if inner.dedup_groups.remove(group_id).is_some() {
            inner.label_revision = inner.label_revision.saturating_add(1);
        }
        Ok(inner.label_revision)
    }
}

#[derive(Debug, Clone, Default)]
struct LabelInner {
    label_revision: u64,
    idempotency: BTreeMap<(String, String), LabelApplyOutcome>,
    targets: BTreeMap<TargetLabelRole, String>,
    statuses: BTreeMap<String, StatusLabelRole>,
    notes: BTreeMap<String, String>,
    dedup_groups: BTreeMap<String, DedupGroup>,
}

impl LabelInner {
    fn upsert(&mut self, update: &LabelUpdate) -> Result<Vec<String>, LabelConflict> {
        if let Some(note) = &update.note {
            self.notes.insert(update.capture_id.clone(), note.clone());
        }

        match update.role.classify() {
            LabelRoleClass::Target(role) => {
                if self.statuses.contains_key(&update.capture_id) {
                    return Err(LabelConflict::label(
                        "Status-labeled captures cannot be target labels.",
                        false,
                    ));
                }
                let previous = self.targets.insert(role, update.capture_id.clone());
                let mut changed = vec![update.capture_id.clone()];
                if let Some(previous) = previous
                    && previous != update.capture_id
                {
                    changed.push(previous);
                }
                Ok(changed)
            }
            LabelRoleClass::Status(StatusLabelRole::NeedsReview) => {
                if self.statuses.get(&update.capture_id) == Some(&StatusLabelRole::Rejected) {
                    return Err(LabelConflict::label(
                        "Rejected captures cannot also need review.",
                        false,
                    ));
                }
                if self
                    .targets
                    .values()
                    .any(|capture_id| capture_id == &update.capture_id)
                {
                    return Err(LabelConflict::label(
                        "Target label captures cannot also need review.",
                        false,
                    ));
                }
                self.statuses
                    .insert(update.capture_id.clone(), StatusLabelRole::NeedsReview);
                Ok(vec![update.capture_id.clone()])
            }
            LabelRoleClass::Status(StatusLabelRole::Rejected) => {
                if self.statuses.get(&update.capture_id) == Some(&StatusLabelRole::NeedsReview) {
                    return Err(LabelConflict::label(
                        "Needs-review captures cannot also be rejected.",
                        false,
                    ));
                }
                if self
                    .targets
                    .values()
                    .any(|capture_id| capture_id == &update.capture_id)
                {
                    return Err(LabelConflict::label(
                        "Target label captures cannot be rejected.",
                        false,
                    ));
                }
                if self
                    .dedup_groups
                    .values()
                    .any(|group| group.capture_ids.contains(&update.capture_id))
                {
                    return Err(LabelConflict::label(
                        "Dedup captures cannot be rejected.",
                        false,
                    ));
                }
                self.statuses
                    .insert(update.capture_id.clone(), StatusLabelRole::Rejected);
                Ok(vec![update.capture_id.clone()])
            }
        }
    }

    fn delete(&mut self, update: &LabelUpdate) -> Vec<String> {
        let mut changed = Vec::new();
        match update.role.classify() {
            LabelRoleClass::Target(role) => {
                if self.targets.get(&role) == Some(&update.capture_id) {
                    self.targets.remove(&role);
                    changed.push(update.capture_id.clone());
                }
            }
            LabelRoleClass::Status(role) => {
                if self.statuses.get(&update.capture_id) == Some(&role) {
                    self.statuses.remove(&update.capture_id);
                    changed.push(update.capture_id.clone());
                }
            }
        }
        changed
    }

    fn draft_for(&self, capture_id: &str) -> LabelDraftFile {
        let labels = self
            .roles_for_capture(capture_id)
            .into_iter()
            .map(|role| LabelDraft::new(role.as_str(), true))
            .collect();
        LabelDraftFile::new(
            capture_id,
            UPDATED_AT,
            labels,
            self.notes.get(capture_id).cloned(),
        )
    }

    fn roles_for_capture(&self, capture_id: &str) -> Vec<LabelRole> {
        let mut roles = Vec::new();
        for role in [
            TargetLabelRole::FirstBoss,
            TargetLabelRole::GoalPositive,
            TargetLabelRole::GoalNegative,
        ] {
            if self.targets.get(&role).is_some_and(|id| id == capture_id) {
                roles.push(LabelRole::from(role));
            }
        }
        if let Some(status) = self.statuses.get(capture_id) {
            roles.push(LabelRole::from(*status));
        }
        roles
    }

    fn snapshot(&self) -> LabelSnapshot {
        LabelSnapshot {
            label_revision: self.label_revision,
            target_labels: LabelTargetSnapshot {
                first_boss: self.targets.get(&TargetLabelRole::FirstBoss).cloned(),
                goal_positive: self.targets.get(&TargetLabelRole::GoalPositive).cloned(),
                goal_negative: self.targets.get(&TargetLabelRole::GoalNegative).cloned(),
            },
            status_labels: self
                .statuses
                .iter()
                .map(|(capture_id, status)| StatusLabelSnapshot {
                    capture_id: capture_id.clone(),
                    status: *status,
                })
                .collect(),
            dedup_groups: self.dedup_groups.values().cloned().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelApplyRequest {
    pub session_id: String,
    pub idempotency_key: String,
    pub updates: Vec<LabelUpdate>,
    pub dedup_updates: Vec<DedupUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelApplyOutcome {
    pub applied: bool,
    pub label_revision: u64,
    pub conflicts: Vec<LabelConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelConflict {
    pub kind: LabelConflictKind,
    pub message: &'static str,
    pub retryable: bool,
}

impl LabelConflict {
    fn label(message: &'static str, retryable: bool) -> Self {
        Self {
            kind: LabelConflictKind::LabelConflict,
            message,
            retryable,
        }
    }

    fn bad_request(message: &'static str, retryable: bool) -> Self {
        Self {
            kind: LabelConflictKind::BadRequest,
            message,
            retryable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelConflictKind {
    LabelConflict,
    BadRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelStoreError {
    BackendUnavailable,
    Conflict(Vec<LabelConflict>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelUpdate {
    pub op: LabelOp,
    pub capture_id: String,
    pub role: LabelRole,
    pub confidence: Option<LabelConfidence>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DedupUpdate {
    pub op: DedupOp,
    pub group_id: String,
    pub expected_relation: Option<DedupRelation>,
    pub capture_ids: Option<Vec<String>>,
    pub changed_features: Option<Vec<String>>,
    pub changed_offset_ranges: Option<Vec<ChangedOffsetRange>>,
    pub status: Option<DedupStatus>,
}

impl DedupUpdate {
    fn group(&self) -> Option<DedupGroup> {
        Some(DedupGroup {
            group_id: self.group_id.clone(),
            expected_relation: self.expected_relation?,
            capture_ids: self.capture_ids.clone()?,
            changed_features: self.changed_features.clone().unwrap_or_default(),
            changed_offset_ranges: self.changed_offset_ranges.clone().unwrap_or_default(),
            status: self.status,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelOp {
    Upsert,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DedupOp {
    Upsert,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelConfidence {
    Candidate,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelRole {
    FirstBoss,
    GoalPositive,
    GoalNegative,
    NeedsReview,
    Rejected,
}

impl LabelRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::FirstBoss => "first_boss",
            Self::GoalPositive => "goal_positive",
            Self::GoalNegative => "goal_negative",
            Self::NeedsReview => "needs_review",
            Self::Rejected => "rejected",
        }
    }

    fn classify(self) -> LabelRoleClass {
        match self {
            Self::FirstBoss => LabelRoleClass::Target(TargetLabelRole::FirstBoss),
            Self::GoalPositive => LabelRoleClass::Target(TargetLabelRole::GoalPositive),
            Self::GoalNegative => LabelRoleClass::Target(TargetLabelRole::GoalNegative),
            Self::NeedsReview => LabelRoleClass::Status(StatusLabelRole::NeedsReview),
            Self::Rejected => LabelRoleClass::Status(StatusLabelRole::Rejected),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelRoleClass {
    Target(TargetLabelRole),
    Status(StatusLabelRole),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetLabelRole {
    FirstBoss,
    GoalPositive,
    GoalNegative,
}

impl From<TargetLabelRole> for LabelRole {
    fn from(role: TargetLabelRole) -> Self {
        match role {
            TargetLabelRole::FirstBoss => Self::FirstBoss,
            TargetLabelRole::GoalPositive => Self::GoalPositive,
            TargetLabelRole::GoalNegative => Self::GoalNegative,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusLabelRole {
    NeedsReview,
    Rejected,
}

impl From<StatusLabelRole> for LabelRole {
    fn from(role: StatusLabelRole) -> Self {
        match role {
            StatusLabelRole::NeedsReview => Self::NeedsReview,
            StatusLabelRole::Rejected => Self::Rejected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelSnapshot {
    pub label_revision: u64,
    pub target_labels: LabelTargetSnapshot,
    pub status_labels: Vec<StatusLabelSnapshot>,
    pub dedup_groups: Vec<DedupGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LabelTargetSnapshot {
    pub first_boss: Option<String>,
    pub goal_positive: Option<String>,
    pub goal_negative: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusLabelSnapshot {
    pub capture_id: String,
    pub status: StatusLabelRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DedupGroup {
    pub group_id: String,
    pub expected_relation: DedupRelation,
    pub capture_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed_features: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed_offset_ranges: Vec<ChangedOffsetRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DedupStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DedupRelation {
    SameCanonicalState,
    DistinctStableState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DedupStatus {
    Candidate,
    Confirmed,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedOffsetRange {
    pub start: u64,
    pub len: u64,
}

fn validate_dedup_group(inner: &LabelInner, group: &DedupGroup) -> Result<(), LabelStoreError> {
    let mut conflicts = Vec::new();
    if !is_contract_id(&group.group_id) {
        conflicts.push(LabelConflict::bad_request(
            "Dedup group id is invalid.",
            false,
        ));
    }
    let mut captures = BTreeSet::new();
    for capture_id in &group.capture_ids {
        if !is_contract_id(capture_id) || !captures.insert(capture_id) {
            conflicts.push(LabelConflict::bad_request(
                "Dedup group capture ids are invalid.",
                false,
            ));
        }
        if inner.statuses.get(capture_id) == Some(&StatusLabelRole::Rejected) {
            conflicts.push(LabelConflict::label(
                "Rejected captures cannot be in dedup groups.",
                false,
            ));
        }
    }
    if group.capture_ids.len() < 2 {
        conflicts.push(LabelConflict::label(
            "Dedup groups require at least two captures.",
            false,
        ));
    }
    if group.changed_features.is_empty() && group.changed_offset_ranges.is_empty() {
        conflicts.push(LabelConflict::label(
            "Dedup groups require changed features or ranges.",
            false,
        ));
    }
    let mut features = BTreeSet::new();
    for feature in &group.changed_features {
        if !valid_public_feature_name(feature) || !features.insert(feature) {
            conflicts.push(LabelConflict::bad_request(
                "Dedup changed feature is invalid.",
                false,
            ));
        }
    }
    for range in &group.changed_offset_ranges {
        if range.start > JSON_SAFE_U64_MAX || range.len > JSON_SAFE_U64_MAX {
            conflicts.push(LabelConflict::bad_request(
                "Dedup changed offset range is invalid.",
                false,
            ));
        }
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(LabelStoreError::Conflict(conflicts))
    }
}

fn valid_note(note: &str) -> bool {
    note.len() <= 512 && !note.chars().any(char::is_control)
}

fn valid_public_feature_name(feature: &str) -> bool {
    !feature.is_empty()
        && feature.len() <= 128
        && feature.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ' ' | '_' | '-' | '.')
        })
}

fn is_contract_id(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.len() <= 128
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
}
