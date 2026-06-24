use axum::{
    body::{Body, to_bytes},
    http::{
        Method, Request, StatusCode,
        header::{ORIGIN, SET_COOKIE},
    },
};
use rom_operator_bridge_service::{
    api::{AppState, router},
    auth::ALLOWED_ORIGIN,
    config::{ENV_BACKEND_MODE, ServiceConfig},
    private_config::{
        ENV_CAPTURE_SPEC_REF, ENV_HYPERVISOR_ENDPOINT, ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT,
        ENV_REAL_SNAPSHOT_REF, ENV_REFERENCE_WORKLOAD_CHECKOUT, ENV_SESSION_SECRET,
        ENV_WORKLOAD_IMAGE_REF,
    },
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tower::ServiceExt;

const GOOD_CREDENTIAL: &str = "operator-credential-from-test-source";
const SESSION_SECRET: &str = "session-secret-from-test-source-32-bytes";
const WORKLOAD_IMAGE_REF: &str = "private-workload-image-ref-from-test";
const CAPTURE_SPEC_REF: &str = "private-capture-spec-ref-from-test";
const SNAPSHOT_REF: &str = "private-snapshot-ref-from-test";

#[tokio::test]
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

fn real_config(private_root: &Path, reference_checkout: &PathBuf) -> ServiceConfig {
    ServiceConfig::from_pairs([
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
        (
            ENV_HYPERVISOR_ENDPOINT.to_string(),
            "unix:///run/dh/grpc.sock".to_string(),
        ),
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
        (ENV_REAL_SNAPSHOT_REF.to_string(), SNAPSHOT_REF.to_string()),
    ])
    .expect("real private config loads")
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
