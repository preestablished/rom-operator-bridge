use rom_operator_bridge_service::{
    config::{ENV_BIND_ADDR, ServiceConfig},
    lease_store::{AllocationKind, LeaseIntent, LeaseStore},
    private_config::{ENV_PRIVATE_ROOT, ENV_SESSION_SECRET, PRIVATE_FILE_MODE},
};

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};

fn store() -> (tempfile::TempDir, LeaseStore) {
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
    (workspace, LeaseStore::new(config.private_config().clone()))
}

#[test]
fn intent_promotes_to_private_lease_and_round_trips() {
    let (workspace, store) = store();
    let intent = LeaseIntent::new(
        "session".into(),
        "run".into(),
        "create_vm_config".into(),
        AllocationKind::CreateVm,
    );
    store.write_intent(&intent).expect("intent durable");
    let record = intent.promote(&dh_proto::v1::Lease {
        slot_id: 7,
        token: vec![0xab, 0xcd],
    });
    store.write_lease(&record).expect("lease durable");
    let loaded = store.load().expect("load");
    assert_eq!(loaded.intents.len(), 1);
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
    let (workspace, store) = store();
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
    let (workspace, store) = store();
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
    let (_workspace, store) = store();
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
            .clear_dangling_intents(&[intent.operation_id.clone()])
            .is_err()
    );
    store
        .remove_lease(&intent.operation_id)
        .expect("remove lease");
    assert_eq!(
        store
            .clear_dangling_intents(&[intent.operation_id.clone()])
            .expect("selected clear"),
        1
    );
    assert!(store.load().expect("load").intents.is_empty());
}
