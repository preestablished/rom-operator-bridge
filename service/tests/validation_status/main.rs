use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{CONTENT_TYPE, COOKIE, ORIGIN, SET_COOKIE},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    artifacts::ARTIFACT_SCHEMA_VERSION,
    auth::ALLOWED_ORIGIN,
    config::ServiceConfig,
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
    sanitization::PublicSanitizer,
    validation_status::{ValidationRunStatus, ValidationRunUpdate, ValidationStatusState},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const PRIVATE_LITERAL: &str = "SECRET_FEATURE_VALUE";
const UNSAFE_SUMMARY: &str = "stdout: failed at /home/operator/private/validation/report.json with feature_bytes and validation report excerpt SECRET_FEATURE_VALUE";

#[test]
fn records_private_validation_run_and_exposes_only_sanitized_status() {
    let (_workspace, config, private_root) = private_config();
    let sanitizer = config
        .private_config()
        .public_sanitizer()
        .with_forbidden_literal(PRIVATE_LITERAL);
    let state = ValidationStatusState::new();

    let public = state
        .record_run(
            config.private_config(),
            &sanitizer,
            ValidationRunUpdate::new(
                "validation-001",
                "2026-06-24T09:00:00Z",
                "phase4-score-plan",
                ValidationRunStatus::Failed,
                UNSAFE_SUMMARY,
            )
            .completed_at("2026-06-24T09:00:03Z")
            .issue_summaries([
                UNSAFE_SUMMARY,
                "Goal route failed sanitized aggregate check.",
            ]),
        )
        .expect("validation run records");

    assert_eq!(public.status, ValidationRunStatus::Failed);
    assert_eq!(public.command_class.as_deref(), Some("phase4_score_plan"));
    assert_eq!(public.summary, "Validation failed.");
    assert_eq!(
        public.issue_summaries,
        [
            "Validation issue redacted.",
            "Goal route failed sanitized aggregate check."
        ]
    );
    assert_public_safe(&sanitizer, &json!(public), &private_root);

    let rows = fs::read_to_string(private_root.join("validation/validation-runs.jsonl"))
        .expect("validation runs artifact reads");
    let row: Value = serde_json::from_str(rows.lines().next().expect("one validation row"))
        .expect("validation row parses");
    assert_eq!(row["schema_version"], ARTIFACT_SCHEMA_VERSION);
    assert_eq!(row["validation_id"], "validation-001");
    assert_eq!(row["command_class"], "phase4_score_plan");
    assert_eq!(row["status"], "failed");
    assert_eq!(row["sanitized_summary"], "Validation failed.");
    assert_public_safe(&sanitizer, &row, &private_root);
}

#[tokio::test]
async fn validation_status_route_returns_sanitized_public_view() {
    let (_workspace, state, private_root) = app_state();
    let app = router(state.clone());
    let cookie = login_cookie(app.clone()).await;

    state
        .record_validation_run(
            ValidationRunUpdate::new(
                "validation-002",
                "2026-06-24T09:10:00Z",
                "redaction-scan",
                ValidationRunStatus::Passed,
                "Validation passed.",
            )
            .completed_at("2026-06-24T09:10:02Z"),
        )
        .expect("validation run records through app state");

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/validation/status")
                .header(ORIGIN, ALLOWED_ORIGIN)
                .header(COOKIE, cookie)
                .body(Body::empty())
                .expect("validation status request builds"),
        )
        .await
        .expect("validation status request succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("validation body reads");
    let json: Value = serde_json::from_slice(&body).expect("validation response parses");

    assert_runtime_schema(&json);
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["status"], "passed");
    assert_eq!(json["command_class"], "redaction_scan");
    assert_eq!(json["summary"], "Validation passed.");
    assert_eq!(json["issue_summaries"], json!([]));
    assert_public_safe(
        &PublicSanitizer::new()
            .with_private_root(&private_root)
            .with_forbidden_literal(GOOD_CREDENTIAL)
            .with_forbidden_literal(SESSION_SECRET),
        &json,
        &private_root,
    );
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

fn app_state() -> (tempfile::TempDir, AppState, PathBuf) {
    let (workspace, config, private_root) = private_config();
    let state = AppState::synthetic_for_tests(config);
    (workspace, state, private_root)
}

async fn login_cookie(app: axum::Router) -> String {
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/session/start")
                .header(ORIGIN, ALLOWED_ORIGIN)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "schema_version": 1,
                        "operator_credential": GOOD_CREDENTIAL,
                        "backend_mode": "synthetic",
                        "requested_capabilities": ["input"]
                    })
                    .to_string(),
                ))
                .expect("login request builds"),
        )
        .await
        .expect("login request succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    response
        .headers()
        .get(SET_COOKIE)
        .expect("set-cookie exists")
        .to_str()
        .expect("set-cookie header is text")
        .to_string()
}

fn assert_public_safe(sanitizer: &PublicSanitizer, value: &Value, private_root: &PathBuf) {
    sanitizer
        .inspect_validation_summary(value)
        .expect("validation status is public-safe");
    let serialized = value.to_string();
    assert!(!serialized.contains(&private_root.display().to_string()));
    assert!(!serialized.contains(GOOD_CREDENTIAL));
    assert!(!serialized.contains(SESSION_SECRET));
    assert!(!serialized.contains(PRIVATE_LITERAL));
    assert!(!serialized.contains("stdout"));
    assert!(!serialized.contains("stderr"));
    assert!(!serialized.contains("feature_bytes"));
    assert!(!serialized.contains("validation report"));
    assert!(!serialized.contains("phase4-score-plan"));
    assert!(!serialized.contains("redaction-scan"));
}

fn assert_runtime_schema(json: &Value) {
    let schema: Value =
        serde_json::from_str(include_str!("../../../contracts/runtime-api.schema.json"))
            .expect("runtime schema parses");
    let validator = jsonschema::validator_for(&schema).expect("runtime schema compiles");
    validator.validate(json).unwrap_or_else(|error| {
        panic!("runtime schema validation failed: {error}");
    });
}
