use rom_operator_bridge_service::sanitization::{PublicSanitizer, SanitizationError};
use serde_json::json;

#[test]
fn rejects_absolute_private_paths() {
    let sanitizer = PublicSanitizer::new();

    assert_eq!(
        sanitizer.inspect_text("failed while opening /home/infra-admin/private/run.dat"),
        Err(SanitizationError::PrivatePath)
    );
    assert_eq!(
        sanitizer.inspect_text("failed while opening C:\\Users\\operator\\rom.sfc"),
        Err(SanitizationError::PrivatePath)
    );
    assert_eq!(
        sanitizer.inspect_text("failed while opening /root/.ssh/id_rsa"),
        Err(SanitizationError::PrivatePath)
    );
    assert_eq!(
        sanitizer.inspect_text("failed while opening /run/secrets/operator-token"),
        Err(SanitizationError::PrivatePath)
    );
    assert_eq!(
        sanitizer.inspect_text("failed while opening /dev/shm/private-capture.bin"),
        Err(SanitizationError::PrivatePath)
    );
}

#[test]
fn rejects_configured_private_roots() {
    let sanitizer = PublicSanitizer::new().with_private_root("/corpus/operator-a/private-rom-root");

    assert_eq!(
        sanitizer.inspect_text("capture stored below /corpus/operator-a/private-rom-root/captures"),
        Err(SanitizationError::ConfiguredPrivateRoot)
    );
}

#[test]
fn rejects_command_output_and_stack_traces() {
    let sanitizer = PublicSanitizer::new();

    assert_eq!(
        sanitizer.inspect_text(
            "Command failed with exit status 101\nstderr: panicked at src/main.rs:12"
        ),
        Err(SanitizationError::CommandOutput)
    );
}

#[test]
fn rejects_feature_bytes_and_raw_payload_snippets() {
    let sanitizer = PublicSanitizer::new();

    assert!(
        sanitizer
            .inspect_json(&json!({
            "schema_version": 1,
            "feature_bytes": "00 ff 9a 12",
            }))
            .is_err()
    );
    assert_eq!(
        sanitizer.inspect_text("raw_payload: 7b226672616d65223a3132337d"),
        Err(SanitizationError::RawPayloadSnippet {
            pattern: "raw payload"
        })
    );
}

#[test]
fn rejects_private_paths_and_literals_in_json_keys() {
    let sanitizer = PublicSanitizer::new()
        .with_private_root("/corpus/operator-a/private-rom-root")
        .with_forbidden_literal("SECRET_ROM_NAME");

    assert_eq!(
        sanitizer.inspect_json(&json!({
            "/home/operator/private/run.dat": "failed"
        })),
        Err(SanitizationError::PrivatePath)
    );
    assert_eq!(
        sanitizer.inspect_json(&json!({
            "/corpus/operator-a/private-rom-root/capture": "failed"
        })),
        Err(SanitizationError::ConfiguredPrivateRoot)
    );
    assert_eq!(
        sanitizer.inspect_json(&json!({
            "SECRET_ROM_NAME": "failed"
        })),
        Err(SanitizationError::ForbiddenLiteral)
    );
}

#[test]
fn rejects_sensitive_field_name_variants() {
    let sanitizer = PublicSanitizer::new();
    let cases = [
        "featureBytes",
        "rawPayload",
        "validationReport",
        "operatorCredential",
        "workerLeaseToken",
        "privatePath",
        "artifactRef",
        "stderr_lines",
        "stdout_text",
        "command_output",
        "private_root_path",
        "private_root_ref",
    ];

    for field in cases {
        assert_eq!(
            sanitizer.inspect_json(&json!({ field: "redacted" })),
            Err(SanitizationError::ForbiddenField {
                field: field.to_string()
            })
        );
    }
}

#[test]
fn rejects_validation_report_excerpts() {
    let sanitizer = PublicSanitizer::new();

    assert_eq!(
        sanitizer.inspect_text("phase4-bundle-check failed: validation report line 17"),
        Err(SanitizationError::ValidationReportExcerpt)
    );
}

#[test]
fn rejects_operator_forbidden_literals() {
    let sanitizer = PublicSanitizer::new().with_forbidden_literal("SECRET_ROM_NAME");

    assert_eq!(
        sanitizer.inspect_text("Synthetic status accidentally mentions SECRET_ROM_NAME"),
        Err(SanitizationError::ForbiddenLiteral)
    );
}

#[test]
fn allows_browser_safe_public_event_and_capture_metadata() {
    let sanitizer = PublicSanitizer::new()
        .with_private_root("/corpus/operator-a/private-rom-root")
        .with_forbidden_literal("SECRET_ROM_NAME");

    sanitizer
        .inspect_event(&json!({
            "schema_version": 1,
            "type": "capture_updated",
            "session_id": "synthetic-session-1",
            "server_seq": 42,
            "payload": {
                "job_id": "synthetic-job-1",
                "status": "completed",
                "capture_id": "synthetic-capture-1",
                "preview_image_url": "/api/capture/synthetic-capture-1/preview",
                "preview_hash": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            }
        }))
        .expect("browser-safe public event is accepted");
}

#[test]
fn allows_browser_safe_capture_metadata() {
    PublicSanitizer::new()
        .inspect_capture_metadata(&json!({
            "capture_id": "synthetic-capture-1",
            "status": "completed",
            "has_preview": true,
            "preview_image_url": "/api/capture/synthetic-capture-1/preview"
        }))
        .expect("browser-safe capture metadata is accepted");
}

#[test]
fn rejects_private_validation_summary_fields() {
    let sanitizer = PublicSanitizer::new();

    assert_eq!(
        sanitizer.inspect_validation_summary(&json!({
            "status": "failed",
            "validation_report": "phase4-bundle-check failed at line 17"
        })),
        Err(SanitizationError::ForbiddenField {
            field: "validation_report".to_string()
        })
    );
}

#[test]
fn allows_public_runtime_route_paths() {
    PublicSanitizer::new()
        .inspect_text("Runtime route /api/run/status is unavailable.")
        .expect("public runtime API route path is accepted");
}

#[test]
fn sanitizes_error_message_to_safe_fallback() {
    let sanitizer = PublicSanitizer::new().with_private_root("/private/root");

    assert_eq!(
        sanitizer.sanitize_error_message("failed to read /private/root/capture.bin"),
        "Request could not be completed."
    );
    assert_eq!(
        sanitizer.sanitize_error_message("Backend unavailable."),
        "Backend unavailable."
    );
}

#[test]
fn sanitizes_auth_and_input_rejection_messages() {
    let sanitizer = PublicSanitizer::new().with_forbidden_literal("SECRET_ROM_NAME");

    assert_eq!(
        sanitizer.sanitize_auth_rejection_message("bad credential for SECRET_ROM_NAME"),
        "Authentication rejected."
    );
    assert_eq!(
        sanitizer.sanitize_input_rejection_message("stderr: stale frame panic"),
        "Input rejected."
    );
}

#[test]
fn returns_empty_public_details() {
    assert_eq!(PublicSanitizer::new().empty_public_details(), json!({}));
}
