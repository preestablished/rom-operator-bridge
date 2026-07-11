use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE, ORIGIN, PRAGMA, SET_COOKIE,
            VARY,
        },
    },
};
use dh_proto::v1 as dh;
use dh_proto::v1::hypervisor_worker_server::{HypervisorWorker, HypervisorWorkerServer};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::ALLOWED_ORIGIN,
    backend::{
        BackendCapabilities, BackendError, BridgeBackend, CaptureJobStatus, CaptureRequest,
        InputScheduleRequest, PlayStreamEvent, RealBackend, SessionState, StartBackendSession,
    },
    config::{ENV_BACKEND_MODE, ServiceConfig},
    input::{PadButton, PadLog, PadWord},
    private_config::{
        ENV_CAPTURE_SPEC_REF, ENV_CREATE_VM_CONFIG_REF, ENV_HYPERVISOR_ENDPOINT, ENV_PRIVATE_ROOT,
        ENV_REAL_SNAPSHOT_REF, ENV_REFERENCE_WORKLOAD_CHECKOUT, ENV_SESSION_SECRET,
        ENV_WORKLOAD_IMAGE_REF,
    },
    sanitization::PublicSanitizer,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::net::UnixListener;
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request as TonicRequest, Response as TonicResponse, Status, transport::Server};
use tower::{Service, ServiceExt};

const SECRET_LITERAL: &str = "private-secret-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const WORKLOAD_IMAGE_REF: &str = "private-workload-image-ref-from-test";
const CAPTURE_SPEC_REF: &str = "private-capture-spec-ref-from-test";
const SNAPSHOT_REF: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const LEASE_TOKEN: &[u8] = b"mock-private-lease-token";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_start_without_attached_worker_returns_sanitized_backend_unavailable() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let config = real_config(&private_root, &reference_checkout);
    let app = router(AppState::from_config(config));

    let response = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(
                json!({
                    "schema_version": 1,
                    "backend_mode": "real",
                    "requested_capabilities": ["input", "preview", "capture"]
                })
                .to_string(),
            ),
        ))
        .await
        .expect("real start request runs");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.headers().get(SET_COOKIE).is_none());

    let body = to_bytes(response.into_body(), 4096)
        .await
        .expect("body reads");
    let json: Value = serde_json::from_slice(&body).expect("error body is json");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["error"]["code"], "backend_unavailable");
    assert_eq!(json["error"]["message"], "Backend unavailable.");
    assert_eq!(json["error"]["retryable"], true);
    assert_eq!(json["error"]["details"], serde_json::json!({}));

    let body = String::from_utf8(body.to_vec()).expect("body is utf8");
    for forbidden in [
        private_root.display().to_string(),
        reference_checkout.display().to_string(),
        "/run/dh/grpc.sock".to_string(),
        WORKLOAD_IMAGE_REF.to_string(),
        CAPTURE_SPEC_REF.to_string(),
        SNAPSHOT_REF.to_string(),
    ] {
        assert!(
            !body.contains(&forbidden),
            "backend unavailable response leaked private value: {forbidden}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_restore_snapshot_lifecycle_calls_worker_and_stays_sanitized() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body_with_capabilities(
                "real",
                &["input", "preview", "capture", "labels"],
            )),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_body = body_json(start).await;
    assert_eq!(start_body["session_id"], "real-session-0000");
    assert_eq!(start_body["run_id"], "real-run-0000");
    assert_eq!(start_body["state"], "paused");
    assert_eq!(start_body["current_frame"], 12);
    assert_eq!(start_body["capabilities"]["input"], true);
    assert_eq!(start_body["capabilities"]["preview"], true);
    assert_eq!(start_body["capabilities"]["capture"], false);
    assert_public_json_sanitized(&start_body, &private_root, &reference_checkout, &server);

    let pause = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/run/pause",
            &cookie,
            Body::from(session_body("real-session-0000")),
        ),
    )
    .await;
    assert_eq!(pause.status(), StatusCode::OK);

    let resume = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/run/resume",
            &cookie,
            Body::from(session_body("real-session-0000")),
        ),
    )
    .await;
    assert_eq!(resume.status(), StatusCode::OK);
    let resume_body = body_json(resume).await;
    assert_eq!(resume_body["state"], "paused");
    assert_eq!(resume_body["current_frame"], 12);

    let status = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/run/status", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body = body_json(status).await;
    assert_eq!(status_body["backend_mode"], "real");
    assert_eq!(status_body["state"], "paused");
    assert_eq!(status_body["current_frame"], 12);
    assert_eq!(status_body["preview_stale"], true);

    let stop = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/session/stop",
            &cookie,
            Body::from(stop_body("real-session-0000")),
        ),
    )
    .await;
    assert_eq!(stop.status(), StatusCode::OK);
    let stop_body = body_json(stop).await;
    assert_eq!(stop_body["state"], "stopped");
    assert_public_json_sanitized(&stop_body, &private_root, &reference_checkout, &server);
    assert_private_artifacts_do_not_contain_lease(&private_root);

    let snapshot_hash = worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .restore_hash
        .clone()
        .expect("restore snapshot was called");
    assert_eq!(snapshot_hash, vec![0x11; 32]);
    let calls = worker.calls();
    assert!(calls.iter().any(|call| *call == "watch_slots"));
    // The restored session is already Paused, so the API pause is idempotent
    // bookkeeping: no worker Pause RPC is dispatched (it could otherwise race
    // a streaming teardown and quantize to the next epoch).
    assert!(!calls.iter().any(|call| *call == "pause"));
    assert!(calls.iter().any(|call| *call == "run"));
    assert!(calls.iter().any(|call| *call == "list_slots"));
    assert!(calls.iter().any(|call| *call == "destroy_vm"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_status_failure_faults_session_and_keeps_stop_available() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);

    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .list_slots_status = Some(tonic::Code::Unavailable);
    let status = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/run/status", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body = body_json(status).await;
    assert_eq!(status_body["state"], "faulted");
    assert_eq!(status_body["backend_mode"], "real");
    assert_public_json_sanitized(&status_body, &private_root, &reference_checkout, &server);

    let stop = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/session/stop",
            &cookie,
            Body::from(stop_body("real-session-0000")),
        ),
    )
    .await;
    assert_eq!(stop.status(), StatusCode::OK);
    let stop_body = body_json(stop).await;
    assert_eq!(stop_body["state"], "stopped");
    assert_public_json_sanitized(&stop_body, &private_root, &reference_checkout, &server);

    let calls = worker.calls();
    assert!(calls.iter().any(|call| *call == "list_slots"));
    assert!(calls.iter().any(|call| *call == "destroy_vm"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_capture_trigger_calls_take_snapshot_and_writes_private_index() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let layout_hash = write_capture_bundle(&reference_checkout);
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));
    let capture_job_id = "real-capture-job-real-session-0000-aaaaaaaabbbbccccddddeeeeeeeeeeee";
    let capture_id = "real-capture-real-session-0000-aaaaaaaabbbbccccddddeeeeeeeeeeee";

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body_with_capabilities(
                "real",
                &["input", "preview", "capture", "labels"],
            )),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_body = body_json(start).await;
    assert_eq!(start_body["capabilities"]["capture"], true);
    assert_eq!(start_body["capabilities"]["labels"], true);

    let trigger = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            Body::from(
                json!({
                    "schema_version": 1,
                    "session_id": "real-session-0000",
                    "idempotency_key": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "observed_preview_frame": 12,
                    "reason": "operator_mark"
                })
                .to_string(),
            ),
        ),
    )
    .await;
    assert_eq!(trigger.status(), StatusCode::OK);
    let trigger_body = body_json(trigger).await;
    assert_eq!(trigger_body["job_id"], capture_job_id);
    assert_eq!(trigger_body["status"], "completed");
    assert_public_json_sanitized(&trigger_body, &private_root, &reference_checkout, &server);

    let replay = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            Body::from(
                json!({
                    "schema_version": 1,
                    "session_id": "real-session-0000",
                    "idempotency_key": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "observed_preview_frame": 12,
                    "reason": "operator_mark"
                })
                .to_string(),
            ),
        ),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::OK);
    let replay_body = body_json(replay).await;
    assert_eq!(replay_body["job_id"], capture_job_id);
    assert_eq!(replay_body["status"], "completed");
    assert_public_json_sanitized(&replay_body, &private_root, &reference_checkout, &server);

    let job = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::GET,
            &format!("/api/capture/jobs/{capture_job_id}"),
            &cookie,
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(job.status(), StatusCode::OK);
    let job_body = body_json(job).await;
    assert_eq!(job_body["status"], "completed");
    assert_eq!(job_body["capture_id"], capture_id);
    assert_eq!(job_body["labelable"], true);
    assert_eq!(job_body["has_preview"], false);
    assert_eq!(job_body["captured_frame"], 12);
    assert_public_json_sanitized(&job_body, &private_root, &reference_checkout, &server);

    let detail = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::GET,
            &format!("/api/capture/{capture_id}"),
            &cookie,
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body = body_json(detail).await;
    assert_eq!(
        detail_body["sanitized_provenance"]["capture_source"],
        "hypervisor"
    );
    assert_eq!(
        detail_body["sanitized_provenance"]["layout_hash"],
        layout_hash
    );
    assert_eq!(
        detail_body["sanitized_provenance"]["capture_spec_hash"],
        "private-capture-spec"
    );
    assert_eq!(detail_body["frame"], 12);
    assert_eq!(detail_body["has_preview"], false);
    assert_eq!(detail_body["preview_image_url"], Value::Null);
    assert_eq!(detail_body["privileged_features_available"], false);
    assert_public_json_sanitized(&detail_body, &private_root, &reference_checkout, &server);

    let labels = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/labels",
            &cookie,
            Body::from(
                json!({
                    "schema_version": 1,
                    "session_id": "real-session-0000",
                    "idempotency_key": "bbbbbbbb-cccc-dddd-eeee-ffffffffffff",
                    "updates": [
                        { "op": "upsert", "capture_id": capture_id, "role": "needs_review" }
                    ],
                    "dedup_updates": []
                })
                .to_string(),
            ),
        ),
    )
    .await;
    assert_eq!(labels.status(), StatusCode::OK);
    let labels_body = body_json(labels).await;
    assert_eq!(labels_body["applied"], true);
    assert_public_json_sanitized(&labels_body, &private_root, &reference_checkout, &server);

    let preview = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::GET,
            &format!("/api/capture/{capture_id}/preview"),
            &cookie,
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(preview.status(), StatusCode::NOT_FOUND);

    let take_snapshot_requests = worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .take_snapshot_requests
        .clone();
    assert_eq!(take_snapshot_requests.len(), 1);
    let take_snapshot = take_snapshot_requests
        .first()
        .expect("take snapshot was called");
    assert_eq!(take_snapshot.seal_input_log, Some(true));
    let lease = take_snapshot.lease.as_ref().expect("lease is set");
    assert_eq!(lease.slot_id, 7);
    assert_eq!(lease.token.as_slice(), LEASE_TOKEN);
    let capture = take_snapshot.capture.as_ref().expect("capture spec is set");
    assert!(capture.framebuffer);
    assert_eq!(capture.ranges.len(), 2);
    assert_eq!(capture.ranges[0].region, "wram");
    assert_eq!(capture.ranges[0].layout_version, 1);
    assert_eq!(capture.ranges[0].offset, 16);
    assert_eq!(capture.ranges[0].len, 2);

    let index_lines = read_lines(&private_root.join("captures/index.jsonl"));
    assert_eq!(index_lines.len(), 1);
    let row: Value = serde_json::from_str(&index_lines[0]).expect("index row parses");
    assert_eq!(row["capture_id"], capture_id);
    assert_eq!(row["capture_source"], "real_take_snapshot");
    assert_eq!(row["layout_hash"], layout_hash);
    assert_eq!(row["feature_bytes"]["len"], 3);
    assert_eq!(row["decoded_order"], json!(["frame_ctr", "frame_low"]));
    assert_eq!(row["decoded_values"], json!([9, 5]));
    assert_eq!(row["framebuffer"]["encoding"], "fb_lz4");
    assert_eq!(row["framebuffer"]["pixel_format"], "xrgb8888");
    assert!(row["feature_bytes"]["bytes"].is_null());
    assert!(row["framebuffer"]["bytes"].is_null());
    let recent: Value = serde_json::from_str(
        &fs::read_to_string(private_root.join("captures/recent-captures.json"))
            .expect("recent captures reads"),
    )
    .expect("recent captures parses");
    assert_eq!(
        recent["captures"].as_array().expect("captures array").len(),
        1
    );
    let manifest_path = private_root.join(format!("captures/{capture_id}/capture-manifest.json"));
    assert!(manifest_path.is_file());
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("capture manifest reads"))
            .expect("capture manifest parses");
    assert_eq!(
        manifest["snapshot_ref"],
        format!("blake3:{}", "55".repeat(32))
    );
    assert_eq!(
        manifest["input_log_id"],
        format!("blake3:{}", "66".repeat(32))
    );
    assert_eq!(
        manifest["state_hash"],
        format!("blake3:{}", "77".repeat(32))
    );
    assert_eq!(
        manifest["machine_config_hash"],
        format!("blake3:{}", "88".repeat(32))
    );
    assert_eq!(manifest["icount"], 44);
    assert_eq!(manifest["vns"], 88);
    assert!(
        private_root
            .join(format!("captures/{capture_id}/label-draft.json"))
            .is_file()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_capture_worker_failure_returns_sanitized_failed_job_without_index() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    write_capture_bundle(&reference_checkout);
    let worker = MockWorker::new();
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .take_snapshot_status = Some(tonic::Code::Unavailable);
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));
    let capture_job_id = "real-capture-job-real-session-0000-aaaaaaaabbbbccccddddeeeeeeeeeeee";

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body_with_capabilities(
                "real",
                &["input", "preview", "capture", "labels"],
            )),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);

    let trigger = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            Body::from(
                json!({
                    "schema_version": 1,
                    "session_id": "real-session-0000",
                    "idempotency_key": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "observed_preview_frame": 12,
                    "reason": "operator_mark"
                })
                .to_string(),
            ),
        ),
    )
    .await;
    assert_eq!(trigger.status(), StatusCode::OK);
    let trigger_body = body_json(trigger).await;
    assert_eq!(trigger_body["job_id"], capture_job_id);
    assert_eq!(trigger_body["status"], "failed");
    assert_public_json_sanitized(&trigger_body, &private_root, &reference_checkout, &server);

    let job = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::GET,
            &format!("/api/capture/jobs/{capture_job_id}"),
            &cookie,
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(job.status(), StatusCode::OK);
    let job_body = body_json(job).await;
    assert_eq!(job_body["status"], "failed");
    assert_eq!(job_body["capture_id"], Value::Null);
    assert_eq!(job_body["labelable"], false);
    assert_eq!(job_body["has_preview"], false);
    assert_eq!(job_body["error"]["code"], "capture_failed");
    assert_eq!(job_body["error"]["details"], json!({}));
    assert_public_json_sanitized(&job_body, &private_root, &reference_checkout, &server);
    assert!(!private_root.join("captures/index.jsonl").exists());
    assert_eq!(
        worker
            .state
            .lock()
            .expect("mock worker mutex poisoned")
            .take_snapshot_requests
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_capture_stale_preview_fails_without_take_snapshot() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    write_capture_bundle(&reference_checkout);
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body_with_capabilities(
                "real",
                &["input", "preview", "capture", "labels"],
            )),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);

    let trigger = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            Body::from(
                json!({
                    "schema_version": 1,
                    "session_id": "real-session-0000",
                    "idempotency_key": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "observed_preview_frame": 11,
                    "reason": "operator_mark"
                })
                .to_string(),
            ),
        ),
    )
    .await;
    assert_eq!(trigger.status(), StatusCode::OK);
    let trigger_body = body_json(trigger).await;
    assert_eq!(trigger_body["status"], "failed");
    assert_public_json_sanitized(&trigger_body, &private_root, &reference_checkout, &server);

    let job = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::GET,
            &format!(
                "/api/capture/jobs/{}",
                trigger_body["job_id"].as_str().expect("job id is string")
            ),
            &cookie,
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(job.status(), StatusCode::OK);
    let job_body = body_json(job).await;
    assert_eq!(job_body["status"], "failed");
    assert_eq!(job_body["capture_id"], Value::Null);
    assert_eq!(job_body["error"]["code"], "frame_stale");
    assert_eq!(job_body["error"]["details"], json!({}));
    assert_public_json_sanitized(&job_body, &private_root, &reference_checkout, &server);

    let replay = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/capture/trigger",
            &cookie,
            Body::from(
                json!({
                    "schema_version": 1,
                    "session_id": "real-session-0000",
                    "idempotency_key": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                    "observed_preview_frame": 11,
                    "reason": "operator_mark"
                })
                .to_string(),
            ),
        ),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::OK);
    let replay_body = body_json(replay).await;
    assert_eq!(replay_body["job_id"], trigger_body["job_id"]);
    assert_eq!(replay_body["status"], "failed");

    assert_eq!(
        worker
            .state
            .lock()
            .expect("mock worker mutex poisoned")
            .take_snapshot_requests
            .len(),
        0
    );
    assert!(!private_root.join("captures/index.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_capture_framebuffer_metadata_mismatch_fails_without_index() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    write_capture_bundle(&reference_checkout);
    let worker = MockWorker::new();
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .take_snapshot_response
        .fb_info
        .as_mut()
        .expect("fb info exists")
        .stride = 32;
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::real_capture_mvp(),
        })
        .expect("real session starts");
    assert_eq!(session.capabilities.capture, true);

    let job = backend
        .trigger_capture(CaptureRequest {
            session_id: session.session_id,
            idempotency_key: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
        })
        .expect("capture job is stored");

    assert_eq!(job.status, CaptureJobStatus::Failed);
    assert_eq!(job.capture_id, None);
    assert!(job.public.is_none());
    assert!(!private_root.join("captures/index.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_capture_capability_fails_closed_for_layout_feature_map_mismatch() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    write_capture_bundle(&reference_checkout);
    fs::write(
        reference_checkout.join("feature-map.yaml"),
        r#"
schema_version: 1
kind: feature-map
regions:
  - name: wram
    size: 131072
features:
  - name: frame_ctr
    region: wram
    offset: 17
    type: u16le
  - name: frame_low
    region: wram
    offset: 18
    type: u8
"#,
    )
    .expect("corrupt feature map writes");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);

    assert_eq!(backend.capabilities().capture, false);
    assert!(matches!(
        backend.trigger_capture(CaptureRequest {
            session_id: "real-session-0000".to_string(),
            idempotency_key: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
        }),
        Err(BackendError::BackendUnavailable)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_capture_index_append_failure_keeps_job_failed_and_replay_idempotent() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    write_capture_bundle(&reference_checkout);
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::real_capture_mvp(),
        })
        .expect("real session starts");
    fs::create_dir_all(private_root.join("captures/index.jsonl"))
        .expect("index path conflict creates");

    let request = CaptureRequest {
        session_id: session.session_id,
        idempotency_key: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
    };
    let first = backend
        .trigger_capture(request.clone())
        .expect("failed capture job returns");
    fs::remove_file(reference_checkout.join("layout.json"))
        .expect("layout file removal makes capture spec unavailable");
    let replay = backend
        .trigger_capture(request)
        .expect("failed capture replay returns same job");
    let capture_id = "real-capture-real-session-0000-aaaaaaaabbbbccccddddeeeeeeeeeeee";

    assert_eq!(first.status, CaptureJobStatus::Failed);
    assert_eq!(first.capture_id, None);
    assert!(first.public.is_none());
    assert_eq!(replay.status, CaptureJobStatus::Failed);
    assert_eq!(replay.job_id, first.job_id);
    assert_eq!(
        worker
            .state
            .lock()
            .expect("mock worker mutex poisoned")
            .take_snapshot_requests
            .len(),
        1
    );
    assert!(
        private_root
            .join(format!(
                "artifacts/feature-bytes/{capture_id}-feature-bytes.bin"
            ))
            .is_file()
    );
    assert!(
        private_root
            .join(format!(
                "artifacts/framebuffer/{capture_id}-framebuffer.fb_lz4"
            ))
            .is_file()
    );
    assert!(
        private_root
            .join(format!("captures/{capture_id}/capture-manifest.json"))
            .is_file()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_input_injection_schedules_pad_set_and_writes_private_padlog() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::synthetic_mvp(),
        })
        .expect("real session starts");
    assert_eq!(session.capabilities.input, true);

    let pad_word = PadWord::from_buttons([PadButton::A, PadButton::Start]);
    let receipt = backend
        .inject_input(input_request(&session.session_id, 13, pad_word))
        .expect("real input schedules");

    assert_eq!(receipt.session_id, session.session_id);
    assert_eq!(receipt.assigned_frame, 13);
    assert_eq!(receipt.pad_word, pad_word);

    let inject_request = worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .inject_inputs
        .first()
        .cloned()
        .expect("inject_inputs was called");
    let lease = inject_request.lease.as_ref().expect("lease is set");
    assert_eq!(lease.slot_id, 7);
    assert_eq!(lease.token.as_slice(), LEASE_TOKEN);
    assert_eq!(inject_request.events.len(), 1);
    let scheduled = inject_request
        .events
        .first()
        .expect("scheduled event exists");
    assert_eq!(scheduled.at, Some(dh::scheduled_event::At::AtFrame(13)));
    let pad_set = match scheduled.event.as_ref() {
        Some(dh::scheduled_event::Event::PadSet(pad_set)) => pad_set,
        other => panic!("expected PadSet event, got {other:?}"),
    };
    assert_eq!(pad_set.port, 0);
    assert_eq!(pad_set.buttons, u32::from(pad_word.raw()));

    let run_root = private_root.join("runs").join(&session.run_id);
    let padlog_text = fs::read_to_string(run_root.join("input.padlog")).expect("padlog is written");
    let parsed = PadLog::parse(&padlog_text).expect("padlog parses");
    assert_eq!(
        parsed
            .frames()
            .iter()
            .map(|word| word.raw())
            .collect::<Vec<_>>(),
        vec![pad_word.raw()]
    );

    let event_lines = read_lines(&run_root.join("padlog-events.jsonl"));
    assert_eq!(event_lines.len(), 1);
    let event: Value = serde_json::from_str(&event_lines[0]).expect("padlog event parses");
    assert_eq!(event["run_id"], session.run_id);
    assert_eq!(event["frame_index"], 0);
    assert_eq!(event["assigned_frame"], 13);
    assert_eq!(event["pad_word"], pad_word.raw());
    assert_eq!(event["client_seq"], 42);
    assert_eq!(event["source_id"], "keyboard");
    assert_eq!(event["status"], "applied");
    assert_private_artifacts_do_not_contain_lease(&private_root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_input_rejects_frame_hint_none_boundary_before_worker_call() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::synthetic_mvp(),
        })
        .expect("real session starts");

    let error = backend
        .inject_input(input_request(
            &session.session_id,
            u64::from(u32::MAX),
            PadWord::from_buttons([PadButton::A]),
        ))
        .expect_err("frame hint sentinel is rejected");

    assert_eq!(error, BackendError::BackendUnavailable);
    assert!(!worker.calls().contains(&"inject_inputs"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_input_invalid_argument_refreshes_frame_and_returns_stale() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::synthetic_mvp(),
        })
        .expect("real session starts");
    {
        let mut state = worker.state.lock().expect("mock worker mutex poisoned");
        state.inject_status = Some(tonic::Code::InvalidArgument);
        state.framebuffer_response = MockWorker::framebuffer_response(20, 123);
    }

    let error = backend
        .inject_input(input_request(
            &session.session_id,
            13,
            PadWord::from_buttons([PadButton::B]),
        ))
        .expect_err("stale input reports frame stale");

    assert_eq!(
        error,
        BackendError::FrameStale {
            requested_frame: 13,
            current_frame: 20,
        }
    );
    let calls = worker.calls();
    assert!(calls.contains(&"inject_inputs"));
    assert!(calls.contains(&"get_framebuffer"));
    let status = backend
        .status(session.session_id)
        .expect("status is still available");
    assert_eq!(status.current_frame, 20);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_input_invalid_argument_with_refresh_failure_returns_unavailable() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::synthetic_mvp(),
        })
        .expect("real session starts");
    {
        let mut state = worker.state.lock().expect("mock worker mutex poisoned");
        state.inject_status = Some(tonic::Code::InvalidArgument);
        state.framebuffer_status = Some(tonic::Code::Unavailable);
    }

    let error = backend
        .inject_input(input_request(
            &session.session_id,
            13,
            PadWord::from_buttons([PadButton::B]),
        ))
        .expect_err("refresh failure is sanitized unavailable");

    assert_eq!(error, BackendError::BackendUnavailable);
    let calls = worker.calls();
    assert!(calls.contains(&"inject_inputs"));
    assert!(calls.contains(&"get_framebuffer"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_input_artifact_failure_quarantines_session_and_stops_worker() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::synthetic_mvp(),
        })
        .expect("real session starts");
    let run_root = private_root.join("runs").join(&session.run_id);
    fs::create_dir(run_root.join("padlog-events.jsonl"))
        .expect("directory blocks padlog event append");

    let error = backend
        .inject_input(input_request(
            &session.session_id,
            13,
            PadWord::from_buttons([PadButton::X]),
        ))
        .expect_err("artifact failure is sanitized");

    assert_eq!(error, BackendError::BackendUnavailable);
    let calls = worker.calls();
    assert!(calls.contains(&"inject_inputs"));
    assert!(calls.contains(&"destroy_vm"));
    assert_eq!(
        backend
            .status(session.session_id)
            .expect_err("quarantined session is inactive"),
        BackendError::BackendUnavailable
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_resume_worker_failure_faults_after_transient_running_state() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    let session = backend
        .start_session(StartBackendSession {
            requested_capabilities: BackendCapabilities::synthetic_mvp(),
        })
        .expect("real session starts");
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .run_status = Some(tonic::Code::Unavailable);

    let error = backend
        .resume(session.session_id.clone())
        .expect_err("worker run failure is sanitized");

    assert_eq!(error, BackendError::BackendUnavailable);
    let status = backend
        .status(session.session_id)
        .expect("faulted session remains inspectable");
    assert_eq!(status.state, SessionState::Faulted);
    assert_ne!(status.state, SessionState::Running);
    assert!(worker.calls().contains(&"run"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_create_vm_invalid_private_config_returns_sanitized_unavailable() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((
            ENV_CREATE_VM_CONFIG_REF,
            "real/create-vm-config.json".to_string(),
        )),
    );
    config
        .private_config()
        .write_private_file(
            "real/create-vm-config.json",
            create_vm_config_json()
                .replace(
                    "3333333333333333333333333333333333333333333333333333333333333333",
                    "not-a-hex-hash",
                )
                .as_bytes(),
        )
        .expect("private create-vm config writes");
    let app = router(AppState::from_config(config));

    let response = app
        .oneshot(runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ))
        .await
        .expect("start request runs");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(response).await;
    assert_eq!(body["error"]["code"], "backend_unavailable");
    assert_eq!(body["error"]["details"], serde_json::json!({}));
    assert_public_json_sanitized(&body, &private_root, &reference_checkout, &server);
    assert!(!worker.calls().iter().any(|call| *call == "create_vm"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_create_vm_start_parses_private_config_and_stops_worker_slot() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((
            ENV_CREATE_VM_CONFIG_REF,
            "real/create-vm-config.json".to_string(),
        )),
    );
    config
        .private_config()
        .write_private_file(
            "real/create-vm-config.json",
            create_vm_config_json().as_bytes(),
        )
        .expect("private create-vm config writes");
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_body = body_json(start).await;
    assert_eq!(start_body["current_frame"], 0);
    assert_public_json_sanitized(&start_body, &private_root, &reference_checkout, &server);

    let create_request = worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .create_vm
        .clone()
        .expect("create vm was called");
    assert_eq!(create_request.entropy_seed, vec![0x22; 32]);
    let machine_config = create_request.config.expect("machine config set");
    assert_eq!(machine_config.base_image_hash, vec![0x33; 32]);
    assert_eq!(machine_config.hash_epochs, dh::HashEpochs::EpochsOn as i32);
    assert_eq!(machine_config.device_set, vec![1, 2, 3]);

    let stop = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/session/stop",
            &cookie,
            Body::from(stop_body("real-session-0000")),
        ),
    )
    .await;
    assert_eq!(stop.status(), StatusCode::OK);
    assert!(worker.calls().iter().any(|call| *call == "destroy_vm"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_framebuffer_preview_routes_return_schema_safe_png() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_body = body_json(start).await;
    assert_eq!(start_body["capabilities"]["preview"], true);

    let metadata_response = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/frame/current", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(metadata_response.status(), StatusCode::OK);
    assert_no_store_headers(metadata_response.headers());
    let metadata_body = to_bytes(metadata_response.into_body(), 16 * 1024)
        .await
        .expect("metadata body reads");
    let metadata: Value = serde_json::from_slice(&metadata_body).expect("metadata json parses");
    assert_matches_runtime_schema(&metadata);
    assert_eq!(metadata["frame"], 12);
    assert_eq!(metadata["stale"], false);
    assert_eq!(metadata["width"], 256);
    assert_eq!(metadata["height"], 224);
    assert_eq!(metadata["format"], "image/png");
    assert_public_json_sanitized_with_worker_text(
        &metadata,
        &private_root,
        &reference_checkout,
        &server,
        "private framebuffer worker failure",
    );

    let image_url = metadata["image_url"]
        .as_str()
        .expect("image url is a string");
    let image_response = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, image_url, &cookie, Body::empty()),
    )
    .await;
    assert_eq!(image_response.status(), StatusCode::OK);
    assert_no_store_headers(image_response.headers());
    assert_eq!(
        image_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let image = to_bytes(image_response.into_body(), 512 * 1024)
        .await
        .expect("image body reads");
    assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(metadata["preview_hash"], sha256_ref(&image));
    assert!(worker.calls().iter().any(|call| *call == "get_framebuffer"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_framebuffer_failure_is_sanitized_and_keeps_session_active() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .framebuffer_status = Some(tonic::Code::FailedPrecondition);
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);

    let preview = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/frame/current", &cookie, Body::empty()),
    )
    .await;
    // Worker FailedPrecondition on the framebuffer path means "no frame to
    // show", not a backend outage: surfaced as 404 frame_unavailable.
    assert_eq!(preview.status(), StatusCode::NOT_FOUND);
    assert_ne!(
        preview
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let body = body_json(preview).await;
    assert_eq!(body["error"]["code"], "frame_unavailable");
    assert_eq!(body["error"]["retryable"], true);
    assert_eq!(body["error"]["details"], serde_json::json!({}));
    assert_public_json_sanitized_with_worker_text(
        &body,
        &private_root,
        &reference_checkout,
        &server,
        "private framebuffer worker failure",
    );

    let session = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/session", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(session.status(), StatusCode::OK);
    let session_body = body_json(session).await;
    assert_eq!(session_body["active"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_framebuffer_rejects_non_schema_dimensions() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    {
        let mut state = worker.state.lock().expect("mock worker mutex poisoned");
        state.framebuffer_response.width = 8;
        state.framebuffer_response.height = 4;
        state.framebuffer_response.stride = 32;
        state.framebuffer_response.pixels = vec![0; 32 * 4];
    }
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);

    let preview = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/frame/current", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(preview.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(preview).await;
    assert_eq!(body["error"]["code"], "backend_unavailable");
    assert_eq!(body["error"]["details"], serde_json::json!({}));
    assert_public_json_sanitized(&body, &private_root, &reference_checkout, &server);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_start_without_preview_request_keeps_preview_capability_ungranted() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body_with_capabilities("real", &["input"])),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_body = body_json(start).await;
    assert_eq!(start_body["capabilities"]["input"], true);
    assert_eq!(start_body["capabilities"]["preview"], false);

    let status = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/run/status", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_body = body_json(status).await;
    assert_eq!(status_body["capabilities"]["preview"], false);

    let metadata_response = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/frame/current", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(metadata_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_ne!(
        metadata_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let metadata_body = body_json(metadata_response).await;
    assert_eq!(metadata_body["error"]["code"], "backend_unavailable");
    assert_eq!(metadata_body["error"]["details"], serde_json::json!({}));
    assert_public_json_sanitized(&metadata_body, &private_root, &reference_checkout, &server);

    let image_response = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::GET,
            "/api/frame/current/image",
            &cookie,
            Body::empty(),
        ),
    )
    .await;
    assert_eq!(image_response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_ne!(
        image_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let image_body = body_json(image_response).await;
    assert_eq!(image_body["error"]["code"], "backend_unavailable");
    assert_eq!(image_body["error"]["details"], serde_json::json!({}));
    assert_public_json_sanitized(&image_body, &private_root, &reference_checkout, &server);

    assert!(!worker.calls().contains(&"get_framebuffer"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_stop_destroy_failure_clears_public_session_with_sanitized_error() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .destroy_fails = true;
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);

    let stop = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/session/stop",
            &cookie,
            Body::from(stop_body("real-session-0000")),
        ),
    )
    .await;
    assert_eq!(stop.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(stop.headers().get(SET_COOKIE).is_some());
    let stop_body = body_json(stop).await;
    assert_eq!(stop_body["error"]["code"], "backend_unavailable");
    assert_eq!(stop_body["error"]["details"], serde_json::json!({}));
    assert_public_json_sanitized(&stop_body, &private_root, &reference_checkout, &server);

    let status = send_request(
        &mut app,
        runtime_request_with_cookie(Method::GET, "/api/run/status", &cookie, Body::empty()),
    )
    .await;
    assert_eq!(status.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_play_step_advances_via_single_captured_run() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);

    let steps = tokio::task::spawn_blocking(move || {
        let session = backend
            .start_session(StartBackendSession {
                requested_capabilities: BackendCapabilities::real_input_preview_mvp(),
            })
            .expect("real session starts");
        backend
            .play_start(session.session_id.clone())
            .expect("play starts");
        let first = backend
            .play_step(session.session_id.clone())
            .expect("first play step");
        let second = backend
            .play_step(session.session_id.clone())
            .expect("second play step");
        let status = backend
            .status(session.session_id.clone())
            .expect("status after play steps");
        (first, second, status)
    })
    .await
    .expect("play steps run");

    // The restored session starts at frame 12; each captured Run advances one.
    assert_eq!(steps.0.frame, 13);
    assert_eq!(steps.1.frame, 14);
    assert_eq!(steps.0.width, 256);
    assert_eq!(steps.0.height, 224);
    assert!(!steps.0.png_bytes.is_empty());
    assert_ne!(steps.0.png_bytes, steps.1.png_bytes);
    assert_eq!(steps.2.current_frame, 14);
    assert_eq!(steps.2.state, SessionState::Playing);

    // One worker round-trip per frame: the Run carries the framebuffer, so no
    // GetFramebuffer is ever dispatched on the Play path.
    let calls = worker.calls();
    assert_eq!(calls.iter().filter(|call| **call == "run").count(), 2);
    assert!(
        !calls.iter().any(|call| *call == "get_framebuffer"),
        "play_step must not issue a separate GetFramebuffer: {calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_streaming_play_keeps_commands_responsive_and_stops_without_pause_rpc() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    // Model the cancel window: the first stop-poll GetFramebuffer calls land
    // while the slot is still Running (FailedPrecondition) and must be
    // retried, not treated as a fault.
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .framebuffer_failed_precondition_remaining = 3;

    let session_id = tokio::task::spawn_blocking(move || {
        let session = backend
            .start_session(StartBackendSession {
                requested_capabilities: BackendCapabilities::real_input_preview_mvp(),
            })
            .expect("real session starts");
        backend
            .play_start(session.session_id.clone())
            .expect("play starts");
        let mut stream = backend
            .play_stream_start(session.session_id.clone())
            .expect("frame stream opens");

        let mut frames = Vec::new();
        while frames.len() < 3 {
            match stream
                .next_frame(std::time::Duration::from_secs(5))
                .expect("next streamed frame")
            {
                PlayStreamEvent::Frame(step) => {
                    assert!(!step.png_bytes.is_empty());
                    frames.push(step.frame);
                }
                PlayStreamEvent::TimedOut => continue,
                PlayStreamEvent::Ended(end) => panic!("stream ended early: {end:?}"),
            }
        }
        assert!(
            frames.windows(2).all(|pair| pair[1] > pair[0]),
            "frame counters must be monotonic: {frames:?}"
        );

        // B2.0: the command lane must stay responsive while the stream is
        // open (a stream dispatched as an ordinary command would park every
        // other RPC behind it for the whole session).
        let started = std::time::Instant::now();
        let status = backend
            .status(session.session_id.clone())
            .expect("status while streaming");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "status must not queue behind the open stream"
        );
        assert_eq!(status.state, SessionState::Playing);
        assert!(status.current_frame >= frames[0]);

        // Stop cancels the stream and parks the session Paused at a frame
        // boundary.
        let boundary = stream.stop().expect("stream stops");
        assert_eq!(boundary.state, SessionState::Paused);
        assert!(boundary.current_frame >= *frames.last().expect("frames observed"));

        // Pause after a streaming teardown is idempotent bookkeeping only.
        let paused = backend
            .pause(session.session_id.clone())
            .expect("pause after streaming stop");
        assert_eq!(paused.state, SessionState::Paused);
        session.session_id
    })
    .await
    .expect("streaming session runs");
    drop(session_id);

    let calls = worker.calls();
    assert!(
        calls.iter().any(|call| *call == "run_with_frame_capture"),
        "streaming play must open RunWithFrameCapture: {calls:?}"
    );
    let state = worker.state.lock().expect("mock worker mutex poisoned");
    assert!(
        state.run_with_frame_capture_requests.iter().all(|request| {
            request.until
                == Some(dh::run_with_frame_capture_request::Until::IcountBudget(
                    u64::MAX / 4,
                ))
        }),
        "normal streaming requests must retain the numeric effectively-unbounded budget"
    );
    drop(state);
    // The plan's pause-vs-stream race: stopping a streamed session must never
    // dispatch a worker Pause (which is epoch-quantized), in the teardown or
    // in the API-level pause that follows it.
    assert!(
        !calls.iter().any(|call| *call == "pause"),
        "no worker Pause RPC may be dispatched on the streaming path: {calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_streaming_segment_budget_end_supports_seamless_reopen() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let backend = real_backend_from_config(&config);
    // Two frames per stream segment, then a BUDGET_REACHED terminal event —
    // the shape of the deliberately bounded segment budget.
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .frame_stream_frame_limit = Some(2);

    tokio::task::spawn_blocking(move || {
        let session = backend
            .start_session(StartBackendSession {
                requested_capabilities: BackendCapabilities::real_input_preview_mvp(),
            })
            .expect("real session starts");
        backend
            .play_start(session.session_id.clone())
            .expect("play starts");

        let mut frames = Vec::new();
        let mut stream = backend
            .play_stream_start(session.session_id.clone())
            .expect("first segment opens");
        loop {
            match stream
                .next_frame(std::time::Duration::from_secs(5))
                .expect("segment event")
            {
                PlayStreamEvent::Frame(step) => frames.push(step.frame),
                PlayStreamEvent::TimedOut => continue,
                PlayStreamEvent::Ended(end) => {
                    assert_eq!(
                        end.reason,
                        rom_operator_bridge_service::backend::PlayStreamEndReason::BudgetReached,
                        "segment ends only on its configured budget"
                    );
                    assert_eq!(end.first_frame_icount, Some(1_300));
                    assert_eq!(end.last_frame_icount, Some(1_400));
                    assert_eq!(end.done_icount, Some(1_400));
                    break;
                }
            }
        }
        assert_eq!(frames, vec![13, 14]);

        // The session is still Playing after a clean segment end, so the Play
        // loop can reopen the next segment where the last one left off.
        let status = backend
            .status(session.session_id.clone())
            .expect("status between segments");
        assert_eq!(status.state, SessionState::Playing);

        let mut next_segment = backend
            .play_stream_start(session.session_id.clone())
            .expect("next segment opens after budget end");
        loop {
            match next_segment
                .next_frame(std::time::Duration::from_secs(5))
                .expect("second segment event")
            {
                PlayStreamEvent::Frame(step) => {
                    assert_eq!(step.frame, 15, "frames continue across segments");
                    break;
                }
                PlayStreamEvent::TimedOut => continue,
                PlayStreamEvent::Ended(end) => panic!("ended early: {end:?}"),
            }
        }
    })
    .await
    .expect("segmented streaming session runs");

    let calls = worker.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == "run_with_frame_capture")
            .count(),
        2,
        "one stream per segment: {calls:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_play_loop_reopens_only_budget_ended_segments() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));
    worker
        .state
        .lock()
        .expect("mock worker mutex poisoned")
        .frame_stream_frame_limit = Some(2);

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_json = body_json(start).await;
    let session_id = start_json["session_id"]
        .as_str()
        .expect("session id is string");

    let play = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/run/play",
            &cookie,
            Body::from(session_body(session_id)),
        ),
    )
    .await;
    assert_eq!(play.status(), StatusCode::OK);

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let opens = worker
                .calls()
                .iter()
                .filter(|call| **call == "run_with_frame_capture")
                .count();
            if opens >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("API Play loop reopens a budget-ended segment");

    let opens_before_cancel = worker
        .calls()
        .iter()
        .filter(|call| **call == "run_with_frame_capture")
        .count();

    let pause = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/run/pause",
            &cookie,
            Body::from(session_body(session_id)),
        ),
    )
    .await;
    assert_eq!(pause.status(), StatusCode::OK);
    let pause_body = body_json(pause).await;
    assert_eq!(pause_body["state"], "paused");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        worker
            .calls()
            .iter()
            .filter(|call| **call == "run_with_frame_capture")
            .count(),
        opens_before_cancel,
        "operator cancellation must stop reopening segments"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_play_loop_faults_transport_eof_without_done() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));
    {
        let mut state = worker.state.lock().expect("mock worker mutex poisoned");
        state.frame_stream_frame_limit = Some(2);
        state.frame_stream_omit_done = true;
    }

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_json = body_json(start).await;
    let session_id = start_json["session_id"]
        .as_str()
        .expect("session id is string");

    let play = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/run/play",
            &cookie,
            Body::from(session_body(session_id)),
        ),
    )
    .await;
    assert_eq!(play.status(), StatusCode::OK);

    let status_body = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let status = send_request(
                &mut app,
                runtime_request_with_cookie(Method::GET, "/api/run/status", &cookie, Body::empty()),
            )
            .await;
            assert_eq!(status.status(), StatusCode::OK);
            let body = body_json(status).await;
            if body["state"] == "faulted" {
                break body;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("unexpected stream EOF transitions the session to faulted");
    assert_eq!(status_body["backend_mode"], "real");
    assert_eq!(status_body["current_frame"], 14);

    assert_eq!(
        worker
            .calls()
            .iter()
            .filter(|call| **call == "run_with_frame_capture")
            .count(),
        1,
        "unexpected EOF must not be classified as a budget boundary"
    );

    let replay = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/run/play",
            &cookie,
            Body::from(session_body(session_id)),
        ),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::SERVICE_UNAVAILABLE);
    let replay_body = body_json(replay).await;
    assert_eq!(replay_body["error"]["code"], "backend_unavailable");
    assert_eq!(
        worker
            .calls()
            .iter()
            .filter(|call| **call == "run_with_frame_capture")
            .count(),
        1,
        "rejected replay must not open another worker stream"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn api_play_loop_does_not_reopen_faulted_stream() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let worker = MockWorker::new();
    let server = WorkerServer::start(worker.clone()).await;
    let config = real_config_with_start(
        &private_root,
        &reference_checkout,
        &format!("unix://{}", server.uds_path.display()),
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    );
    let mut app = router(AppState::from_config(config));
    {
        let mut state = worker.state.lock().expect("mock worker mutex poisoned");
        state.frame_stream_frame_limit = Some(2);
        state.frame_stream_stop_reason = dh::StopReason::Faulted;
    }

    let start = send_request(
        &mut app,
        runtime_request(
            Method::POST,
            "/api/session/start",
            Body::from(start_body("real")),
        ),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let cookie = response_cookie(&start);
    let start_json = body_json(start).await;
    let session_id = start_json["session_id"]
        .as_str()
        .expect("session id is string");

    let play = send_request(
        &mut app,
        runtime_request_with_cookie(
            Method::POST,
            "/api/run/play",
            &cookie,
            Body::from(session_body(session_id)),
        ),
    )
    .await;
    assert_eq!(play.status(), StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    assert_eq!(
        worker
            .calls()
            .iter()
            .filter(|call| **call == "run_with_frame_capture")
            .count(),
        1,
        "faulted streams must terminate rather than reopen"
    );
}

fn real_config(private_root: &Path, reference_checkout: &PathBuf) -> ServiceConfig {
    real_config_with_start(
        private_root,
        reference_checkout,
        "unix:///run/dh/grpc.sock",
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    )
}

fn write_capture_bundle(reference_checkout: &Path) -> String {
    fs::create_dir_all(reference_checkout).expect("reference checkout directory creates");
    let feature_map = r#"
schema_version: 1
kind: feature-map
meta:
  name: real-capture-test
  workload: rom-operator-bridge-test
  game_revision: "test"
  version: 1
regions:
  - name: wram
    size: 131072
features:
  - name: frame_ctr
    region: wram
    offset: 16
    type: u16le
    semantics: counter
    stability: stable
  - name: frame_low
    region: wram
    offset: 18
    type: u8
    semantics: counter
    stability: stable
"#;
    fs::write(reference_checkout.join("feature-map.yaml"), feature_map)
        .expect("feature map writes");
    let ranges = json!([
        { "region": "wram", "layout_version": 1, "offset": 16, "len": 2 },
        { "region": "wram", "layout_version": 1, "offset": 18, "len": 1 }
    ]);
    let map_hash = format!("blake3:{}", blake3::hash(feature_map.as_bytes()).to_hex());
    let compiler_or_exporter_commit = "5555555555555555555555555555555555555555";
    let preimage = json!({
        "ranges": ranges,
        "total_len": 3,
        "compiled_from_feature_map_hash": map_hash,
        "capture_spec_hash": CAPTURE_SPEC_REF,
        "compiler_or_exporter_commit": compiler_or_exporter_commit
    });
    let layout_hash = format!(
        "blake3:{}",
        blake3::hash(&serde_json::to_vec(&preimage).expect("layout hash preimage serializes"))
            .to_hex()
    );
    fs::write(
        reference_checkout.join("layout.json"),
        serde_json::to_string_pretty(&json!({
            "ranges": preimage["ranges"].clone(),
            "total_len": preimage["total_len"].clone(),
            "blake3": layout_hash.clone(),
            "compiled_from_feature_map_hash": preimage["compiled_from_feature_map_hash"].clone(),
            "capture_spec_hash": preimage["capture_spec_hash"].clone(),
            "compiler_or_exporter_commit": preimage["compiler_or_exporter_commit"].clone()
        }))
        .expect("layout json serializes"),
    )
    .expect("layout writes");
    layout_hash
}

fn real_config_with_start(
    private_root: &Path,
    reference_checkout: &PathBuf,
    endpoint: &str,
    start_source: Option<(&'static str, String)>,
) -> ServiceConfig {
    let mut values = vec![
        (ENV_BACKEND_MODE.to_string(), "real".to_string()),
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (ENV_SESSION_SECRET.to_string(), SESSION_SECRET.to_string()),
        (ENV_HYPERVISOR_ENDPOINT.to_string(), endpoint.to_string()),
        (
            ENV_WORKLOAD_IMAGE_REF.to_string(),
            WORKLOAD_IMAGE_REF.to_string(),
        ),
        (
            ENV_CAPTURE_SPEC_REF.to_string(),
            CAPTURE_SPEC_REF.to_string(),
        ),
        (
            ENV_REFERENCE_WORKLOAD_CHECKOUT.to_string(),
            reference_checkout.display().to_string(),
        ),
    ];
    if let Some((env, value)) = start_source {
        values.push((env.to_string(), value));
    }
    ServiceConfig::from_pairs(values).expect("real private config loads")
}

fn real_backend_from_config(config: &ServiceConfig) -> RealBackend {
    RealBackend::new(
        config.private_config().clone(),
        config
            .private_config()
            .real_runtime_config()
            .expect("real runtime config is present")
            .clone(),
    )
}

fn runtime_request(method: Method, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
        .header("content-type", "application/json")
        .body(body)
        .expect("request builds")
}

fn runtime_request_with_cookie(
    method: Method,
    uri: &str,
    cookie: &str,
    body: Body,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(ORIGIN, ALLOWED_ORIGIN)
        .header("content-type", "application/json")
        .header("cookie", cookie)
        .body(body)
        .expect("request builds")
}

async fn send_request(app: &mut axum::Router, request: Request<Body>) -> axum::response::Response {
    tower::ServiceExt::<Request<Body>>::ready(app)
        .await
        .expect("router is ready")
        .call(request)
        .await
        .expect("request runs")
}

fn start_body(backend_mode: &str) -> String {
    start_body_with_capabilities(backend_mode, &["input", "preview", "capture"])
}

fn start_body_with_capabilities(backend_mode: &str, capabilities: &[&str]) -> String {
    json!({
        "schema_version": 1,
        "backend_mode": backend_mode,
        "requested_capabilities": capabilities
    })
    .to_string()
}

fn session_body(session_id: &str) -> String {
    json!({
        "schema_version": 1,
        "session_id": session_id
    })
    .to_string()
}

fn stop_body(session_id: &str) -> String {
    json!({
        "schema_version": 1,
        "session_id": session_id,
        "reason": "operator_stop"
    })
    .to_string()
}

fn input_request(session_id: &str, target_frame: u64, pad_word: PadWord) -> InputScheduleRequest {
    InputScheduleRequest {
        session_id: session_id.to_string(),
        target_frame,
        pad_word,
        client_seq: 42,
        source_id: "keyboard".to_string(),
    }
}

async fn body_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("body reads");
    serde_json::from_slice(&body).expect("body is json")
}

fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .expect("jsonl file reads")
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

fn response_cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(SET_COOKIE)
        .expect("response sets session cookie")
        .to_str()
        .expect("cookie is header text")
        .to_string()
}

fn assert_public_json_sanitized(
    value: &Value,
    private_root: &Path,
    reference_checkout: &Path,
    server: &WorkerServer,
) {
    let body = value.to_string();
    for forbidden in [
        private_root.display().to_string(),
        reference_checkout.display().to_string(),
        format!("unix://{}", server.uds_path.display()),
        server.uds_path.display().to_string(),
        WORKLOAD_IMAGE_REF.to_string(),
        CAPTURE_SPEC_REF.to_string(),
        SNAPSHOT_REF.to_string(),
        String::from_utf8(LEASE_TOKEN.to_vec()).expect("lease token is utf8"),
    ] {
        assert!(
            !body.contains(&forbidden),
            "public response leaked private value: {forbidden}"
        );
    }
}

fn assert_public_json_sanitized_with_worker_text(
    value: &Value,
    private_root: &Path,
    reference_checkout: &Path,
    server: &WorkerServer,
    worker_text: &str,
) {
    assert_public_json_sanitized(value, private_root, reference_checkout, server);
    let endpoint = format!("unix://{}", server.uds_path.display());
    let lease_token = String::from_utf8(LEASE_TOKEN.to_vec()).expect("lease token utf8");
    PublicSanitizer::new()
        .with_private_root(private_root)
        .with_forbidden_literal(SECRET_LITERAL)
        .with_forbidden_literal(SESSION_SECRET)
        .with_forbidden_literal(worker_text)
        .with_forbidden_literal(endpoint)
        .with_forbidden_literal(lease_token)
        .inspect_json(value)
        .expect("json is public-safe");
}

fn assert_no_store_headers(headers: &axum::http::HeaderMap) {
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

fn assert_matches_runtime_schema(json: &Value) {
    let schema: Value =
        serde_json::from_str(include_str!("../../../contracts/runtime-api.schema.json"))
            .expect("runtime schema parses");
    let validator = jsonschema::validator_for(&schema).expect("runtime schema compiles");
    validator.validate(json).unwrap_or_else(|error| {
        panic!("runtime schema validation failed: {error}");
    });
}

fn sha256_ref(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut hex, "{byte:02x}").expect("hex write succeeds");
    }
    format!("sha256:{hex}")
}

fn assert_private_artifacts_do_not_contain_lease(private_root: &Path) {
    let run_dir = private_root.join("runs").join("real-run-0000");
    for file_name in [
        "run-manifest.json",
        "bridge-events.jsonl",
        "input.padlog",
        "padlog-events.jsonl",
    ] {
        let path = run_dir.join(file_name);
        if !path.exists() || !path.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        assert!(
            !contents.contains(&String::from_utf8(LEASE_TOKEN.to_vec()).expect("lease token utf8")),
            "private artifact persisted lease token: {}",
            path.display()
        );
    }
}

fn create_vm_config_json() -> String {
    json!({
        "schema_version": 1,
        "machine_config": {
            "version": 1,
            "mem_bytes": 134217728_u64,
            "vcpus": 1,
            "clock_num": 1,
            "clock_den": 1,
            "base_image_hash": "3333333333333333333333333333333333333333333333333333333333333333",
            "boot": {
                "elf": {
                    "kernel_hash": "4444444444444444444444444444444444444444444444444444444444444444",
                    "cmdline": "1000000"
                }
            },
            "epoch_len": 50000000_u64,
            "hash_epochs": "epochs_on",
            "skid_margin": 8192,
            "cpuid_table": [
                {
                    "function": 1,
                    "index": 0,
                    "flags": 0,
                    "eax": 1,
                    "ebx": 2,
                    "ecx": 3,
                    "edx": 4
                }
            ],
            "device_set": [3, 1, 2]
        },
        "entropy_seed": "2222222222222222222222222222222222222222222222222222222222222222"
    })
    .to_string()
}

struct WorkerServer {
    _dir: tempfile::TempDir,
    uds_path: PathBuf,
    handle: tokio::task::JoinHandle<()>,
}

impl WorkerServer {
    async fn start(worker: MockWorker) -> Self {
        let dir = tempfile::tempdir().expect("worker tempdir creates");
        let uds_path = dir.path().join("dh-workerd.sock");
        let listener = UnixListener::bind(&uds_path).expect("bind worker uds");
        let incoming = UnixListenerStream::new(listener);
        let handle = tokio::spawn(async move {
            Server::builder()
                .add_service(HypervisorWorkerServer::new(worker))
                .serve_with_incoming(incoming)
                .await
                .expect("mock worker server runs");
        });
        Self {
            _dir: dir,
            uds_path,
            handle,
        }
    }
}

impl Drop for WorkerServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[derive(Clone, Default)]
struct MockWorker {
    state: Arc<Mutex<MockWorkerState>>,
}

struct MockWorkerState {
    calls: Vec<&'static str>,
    active_slot: Option<dh::SlotInfo>,
    restore_hash: Option<Vec<u8>>,
    create_vm: Option<dh::CreateVmRequest>,
    take_snapshot_requests: Vec<dh::TakeSnapshotRequest>,
    inject_inputs: Vec<dh::InjectInputsRequest>,
    run_with_frame_capture_requests: Vec<dh::RunWithFrameCaptureRequest>,
    destroy_fails: bool,
    inject_status: Option<tonic::Code>,
    inject_scheduled: u32,
    run_status: Option<tonic::Code>,
    list_slots_status: Option<tonic::Code>,
    take_snapshot_status: Option<tonic::Code>,
    take_snapshot_response: dh::TakeSnapshotResponse,
    framebuffer_status: Option<tonic::Code>,
    framebuffer_response: dh::GetFramebufferResponse,
    /// Fail this many GetFramebuffer calls with FailedPrecondition first
    /// (models the window where a cancelled stream has not yet parked the
    /// slot Paused).
    framebuffer_failed_precondition_remaining: u32,
    /// Frames per RunWithFrameCapture stream before a BUDGET_REACHED terminal
    /// event; None streams until the client disconnects.
    frame_stream_frame_limit: Option<u32>,
    frame_stream_stop_reason: dh::StopReason,
    /// Close the mock transport at the frame limit without a terminal Done.
    frame_stream_omit_done: bool,
    icount: u64,
    frame_counter: u32,
}

impl Default for MockWorkerState {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            active_slot: None,
            restore_hash: None,
            create_vm: None,
            take_snapshot_requests: Vec::new(),
            inject_inputs: Vec::new(),
            run_with_frame_capture_requests: Vec::new(),
            destroy_fails: false,
            inject_status: None,
            inject_scheduled: 1,
            run_status: None,
            list_slots_status: None,
            take_snapshot_status: None,
            take_snapshot_response: MockWorker::take_snapshot_response(),
            framebuffer_status: None,
            framebuffer_response: MockWorker::framebuffer_response(12, 0),
            framebuffer_failed_precondition_remaining: 0,
            frame_stream_frame_limit: None,
            frame_stream_stop_reason: dh::StopReason::BudgetReached,
            frame_stream_omit_done: false,
            icount: 0,
            frame_counter: 12,
        }
    }
}

impl MockWorker {
    fn new() -> Self {
        Self::default()
    }

    fn calls(&self) -> Vec<&'static str> {
        self.state
            .lock()
            .expect("mock worker mutex poisoned")
            .calls
            .clone()
    }

    fn lease() -> dh::Lease {
        dh::Lease {
            slot_id: 7,
            token: LEASE_TOKEN.to_vec(),
        }
    }

    fn active_slot(icount: u64) -> dh::SlotInfo {
        dh::SlotInfo {
            slot_id: 7,
            state: dh::SlotState::PausedS as i32,
            icount,
            base: None,
            live_children: 0,
        }
    }

    fn framebuffer_response(frame_counter: u32, icount: u64) -> dh::GetFramebufferResponse {
        let width = 256_u32;
        let height = 224_u32;
        let stride = width * 4;
        let mut pixels = Vec::with_capacity((stride * height) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.push(y as u8);
                pixels.push((x ^ y) as u8);
                pixels.push(x as u8);
                pixels.push(0xaa);
            }
        }
        dh::GetFramebufferResponse {
            width,
            height,
            stride,
            format: dh::PixelFormat::Xrgb8888 as i32,
            frame_counter,
            icount,
            pixels,
        }
    }

    /// A 256x224 XRGB8888 framebuffer in the worker's `fb_lz4` wire format
    /// (lz4 block with prepended size), varying with the frame counter.
    fn captured_framebuffer(frame_counter: u32) -> (Vec<u8>, dh::FbInfo) {
        let width = 256_u32;
        let height = 224_u32;
        let stride = width * 4;
        let mut pixels = Vec::with_capacity((stride * height) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.push((y as u8).wrapping_add(frame_counter as u8));
                pixels.push((x ^ y) as u8);
                pixels.push(x as u8);
                pixels.push(0xaa);
            }
        }
        (
            lz4_flex::compress_prepend_size(&pixels),
            dh::FbInfo {
                width,
                height,
                stride,
                format: dh::PixelFormat::Xrgb8888 as i32,
                frame_counter,
            },
        )
    }

    fn take_snapshot_response() -> dh::TakeSnapshotResponse {
        let width = 4_u32;
        let height = 2_u32;
        let stride = width * 4;
        let mut pixels = Vec::with_capacity((stride * height) as usize);
        for byte in 0..(stride * height) {
            pixels.push((byte & 0xff) as u8);
        }
        dh::TakeSnapshotResponse {
            snapshot: Some(dh::SnapshotRef {
                hash: vec![0x55; 32],
            }),
            input_log_id: vec![0x66; 32],
            icount: 44,
            vns: 88,
            state_hash: Some(dh::StateHash {
                hash: vec![0x77; 32],
            }),
            dirty_pages: 1,
            machine_config_hash: vec![0x88; 32],
            determinism_class: None,
            feature_bytes: vec![9, 0, 5],
            fb_lz4: lz4_flex::compress_prepend_size(&pixels),
            fb_info: Some(dh::FbInfo {
                width,
                height,
                stride,
                format: dh::PixelFormat::Xrgb8888 as i32,
                frame_counter: 12,
            }),
            frame_counter: 12,
        }
    }
}

#[tonic::async_trait]
impl HypervisorWorker for MockWorker {
    async fn create_vm(
        &self,
        request: TonicRequest<dh::CreateVmRequest>,
    ) -> Result<TonicResponse<dh::CreateVmResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("create_vm");
        let request = request.into_inner();
        state.create_vm = Some(request);
        state.icount = 0;
        state.active_slot = Some(Self::active_slot(0));
        Ok(TonicResponse::new(dh::CreateVmResponse {
            lease: Some(Self::lease()),
            icount: 0,
        }))
    }

    async fn restore_snapshot(
        &self,
        request: TonicRequest<dh::RestoreSnapshotRequest>,
    ) -> Result<TonicResponse<dh::RestoreSnapshotResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("restore_snapshot");
        state.restore_hash = request.into_inner().snapshot.map(|snapshot| snapshot.hash);
        state.icount = 0;
        state.active_slot = Some(Self::active_slot(0));
        Ok(TonicResponse::new(dh::RestoreSnapshotResponse {
            lease: Some(Self::lease()),
            config: None,
            state_hash: None,
            frame_counter: 12,
        }))
    }

    async fn fork(
        &self,
        _request: TonicRequest<dh::ForkRequest>,
    ) -> Result<TonicResponse<dh::ForkResponse>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
    }

    async fn destroy_vm(
        &self,
        _request: TonicRequest<dh::DestroyVmRequest>,
    ) -> Result<TonicResponse<dh::DestroyVmResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("destroy_vm");
        if state.destroy_fails {
            return Err(Status::unavailable("private worker destroy failure"));
        }
        state.active_slot = None;
        Ok(TonicResponse::new(dh::DestroyVmResponse {}))
    }

    async fn inject_inputs(
        &self,
        request: TonicRequest<dh::InjectInputsRequest>,
    ) -> Result<TonicResponse<dh::InjectInputsResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("inject_inputs");
        state.inject_inputs.push(request.into_inner());
        if let Some(code) = state.inject_status {
            return Err(Status::new(
                code,
                "private inject worker failure at /private/input",
            ));
        }
        Ok(TonicResponse::new(dh::InjectInputsResponse {
            scheduled: state.inject_scheduled,
        }))
    }

    async fn run(
        &self,
        request: TonicRequest<dh::RunRequest>,
    ) -> Result<TonicResponse<dh::RunResponse>, Status> {
        let request = request.into_inner();
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("run");
        if let Some(code) = state.run_status {
            return Err(Status::new(
                code,
                "private run worker failure at /private/run",
            ));
        }
        state.icount = state.icount.saturating_add(11);
        let icount = state.icount;
        state.active_slot = Some(Self::active_slot(icount));
        // A captured frame-budget Run (the Play fast path) returns the
        // framebuffer inline; a plain Run does not.
        let (fb_lz4, fb_info) = if request.capture.is_some_and(|capture| capture.framebuffer) {
            state.frame_counter += 1;
            let (fb_lz4, fb_info) = Self::captured_framebuffer(state.frame_counter);
            (fb_lz4, Some(fb_info))
        } else {
            (Vec::new(), None)
        };
        Ok(TonicResponse::new(dh::RunResponse {
            reason: dh::StopReason::BudgetReached as i32,
            icount,
            vns: 0,
            state_hash: None,
            frames_elapsed: 1,
            sdk_event: None,
            feature_bytes: Vec::new(),
            fb_lz4,
            fb_info,
        }))
    }

    async fn pause(
        &self,
        _request: TonicRequest<dh::PauseRequest>,
    ) -> Result<TonicResponse<dh::PauseResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("pause");
        Ok(TonicResponse::new(dh::PauseResponse {
            icount: state.icount,
            vns: 0,
            state_hash: None,
        }))
    }

    async fn take_snapshot(
        &self,
        request: TonicRequest<dh::TakeSnapshotRequest>,
    ) -> Result<TonicResponse<dh::TakeSnapshotResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("take_snapshot");
        state.take_snapshot_requests.push(request.into_inner());
        if let Some(code) = state.take_snapshot_status {
            return Err(Status::new(
                code,
                "private capture worker failure at /private/capture",
            ));
        }
        state.icount = state.take_snapshot_response.icount;
        state.active_slot = Some(Self::active_slot(state.icount));
        Ok(TonicResponse::new(state.take_snapshot_response.clone()))
    }

    async fn quiesce(
        &self,
        _request: TonicRequest<dh::QuiesceRequest>,
    ) -> Result<TonicResponse<dh::QuiesceResponse>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
    }

    async fn read_guest_memory(
        &self,
        _request: TonicRequest<dh::ReadGuestMemoryRequest>,
    ) -> Result<TonicResponse<dh::ReadGuestMemoryResponse>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
    }

    async fn get_framebuffer(
        &self,
        _request: TonicRequest<dh::GetFramebufferRequest>,
    ) -> Result<TonicResponse<dh::GetFramebufferResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("get_framebuffer");
        if state.framebuffer_failed_precondition_remaining > 0 {
            state.framebuffer_failed_precondition_remaining -= 1;
            return Err(Status::failed_precondition(
                "GetFramebuffer requires Paused slot, got Running",
            ));
        }
        if let Some(code) = state.framebuffer_status {
            return Err(Status::new(
                code,
                "private framebuffer worker failure at /private/framebuffer",
            ));
        }
        Ok(TonicResponse::new(state.framebuffer_response.clone()))
    }

    type StreamGuestEventsStream = ReceiverStream<Result<dh::GuestEvent, Status>>;

    async fn stream_guest_events(
        &self,
        _request: TonicRequest<dh::StreamGuestEventsRequest>,
    ) -> Result<TonicResponse<Self::StreamGuestEventsStream>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
    }

    type VerifyReplayStream = ReceiverStream<Result<dh::VerifyReplayProgress, Status>>;

    async fn verify_replay(
        &self,
        _request: TonicRequest<dh::VerifyReplayRequest>,
    ) -> Result<TonicResponse<Self::VerifyReplayStream>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
    }

    type RunWithFrameCaptureStream = ReceiverStream<Result<dh::FrameCaptureEvent, Status>>;

    async fn run_with_frame_capture(
        &self,
        request: TonicRequest<dh::RunWithFrameCaptureRequest>,
    ) -> Result<TonicResponse<Self::RunWithFrameCaptureStream>, Status> {
        let (start_frame, frame_limit, stop_reason, omit_done) = {
            let mut state = self.state.lock().expect("mock worker mutex poisoned");
            state.calls.push("run_with_frame_capture");
            state
                .run_with_frame_capture_requests
                .push(request.into_inner());
            (
                state.frame_counter,
                state.frame_stream_frame_limit,
                state.frame_stream_stop_reason,
                state.frame_stream_omit_done,
            )
        };
        // Emit frames until the bridge drops the stream (send fails) or the
        // configured segment budget elapses (BUDGET_REACHED terminal), like
        // the real worker: the emit loop IS the backpressure, and either exit
        // parks the slot paused (the mock slot is always PausedS already).
        let worker_state = self.state.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tokio::spawn(async move {
            let mut frame_counter = start_frame;
            let mut sent = 0_u32;
            loop {
                if frame_limit.is_some_and(|limit| sent >= limit) {
                    if omit_done {
                        break;
                    }
                    let done = dh::FrameCaptureEvent {
                        msg: Some(dh::frame_capture_event::Msg::Done(dh::RunResponse {
                            reason: stop_reason as i32,
                            icount: u64::from(frame_counter) * 100,
                            vns: 0,
                            state_hash: None,
                            frames_elapsed: u64::from(sent),
                            sdk_event: None,
                            feature_bytes: Vec::new(),
                            fb_lz4: Vec::new(),
                            fb_info: None,
                        })),
                    };
                    let _ = tx.send(Ok(done)).await;
                    break;
                }
                frame_counter += 1;
                let (fb_lz4, fb_info) = MockWorker::captured_framebuffer(frame_counter);
                let event = dh::FrameCaptureEvent {
                    msg: Some(dh::frame_capture_event::Msg::Frame(dh::CapturedFrame {
                        frame_index: frame_counter,
                        icount: u64::from(frame_counter) * 100,
                        fb_lz4,
                        fb_info: Some(fb_info),
                    })),
                };
                if tx.send(Ok(event)).await.is_err() {
                    break;
                }
                sent += 1;
                let mut state = worker_state.lock().expect("mock worker mutex poisoned");
                state.frame_counter = frame_counter;
                state.framebuffer_response =
                    MockWorker::framebuffer_response(frame_counter, u64::from(frame_counter) * 100);
            }
        });
        Ok(TonicResponse::new(ReceiverStream::new(rx)))
    }

    async fn get_worker_info(
        &self,
        _request: TonicRequest<dh::GetWorkerInfoRequest>,
    ) -> Result<TonicResponse<dh::GetWorkerInfoResponse>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
    }

    async fn list_slots(
        &self,
        _request: TonicRequest<dh::ListSlotsRequest>,
    ) -> Result<TonicResponse<dh::ListSlotsResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("list_slots");
        if let Some(code) = state.list_slots_status {
            return Err(Status::new(
                code,
                "private list slots worker failure at /private/status",
            ));
        }
        Ok(TonicResponse::new(dh::ListSlotsResponse {
            slots: state.active_slot.clone().into_iter().collect(),
        }))
    }

    type WatchSlotsStream = ReceiverStream<Result<dh::SlotEvent, Status>>;

    async fn watch_slots(
        &self,
        _request: TonicRequest<dh::WatchSlotsRequest>,
    ) -> Result<TonicResponse<Self::WatchSlotsStream>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("watch_slots");
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(TonicResponse::new(ReceiverStream::new(rx)))
    }
}
