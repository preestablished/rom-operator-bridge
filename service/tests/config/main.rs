use rom_operator_bridge_service::{
    backend::BackendMode,
    config::{ConfigError, ENV_BACKEND_MODE, ENV_BIND_ADDR, ServiceConfig},
    private_config::{
        ENV_CONFIG_FILE, ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET,
        ENV_STATIC_PUBLISH_ROOT, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE, PRIVATE_RUN_DIRS,
        PrivateConfigError,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;

#[test]
fn placeholder_config_loads_without_private_values() {
    let config = ServiceConfig::from_pairs([(ENV_BIND_ADDR, "127.0.0.1:0")]).expect("config loads");

    assert_eq!(config.backend_mode(), BackendMode::Synthetic);
    assert!(config.private_config().is_placeholder());
    assert!(config.private_config().private_root().is_none());
    assert!(!config.private_config().operator_credential_configured());
    assert!(!config.private_config().session_secret_configured());
}

#[cfg(unix)]
#[test]
fn config_file_loads_private_values_and_creates_private_dirs() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config_file = workspace.path().join("rom-operator-bridge.env");
    write_private_env_file(
        &config_file,
        &[
            (ENV_BIND_ADDR, "127.0.0.1:9000".to_string()),
            (ENV_BACKEND_MODE, "synthetic".to_string()),
            (ENV_PRIVATE_ROOT, private_root.display().to_string()),
            (
                ENV_OPERATOR_CREDENTIAL,
                "operator-credential-from-test-source".to_string(),
            ),
            (
                ENV_SESSION_SECRET,
                "session-secret-from-test-source-32-bytes".to_string(),
            ),
        ],
    );

    let config = ServiceConfig::from_pairs([
        (ENV_CONFIG_FILE, config_file.display().to_string()),
        (ENV_BIND_ADDR, "127.0.0.1:7777".to_string()),
    ])
    .expect("config file loads");

    assert_eq!(
        config.bind_addr(),
        "127.0.0.1:7777".parse().expect("override bind addr parses")
    );
    assert_eq!(
        config.private_config().private_root(),
        Some(private_root.as_path())
    );
    assert_eq!(mode(&private_root), PRIVATE_DIR_MODE);

    for dir in PRIVATE_RUN_DIRS {
        let path = private_root.join(dir);
        assert!(path.is_dir(), "{path:?} should be created");
        assert_eq!(mode(&path), PRIVATE_DIR_MODE);
    }
}

#[test]
fn complete_private_config_requires_secrets() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");

    assert_eq!(
        ServiceConfig::from_pairs([(ENV_PRIVATE_ROOT, private_root.display().to_string())]),
        Err(ConfigError::PrivateConfig(PrivateConfigError::MissingEnv {
            env: ENV_OPERATOR_CREDENTIAL,
        }))
    );

    assert_eq!(
        ServiceConfig::from_pairs([(ENV_BACKEND_MODE, "real".to_string())]),
        Err(ConfigError::PrivateConfig(PrivateConfigError::MissingEnv {
            env: ENV_PRIVATE_ROOT,
        }))
    );
}

#[cfg(unix)]
#[test]
fn private_config_file_must_be_0600() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config_file = workspace.path().join("rom-operator-bridge.env");
    write_private_env_file(
        &config_file,
        &[
            (ENV_PRIVATE_ROOT, private_root.display().to_string()),
            (
                ENV_OPERATOR_CREDENTIAL,
                "operator-credential-from-test-source".to_string(),
            ),
            (
                ENV_SESSION_SECRET,
                "session-secret-from-test-source-32-bytes".to_string(),
            ),
        ],
    );
    fs::set_permissions(&config_file, fs::Permissions::from_mode(0o644)).expect("mode updates");

    assert_eq!(
        ServiceConfig::from_pairs([(ENV_CONFIG_FILE, config_file.display().to_string())]),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::InsecureFileMode {
                path: config_file,
                mode: 0o644,
            }
        ))
    );
}

#[cfg(unix)]
#[test]
fn private_config_file_must_not_be_a_symlink() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let real_config_file = workspace.path().join("real-rom-operator-bridge.env");
    let symlink_config_file = workspace.path().join("rom-operator-bridge.env");
    write_private_env_file(
        &real_config_file,
        &[
            (ENV_PRIVATE_ROOT, private_root.display().to_string()),
            (
                ENV_OPERATOR_CREDENTIAL,
                "operator-credential-from-test-source".to_string(),
            ),
            (
                ENV_SESSION_SECRET,
                "session-secret-from-test-source-32-bytes".to_string(),
            ),
        ],
    );
    symlink(&real_config_file, &symlink_config_file).expect("symlink creates");

    assert_eq!(
        ServiceConfig::from_pairs([(ENV_CONFIG_FILE, symlink_config_file.display().to_string())]),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::SymlinkPath {
                path: symlink_config_file,
            }
        ))
    );
}

#[cfg(unix)]
#[test]
fn world_writable_private_root_is_rejected() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    fs::create_dir(&private_root).expect("private root creates");
    fs::set_permissions(&private_root, fs::Permissions::from_mode(0o777)).expect("mode updates");

    assert_eq!(
        ServiceConfig::from_pairs(complete_private_pairs(&private_root)),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::WorldWritableRoot {
                path: private_root.clone(),
            }
        ))
    );

    fs::set_permissions(&private_root, fs::Permissions::from_mode(PRIVATE_DIR_MODE))
        .expect("mode restores for cleanup");
}

#[test]
fn private_root_inside_static_publish_root_is_rejected() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let static_root = workspace.path().join("static-publish");
    let private_root = static_root.join("bridge-private");

    let mut pairs = complete_private_pairs(&private_root);
    pairs.push((
        ENV_STATIC_PUBLISH_ROOT.to_string(),
        static_root.display().to_string(),
    ));

    assert_eq!(
        ServiceConfig::from_pairs(pairs),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::PrivateRootInsideStaticPublishRoot {
                private_root,
                static_publish_root: static_root,
            }
        ))
    );
}

#[cfg(unix)]
#[test]
fn private_file_writer_enforces_0600_and_rejects_escaping_paths() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config =
        ServiceConfig::from_pairs(complete_private_pairs(&private_root)).expect("config loads");

    let private_file = config
        .private_config()
        .write_private_file("runs/run-0001.json", br#"{"schema_version":1}"#)
        .expect("private file writes");

    assert_eq!(private_file, private_root.join("runs/run-0001.json"));
    assert_eq!(mode(&private_file), PRIVATE_FILE_MODE);

    fs::set_permissions(&private_file, fs::Permissions::from_mode(0o644)).expect("mode updates");
    assert_eq!(
        config
            .private_config()
            .write_private_file("runs/run-0001.json", b"overwrite"),
        Err(PrivateConfigError::InsecureFileMode {
            path: private_file,
            mode: 0o644,
        })
    );

    assert!(matches!(
        config.private_config().write_private_file("../escape", b""),
        Err(PrivateConfigError::UnsafeRelativePath { .. })
    ));
    assert!(matches!(
        config
            .private_config()
            .write_private_file(private_root.join("absolute"), b""),
        Err(PrivateConfigError::UnsafeRelativePath { .. })
    ));
}

#[test]
fn committed_files_do_not_include_private_config_or_values() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("service has repo parent");
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo_root)
        .output()
        .expect("git ls-files runs");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for path in String::from_utf8_lossy(&output.stdout).split('\0') {
        if path.is_empty() {
            continue;
        }
        assert!(
            !path.ends_with(".env") && !path.ends_with(".rombridge.env"),
            "private config file must not be committed: {path}"
        );

        let contents = fs::read_to_string(repo_root.join(path)).unwrap_or_default();
        for line in contents.lines() {
            let line = line.trim();
            assert!(
                !line.starts_with(&format!("{ENV_PRIVATE_ROOT}=/")),
                "committed file contains a concrete private root assignment: {path}"
            );

            for secret_env in [
                ENV_OPERATOR_CREDENTIAL,
                ENV_SESSION_SECRET,
                "PRIVATE_ROM_PATH",
                "WORKER_LEASE_TOKEN",
            ] {
                let assignment = format!("{secret_env}=");
                if let Some(value) = line.strip_prefix(&assignment) {
                    assert!(
                        value.trim_start().starts_with('<'),
                        "committed file contains a concrete private config assignment: {path}"
                    );
                }
            }
        }
    }
}

fn complete_private_pairs(private_root: &Path) -> Vec<(String, String)> {
    vec![
        (
            ENV_PRIVATE_ROOT.to_string(),
            private_root.display().to_string(),
        ),
        (
            ENV_OPERATOR_CREDENTIAL.to_string(),
            "operator-credential-from-test-source".to_string(),
        ),
        (
            ENV_SESSION_SECRET.to_string(),
            "session-secret-from-test-source-32-bytes".to_string(),
        ),
    ]
}

fn write_private_env_file(path: &Path, pairs: &[(&str, String)]) {
    let contents = pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}\n"))
        .collect::<String>();
    fs::write(path, contents).expect("env file writes");
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .expect("env file mode updates");
}

#[cfg(unix)]
fn mode(path: &PathBuf) -> u32 {
    fs::metadata(path)
        .expect("metadata reads")
        .permissions()
        .mode()
        & 0o777
}
