use axum::{
    body::{Body, to_bytes},
    http::{
        HeaderName, HeaderValue, Method, Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE, COOKIE, ORIGIN, PRAGMA,
            SET_COOKIE, VARY,
        },
    },
};
use futures_util::StreamExt;
use rom_operator_bridge_service::{
    api::{AppState, router},
    artifacts::{LabelDraftFile, RecentCapturesFile},
    auth::ALLOWED_ORIGIN,
    backend::{
        BackendCapabilities, BackendMode, BackendResult, BackendSession, BridgeBackend, CaptureJob,
        CaptureJobStatus, CaptureRequest, FramePreview, InputScheduleReceipt, InputScheduleRequest,
        RunBoundary, RunStatus, SessionId, SessionState, StartBackendSession, StopReason,
        StoppedSession,
    },
    config::ServiceConfig,
    framebuffer::{SYNTHETIC_FRAME_HEIGHT, SYNTHETIC_FRAME_WIDTH, synthetic_frame_png},
    private_config::{ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET},
    sanitization::PublicSanitizer,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tokio::{
    net::TcpListener,
    time::{Duration, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const SESSION_ID: &str = "synthetic-session-capture";
const RUN_ID: &str = "synthetic-run-capture";
const JSON_SAFE_U64_MAX: u64 = 9_007_199_254_740_991;

type TestSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[tokio::test]
async fn trigger_is_idempotent_and_rejects_parallel_active_capture() {
    let (_workspace, app, _private_root) = capture_app(CaptureBackend::new([7]));
    let cookie = login_cookie(app.clone()).await;

    let first = trigger_capture(
        app.clone(),
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000001",
        7,
    )
    .await;
    assert_eq!(first["status"], "requested");
    assert_eq!(first["requested_frame"], 7);
    assert_eq!(first["scheduled_frame"], 8);

    let status = request_json(app.clone(), runtime_get("/api/run/status", &cookie)).await;
    assert_eq!(status["active_capture_job_id"], first["job_id"]);

    let same = trigger_capture(
        app.clone(),
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000001",
        7,
    )
    .await;
    assert_eq!(same["job_id"], first["job_id"]);
    assert_eq!(same["status"], "requested");

    let active_conflict = app
        .oneshot(runtime_json_request(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID,
                "idempotency_key": "00000000-0000-4000-8000-000000000002",
                "observed_preview_frame": 7,
                "reason": "operator_mark"
            }),
        ))
        .await
        .expect("conflicting capture trigger runs");
    assert_eq!(active_conflict.status(), StatusCode::CONFLICT);
    let error = json_body(active_conflict).await;
    assert_eq!(error["error"]["code"], "capture_in_progress");
}

#[tokio::test]
async fn trigger_publishes_capture_updated_event_for_api_owned_jobs() {
    let (_workspace, app, _private_root) = capture_app(CaptureBackend::new([7]));
    let cookie = login_cookie(app.clone()).await;
    let server = WsServer::start(app.clone()).await;
    let mut ws = server.connect(&cookie).await;

    let snapshot = read_events(&mut ws, 4).await;
    assert_eq!(
        event_types(&snapshot),
        [
            "session_updated",
            "run_updated",
            "label_updated",
            "validation_updated"
        ]
    );

    let trigger = trigger_capture(
        app,
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000008",
        7,
    )
    .await;
    let updates = read_events(&mut ws, 1).await;

    assert_eq!(updates[0]["type"], "capture_updated");
    assert_eq!(updates[0]["payload"]["job_id"], trigger["job_id"]);
    assert_eq!(updates[0]["payload"]["status"], "requested");
    assert_eq!(updates[0]["payload"]["capture_id"], Value::Null);
}

#[tokio::test]
async fn trigger_rejects_stale_cached_preview_and_future_or_overflow_frames() {
    let (_workspace, app, _private_root) = capture_app(CaptureBackend::new([7, 10]));
    let cookie = login_cookie(app.clone()).await;

    let metadata = request_json(app.clone(), runtime_get("/api/frame/current", &cookie)).await;
    assert_eq!(metadata["frame"], 7);

    let failed = trigger_capture(
        app.clone(),
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000005",
        7,
    )
    .await;
    assert_eq!(failed["status"], "failed");
    let failed_job = request_json(
        app.clone(),
        runtime_get(
            &format!(
                "/api/capture/jobs/{}",
                failed["job_id"].as_str().expect("failed job id")
            ),
            &cookie,
        ),
    )
    .await;
    assert_eq!(failed_job["error"]["code"], "frame_stale");
    assert_eq!(failed_job["error"]["retryable"], true);

    let future = app
        .clone()
        .oneshot(runtime_json_request(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID,
                "idempotency_key": "00000000-0000-4000-8000-000000000006",
                "observed_preview_frame": 99,
                "reason": "operator_mark"
            }),
        ))
        .await
        .expect("future capture trigger runs");
    assert_eq!(future.status(), StatusCode::BAD_REQUEST);

    let boundary = app
        .oneshot(runtime_json_request(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID,
                "idempotency_key": "00000000-0000-4000-8000-000000000007",
                "observed_preview_frame": JSON_SAFE_U64_MAX,
                "reason": "operator_mark"
            }),
        ))
        .await
        .expect("boundary capture trigger runs");
    assert_eq!(boundary.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stopping_session_clears_active_capture_for_reused_session_id() {
    let (_workspace, app, _private_root) = capture_app(CaptureBackend::new([7]));
    let cookie = login_cookie(app.clone()).await;

    let first = trigger_capture(
        app.clone(),
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000003",
        7,
    )
    .await;
    assert_eq!(first["status"], "requested");

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
    let next = trigger_capture(
        app,
        &next_cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000004",
        7,
    )
    .await;
    assert_eq!(next["status"], "requested");
}

#[tokio::test]
async fn capture_completes_to_recent_detail_and_preview_after_durable_private_index() {
    let (_workspace, app, private_root) = capture_app(CaptureBackend::new([3, 4]));
    let cookie =
        login_cookie_with_capabilities(app.clone(), &["capture", "preview", "privileged_features"])
            .await;

    let first = complete_capture(
        app.clone(),
        &cookie,
        3,
        "00000000-0000-4000-8000-000000000010",
    )
    .await;
    assert_eq!(first["status"], "completed");
    assert_eq!(first["labelable"], true);
    let first_capture_id = first["capture_id"]
        .as_str()
        .expect("capture id")
        .to_string();

    let second = complete_capture(
        app.clone(),
        &cookie,
        4,
        "00000000-0000-4000-8000-000000000011",
    )
    .await;
    assert_eq!(second["status"], "completed");
    let second_capture_id = second["capture_id"]
        .as_str()
        .expect("capture id")
        .to_string();

    let recent = request_json(
        app.clone(),
        runtime_get("/api/capture/recent?limit=1", &cookie),
    )
    .await;
    assert_matches_runtime_schema(&recent);
    assert_eq!(recent["captures"].as_array().expect("captures").len(), 1);
    assert_eq!(recent["captures"][0]["capture_id"], second_capture_id);
    assert_eq!(recent["captures"][0]["status"], "completed");
    assert_eq!(recent["captures"][0]["has_preview"], true);
    assert_eq!(recent["next_cursor"], "1");

    let next = request_json(
        app.clone(),
        runtime_get(
            recent["next_cursor"]
                .as_str()
                .map(|cursor| format!("/api/capture/recent?cursor={cursor}&limit=1"))
                .as_deref()
                .expect("next cursor exists"),
            &cookie,
        ),
    )
    .await;
    assert_eq!(next["captures"][0]["capture_id"], first_capture_id);
    assert_eq!(next["next_cursor"], Value::Null);

    let detail = request_json(
        app.clone(),
        runtime_get(&format!("/api/capture/{second_capture_id}"), &cookie),
    )
    .await;
    assert_matches_runtime_schema(&detail);
    assert_eq!(detail["capture_id"], second_capture_id);
    assert_eq!(detail["privileged_features_available"], true);
    assert_eq!(
        detail["sanitized_provenance"]["capture_source"],
        "synthetic"
    );
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&detail)
        .expect("capture detail is public-safe");

    let features_response = app
        .clone()
        .oneshot(runtime_get(
            &format!("/api/capture/{second_capture_id}/features"),
            &cookie,
        ))
        .await
        .expect("capture features request runs");
    assert_eq!(features_response.status(), StatusCode::OK);
    assert_no_store_headers(features_response.headers());
    let features = json_body(features_response).await;
    assert_matches_runtime_schema(&features);
    assert_eq!(features["capture_id"], second_capture_id);
    assert_eq!(features["available"], true);
    assert_eq!(features["features"][0]["name"], "screen.room_id");
    assert_eq!(features["features"][0]["value"], 2.0);
    assert_eq!(features["features"][1]["name"], "player.health");
    assert_eq!(features["features"][1]["value"], 0.5);
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&features)
        .expect("capture features response is route-scoped and public-safe");

    let unauthenticated_features = app
        .clone()
        .oneshot(runtime_get_without_cookie(&format!(
            "/api/capture/{second_capture_id}/features"
        )))
        .await
        .expect("unauthenticated capture features request runs");
    assert_eq!(unauthenticated_features.status(), StatusCode::UNAUTHORIZED);
    assert_private_no_store_headers(unauthenticated_features.headers());
    let unauthenticated_features = json_body(unauthenticated_features).await;
    assert_eq!(
        unauthenticated_features["error"]["code"],
        "session_inactive"
    );
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&unauthenticated_features)
        .expect("unauthenticated feature error is public-safe");

    let preview_url = detail["preview_image_url"]
        .as_str()
        .expect("preview image url");
    let preview = app
        .oneshot(runtime_get(preview_url, &cookie))
        .await
        .expect("capture preview request runs");
    assert_eq!(preview.status(), StatusCode::OK);
    assert_no_store_headers(preview.headers());
    assert_eq!(
        preview
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let png = to_bytes(preview.into_body(), 512 * 1024)
        .await
        .expect("preview body reads");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

    let recent_file = fs::read_to_string(private_root.join("captures/recent-captures.json"))
        .expect("recent captures file is durable");
    let persisted: RecentCapturesFile =
        serde_json::from_str(&recent_file).expect("recent captures file parses");
    assert_eq!(persisted.captures[0].capture_id, second_capture_id);
    assert_eq!(persisted.captures[1].capture_id, first_capture_id);
}

#[tokio::test]
async fn capture_features_route_requires_privileged_capability_grant() {
    let (_workspace, app, private_root) = capture_app(CaptureBackend::new([3]));
    let cookie = login_cookie(app.clone()).await;
    let capture = complete_capture(
        app.clone(),
        &cookie,
        3,
        "00000000-0000-4000-8000-000000000012",
    )
    .await;
    let capture_id = capture["capture_id"].as_str().expect("capture id");

    let response = app
        .oneshot(runtime_get(
            &format!("/api/capture/{capture_id}/features"),
            &cookie,
        ))
        .await
        .expect("capture features request runs");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_private_no_store_headers(response.headers());
    let body = json_body(response).await;
    assert_eq!(body["error"]["code"], "auth_rejected");
    assert_eq!(body["error"]["details"], json!({}));
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&body)
        .expect("non-privileged feature error is public-safe");
}

#[tokio::test]
async fn synthetic_capture_labels_round_trip_private_files_and_event_refreshes() {
    let (_workspace, app, private_root) = capture_app(CaptureBackend::new([2, 3, 4]));
    let cookie =
        login_cookie_with_capabilities(app.clone(), &["capture", "preview", "labels"]).await;
    let server = WsServer::start(app.clone()).await;
    let mut ws = server.connect(&cookie).await;

    let snapshot = read_events(&mut ws, 4).await;
    assert_eq!(
        event_types(&snapshot),
        [
            "session_updated",
            "run_updated",
            "label_updated",
            "validation_updated"
        ]
    );

    let stale = trigger_capture(
        app.clone(),
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000050",
        1,
    )
    .await;
    assert_eq!(stale["status"], "failed");
    let stale_events = read_events(&mut ws, 1).await;
    assert_eq!(stale_events[0]["type"], "capture_updated");
    assert_eq!(stale_events[0]["payload"]["status"], "failed");
    assert_eq!(stale_events[0]["payload"]["capture_id"], Value::Null);

    let first = complete_capture(
        app.clone(),
        &cookie,
        2,
        "00000000-0000-4000-8000-000000000051",
    )
    .await;
    assert_eq!(first["status"], "completed");
    let first_events = read_events(&mut ws, 3).await;
    assert_eq!(
        first_events
            .iter()
            .map(|event| event["payload"]["status"].as_str().expect("status"))
            .collect::<Vec<_>>(),
        ["requested", "capturing", "completed"]
    );
    let first_capture_id = first["capture_id"]
        .as_str()
        .expect("first capture id")
        .to_string();

    let second = complete_capture(
        app.clone(),
        &cookie,
        3,
        "00000000-0000-4000-8000-000000000052",
    )
    .await;
    assert_eq!(second["status"], "completed");
    let second_events = read_events(&mut ws, 3).await;
    assert_eq!(
        second_events
            .iter()
            .map(|event| event["payload"]["status"].as_str().expect("status"))
            .collect::<Vec<_>>(),
        ["requested", "capturing", "completed"]
    );
    let second_capture_id = second["capture_id"]
        .as_str()
        .expect("second capture id")
        .to_string();

    let persisted: RecentCapturesFile = serde_json::from_str(
        &fs::read_to_string(private_root.join("captures/recent-captures.json"))
            .expect("synthetic recent captures file is durable"),
    )
    .expect("synthetic recent captures file parses");
    assert_eq!(persisted.captures[0].capture_id, second_capture_id);
    assert_eq!(persisted.captures[1].capture_id, first_capture_id);
    assert_eq!(persisted.captures[0].status, "completed");

    let note = "synthetic operator note for private draft";
    let labels = apply_labels_with_dedup(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000000053",
        json!([{
            "op": "upsert",
            "capture_id": first_capture_id,
            "role": "goal_positive",
            "confidence": "confirmed",
            "note": note
        }]),
        json!([{
            "op": "upsert",
            "group_id": "dedup-synthetic-flow",
            "expected_relation": "same_canonical_state",
            "capture_ids": [first_capture_id, second_capture_id],
            "changed_features": ["public rng guard"],
            "status": "candidate"
        }]),
    )
    .await;
    assert_eq!(labels["applied"], true);
    assert_eq!(labels["label_revision"], 1);
    assert!(!labels.to_string().contains(note));
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(GOOD_CREDENTIAL)
        .with_forbidden_literal(SESSION_SECRET)
        .inspect_json(&labels)
        .expect("label response is public-safe");

    let label_event = read_events(&mut ws, 1).await;
    assert_eq!(label_event[0]["type"], "label_updated");
    assert_eq!(label_event[0]["payload"]["label_revision"], 1);
    assert_eq!(label_event[0]["payload"]["applied"], true);

    let snapshot = request_json(app.clone(), runtime_get("/api/labels", &cookie)).await;
    assert_eq!(snapshot["label_revision"], 1);
    assert_eq!(
        snapshot["dedup_groups"][0]["group_id"],
        "dedup-synthetic-flow"
    );
    assert_eq!(
        snapshot["dedup_groups"][0]["expected_relation"],
        "same_canonical_state"
    );
    assert_eq!(
        snapshot["dedup_groups"][0]["changed_features"],
        json!(["public rng guard"])
    );
    assert_eq!(snapshot["dedup_groups"][0]["status"], "candidate");
    assert_eq!(
        snapshot["dedup_groups"][0]["capture_ids"],
        json!([first_capture_id.as_str(), second_capture_id.as_str()])
    );

    let first_draft = read_label_draft(&private_root, &first_capture_id);
    assert_eq!(first_draft.private_note.as_deref(), Some(note));
    assert_eq!(first_draft.labels[0].label, "goal_positive");
    let second_draft = read_label_draft(&private_root, &second_capture_id);
    assert!(second_draft.labels.is_empty());

    let recent = request_json(app.clone(), runtime_get("/api/capture/recent", &cookie)).await;
    assert_eq!(recent["captures"][1]["capture_id"], first_capture_id);
    assert_eq!(recent["captures"][1]["labels"], json!(["goal_positive"]));
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(note)
        .inspect_json(&recent)
        .expect("capture recent stays public-safe after labeling");
    let detail = request_json(
        app.clone(),
        runtime_get(&format!("/api/capture/{first_capture_id}"), &cookie),
    )
    .await;
    assert_eq!(detail["labels"], json!(["goal_positive"]));
    assert_eq!(
        detail["sanitized_provenance"]["capture_source"],
        "synthetic"
    );
    PublicSanitizer::new()
        .with_private_root(&private_root)
        .with_forbidden_literal(note)
        .inspect_json(&detail)
        .expect("capture detail stays public-safe after labeling");

    let conflict = apply_labels_with_dedup(
        app.clone(),
        &cookie,
        "00000000-0000-4000-8000-000000000054",
        json!([{ "op": "upsert", "capture_id": first_capture_id, "role": "rejected" }]),
        json!([]),
    )
    .await;
    assert_eq!(conflict["applied"], false);
    assert_eq!(conflict["label_revision"], 1);
    assert_eq!(conflict["conflicts"][0]["code"], "label_conflict");

    let conflict_event = read_events(&mut ws, 1).await;
    assert_eq!(conflict_event[0]["type"], "label_updated");
    assert_eq!(conflict_event[0]["payload"]["label_revision"], 1);
    assert_eq!(conflict_event[0]["payload"]["applied"], false);
}

#[tokio::test]
async fn failed_stale_capture_is_retryable_with_a_new_idempotency_key() {
    let (_workspace, app, _private_root) = capture_app(CaptureBackend::new([10]));
    let cookie = login_cookie(app.clone()).await;

    let failed = trigger_capture(
        app.clone(),
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000020",
        9,
    )
    .await;
    assert_eq!(failed["status"], "failed");

    let failed_job = request_json(
        app.clone(),
        runtime_get(
            &format!(
                "/api/capture/jobs/{}",
                failed["job_id"].as_str().expect("failed job id route")
            ),
            &cookie,
        ),
    )
    .await;
    assert_eq!(failed_job["status"], "failed");
    assert_eq!(failed_job["error"]["code"], "frame_stale");
    assert_eq!(failed_job["error"]["retryable"], true);

    let retry = complete_capture(
        app.clone(),
        &cookie,
        10,
        "00000000-0000-4000-8000-000000000021",
    )
    .await;
    assert_eq!(retry["status"], "completed");
    assert_eq!(retry["requested_frame"], 10);
}

#[tokio::test]
async fn not_labelable_capture_is_indexed_newest_first() {
    let (_workspace, app, _private_root) = capture_app(CaptureBackend::new([0]));
    let cookie = login_cookie(app.clone()).await;

    let job = complete_capture(
        app.clone(),
        &cookie,
        0,
        "00000000-0000-4000-8000-000000000030",
    )
    .await;
    assert_eq!(job["status"], "not_labelable");
    assert_eq!(job["labelable"], false);
    assert_eq!(job["capture_id"].is_string(), true);

    let recent = request_json(app, runtime_get("/api/capture/recent", &cookie)).await;
    assert_eq!(recent["captures"][0]["status"], "not_labelable");
    assert_eq!(recent["captures"][0]["labelable"], false);
    assert_eq!(recent["captures"][0]["capture_id"], job["capture_id"]);
}

#[tokio::test]
async fn completed_status_is_not_returned_until_private_index_is_durable() {
    let (_workspace, app, private_root) = capture_app(CaptureBackend::new([12]));
    let cookie = login_cookie(app.clone()).await;

    let trigger = trigger_capture(
        app.clone(),
        &cookie,
        SESSION_ID,
        "00000000-0000-4000-8000-000000000040",
        12,
    )
    .await;
    let job_uri = format!(
        "/api/capture/jobs/{}",
        trigger["job_id"].as_str().expect("job id")
    );
    let capturing = request_json(app.clone(), runtime_get(&job_uri, &cookie)).await;
    assert_eq!(capturing["status"], "capturing");

    fs::remove_dir_all(private_root.join("captures")).expect("captures dir removed");
    fs::write(private_root.join("captures"), b"not a directory")
        .expect("blocking captures file written");

    let failed_completion = app
        .clone()
        .oneshot(runtime_get(&job_uri, &cookie))
        .await
        .expect("job completion poll runs");
    assert_eq!(failed_completion.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error = json_body(failed_completion).await;
    assert_eq!(error["error"]["code"], "backend_unavailable");

    fs::remove_file(private_root.join("captures")).expect("blocking captures file removed");

    let completed = request_json(app, runtime_get(&job_uri, &cookie)).await;
    assert_eq!(completed["status"], "completed");
    assert!(private_root.join("captures/recent-captures.json").exists());
}

async fn complete_capture(
    app: axum::Router,
    cookie: &str,
    frame: u64,
    idempotency_key: &str,
) -> Value {
    let trigger = trigger_capture(app.clone(), cookie, SESSION_ID, idempotency_key, frame).await;
    let job_uri = format!(
        "/api/capture/jobs/{}",
        trigger["job_id"].as_str().expect("job id")
    );

    let capturing = request_json(app.clone(), runtime_get(&job_uri, cookie)).await;
    assert_eq!(capturing["status"], "capturing");
    request_json(app, runtime_get(&job_uri, cookie)).await
}

async fn trigger_capture(
    app: axum::Router,
    cookie: &str,
    session_id: &str,
    idempotency_key: &str,
    observed_preview_frame: u64,
) -> Value {
    request_json(
        app,
        runtime_json_request(
            Method::POST,
            "/api/capture/trigger",
            cookie,
            json!({
                "schema_version": 1,
                "session_id": session_id,
                "idempotency_key": idempotency_key,
                "observed_preview_frame": observed_preview_frame,
                "reason": "operator_mark"
            }),
        ),
    )
    .await
}

async fn apply_labels_with_dedup(
    app: axum::Router,
    cookie: &str,
    idempotency_key: &str,
    updates: Value,
    dedup_updates: Value,
) -> Value {
    request_json(
        app,
        runtime_json_request(
            Method::POST,
            "/api/labels",
            cookie,
            json!({
                "schema_version": 1,
                "session_id": SESSION_ID,
                "idempotency_key": idempotency_key,
                "updates": updates,
                "dedup_updates": dedup_updates
            }),
        ),
    )
    .await
}

async fn login_cookie(app: axum::Router) -> String {
    login_cookie_with_capabilities(app, &["capture", "preview"]).await
}

async fn login_cookie_with_capabilities(app: axum::Router, capabilities: &[&str]) -> String {
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
                        "requested_capabilities": capabilities
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

fn read_label_draft(private_root: &Path, capture_id: &str) -> LabelDraftFile {
    let path = private_root
        .join("captures")
        .join(capture_id)
        .join("label-draft.json");
    serde_json::from_str(&fs::read_to_string(path).expect("label draft exists"))
        .expect("label draft parses")
}

async fn request_json(app: axum::Router, request: Request<Body>) -> Value {
    let response = app.oneshot(request).await.expect("request runs");
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
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

fn runtime_get_without_cookie(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
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

fn assert_no_store_headers(headers: &axum::http::HeaderMap) {
    assert_private_no_store_headers(headers);
    assert_eq!(
        headers
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some(ALLOWED_ORIGIN)
    );
    assert_eq!(
        headers.get(VARY).and_then(|value| value.to_str().ok()),
        Some("Origin")
    );
}

fn assert_private_no_store_headers(headers: &axum::http::HeaderMap) {
    assert_eq!(
        headers
            .get(CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        headers.get(PRAGMA).and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );
    assert_eq!(
        headers
            .get("x-content-type-options")
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
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

struct WsServer {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl WsServer {
    async fn start(app: axum::Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let addr = listener.local_addr().expect("listener addr is available");
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server runs");
        });

        Self { addr, handle }
    }

    async fn connect(&self, cookie: &str) -> TestSocket {
        let mut request = format!("ws://{}/ws/events", self.addr)
            .into_client_request()
            .expect("websocket request builds");
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_static(ALLOWED_ORIGIN));
        request.headers_mut().insert(
            HeaderName::from_static("host"),
            HeaderValue::from_static("rombridge.birb.homes"),
        );
        request.headers_mut().insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_str(cookie).expect("cookie header parses"),
        );

        connect_async(request)
            .await
            .map(|(socket, _)| socket)
            .expect("websocket connects")
    }
}

impl Drop for WsServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn read_events(ws: &mut TestSocket, count: usize) -> Vec<Value> {
    let mut messages = Vec::with_capacity(count);
    for _ in 0..count {
        let message = timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("event message is available before timeout")
            .expect("event message is available")
            .expect("event message succeeds");
        let Message::Text(text) = message else {
            panic!("expected text websocket response");
        };
        let json: Value = serde_json::from_str(&text).expect("event json parses");
        assert_matches_runtime_schema(&json);
        messages.push(json);
    }
    messages
}

fn event_types(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .map(|message| message["type"].as_str().expect("event type is string"))
        .collect()
}

fn capture_app(backend: CaptureBackend) -> (tempfile::TempDir, axum::Router, PathBuf) {
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
struct CaptureBackend {
    preview_frames: Mutex<VecDeque<u64>>,
    state: Mutex<SessionState>,
}

impl CaptureBackend {
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

impl BridgeBackend for CaptureBackend {
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
            preview_stale: false,
            active_capture_job_id: None,
        })
    }

    fn pause(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Paused,
            current_frame: self.current_frame(),
            preview_stale: false,
        })
    }

    fn resume(&self, session_id: SessionId) -> BackendResult<RunBoundary> {
        Ok(RunBoundary {
            session_id,
            state: SessionState::Running,
            current_frame: self.current_frame(),
            preview_stale: false,
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
            job_id: "backend-capture-job".to_string(),
            status: CaptureJobStatus::Running,
            capture_id: None,
            public: None,
        })
    }

    fn capture_job(&self, job_id: String) -> BackendResult<CaptureJob> {
        Ok(CaptureJob {
            job_id,
            status: CaptureJobStatus::Running,
            capture_id: None,
            public: None,
        })
    }
}
