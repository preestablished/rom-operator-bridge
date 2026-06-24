use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{ORIGIN, SET_COOKIE},
    },
};
use dh_proto::v1 as dh;
use dh_proto::v1::hypervisor_worker_server::{HypervisorWorker, HypervisorWorkerServer};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::ALLOWED_ORIGIN,
    config::{ENV_BACKEND_MODE, ServiceConfig},
    private_config::{
        ENV_CAPTURE_SPEC_REF, ENV_CREATE_VM_CONFIG_REF, ENV_HYPERVISOR_ENDPOINT,
        ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_REAL_SNAPSHOT_REF,
        ENV_REFERENCE_WORKLOAD_CHECKOUT, ENV_SESSION_SECRET, ENV_WORKLOAD_IMAGE_REF,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::net::UnixListener;
use tokio_stream::wrappers::{ReceiverStream, UnixListenerStream};
use tonic::{Request as TonicRequest, Response as TonicResponse, Status, transport::Server};
use tower::{Service, ServiceExt};

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
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
                    "operator_credential": GOOD_CREDENTIAL,
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
            Body::from(start_body("real")),
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
    assert_eq!(start_body["capabilities"]["input"], false);
    assert_eq!(start_body["capabilities"]["preview"], false);
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
    assert!(calls.iter().any(|call| *call == "pause"));
    assert!(calls.iter().any(|call| *call == "run"));
    assert!(calls.iter().any(|call| *call == "list_slots"));
    assert!(calls.iter().any(|call| *call == "destroy_vm"));
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

fn real_config(private_root: &Path, reference_checkout: &PathBuf) -> ServiceConfig {
    real_config_with_start(
        private_root,
        reference_checkout,
        "unix:///run/dh/grpc.sock",
        Some((ENV_REAL_SNAPSHOT_REF, SNAPSHOT_REF.to_string())),
    )
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
        (
            ENV_OPERATOR_CREDENTIAL.to_string(),
            GOOD_CREDENTIAL.to_string(),
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
    json!({
        "schema_version": 1,
        "operator_credential": GOOD_CREDENTIAL,
        "backend_mode": backend_mode,
        "requested_capabilities": ["input", "preview", "capture"]
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

async fn body_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("body reads");
    serde_json::from_slice(&body).expect("body is json")
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

fn assert_private_artifacts_do_not_contain_lease(private_root: &Path) {
    let run_dir = private_root.join("runs").join("real-run-0000");
    for file_name in ["run-manifest.json", "bridge-events.jsonl"] {
        let path = run_dir.join(file_name);
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

#[derive(Default)]
struct MockWorkerState {
    calls: Vec<&'static str>,
    active_slot: Option<dh::SlotInfo>,
    restore_hash: Option<Vec<u8>>,
    create_vm: Option<dh::CreateVmRequest>,
    destroy_fails: bool,
    icount: u64,
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
        _request: TonicRequest<dh::InjectInputsRequest>,
    ) -> Result<TonicResponse<dh::InjectInputsResponse>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
    }

    async fn run(
        &self,
        _request: TonicRequest<dh::RunRequest>,
    ) -> Result<TonicResponse<dh::RunResponse>, Status> {
        let mut state = self.state.lock().expect("mock worker mutex poisoned");
        state.calls.push("run");
        state.icount = state.icount.saturating_add(11);
        let icount = state.icount;
        state.active_slot = Some(Self::active_slot(icount));
        Ok(TonicResponse::new(dh::RunResponse {
            reason: dh::StopReason::BudgetReached as i32,
            icount,
            vns: 0,
            state_hash: None,
            frames_elapsed: 1,
            sdk_event: None,
            feature_bytes: Vec::new(),
            fb_lz4: Vec::new(),
            fb_info: None,
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
        _request: TonicRequest<dh::TakeSnapshotRequest>,
    ) -> Result<TonicResponse<dh::TakeSnapshotResponse>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
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
        Err(Status::unimplemented("not used by bridge bp8"))
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
        _request: TonicRequest<dh::RunWithFrameCaptureRequest>,
    ) -> Result<TonicResponse<Self::RunWithFrameCaptureStream>, Status> {
        Err(Status::unimplemented("not used by bridge bp8"))
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
