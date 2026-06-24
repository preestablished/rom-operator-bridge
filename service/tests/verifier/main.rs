use rom_operator_bridge_service::{
    config::ServiceConfig,
    labels::{
        ChangedOffsetRange, DedupGroup, DedupRelation, DedupStatus, LabelSnapshot,
        LabelTargetSnapshot, StatusLabelRole, StatusLabelSnapshot,
    },
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
    verifier::{PrivateVerifierPath, VerifierTransformError, write_phase4_verifier_inputs},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";

#[test]
fn score_plan_inputs_cover_required_target_labels() {
    let (_workspace, config, private_root) = private_config();
    let snapshot = verifier_snapshot();

    let inputs = write_phase4_verifier_inputs(config.private_config(), &snapshot)
        .expect("verifier inputs write");

    assert_eq!(inputs.score_plan.first_boss, "capture-first");
    assert_eq!(inputs.score_plan.goal_positive, "capture-positive");
    assert_eq!(inputs.score_plan.goal_negative, "capture-negative");
    assert_eq!(
        inputs.score_plan.arguments,
        [
            "--captures",
            "captures/index.jsonl",
            "--out",
            "validation/score-plan.json",
            "--first-boss",
            "capture-first",
            "--goal-positive",
            "capture-positive",
            "--goal-negative",
            "capture-negative",
        ]
    );

    let score_plan_input = read_json(private_root.join("validation/phase4-score-plan-input.json"));
    assert_eq!(score_plan_input["schema_version"], 1);
    assert_eq!(score_plan_input["kind"], "phase4-score-plan-input");
    assert_eq!(score_plan_input["command_class"], "phase4-score-plan");
    assert_eq!(score_plan_input["label_revision"], 7);
    assert_eq!(score_plan_input["first_boss"], "capture-first");
    assert_eq!(score_plan_input["goal_positive"], "capture-positive");
    assert_eq!(score_plan_input["goal_negative"], "capture-negative");
    assert_eq!(score_plan_input["captures"], "captures/index.jsonl");
    assert_eq!(score_plan_input["out"], "validation/score-plan.json");
    assert_eq!(
        score_plan_input["report"],
        "validation/phase4-score-plan.json"
    );
    assert_eq!(score_plan_input["dedup_groups"], "dedup-groups.jsonl");
    assert_no_private_root(&score_plan_input, &private_root);
}

#[test]
fn dedup_artifact_generation_writes_private_jsonl() {
    let (_workspace, config, private_root) = private_config();
    let snapshot = verifier_snapshot();

    write_phase4_verifier_inputs(config.private_config(), &snapshot)
        .expect("verifier inputs write");

    let lines =
        fs::read_to_string(private_root.join("dedup-groups.jsonl")).expect("dedup artifact reads");
    let rows: Vec<Value> = lines
        .lines()
        .map(|line| serde_json::from_str(line).expect("dedup row parses"))
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["group_id"], "dedup-same");
    assert_eq!(rows[0]["expected_relation"], "same_canonical_state");
    assert_eq!(
        rows[0]["capture_ids"],
        json!(["capture-first", "capture-positive"])
    );
    assert_eq!(rows[0]["changed_features"], json!(["volatile_rng"]));
    assert_eq!(rows[1]["group_id"], "dedup-distinct");
    assert_eq!(rows[1]["expected_relation"], "distinct_stable_state");
    assert_eq!(rows[1]["changed_offset_ranges"][0]["start"], 16);
    assert_eq!(rows[1]["changed_offset_ranges"][0]["len"], 4);

    for row in rows {
        assert_no_private_root(&row, &private_root);
    }
}

#[test]
fn missing_required_labels_are_reported_without_writing_outputs() {
    let (_workspace, config, private_root) = private_config();
    let snapshot = LabelSnapshot {
        label_revision: 1,
        target_labels: LabelTargetSnapshot {
            first_boss: Some("capture-first".to_string()),
            goal_positive: None,
            goal_negative: None,
        },
        status_labels: Vec::new(),
        dedup_groups: Vec::new(),
    };

    let error = write_phase4_verifier_inputs(config.private_config(), &snapshot)
        .expect_err("required labels fail");
    let VerifierTransformError::MissingRequiredLabels(missing) = error else {
        panic!("expected missing required labels error");
    };
    assert_eq!(missing, vec!["goal_positive", "goal_negative"]);
    assert!(
        !private_root
            .join("validation/phase4-score-plan-input.json")
            .exists()
    );
    assert!(!private_root.join("dedup-groups.jsonl").exists());
}

#[test]
fn conflicting_labels_are_reported_before_artifact_generation() {
    let (_workspace, config, private_root) = private_config();
    let snapshot = LabelSnapshot {
        label_revision: 1,
        target_labels: LabelTargetSnapshot {
            first_boss: Some("capture-shared".to_string()),
            goal_positive: Some("capture-shared".to_string()),
            goal_negative: Some("capture-negative".to_string()),
        },
        status_labels: vec![StatusLabelSnapshot {
            capture_id: "capture-negative".to_string(),
            status: StatusLabelRole::Rejected,
        }],
        dedup_groups: Vec::new(),
    };

    let error = write_phase4_verifier_inputs(config.private_config(), &snapshot)
        .expect_err("conflicting labels fail");
    let VerifierTransformError::ConflictingLabels(conflicts) = error else {
        panic!("expected conflicting labels error");
    };
    assert_eq!(conflicts.len(), 2);
    assert!(
        conflicts
            .iter()
            .any(|conflict| conflict.code == "target_role_conflict"
                && conflict.capture_id == "capture-shared")
    );
    assert!(
        conflicts
            .iter()
            .any(|conflict| conflict.code == "target_rejected_conflict"
                && conflict.capture_id == "capture-negative")
    );
    assert!(
        !private_root
            .join("validation/phase4-score-plan-input.json")
            .exists()
    );
    assert!(!private_root.join("dedup-groups.jsonl").exists());
}

#[test]
fn private_config_and_report_paths_stay_server_side() {
    let (_workspace, config, private_root) = private_config();
    let snapshot = verifier_snapshot();

    let inputs = write_phase4_verifier_inputs(config.private_config(), &snapshot)
        .expect("verifier inputs write");

    assert_eq!(
        inputs.private_artifacts.score_plan_input.relative_path(),
        Path::new("validation/phase4-score-plan-input.json")
    );
    assert_eq!(
        inputs.private_artifacts.score_plan_report.relative_path(),
        Path::new("validation/phase4-score-plan.json")
    );
    assert_eq!(
        inputs.private_artifacts.dedup_groups.relative_path(),
        Path::new("dedup-groups.jsonl")
    );
    assert_private_path_display_is_redacted(&inputs.private_artifacts.score_plan_input);
    assert_private_path_display_is_redacted(&inputs.private_artifacts.score_plan_report);
    assert!(
        !format!("{:?}", inputs.private_artifacts).contains(&private_root.display().to_string())
    );
    assert!(!format!("{:?}", inputs.private_artifacts).contains("validation/score-plan.json"));
}

fn verifier_snapshot() -> LabelSnapshot {
    LabelSnapshot {
        label_revision: 7,
        target_labels: LabelTargetSnapshot {
            first_boss: Some("capture-first".to_string()),
            goal_positive: Some("capture-positive".to_string()),
            goal_negative: Some("capture-negative".to_string()),
        },
        status_labels: Vec::new(),
        dedup_groups: vec![
            DedupGroup {
                group_id: "dedup-same".to_string(),
                expected_relation: DedupRelation::SameCanonicalState,
                capture_ids: vec!["capture-first".to_string(), "capture-positive".to_string()],
                changed_features: vec!["volatile_rng".to_string()],
                changed_offset_ranges: Vec::new(),
                status: Some(DedupStatus::Confirmed),
            },
            DedupGroup {
                group_id: "dedup-distinct".to_string(),
                expected_relation: DedupRelation::DistinctStableState,
                capture_ids: vec![
                    "capture-positive".to_string(),
                    "capture-negative".to_string(),
                ],
                changed_features: Vec::new(),
                changed_offset_ranges: vec![ChangedOffsetRange { start: 16, len: 4 }],
                status: Some(DedupStatus::Candidate),
            },
        ],
    }
}

fn private_config() -> (tempfile::TempDir, ServiceConfig, PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config = ServiceConfig::from_pairs([
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (
            ENV_OPERATOR_CREDENTIAL.to_string(),
            GOOD_CREDENTIAL.to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
    ])
    .expect("private config loads");
    (workspace, config, private_root)
}

fn read_json(path: impl AsRef<Path>) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("json file reads")).expect("json parses")
}

fn assert_no_private_root(value: &Value, private_root: &Path) {
    assert!(
        !value
            .to_string()
            .contains(&private_root.display().to_string())
    );
}

fn assert_private_path_display_is_redacted(path: &PrivateVerifierPath) {
    assert_eq!(format!("{path}"), "[private verifier path]");
    assert_eq!(format!("{path:?}"), "[private verifier path]");
}
