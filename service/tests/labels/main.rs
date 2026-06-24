use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{COOKIE, ORIGIN, SET_COOKIE},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    artifacts::LabelDraftFile,
    auth::ALLOWED_ORIGIN,
    backend::{
        BackendCapabilities, BackendMode, BackendResult, BackendSession, BridgeBackend, CaptureJob,
        CaptureJobStatus, CaptureRequest, FramePreview, InputScheduleReceipt, InputScheduleRequest,
        RunBoundary, RunStatus, SessionId, SessionState, StartBackendSession, StopReason,
        StoppedSession,
    },
    config::ServiceConfig,
    framebuffer::{SYNTHETIC_FRAME_HEIGHT, SYNTHETIC_FRAME_WIDTH, synthetic_frame_png},
    labels::{
        ChangedOffsetRange, DedupGroup, DedupRelation, DedupStatus, LabelState, LabelStoreError,
    },
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const SESSION_ID: &str = "synthetic-session-labels";
const RUN_ID: &str = "synthetic-run-labels";

#[tokio::test]
async fn target_labels_are_unique_idempotent_and_private_notes_stay_private() {
    let (_workspace, app, private_root) = labels_app(LabelBackend::new([3, 4]));
    let cookie = login_cookie(app.clone()).await;
    let first = complete_capture(
        app.clone(),
        &cookie,
        3,
        "00000000-0000-4000-8000-000000001001",
    )
    .await;
    let second = complete_capture(
        app.clone(),
        &cookie,
        4,
        "00000000-0000-4000-8000-000000001002",
    )
    .await;
    let first_id = first["capture_id"].as_str().expect("capture id");
    let second_id = second["capture_id"].as_str().expect("capture id");

    let note = "render-safe <b>& \"quoted\" note";
    let first_label = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000001101",
        json!([{
            "op": "upsert",
            "capture_id": first_id,
            "role": "first_boss",
            "confidence": "confirmed",
            "note": note
        }]),
    )
    .await;
    assert_eq!(first_label["applied"], true);
    assert_eq!(first_label["label_revision"], 1);
    assert!(!first_label.to_string().contains(note));

    let idempotent = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000001101",
        json!([{
            "op": "upsert",
            "capture_id": first_id,
            "role": "first_boss",
            "confidence": "confirmed",
            "note": note
        }]),
    )
    .await;
    assert_eq!(idempotent["label_revision"], 1);

    let replacement = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000001102",
        json!([{
            "op": "upsert",
            "capture_id": second_id,
            "role": "first_boss",
            "confidence": "candidate"
        }]),
    )
    .await;
    assert_eq!(replacement["applied"], true);
    assert_eq!(replacement["label_revision"], 2);

    let snapshot = request_json(app.clone(), runtime_get("/api/labels", &cookie)).await;
    assert_matches_runtime_schema(&snapshot);
    assert_eq!(snapshot["label_revision"], 2);
    assert_eq!(snapshot["target_labels"]["first_boss"], second_id);
    assert_eq!(snapshot["target_labels"]["goal_positive"], Value::Null);
    assert_eq!(snapshot["target_labels"]["goal_negative"], Value::Null);

    let recent = request_json(app.clone(), runtime_get("/api/capture/recent", &cookie)).await;
    assert_eq!(recent["captures"][0]["capture_id"], second_id);
    assert_eq!(recent["captures"][0]["labels"], json!(["first_boss"]));
    let detail = request_json(
        app.clone(),
        runtime_get(&format!("/api/capture/{second_id}"), &cookie),
    )
    .await;
    assert_eq!(detail["labels"], json!(["first_boss"]));

    let first_draft = read_draft(&private_root, first_id);
    assert!(first_draft.labels.is_empty());
    assert_eq!(first_draft.private_note.as_deref(), Some(note));
    let second_draft = read_draft(&private_root, second_id);
    assert_eq!(second_draft.labels[0].label, "first_boss");
}

#[tokio::test]
async fn rejected_and_needs_review_conflicts_are_reported_without_revision_bump() {
    let (_workspace, app, _private_root) = labels_app(LabelBackend::new([8, 9]));
    let cookie = login_cookie(app.clone()).await;
    let first = complete_capture(
        app.clone(),
        &cookie,
        8,
        "00000000-0000-4000-8000-000000002001",
    )
    .await;
    let second = complete_capture(
        app.clone(),
        &cookie,
        9,
        "00000000-0000-4000-8000-000000002002",
    )
    .await;
    let first_id = first["capture_id"].as_str().expect("capture id");
    let second_id = second["capture_id"].as_str().expect("capture id");

    let target = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002101",
        json!([{ "op": "upsert", "capture_id": first_id, "role": "goal_positive" }]),
    )
    .await;
    assert_eq!(target["label_revision"], 1);

    let rejected_target = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002102",
        json!([{ "op": "upsert", "capture_id": first_id, "role": "rejected" }]),
    )
    .await;
    assert_eq!(rejected_target["applied"], false);
    assert_eq!(rejected_target["label_revision"], 1);
    assert_eq!(rejected_target["conflicts"][0]["code"], "label_conflict");

    let review_target = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002108",
        json!([{ "op": "upsert", "capture_id": first_id, "role": "needs_review" }]),
    )
    .await;
    assert_eq!(review_target["applied"], false);
    assert_eq!(review_target["conflicts"][0]["code"], "label_conflict");

    let needs_review = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002103",
        json!([{ "op": "upsert", "capture_id": second_id, "role": "needs_review" }]),
    )
    .await;
    assert_eq!(needs_review["label_revision"], 2);

    let rejected_review = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002104",
        json!([{ "op": "upsert", "capture_id": second_id, "role": "rejected" }]),
    )
    .await;
    assert_eq!(rejected_review["applied"], false);
    assert_eq!(rejected_review["label_revision"], 2);

    let delete_review = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002105",
        json!([{ "op": "delete", "capture_id": second_id, "role": "needs_review" }]),
    )
    .await;
    assert_eq!(delete_review["label_revision"], 3);

    let rejected = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002106",
        json!([{ "op": "upsert", "capture_id": second_id, "role": "rejected" }]),
    )
    .await;
    assert_eq!(rejected["label_revision"], 4);

    let reviewed_target = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000002107",
        json!([{ "op": "upsert", "capture_id": second_id, "role": "goal_negative" }]),
    )
    .await;
    assert_eq!(reviewed_target["applied"], false);
    assert_eq!(reviewed_target["conflicts"][0]["code"], "label_conflict");

    let snapshot = request_json(app, runtime_get("/api/labels", &cookie)).await;
    assert_eq!(snapshot["status_labels"][0]["capture_id"], second_id);
    assert_eq!(snapshot["status_labels"][0]["status"], "rejected");
}

#[tokio::test]
async fn notes_schema_and_active_capture_validation_reject_bad_updates() {
    let (_workspace, app, _private_root) = labels_app(LabelBackend::new([12]));
    let cookie = login_cookie(app.clone()).await;
    let capture = complete_capture(
        app.clone(),
        &cookie,
        12,
        "00000000-0000-4000-8000-000000003001",
    )
    .await;
    let capture_id = capture["capture_id"].as_str().expect("capture id");

    let control_note = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000003101",
        json!([{ "op": "upsert", "capture_id": capture_id, "role": "needs_review", "note": "bad\nnote" }]),
    )
    .await;
    assert_eq!(control_note["applied"], false);
    assert_eq!(control_note["conflicts"][0]["code"], "bad_request");

    let long_note = "x".repeat(513);
    let long = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000003102",
        json!([{ "op": "upsert", "capture_id": capture_id, "role": "needs_review", "note": long_note }]),
    )
    .await;
    assert_eq!(long["applied"], false);
    assert_eq!(long["conflicts"][0]["code"], "bad_request");

    let outside_active_run = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000003103",
        json!([{ "op": "upsert", "capture_id": "missing-capture", "role": "needs_review" }]),
    )
    .await;
    assert_eq!(outside_active_run["applied"], false);
    assert_eq!(outside_active_run["conflicts"][0]["code"], "label_conflict");

    let empty_updates = app
        .clone()
        .oneshot(labels_request(
            &cookie,
            "00000000-0000-4000-8000-000000003104",
            json!([]),
        ))
        .await
        .expect("empty label request runs");
    assert_eq!(empty_updates.status(), StatusCode::BAD_REQUEST);

    let unknown_field = app
        .oneshot(runtime_json_request(
            Method::POST,
            "/api/labels",
            &cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID,
                "idempotency_key": "00000000-0000-4000-8000-000000003105",
                "updates": [{
                    "op": "upsert",
                    "capture_id": capture_id,
                    "role": "needs_review",
                    "private_path": "/tmp/secret"
                }]
            }),
        ))
        .await
        .expect("unknown field request runs");
    assert!(matches!(
        unknown_field.status(),
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ));
}

#[tokio::test]
async fn session_boundaries_clear_labels_and_old_captures() {
    let (_workspace, app, _private_root) = labels_app(LabelBackend::new([5]));
    let cookie = login_cookie(app.clone()).await;
    let capture = complete_capture(
        app.clone(),
        &cookie,
        5,
        "00000000-0000-4000-8000-000000004001",
    )
    .await;
    let capture_id = capture["capture_id"].as_str().expect("capture id");
    let labeled = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000004101",
        json!([{ "op": "upsert", "capture_id": capture_id, "role": "needs_review" }]),
    )
    .await;
    assert_eq!(labeled["label_revision"], 1);

    let stop = app
        .clone()
        .oneshot(runtime_json_request(
            Method::POST,
            "/api/session/stop",
            &cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID,
                "reason": "operator_stop"
            }),
        ))
        .await
        .expect("stop request runs");
    assert_eq!(stop.status(), StatusCode::OK);

    let next_cookie = login_cookie(app.clone()).await;
    let snapshot = request_json(app.clone(), runtime_get("/api/labels", &next_cookie)).await;
    assert_eq!(snapshot["label_revision"], 0);
    assert!(
        snapshot["status_labels"]
            .as_array()
            .expect("status labels")
            .is_empty()
    );

    let stale_label = apply_labels(
        app,
        &next_cookie,
        "00000000-0000-4000-8000-000000004102",
        json!([{ "op": "upsert", "capture_id": capture_id, "role": "needs_review" }]),
    )
    .await;
    assert_eq!(stale_label["applied"], false);
    assert_eq!(stale_label["conflicts"][0]["code"], "label_conflict");
}

#[tokio::test]
async fn failed_multi_draft_write_rolls_back_published_drafts() {
    let (_workspace, app, private_root) = labels_app(LabelBackend::new([15, 16]));
    let cookie = login_cookie(app.clone()).await;
    let first = complete_capture(
        app.clone(),
        &cookie,
        15,
        "00000000-0000-4000-8000-000000005001",
    )
    .await;
    let second = complete_capture(
        app.clone(),
        &cookie,
        16,
        "00000000-0000-4000-8000-000000005002",
    )
    .await;
    let first_id = first["capture_id"].as_str().expect("capture id");
    let second_id = second["capture_id"].as_str().expect("capture id");

    let first_label = apply_labels(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000005101",
        json!([{ "op": "upsert", "capture_id": first_id, "role": "first_boss" }]),
    )
    .await;
    assert_eq!(first_label["label_revision"], 1);

    fs::write(
        private_root.join("captures").join(second_id),
        b"not a directory",
    )
    .expect("blocking capture path written");

    let failed = app
        .oneshot(labels_request(
            &cookie,
            "00000000-0000-4000-8000-000000005102",
            json!([{ "op": "upsert", "capture_id": second_id, "role": "first_boss" }]),
        ))
        .await
        .expect("failed label replacement request runs");
    assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);

    let first_draft = read_draft(&private_root, first_id);
    assert_eq!(first_draft.labels[0].label, "first_boss");
}

#[test]
fn dedup_groups_update_delete_and_validate_shape() {
    let labels = LabelState::new();
    let revision = labels
        .upsert_dedup_group(DedupGroup {
            group_id: "dedup-001".to_string(),
            expected_relation: DedupRelation::SameCanonicalState,
            capture_ids: vec!["capture-a".to_string(), "capture-b".to_string()],
            changed_features: vec!["volatile_rng".to_string()],
            changed_offset_ranges: Vec::new(),
            status: Some(DedupStatus::Candidate),
        })
        .expect("dedup group upserts");
    assert_eq!(revision, 1);
    let snapshot = labels.snapshot();
    assert_eq!(snapshot.dedup_groups[0].group_id, "dedup-001");
    assert_eq!(
        snapshot.dedup_groups[0].expected_relation,
        DedupRelation::SameCanonicalState
    );

    assert!(matches!(
        labels.upsert_dedup_group(DedupGroup {
            group_id: "bad/group".to_string(),
            expected_relation: DedupRelation::DistinctStableState,
            capture_ids: vec!["capture-a".to_string()],
            changed_features: Vec::new(),
            changed_offset_ranges: vec![ChangedOffsetRange { start: 0, len: 1 }],
            status: Some(DedupStatus::Conflict),
        }),
        Err(LabelStoreError::Conflict(_))
    ));
    assert!(matches!(
        labels.upsert_dedup_group(DedupGroup {
            group_id: "dedup-duplicate-feature".to_string(),
            expected_relation: DedupRelation::SameCanonicalState,
            capture_ids: vec!["capture-a".to_string(), "capture-b".to_string()],
            changed_features: vec!["volatile_rng".to_string(), "volatile_rng".to_string()],
            changed_offset_ranges: Vec::new(),
            status: Some(DedupStatus::Confirmed),
        }),
        Err(LabelStoreError::Conflict(_))
    ));

    let rejected = labels
        .apply(
            rom_operator_bridge_service::labels::LabelApplyRequest {
                session_id: SESSION_ID.to_string(),
                idempotency_key: "00000000-0000-4000-8000-000000006001".to_string(),
                updates: vec![rom_operator_bridge_service::labels::LabelUpdate {
                    op: rom_operator_bridge_service::labels::LabelOp::Upsert,
                    capture_id: "capture-rejected".to_string(),
                    role: rom_operator_bridge_service::labels::LabelRole::Rejected,
                    confidence: None,
                    note: None,
                }],
            },
            |_| true,
            None,
        )
        .expect("rejected label applies");
    assert_eq!(rejected.label_revision, 2);
    assert!(matches!(
        labels.upsert_dedup_group(DedupGroup {
            group_id: "dedup-rejected".to_string(),
            expected_relation: DedupRelation::SameCanonicalState,
            capture_ids: vec!["capture-rejected".to_string(), "capture-b".to_string()],
            changed_features: vec!["volatile_rng".to_string()],
            changed_offset_ranges: Vec::new(),
            status: Some(DedupStatus::Conflict),
        }),
        Err(LabelStoreError::Conflict(_))
    ));

    let revision = labels
        .delete_dedup_group("dedup-001")
        .expect("dedup group deletes");
    assert_eq!(revision, 3);
    assert!(labels.snapshot().dedup_groups.is_empty());
}

async fn complete_capture(
    app: axum::Router,
    cookie: &str,
    frame: u64,
    idempotency_key: &str,
) -> Value {
    let trigger = request_json(
        app.clone(),
        runtime_json_request(
            Method::POST,
            "/api/capture/trigger",
            cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID,
                "idempotency_key": idempotency_key,
                "observed_preview_frame": frame,
                "reason": "operator_mark"
            }),
        ),
    )
    .await;
    let job_uri = format!(
        "/api/capture/jobs/{}",
        trigger["job_id"].as_str().expect("job id")
    );
    let capturing = request_json(app.clone(), runtime_get(&job_uri, cookie)).await;
    assert_eq!(capturing["status"], "capturing");
    let completed = request_json(app, runtime_get(&job_uri, cookie)).await;
    assert_eq!(completed["status"], "completed");
    completed
}

async fn apply_labels(
    app: axum::Router,
    cookie: &str,
    idempotency_key: &str,
    updates: Value,
) -> Value {
    request_json(app, labels_request(cookie, idempotency_key, updates)).await
}

fn labels_request(cookie: &str, idempotency_key: &str, updates: Value) -> Request<Body> {
    runtime_json_request(
        Method::POST,
        "/api/labels",
        cookie,
        json!({
            "schema_version": 1,
            "session_id": SESSION_ID,
            "idempotency_key": idempotency_key,
            "updates": updates
        }),
    )
}

async fn login_cookie(app: axum::Router) -> String {
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/session/start")
                .header(ORIGIN, ALLOWED_ORIGIN)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "schema_version": 1,
                        "operator_credential": GOOD_CREDENTIAL,
                        "backend_mode": "synthetic",
                        "requested_capabilities": ["capture", "labels"]
                    })
                    .to_string(),
                ))
                .expect("login request builds"),
        )
        .await
        .expect("login request runs");
    assert_eq!(response.status(), StatusCode::OK);
    response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| cookie.split(';').next())
        .expect("session cookie pair exists")
        .to_string()
}

async fn request_json(app: axum::Router, request: Request<Body>) -> Value {
    let response = app.oneshot(request).await.expect("request runs");
    assert_eq!(response.status(), StatusCode::OK);
    let json = json_body(response).await;
    assert_matches_runtime_schema(&json);
    json
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 8192)
        .await
        .expect("body reads");
    serde_json::from_slice(&body).expect("json parses")
}

fn runtime_get(uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
        .header(COOKIE, cookie)
        .body(Body::empty())
        .expect("runtime request builds")
}

fn runtime_json_request(method: Method, uri: &str, cookie: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
        .header(COOKIE, cookie)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("runtime json request builds")
}

fn read_draft(private_root: &Path, capture_id: &str) -> LabelDraftFile {
    let path = private_root
        .join("captures")
        .join(capture_id)
        .join("label-draft.json");
    serde_json::from_str(&fs::read_to_string(path).expect("label draft exists"))
        .expect("label draft parses")
}

fn assert_matches_runtime_schema(json: &Value) {
    let schema: Value =
        serde_json::from_str(include_str!("../../../contracts/runtime-api.schema.json"))
            .expect("runtime schema parses");
    let validator = jsonschema::validator_for(&schema).expect("runtime schema compiles");
    validator.validate(json).unwrap_or_else(|error| {
        panic!("runtime schema validation failed: {error}");
    });
}

fn labels_app(backend: LabelBackend) -> (tempfile::TempDir, axum::Router, PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let app = router(AppState::for_tests_with_backend(
        config(&private_root),
        rom_operator_bridge_service::auth::AuthState::new(),
        std::sync::Arc::new(backend),
    ));
    (workspace, app, private_root)
}

fn config(private_root: &Path) -> ServiceConfig {
    ServiceConfig::from_pairs([
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
    .expect("private config loads")
}

#[derive(Debug)]
struct LabelBackend {
    preview_frames: Mutex<VecDeque<u64>>,
    state: Mutex<SessionState>,
}

impl LabelBackend {
    fn new<const N: usize>(frames: [u64; N]) -> Self {
        Self {
            preview_frames: Mutex::new(VecDeque::from(frames)),
            state: Mutex::new(SessionState::Running),
        }
    }

    fn preview_frame(&self) -> u64 {
        let mut frames = self.preview_frames.lock().expect("preview mutex poisoned");
        if frames.len() > 1 {
            frames.pop_front().expect("preview frame exists")
        } else {
            *frames.front().expect("preview frame exists")
        }
    }

    fn current_frame(&self) -> u64 {
        *self
            .preview_frames
            .lock()
            .expect("preview mutex poisoned")
            .front()
            .expect("preview frame exists")
    }
}

impl BridgeBackend for LabelBackend {
    fn mode(&self) -> BackendMode {
        BackendMode::Synthetic
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::synthetic_mvp()
    }

    fn start_session(&self, _request: StartBackendSession) -> BackendResult<BackendSession> {
        Ok(BackendSession {
            session_id: SESSION_ID.to_string(),
            run_id: RUN_ID.to_string(),
            state: *self.state.lock().expect("state mutex poisoned"),
            current_frame: self.current_frame(),
            capabilities: self.capabilities(),
        })
    }

    fn stop_session(
        &self,
        session_id: SessionId,
        _reason: StopReason,
    ) -> BackendResult<StoppedSession> {
        Ok(StoppedSession {
            session_id,
            state: SessionState::Stopped,
            final_frame: self.current_frame(),
        })
    }

    fn status(&self, _session_id: SessionId) -> BackendResult<RunStatus> {
        Ok(RunStatus {
            session_id: SESSION_ID.to_string(),
            run_id: RUN_ID.to_string(),
            state: *self.state.lock().expect("state mutex poisoned"),
            backend_mode: self.mode(),
            current_frame: self.current_frame(),
            capabilities: self.capabilities(),
            last_applied_input_frame: 0,
            last_preview_frame: self.current_frame(),
            active_capture_job_id: None,
        })
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Paused,
            current_frame: self.current_frame(),
        })
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Running,
            current_frame: self.current_frame(),
        })
    }

    fn inject_input(&self, request: InputScheduleRequest) -> BackendResult<InputScheduleReceipt> {
        Ok(InputScheduleReceipt {
            session_id: request.session_id,
            assigned_frame: request.target_frame,
            pad_word: request.pad_word,
        })
    }

    fn framebuffer(&self, _session_id: SessionId) -> BackendResult<FramePreview> {
        let frame = self.preview_frame();
        Ok(FramePreview {
            session_id: SESSION_ID.to_string(),
            frame,
            width: SYNTHETIC_FRAME_WIDTH,
            height: SYNTHETIC_FRAME_HEIGHT,
            png_bytes: synthetic_frame_png(frame),
        })
    }

    fn trigger_capture(&self, _request: CaptureRequest) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id: "backend-label-capture-job".to_string(),
            status: CaptureJobStatus::Running,
            capture_id: None,
        })
    }

    fn capture_job(&self, job_id: String) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id,
            status: CaptureJobStatus::Running,
            capture_id: None,
        })
    }
}
