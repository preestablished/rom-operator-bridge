use rom_operator_bridge_service::{
    config::{ENV_BACKEND_MODE, ENV_BIND_ADDR, ServiceConfig},
    lease_store::{AllocationKind, LeaseIntent, LeaseStore},
    private_config::{ENV_PRIVATE_ROOT, ENV_SESSION_SECRET, PRIVATE_FILE_MODE},
};

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::{Command, Output};

fn store() -> (
    tempfile::TempDir,
    rom_operator_bridge_service::private_config::BridgePrivateConfig,
    LeaseStore,
) {
    let workspace = tempfile::tempdir().expect("tempdir");
    let root = workspace.path().join("private");
    let config = ServiceConfig::from_pairs([
        (ENV_BIND_ADDR, "127.0.0.1:0".to_string()),
        (ENV_PRIVATE_ROOT, root.display().to_string()),
        (
            ENV_SESSION_SECRET,
            "lease-store-test-secret-value-32-bytes".to_string(),
        ),
    ])
    .expect("private config");
    let private = config.private_config().clone();
    (workspace, private.clone(), LeaseStore::new(private))
}

#[test]
fn intent_promotes_to_private_lease_and_round_trips() {
    let (workspace, _config, store) = store();
    let mut intent = LeaseIntent::new(
        "session".into(),
        "run".into(),
        "create_vm_config".into(),
        AllocationKind::CreateVm,
    );
    store.write_intent(&intent).expect("intent durable");
    intent.run_id = "run2".into();
    store
        .write_intent(&intent)
        .expect("atomic intent replacement");
    let record = intent.promote(&dh_proto::v1::Lease {
        slot_id: 7,
        token: vec![0xab, 0xcd],
    });
    store.write_lease(&record).expect("lease durable");
    let loaded = store.load().expect("load");
    assert_eq!(loaded.intents.len(), 1);
    assert_eq!(loaded.intents[0].run_id, "run2");
    assert_eq!(loaded.leases.len(), 1);
    assert_eq!(
        loaded.leases[0].lease().expect("decode").token,
        vec![0xab, 0xcd]
    );
    let root = workspace.path().join("private");
    let mode = std::fs::metadata(
        root.join("leases/active")
            .join(format!("{}.json", intent.operation_id)),
    )
    .expect("metadata")
    .permissions()
    .mode()
        & 0o777;
    assert_eq!(mode, PRIVATE_FILE_MODE);
    store
        .remove_lease(&intent.operation_id)
        .expect("durable removal");
    store
        .remove_lease(&intent.operation_id)
        .expect("idempotent removal");
}

#[test]
fn malformed_and_unknown_records_fail_closed() {
    let (workspace, _config, store) = store();
    let root = workspace.path().join("private");
    let invalid = root.join("leases/intents/not-a-uuid.json");
    std::fs::write(&invalid, b"{}").expect("seed invalid");
    std::fs::set_permissions(&invalid, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .expect("private mode");
    assert_eq!(store.load().expect("load").invalid, 1);
    assert!(
        store
            .clear_dangling_intents(&["00000000-0000-4000-8000-000000000000".into()])
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn symlinked_record_is_rejected() {
    let (workspace, _config, store) = store();
    let root = workspace.path().join("private");
    let target = workspace.path().join("outside");
    std::fs::write(&target, b"{}").expect("outside");
    symlink(
        &target,
        root.join("leases/intents/00000000-0000-4000-8000-000000000000.json"),
    )
    .expect("symlink");
    assert!(store.load().is_err());
}

#[test]
fn selected_dangling_intent_acknowledgement_refuses_active_records() {
    let (_workspace, _config, store) = store();
    let intent = LeaseIntent::new(
        "session".into(),
        "run".into(),
        "create_vm_config".into(),
        AllocationKind::CreateVm,
    );
    store.write_intent(&intent).expect("intent");
    store
        .write_lease(&intent.promote(&dh_proto::v1::Lease {
            slot_id: 7,
            token: vec![1],
        }))
        .expect("lease");
    assert!(
        store
            .clear_dangling_intents(std::slice::from_ref(&intent.operation_id))
            .is_err()
    );
    store
        .remove_lease(&intent.operation_id)
        .expect("remove lease");
    assert_eq!(
        store
            .clear_dangling_intents(std::slice::from_ref(&intent.operation_id))
            .expect("selected clear"),
        1
    );
    assert!(store.load().expect("load").intents.is_empty());
}

#[test]
fn operator_acknowledgement_rejects_duplicates_and_runtime_lock_contention() {
    let (_workspace, config, store) = store();
    let intent = LeaseIntent::new(
        "session".into(),
        "run".into(),
        "create_vm_config".into(),
        AllocationKind::CreateVm,
    );
    store.write_intent(&intent).expect("intent");
    assert!(
        store
            .clear_dangling_intents(&[intent.operation_id.clone(), intent.operation_id.clone()])
            .is_err()
    );
    let _service_lock = config.acquire_bridge_runtime_lock().expect("service lock");
    assert!(config.acquire_bridge_runtime_lock().is_err());
}

#[test]
fn unknown_schema_and_malformed_token_are_retained_as_invalid_evidence() {
    let (workspace, _config, store) = store();
    let root = workspace.path().join("private");
    let intent_id = "00000000-0000-4000-8000-000000000001";
    let lease_id = "00000000-0000-4000-8000-000000000002";
    let intent_path = root
        .join("leases/intents")
        .join(format!("{intent_id}.json"));
    let lease_path = root.join("leases/active").join(format!("{lease_id}.json"));
    std::fs::write(
        &intent_path,
        format!(r#"{{"schema_version":2,"operation_id":"{intent_id}","session_id":"s","run_id":"r","source":"x","created_at":"0","allocation_kind":"create_vm"}}"#),
    )
    .expect("unknown schema");
    std::fs::write(
        &lease_path,
        format!(r#"{{"schema_version":1,"operation_id":"{lease_id}","session_id":"s","run_id":"r","source":"x","created_at":"0","allocation_kind":"create_vm","slot_id":0,"token_hex":"ABCZ","lease_recorded_at":"0"}}"#),
    )
    .expect("bad token");
    for path in [&intent_path, &lease_path] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
            .expect("private mode");
    }
    let loaded = store.load().expect("invalid records load fail-closed");
    assert_eq!(loaded.invalid, 2);
    assert!(intent_path.exists());
    assert!(lease_path.exists());
}

#[test]
fn validated_atomic_temporary_files_are_safely_ignored_after_crash() {
    let (workspace, _config, store) = store();
    let root = workspace.path().join("private");
    let id = "00000000-0000-4000-8000-000000000003";
    let temp = root
        .join("leases/active")
        .join(format!(".tmp-{id}.json-123-456"));
    std::fs::write(&temp, b"partial").expect("crash temp");
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .expect("private temp mode");
    let loaded = store.load().expect("recognized temp is ignored");
    assert_eq!(loaded.invalid, 0);
    assert!(temp.exists(), "evidence is left for safe later cleanup");
}

fn run_clear_command(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rom-operator-bridge-service"))
        .env_clear()
        .env(ENV_BIND_ADDR, "127.0.0.1:0")
        .env(ENV_BACKEND_MODE, "synthetic")
        .env(ENV_PRIVATE_ROOT, root)
        .env(ENV_SESSION_SECRET, "lease-store-test-secret-value-32-bytes")
        .args(args)
        .output()
        .expect("operator command runs")
}

#[test]
fn operator_command_requires_confirmations_and_a_stopped_bridge() {
    let (workspace, config, store) = store();
    let root = workspace.path().join("private");
    let intent = LeaseIntent::new(
        "session".into(),
        "run".into(),
        "create_vm_config".into(),
        AllocationKind::CreateVm,
    );
    store.write_intent(&intent).expect("intent");
    let missing = run_clear_command(&root, &["clear-dangling-intents", &intent.operation_id]);
    assert!(!missing.status.success());
    assert_eq!(store.load().expect("still retained").intents.len(), 1);

    let service_lock = config.acquire_bridge_runtime_lock().expect("service lock");
    let running = run_clear_command(
        &root,
        &[
            "clear-dangling-intents",
            "--bridge-stopped",
            "--worker-restarted",
            "--full-capacity",
            &intent.operation_id,
        ],
    );
    assert!(!running.status.success());
    drop(service_lock);

    let cleared = run_clear_command(
        &root,
        &[
            "clear-dangling-intents",
            "--bridge-stopped",
            "--worker-restarted",
            "--full-capacity",
            &intent.operation_id,
        ],
    );
    assert!(cleared.status.success());
    let stdout = String::from_utf8(cleared.stdout).expect("utf8 output");
    assert!(stdout.contains("dangling=1"));
    assert!(stdout.contains(&intent.operation_id));
    assert!(store.load().expect("cleared").intents.is_empty());
}
