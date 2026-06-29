use rom_operator_bridge_service::{
    backend::BackendMode,
    config::{
        ConfigError, ENV_ALLOWED_ORIGINS, ENV_BACKEND_MODE, ENV_BIND_ADDR, ENV_COOKIE_SECURE,
        ENV_DEPLOYMENT_PROFILES, ENV_EXPOSURE_MODE, ENV_PUBLIC_ORIGIN, ServiceConfig,
    },
    private_config::{
        ENV_CAPTURE_SPEC_REF, ENV_CONFIG_FILE, ENV_CREATE_VM_CONFIG_REF, ENV_HYPERVISOR_ENDPOINT,
        ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_PRIVATE_ROOT_ALIAS, ENV_REAL_SNAPSHOT_REF,
        ENV_REFERENCE_WORKLOAD_CHECKOUT, ENV_SESSION_SECRET, ENV_STATIC_PUBLISH_ROOT,
        ENV_WORKLOAD_IMAGE_REF, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE, PRIVATE_ROOT_MARKER,
        PRIVATE_RUN_DIRS, PrivateConfigError,
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

const HTTPS_ORIGIN: &str = "https://rombridge.birb.homes";
const TAILSCALE_ORIGIN: &str = "http://tailrombridge.birb.homes";

#[test]
fn placeholder_config_loads_without_private_values() {
    let config = ServiceConfig::from_pairs([(ENV_BIND_ADDR, "127.0.0.1:0")]).expect("config loads");

    assert_eq!(config.backend_mode(), BackendMode::Synthetic);
    assert!(config.private_config().is_placeholder());
    assert!(config.private_config().private_root().is_none());
    assert!(config.private_config().static_publish_root().is_none());
    assert!(!config.private_config().operator_credential_configured());
    assert!(!config.private_config().session_secret_configured());
}

#[test]
fn deployment_profiles_load_https_and_tailscale_http_security_settings() {
    let config = ServiceConfig::from_pairs([
        (ENV_DEPLOYMENT_PROFILES, "https-origin,tailscale-http"),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_HTTPS_ORIGIN_PUBLIC_ORIGIN",
            HTTPS_ORIGIN,
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_PUBLIC_ORIGIN",
            TAILSCALE_ORIGIN,
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_ALLOWED_ORIGINS",
            TAILSCALE_ORIGIN,
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_COOKIE_SECURE",
            "false",
        ),
        (
            "ROM_OPERATOR_BRIDGE_PROFILE_TAILSCALE_HTTP_EXPOSURE_MODE",
            "tailscale-http",
        ),
    ])
    .expect("deployment profiles load");

    let https = config
        .deployment_security()
        .profile_for_origin(HTTPS_ORIGIN)
        .expect("https origin is accepted");
    assert_eq!(https.id(), "https-origin");
    assert!(https.cookie_secure());
    assert_eq!(
        https.static_csp(),
        "default-src 'self'; connect-src 'self' wss://rombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
    );

    let tailscale = config
        .deployment_security()
        .profile_for_origin(TAILSCALE_ORIGIN)
        .expect("tailscale origin is accepted");
    assert_eq!(tailscale.id(), "tailscale-http");
    assert!(!tailscale.cookie_secure());
    assert_eq!(
        tailscale.static_csp(),
        "default-src 'self'; connect-src 'self' ws://tailrombridge.birb.homes; img-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"
    );
    assert_eq!(
        config
            .deployment_security()
            .profile_for_host_header(Some("tailrombridge.birb.homes:80"))
            .map(|profile| profile.id()),
        Some("tailscale-http")
    );
    assert_eq!(
        config
            .deployment_security()
            .profile_for_host_header(Some("rombridge.birb.homes:443"))
            .map(|profile| profile.id()),
        Some("https-origin")
    );
}

#[test]
fn insecure_cookie_policy_requires_tailscale_http_origins() {
    assert_eq!(
        ServiceConfig::from_pairs([
            (ENV_COOKIE_SECURE, "false"),
            (ENV_PUBLIC_ORIGIN, TAILSCALE_ORIGIN),
            (ENV_ALLOWED_ORIGINS, TAILSCALE_ORIGIN),
        ]),
        Err(ConfigError::InvalidCookiePolicy {
            env: ENV_COOKIE_SECURE
        })
    );

    assert_eq!(
        ServiceConfig::from_pairs([
            (ENV_COOKIE_SECURE, "false"),
            (ENV_PUBLIC_ORIGIN, HTTPS_ORIGIN),
            (ENV_ALLOWED_ORIGINS, HTTPS_ORIGIN),
            (ENV_EXPOSURE_MODE, "tailscale-http"),
        ]),
        Err(ConfigError::InvalidCookiePolicy {
            env: ENV_COOKIE_SECURE
        })
    );

    ServiceConfig::from_pairs([
        (ENV_COOKIE_SECURE, "false"),
        (ENV_PUBLIC_ORIGIN, TAILSCALE_ORIGIN),
        (ENV_ALLOWED_ORIGINS, TAILSCALE_ORIGIN),
        (ENV_EXPOSURE_MODE, "tailscale-http"),
    ])
    .expect("tailscale http can opt out of secure cookies");
}

#[test]
fn deployment_profiles_reject_ambiguous_public_hosts() {
    assert_eq!(
        ServiceConfig::from_pairs([
            (ENV_DEPLOYMENT_PROFILES, "one,two"),
            (
                "ROM_OPERATOR_BRIDGE_PROFILE_ONE_PUBLIC_ORIGIN",
                "https://rombridge.birb.homes",
            ),
            (
                "ROM_OPERATOR_BRIDGE_PROFILE_TWO_PUBLIC_ORIGIN",
                "https://rombridge.birb.homes:443",
            ),
        ]),
        Err(ConfigError::DuplicateDeploymentProfile {
            env: ENV_DEPLOYMENT_PROFILES
        })
    );

    assert_eq!(
        ServiceConfig::from_pairs([
            (ENV_DEPLOYMENT_PROFILES, "one,two"),
            (
                "ROM_OPERATOR_BRIDGE_PROFILE_ONE_PUBLIC_ORIGIN",
                "https://shared.example",
            ),
            (
                "ROM_OPERATOR_BRIDGE_PROFILE_TWO_PUBLIC_ORIGIN",
                "http://shared.example",
            ),
        ]),
        Err(ConfigError::DuplicateDeploymentProfile {
            env: ENV_DEPLOYMENT_PROFILES
        })
    );
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
            (ENV_BIND_ADDR, "\"127.0.0.1:9000\"".to_string()),
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
    assert!(config.private_config().static_publish_root().is_none());
    assert_eq!(mode(&private_root), PRIVATE_DIR_MODE);
    assert_eq!(
        mode(&private_root.join(PRIVATE_ROOT_MARKER)),
        PRIVATE_FILE_MODE
    );

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

#[test]
fn real_mode_requires_private_runtime_inputs() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let mut pairs = complete_private_pairs(&private_root);
    pairs.push((ENV_BACKEND_MODE.to_string(), "real".to_string()));

    assert_eq!(
        ServiceConfig::from_pairs(pairs),
        Err(ConfigError::PrivateConfig(PrivateConfigError::MissingEnv {
            env: ENV_WORKLOAD_IMAGE_REF,
        }))
    );
}

#[test]
fn real_mode_loads_snapshot_runtime_config_with_default_uds() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let config = ServiceConfig::from_pairs(complete_real_snapshot_pairs(
        &private_root,
        &reference_checkout,
    ))
    .expect("real config loads");

    assert_eq!(config.backend_mode(), BackendMode::Real);
    let runtime = config
        .private_config()
        .real_runtime_config()
        .expect("real runtime config exists");
    assert!(runtime.hypervisor_endpoint().is_unix());
    assert!(runtime.start_source().is_snapshot());

    let sanitizer = config.private_config().public_sanitizer();
    assert!(
        sanitizer
            .inspect_text("snapshot ref: private-snapshot-ref-from-test")
            .is_err()
    );
    assert!(
        sanitizer
            .inspect_text("workload image: private-workload-image-ref-from-test")
            .is_err()
    );
    assert!(
        sanitizer
            .inspect_text(&format!("checkout {}", reference_checkout.display()))
            .is_err()
    );
}

#[test]
fn real_mode_rejects_ambiguous_start_sources_and_bad_endpoint() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let reference_checkout = workspace.path().join("reference-workload");
    let mut pairs = complete_real_snapshot_pairs(&private_root, &reference_checkout);
    pairs.push((
        ENV_CREATE_VM_CONFIG_REF.to_string(),
        "private-create-vm-config-ref-from-test".to_string(),
    ));

    assert_eq!(
        ServiceConfig::from_pairs(pairs),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::ConflictingRealStartRefs {
                envs: &[ENV_REAL_SNAPSHOT_REF, ENV_CREATE_VM_CONFIG_REF],
            }
        ))
    );

    let mut pairs = complete_real_snapshot_pairs(&private_root, &reference_checkout);
    replace_pair(&mut pairs, ENV_HYPERVISOR_ENDPOINT, "grpc://127.0.0.1:7400");

    assert_eq!(
        ServiceConfig::from_pairs(pairs),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::InvalidEndpoint {
                env: ENV_HYPERVISOR_ENDPOINT,
            }
        ))
    );
}

#[test]
fn bridge_private_root_alias_configures_private_root() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config = ServiceConfig::from_pairs([
        (
            ENV_PRIVATE_ROOT_ALIAS.to_string(),
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
    ])
    .expect("alias config loads");

    assert_eq!(
        config.private_config().private_root(),
        Some(private_root.as_path())
    );
}

#[test]
fn placeholder_secret_values_are_rejected() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");

    assert_eq!(
        ServiceConfig::from_pairs([
            (
                ENV_PRIVATE_ROOT.to_string(),
                private_root.display().to_string(),
            ),
            (ENV_OPERATOR_CREDENTIAL.to_string(), "change-me".to_string()),
            (
                ENV_SESSION_SECRET.to_string(),
                "session-secret-from-test-source-32-bytes".to_string(),
            ),
        ]),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::PlaceholderSecret {
                env: ENV_OPERATOR_CREDENTIAL,
            }
        ))
    );
}

#[cfg(unix)]
#[test]
fn env_file_parser_accepts_export_whitespace_and_quotes() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config_file = workspace.path().join("rom-operator-bridge.env");
    fs::write(
        &config_file,
        format!(
            r#"
# comments and blank lines are ignored
export {ENV_BIND_ADDR} = "127.0.0.1:8888"
{ENV_PRIVATE_ROOT} = '{}'
{ENV_OPERATOR_CREDENTIAL} = "operator-credential-from-test-source"
{ENV_SESSION_SECRET} = 'session-secret-from-test-source-32-bytes'
"#,
            private_root.display(),
        ),
    )
    .expect("env file writes");
    fs::set_permissions(&config_file, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .expect("mode updates");

    let config = ServiceConfig::from_pairs([(ENV_CONFIG_FILE, config_file.display().to_string())])
        .expect("config file loads");

    assert_eq!(
        config.bind_addr(),
        "127.0.0.1:8888".parse().expect("bind addr parses")
    );
    assert_eq!(
        config.private_config().private_root(),
        Some(private_root.as_path())
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
fn broad_private_roots_are_rejected() {
    assert_eq!(
        ServiceConfig::from_pairs(complete_private_pairs(Path::new("/"))),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::BroadPrivateRoot {
                path: PathBuf::from("/"),
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

#[cfg(unix)]
#[test]
fn existing_private_root_must_be_dedicated_and_presecured() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let existing_root = workspace.path().join("bridge-private");
    fs::create_dir(&existing_root).expect("private root creates");
    fs::set_permissions(&existing_root, fs::Permissions::from_mode(0o755)).expect("mode updates");

    assert_eq!(
        ServiceConfig::from_pairs(complete_private_pairs(&existing_root)),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::InsecureDirectoryMode {
                path: existing_root.clone(),
                mode: 0o755,
            }
        ))
    );

    fs::set_permissions(&existing_root, fs::Permissions::from_mode(PRIVATE_DIR_MODE))
        .expect("mode updates");
    fs::write(existing_root.join("operator-note.txt"), "existing data").expect("file writes");

    assert_eq!(
        ServiceConfig::from_pairs(complete_private_pairs(&existing_root)),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::UnmarkedNonEmptyRoot {
                path: existing_root,
            }
        ))
    );
}

#[cfg(unix)]
#[test]
fn intermediate_symlink_components_are_rejected_for_roots_and_writes() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let public_target = workspace.path().join("static-publish");
    fs::create_dir(&public_target).expect("public target creates");
    let symlink_component = workspace.path().join("link-to-static");
    symlink(&public_target, &symlink_component).expect("symlink creates");
    let redirected_private_root = symlink_component.join("bridge-private");

    assert_eq!(
        ServiceConfig::from_pairs(complete_private_pairs(&redirected_private_root)),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::SymlinkPath {
                path: symlink_component.clone(),
            }
        ))
    );

    let private_root = workspace.path().join("bridge-private");
    let config =
        ServiceConfig::from_pairs(complete_private_pairs(&private_root)).expect("config loads");
    fs::remove_dir(private_root.join("runs")).expect("runs dir removes");
    symlink(&public_target, private_root.join("runs")).expect("runs symlink creates");

    assert_eq!(
        config
            .private_config()
            .write_private_file("runs/run-0001.json", b"{}"),
        Err(PrivateConfigError::SymlinkPath {
            path: private_root.join("runs"),
        })
    );
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

#[test]
fn static_publish_root_inside_private_root_is_rejected() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let static_root = private_root.join("static-publish");

    let mut pairs = complete_private_pairs(&private_root);
    pairs.push((
        ENV_STATIC_PUBLISH_ROOT.to_string(),
        static_root.display().to_string(),
    ));

    assert_eq!(
        ServiceConfig::from_pairs(pairs),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::StaticPublishRootInsidePrivateRoot {
                private_root,
                static_publish_root: static_root,
            }
        ))
    );
}

#[cfg(unix)]
#[test]
fn symlinked_static_publish_root_alias_is_rejected() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let actual_static_root = workspace.path().join("static-real");
    fs::create_dir(&actual_static_root).expect("static root creates");
    let static_root_alias = workspace.path().join("static-link");
    symlink(&actual_static_root, &static_root_alias).expect("static symlink creates");
    let private_root = actual_static_root.join("bridge-private");

    let mut pairs = complete_private_pairs(&private_root);
    pairs.push((
        ENV_STATIC_PUBLISH_ROOT.to_string(),
        static_root_alias.display().to_string(),
    ));

    assert_eq!(
        ServiceConfig::from_pairs(pairs),
        Err(ConfigError::PrivateConfig(
            PrivateConfigError::SymlinkPath {
                path: static_root_alias,
            }
        ))
    );
}

#[test]
fn config_error_debug_does_not_expose_private_paths() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let static_root = workspace.path().join("static-publish");
    let private_root = static_root.join("bridge-private");

    let mut pairs = complete_private_pairs(&private_root);
    pairs.push((
        ENV_STATIC_PUBLISH_ROOT.to_string(),
        static_root.display().to_string(),
    ));

    let error = ServiceConfig::from_pairs(pairs).expect_err("config should fail");
    let debug = format!("{error:?}");
    let display = error.to_string();

    for text in [debug, display] {
        assert!(!text.contains(&private_root.display().to_string()));
        assert!(!text.contains(&static_root.display().to_string()));
    }
}

#[test]
fn private_config_sanitizer_rejects_configured_root_and_secrets() {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let static_root = workspace.path().join("static-publish");
    let mut pairs = complete_private_pairs(&private_root);
    pairs.push((
        ENV_STATIC_PUBLISH_ROOT.to_string(),
        static_root.display().to_string(),
    ));
    let config = ServiceConfig::from_pairs(pairs).expect("config loads");
    assert_eq!(
        config.private_config().static_publish_root(),
        Some(static_root.as_path())
    );
    let sanitizer = config.private_config().public_sanitizer();

    assert!(
        sanitizer
            .inspect_text(&format!("opened {}", private_root.display()))
            .is_err()
    );
    assert!(
        sanitizer
            .inspect_text(&format!("served {}", static_root.display()))
            .is_err()
    );
    assert!(
        sanitizer
            .inspect_text("operator-credential-from-test-source")
            .is_err()
    );
    assert!(
        sanitizer
            .inspect_text("session-secret-from-test-source-32-bytes")
            .is_err()
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
            if let Some(value) = assignment_value(line, ENV_PRIVATE_ROOT) {
                assert!(
                    !value.starts_with('/'),
                    "committed file contains a concrete private root assignment: {path}"
                );
            }
            if let Some(value) = assignment_value(line, ENV_PRIVATE_ROOT_ALIAS) {
                assert!(
                    !value.starts_with('/'),
                    "committed file contains a concrete private root assignment: {path}"
                );
            }

            for secret_env in [
                ENV_OPERATOR_CREDENTIAL,
                ENV_SESSION_SECRET,
                ENV_HYPERVISOR_ENDPOINT,
                ENV_WORKLOAD_IMAGE_REF,
                ENV_CAPTURE_SPEC_REF,
                ENV_REFERENCE_WORKLOAD_CHECKOUT,
                ENV_REAL_SNAPSHOT_REF,
                ENV_CREATE_VM_CONFIG_REF,
                "PRIVATE_ROM_PATH",
                "WORKER_LEASE_TOKEN",
            ] {
                if let Some(value) = assignment_value(line, secret_env) {
                    assert!(
                        value.starts_with('<') || value.contains('$'),
                        "committed file contains a concrete private config assignment: {path}"
                    );
                }
            }
        }
    }
}

#[test]
fn committed_scan_detects_assignment_variants() {
    assert_eq!(
        assignment_value(
            "export ROM_OPERATOR_BRIDGE_PRIVATE_ROOT = \"/private/root\"",
            ENV_PRIVATE_ROOT,
        ),
        Some("/private/root".to_string())
    );
    assert_eq!(
        assignment_value(
            "WORKER_LEASE_TOKEN: token-from-private-run",
            "WORKER_LEASE_TOKEN"
        ),
        Some("token-from-private-run".to_string())
    );
    assert_eq!(
        assignment_value(
            "ROM_OPERATOR_BRIDGE_OPERATOR_CREDENTIAL=<operator-credential-from-secret-source>",
            ENV_OPERATOR_CREDENTIAL,
        ),
        Some("<operator-credential-from-secret-source>".to_string())
    );
    assert_eq!(
        assignment_value(
            "BRIDGE_CAPTURE_SPEC_REF='$BRIDGE_CAPTURE_SPEC_REF'",
            ENV_CAPTURE_SPEC_REF
        ),
        Some("$BRIDGE_CAPTURE_SPEC_REF".to_string())
    );
    assert_eq!(
        assignment_value(
            "BRIDGE_HYPERVISOR_ENDPOINT=\"unix://$O73_WORKER_UDS\"",
            ENV_HYPERVISOR_ENDPOINT,
        ),
        Some("unix://$O73_WORKER_UDS".to_string())
    );
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

fn complete_real_snapshot_pairs(
    private_root: &Path,
    reference_checkout: &Path,
) -> Vec<(String, String)> {
    let mut pairs = complete_private_pairs(private_root);
    pairs.extend([
        (ENV_BACKEND_MODE.to_string(), "real".to_string()),
        (
            ENV_WORKLOAD_IMAGE_REF.to_string(),
            "private-workload-image-ref-from-test".to_string(),
        ),
        (
            ENV_CAPTURE_SPEC_REF.to_string(),
            "private-capture-spec-ref-from-test".to_string(),
        ),
        (
            ENV_REFERENCE_WORKLOAD_CHECKOUT.to_string(),
            reference_checkout.display().to_string(),
        ),
        (
            ENV_REAL_SNAPSHOT_REF.to_string(),
            "private-snapshot-ref-from-test".to_string(),
        ),
    ]);
    pairs
}

fn replace_pair(pairs: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some((_, existing)) = pairs.iter_mut().find(|(candidate, _)| candidate == key) {
        *existing = value.to_string();
    } else {
        pairs.push((key.to_string(), value.to_string()));
    }
}

fn assignment_value(line: &str, key: &str) -> Option<String> {
    let line = line.trim();
    let line = line.strip_prefix("export ").unwrap_or(line).trim();

    ['=', ':'].into_iter().find_map(|separator| {
        let (candidate_key, value) = line.split_once(separator)?;
        (candidate_key.trim() == key).then(|| unquote(value.trim()).to_string())
    })
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
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
