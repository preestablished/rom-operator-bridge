use rom_operator_bridge_service::{
    artifacts::{
        ARTIFACT_SCHEMA_VERSION, ArtifactError, BridgeEventRow, CaptureFeatureBytesIndex,
        CaptureFramebufferIndex, CaptureIndexRow, CapturePayloadKind, CaptureSummary,
        InputRejectionRow, LabelDraft, LabelDraftFile, PrivateArtifactStore, RecentCapturesFile,
        RunManifest, ValidationRunRow,
    },
    backend::BackendMode,
    private_config::{ENV_PRIVATE_ROOT, ENV_SESSION_SECRET, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE},
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
#[test]
fn writes_schema_versioned_run_manifest_with_private_modes() {
    let (_workspace, config, private_root) = private_config();
    let store = PrivateArtifactStore::new(config.private_config());

    let artifact = store
        .write_run_manifest(&RunManifest::new(
            "run-001",
            "2026-06-23T00:00:00Z",
            BackendMode::Synthetic,
            1,
        ))
        .expect("manifest writes");

    assert_eq!(
        artifact.relative_path(),
        Path::new("runs/run-001/run-manifest.json")
    );
    assert_eq!(artifact.to_string(), "[private artifact]");
    assert!(!format!("{artifact:?}").contains(&private_root.display().to_string()));
    assert!(!format!("{artifact:?}").contains("run-001"));
    assert!(!format!("{artifact:?}").contains("run-manifest.json"));

    let private_path = private_root.join(artifact.relative_path());
    assert_eq!(mode(&private_path), PRIVATE_FILE_MODE);
    assert_eq!(mode(&private_root.join("runs")), PRIVATE_DIR_MODE);
    assert_eq!(mode(&private_root.join("runs/run-001")), PRIVATE_DIR_MODE);

    let json: Value =
        serde_json::from_str(&fs::read_to_string(private_path).expect("manifest reads"))
            .expect("manifest parses");
    assert_eq!(json["schema_version"], ARTIFACT_SCHEMA_VERSION);
    assert_eq!(json["run_id"], "run-001");
    assert_eq!(json["backend_mode"], "synthetic");
}

#[cfg(unix)]
#[test]
fn appends_bridge_events_and_input_rejections_as_jsonl() {
    let (_workspace, config, private_root) = private_config();
    let store = PrivateArtifactStore::new(config.private_config());

    let event_ref = store
        .append_bridge_event(
            "run-001",
            &BridgeEventRow::new(
                "run-001",
                1,
                "2026-06-23T00:00:01Z",
                "session_started",
                "session started",
            ),
        )
        .expect("first event appends");
    store
        .append_bridge_event(
            "run-001",
            &BridgeEventRow::new(
                "run-001",
                2,
                "2026-06-23T00:00:02Z",
                "session_paused",
                "session paused",
            ),
        )
        .expect("second event appends");

    let event_lines = read_lines(&artifact_path(&private_root, &event_ref));
    assert_eq!(event_lines.len(), 2);
    let first_event: BridgeEventRow =
        serde_json::from_str(&event_lines[0]).expect("first event parses");
    let second_event: BridgeEventRow =
        serde_json::from_str(&event_lines[1]).expect("second event parses");
    assert_eq!(first_event.server_seq, 1);
    assert_eq!(second_event.server_seq, 2);

    let rejection_ref = store
        .append_input_rejection(
            "run-001",
            &InputRejectionRow::new(
                "run-001",
                7,
                "2026-06-23T00:00:03Z",
                "frame_stale",
                "Input rejected.",
            ),
        )
        .expect("input rejection appends");

    let rejection_lines = read_lines(&artifact_path(&private_root, &rejection_ref));
    assert_eq!(rejection_lines.len(), 1);
    let rejection: InputRejectionRow =
        serde_json::from_str(&rejection_lines[0]).expect("rejection parses");
    assert_eq!(rejection.client_seq, 7);
    assert_eq!(rejection.reason_code, "frame_stale");
    assert_eq!(
        mode(&artifact_path(&private_root, &event_ref)),
        PRIVATE_FILE_MODE
    );
    assert_eq!(
        mode(&artifact_path(&private_root, &rejection_ref)),
        PRIVATE_FILE_MODE
    );
}

#[cfg(unix)]
#[test]
fn writes_atomic_snapshot_artifacts_and_validation_rows() {
    let (_workspace, config, private_root) = private_config();
    let store = PrivateArtifactStore::new(config.private_config());

    store
        .write_recent_captures(&RecentCapturesFile::new(vec![CaptureSummary::new(
            "capture-001",
            "2026-06-23T00:00:04Z",
            "capturing",
            false,
        )]))
        .expect("initial recent captures writes");
    let recent_ref = store
        .write_recent_captures(&RecentCapturesFile::new(vec![CaptureSummary::new(
            "capture-001",
            "2026-06-23T00:00:04Z",
            "completed",
            true,
        )]))
        .expect("replacement recent captures writes");

    let recent_path = artifact_path(&private_root, &recent_ref);
    let recent_json: RecentCapturesFile =
        serde_json::from_str(&fs::read_to_string(&recent_path).unwrap())
            .expect("recent captures parses");
    assert_eq!(recent_json.schema_version, ARTIFACT_SCHEMA_VERSION);
    assert_eq!(recent_json.captures[0].status, "completed");
    assert_eq!(mode(&recent_path), PRIVATE_FILE_MODE);
    assert_no_temporary_files(&private_root.join("captures"));

    let label_ref = store
        .write_label_draft(&LabelDraftFile::new(
            "capture-001",
            "2026-06-23T00:00:05Z",
            vec![LabelDraft::new("first_boss", true)],
            Some("private operator note".to_string()),
        ))
        .expect("label draft writes");
    assert_eq!(
        mode(&artifact_path(&private_root, &label_ref)),
        PRIVATE_FILE_MODE
    );

    let validation_ref = store
        .append_validation_run(&ValidationRunRow::new(
            "validation-001",
            "2026-06-23T00:00:06Z",
            "bundle_check",
            "failed",
            "validation failed",
        ))
        .expect("validation row appends");
    store
        .append_validation_run(&ValidationRunRow::new(
            "validation-002",
            "2026-06-23T00:00:07Z",
            "redaction_scan",
            "passed",
            "validation passed",
        ))
        .expect("second validation row appends");

    assert_eq!(
        read_lines(&artifact_path(&private_root, &validation_ref)).len(),
        2
    );
}

#[test]
fn artifact_writers_reject_bad_schema_versions_and_path_ids() {
    let (_workspace, config, _private_root) = private_config();
    let store = PrivateArtifactStore::new(config.private_config());

    let mut manifest =
        RunManifest::new("run-001", "2026-06-23T00:00:00Z", BackendMode::Synthetic, 1);
    manifest.schema_version = ARTIFACT_SCHEMA_VERSION + 1;
    assert!(matches!(
        store.write_run_manifest(&manifest),
        Err(ArtifactError::UnsupportedSchemaVersion { .. })
    ));

    assert!(matches!(
        store.write_run_manifest(&RunManifest::new(
            "../run-001",
            "2026-06-23T00:00:00Z",
            BackendMode::Synthetic,
            1,
        )),
        Err(ArtifactError::InvalidIdentifier {
            field: "run_id",
            ..
        })
    ));

    assert!(matches!(
        store.write_label_draft(&LabelDraftFile::new(
            "capture/001",
            "2026-06-23T00:00:05Z",
            vec![],
            None,
        )),
        Err(ArtifactError::InvalidIdentifier {
            field: "capture_id",
            ..
        })
    ));

    assert!(matches!(
        store.write_recent_captures(&RecentCapturesFile::new(vec![CaptureSummary::new(
            "../capture-001",
            "2026-06-23T00:00:04Z",
            "completed",
            true,
        )])),
        Err(ArtifactError::InvalidIdentifier {
            field: "capture_id",
            ..
        })
    ));

    assert!(matches!(
        store.append_validation_run(&ValidationRunRow::new(
            "validation/001",
            "2026-06-23T00:00:06Z",
            "bundle_check",
            "failed",
            "validation failed",
        )),
        Err(ArtifactError::InvalidIdentifier {
            field: "validation_id",
            ..
        })
    ));

    assert!(matches!(
        store.append_bridge_event(
            "run-001",
            &BridgeEventRow::new(
                "run-002",
                1,
                "2026-06-23T00:00:01Z",
                "session_started",
                "session started",
            ),
        ),
        Err(ArtifactError::MismatchedIdentifier {
            field: "run_id",
            ..
        })
    ));
}

#[cfg(unix)]
#[test]
fn writes_capture_payloads_and_appends_capture_index_rows() {
    let (_workspace, config, private_root) = private_config();
    let store = PrivateArtifactStore::new(config.private_config());

    let feature_payload = store
        .write_capture_payload(
            "real-capture-0000",
            CapturePayloadKind::FeatureBytes,
            "feature-bytes.bin",
            &[1, 0, 2, 3],
        )
        .expect("feature payload writes");
    let framebuffer_payload = store
        .write_capture_payload(
            "real-capture-0000",
            CapturePayloadKind::Framebuffer,
            "framebuffer.fb_lz4",
            &[4, 5, 6, 7],
        )
        .expect("framebuffer payload writes");

    assert_eq!(feature_payload.len, 4);
    assert!(feature_payload.blake3.starts_with("blake3:"));
    assert_eq!(
        mode(&artifact_path(&private_root, &feature_payload.artifact_ref)),
        PRIVATE_FILE_MODE
    );
    assert_eq!(
        mode(&private_root.join("artifacts/feature-bytes")),
        PRIVATE_DIR_MODE
    );
    assert!(!format!("{feature_payload:?}").contains("real-capture-0000"));

    let row = capture_index_row(
        "real-capture-0000",
        feature_payload.artifact_ref.artifact_ref(),
        feature_payload.blake3,
        framebuffer_payload.artifact_ref.artifact_ref(),
        framebuffer_payload.blake3,
    );
    let index_ref = store
        .append_capture_index_row(&row)
        .expect("capture index row appends");
    let mut second = row.clone();
    second.capture_id = "real-capture-0001".to_string();
    store
        .append_capture_index_row(&second)
        .expect("second capture index row appends");

    let index_path = artifact_path(&private_root, &index_ref);
    assert_eq!(mode(&index_path), PRIVATE_FILE_MODE);
    let lines = read_lines(&index_path);
    assert_eq!(lines.len(), 2);
    let parsed: Value = serde_json::from_str(&lines[0]).expect("index row parses");
    assert_eq!(parsed["capture_id"], "real-capture-0000");
    assert_eq!(parsed["capture_source"], "real_take_snapshot");
    assert!(parsed["feature_bytes"]["bytes"].is_null());
    assert!(parsed["framebuffer"]["bytes"].is_null());
}

#[test]
fn capture_payloads_and_index_rows_reject_invalid_inputs() {
    let (_workspace, config, _private_root) = private_config();
    let store = PrivateArtifactStore::new(config.private_config());

    assert!(matches!(
        store.write_capture_payload(
            "../capture",
            CapturePayloadKind::FeatureBytes,
            "feature-bytes.bin",
            &[1],
        ),
        Err(ArtifactError::InvalidIdentifier {
            field: "capture_id",
            ..
        })
    ));
    assert!(matches!(
        store.write_capture_payload(
            "real-capture-0000",
            CapturePayloadKind::FeatureBytes,
            "../feature-bytes.bin",
            &[1],
        ),
        Err(ArtifactError::InvalidIdentifier {
            field: "payload_name",
            ..
        })
    ));
    store
        .write_capture_payload(
            "real-capture-immutable",
            CapturePayloadKind::FeatureBytes,
            "feature-bytes.bin",
            &[1],
        )
        .expect("first immutable payload writes");
    assert!(matches!(
        store.write_capture_payload(
            "real-capture-immutable",
            CapturePayloadKind::FeatureBytes,
            "feature-bytes.bin",
            &[2],
        ),
        Err(ArtifactError::ExistingCapturePayload)
    ));

    let mut row = capture_index_row(
        "real-capture-0000",
        "artifact:feature".to_string(),
        "blake3:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        "artifact:framebuffer".to_string(),
        "blake3:1111111111111111111111111111111111111111111111111111111111111111".to_string(),
    );
    row.decoded_values.clear();
    assert!(matches!(
        store.append_capture_index_row(&row),
        Err(ArtifactError::InvalidCaptureIndexRow)
    ));
}

fn capture_index_row(
    capture_id: &str,
    feature_ref: String,
    feature_hash: String,
    framebuffer_ref: String,
    framebuffer_hash: String,
) -> CaptureIndexRow {
    CaptureIndexRow {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        capture_id: capture_id.to_string(),
        node_ref: format!("run:real-run-0000:capture:{capture_id}"),
        capture_source: "real_take_snapshot".to_string(),
        frame_counter: 12,
        layout_hash: "blake3:2222222222222222222222222222222222222222222222222222222222222222"
            .to_string(),
        feature_bytes: CaptureFeatureBytesIndex {
            artifact_ref: feature_ref,
            len: 4,
            blake3: feature_hash,
        },
        decoded_order: vec!["frame_ctr".to_string(), "frame_low".to_string()],
        decoded_values: vec![1, 2],
        framebuffer: CaptureFramebufferIndex {
            artifact_ref: framebuffer_ref,
            encoding: "fb_lz4".to_string(),
            width: 256,
            height: 224,
            stride: 1024,
            pixel_format: "xrgb8888".to_string(),
            uncompressed_len: 229376,
            blake3: framebuffer_hash,
        },
    }
}

fn private_config() -> (
    tempfile::TempDir,
    rom_operator_bridge_service::config::ServiceConfig,
    PathBuf,
) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config = rom_operator_bridge_service::config::ServiceConfig::from_pairs([
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (
            ENV_SESSION_SECRET.to_string(),
            "session-secret-from-test-source-32-bytes".to_string(),
        ),
    ])
    .expect("private config loads");

    (workspace, config, private_root)
}

fn read_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .expect("jsonl reads")
        .lines()
        .map(str::to_string)
        .collect()
}

fn artifact_path(
    private_root: &Path,
    artifact: &rom_operator_bridge_service::artifacts::PrivateArtifactRef,
) -> PathBuf {
    private_root.join(artifact.relative_path())
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    fs::metadata(path)
        .expect("metadata reads")
        .permissions()
        .mode()
        & 0o777
}

fn assert_no_temporary_files(path: &Path) {
    for entry in fs::read_dir(path).expect("directory reads") {
        let entry = entry.expect("entry reads");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        assert!(
            !name.starts_with(".tmp-"),
            "temporary artifact file remained: {name}"
        );
    }
}
