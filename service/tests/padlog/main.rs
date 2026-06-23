use rom_operator_bridge_service::{
    artifacts::{ARTIFACT_SCHEMA_VERSION, ArtifactError, PadLogEventRow, PrivateArtifactStore},
    config::ServiceConfig,
    input::{AppliedInputFrame, PadButton, PadLog, PadLogError, PadWord},
    private_config::{
        ENV_OPERATOR_CREDENTIAL, ENV_PRIVATE_ROOT, ENV_SESSION_SECRET, PRIVATE_FILE_MODE,
    },
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn writes_canonical_padlog_rows_and_round_trips_internal_parser() {
    let rom_hash = [0xabu8; 32];
    let frames = [
        PadWord::ZERO,
        PadWord::ZERO,
        PadWord::from_buttons([PadButton::A]),
        PadWord::from_buttons([PadButton::A]),
        PadWord::from_buttons([PadButton::B]),
        PadWord::ZERO,
        PadWord::ZERO,
        PadWord::ZERO,
    ];
    let log = PadLog::new(frames.to_vec()).with_rom_blake3(rom_hash);

    let text = log.write_canonical();

    assert_eq!(
        text,
        concat!(
            "padlog v1 rom=abababababababababababababababababababababababababababababababab\n",
            "2x0000\n",
            "2x0001\n",
            "0002\n",
            "3x0000\n"
        )
    );
    assert!(text.ends_with('\n'));
    assert!(!text.ends_with("\n\n"));
    assert_eq!(PadLog::parse(&text).expect("padlog parses"), log);
}

#[test]
fn rejects_reserved_bits_in_raw_frames_and_text() {
    assert_eq!(
        PadLog::from_raw_frames([0x0000, 0x1000]),
        Err(PadLogError::ReservedBitsInFrames {
            index: 1,
            word: 0x1000,
        })
    );
    assert_eq!(
        PadLog::parse("padlog v1\nf000\n"),
        Err(PadLogError::ReservedBitsSet {
            line: 2,
            word: 0xf000,
        })
    );
}

#[test]
fn applied_frame_rows_keep_only_pad_words_in_padlog_text() {
    let log = PadLog::from_applied_frames([
        AppliedInputFrame {
            frame: 41,
            pad_word: PadButton::A.mask(),
        },
        AppliedInputFrame {
            frame: 42,
            pad_word: PadButton::A.mask(),
        },
        AppliedInputFrame {
            frame: 99,
            pad_word: PadButton::Start.mask(),
        },
    ])
    .expect("applied frames convert");

    let text = log.write_canonical();

    assert_eq!(text, "padlog v1\n2x0001\n0400\n");
    for forbidden in ["client_seq", "source_id", "assigned_frame"] {
        assert!(!text.contains(forbidden), "padlog leaked {forbidden}");
    }
}

#[test]
fn parser_matches_refwork_edge_cases() {
    let parsed = PadLog::parse("# top\npadlog v1 # trailing\n# mid\n2X0001 # run\n\n0002\n")
        .expect("comments blanks and uppercase run separator parse");
    assert_eq!(
        parsed.frames(),
        [
            PadWord::from_buttons([PadButton::A]),
            PadWord::from_buttons([PadButton::A]),
            PadWord::from_buttons([PadButton::B]),
        ]
    );
    assert_eq!(
        PadLog::parse("padlog v2\n"),
        Err(PadLogError::UnsupportedVersion {
            line: 1,
            version: "v2".to_string(),
        })
    );
    assert_eq!(
        PadLog::parse("padlog v1 rom=zz\n"),
        Err(PadLogError::BadRomHash { line: 1 })
    );
    assert_eq!(
        PadLog::parse("padlog v1\n0x0000\n"),
        Err(PadLogError::ZeroRun { line: 2 })
    );
    assert_eq!(
        PadLog::parse(&format!(
            "padlog v1\n{}x0000\n{}x0000\n",
            rom_operator_bridge_service::input::MAX_PADLOG_FRAMES,
            1
        )),
        Err(PadLogError::TooManyFrames { line: 3 })
    );
}

#[test]
fn parser_rejects_run_length_overflow_before_allocation() {
    assert_eq!(
        PadLog::parse("padlog v1\n0000\n18446744073709551615x0000\n"),
        Err(PadLogError::TooManyFrames { line: 3 })
    );
}

#[cfg(unix)]
#[test]
fn writes_padlog_and_private_event_sidecar_without_rich_fields_in_padlog() {
    let (_workspace, config, private_root) = private_config();
    let store = PrivateArtifactStore::new(config.private_config());
    let padlog = PadLog::from_raw_frames([PadButton::A.mask(), PadButton::A.mask()])
        .expect("padlog frames validate");

    let padlog_ref = store
        .write_padlog("run-001", &padlog)
        .expect("padlog writes");
    let event_ref = store
        .append_padlog_event(
            "run-001",
            &PadLogEventRow::new(
                "run-001",
                0,
                42,
                PadButton::A.mask(),
                7,
                "keyboard-primary",
                "applied",
                "input accepted",
            ),
        )
        .expect("padlog event appends");

    assert_eq!(
        padlog_ref.relative_path(),
        Path::new("runs/run-001/input.padlog")
    );
    assert_eq!(
        event_ref.relative_path(),
        Path::new("runs/run-001/padlog-events.jsonl")
    );

    let padlog_path = private_root.join(padlog_ref.relative_path());
    let event_path = private_root.join(event_ref.relative_path());
    assert_eq!(mode(&padlog_path), PRIVATE_FILE_MODE);
    assert_eq!(mode(&event_path), PRIVATE_FILE_MODE);

    let padlog_text = fs::read_to_string(&padlog_path).expect("padlog reads");
    assert_eq!(padlog_text, "padlog v1\n2x0001\n");
    for forbidden in [
        "run-001",
        "keyboard-primary",
        "client_seq",
        "assigned_frame",
    ] {
        assert!(
            !padlog_text.contains(forbidden),
            "padlog leaked {forbidden}"
        );
    }

    let event_line = fs::read_to_string(&event_path).expect("event reads");
    let event: Value = serde_json::from_str(event_line.trim()).expect("event parses");
    assert_eq!(event["schema_version"], ARTIFACT_SCHEMA_VERSION);
    assert_eq!(event["run_id"], "run-001");
    assert_eq!(event["client_seq"], 7);
    assert_eq!(event["source_id"], "keyboard-primary");
    assert_eq!(event["assigned_frame"], 42);

    assert!(matches!(
        store.append_padlog_event(
            "run-001",
            &PadLogEventRow::new(
                "run-001",
                1,
                43,
                0xf000,
                8,
                "keyboard-primary",
                "rejected",
                "reserved bits",
            ),
        ),
        Err(ArtifactError::InvalidPadWord {
            pad_word: 0xf000,
            reserved: 0xf000,
        })
    ));
}

#[test]
fn reference_parser_accepts_canonical_output_when_accessible() {
    let reference_crate = reference_workload_crate();
    if !reference_crate.exists() {
        eprintln!(
            "skipping reference parser check; {} is not present",
            reference_crate.display()
        );
        return;
    }

    let frames = vec![0x0000, 0x0000, PadButton::A.mask(), PadButton::Start.mask()];
    let text = PadLog::from_raw_frames(frames.clone())
        .expect("frames validate")
        .with_rom_blake3([0xabu8; 32])
        .write_canonical();
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let src_dir = workspace.path().join("src");
    fs::create_dir(&src_dir).expect("src dir creates");
    fs::write(
        workspace.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "padlog-reference-check"
version = "0.1.0"
edition = "2024"

[dependencies]
refwork-script = {{ path = "{}" }}
"#,
            reference_crate.display()
        ),
    )
    .expect("manifest writes");
    fs::write(
        src_dir.join("main.rs"),
        format!(
            r#"fn main() {{
    let text = {text:?};
    let log = refwork_script::parse(text).expect("reference parser accepts bridge padlog");
    assert_eq!(log.frames, {frames:?});
    assert_eq!(refwork_script::write(&log), text);
}}"#
        ),
    )
    .expect("main writes");

    let output = Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(workspace.path().join("Cargo.toml"))
        .output()
        .expect("reference parser command runs");

    assert!(
        output.status.success(),
        "reference parser rejected bridge padlog\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn private_config() -> (tempfile::TempDir, ServiceConfig, PathBuf) {
    let workspace = tempfile::tempdir().expect("tempdir creates");
    let private_root = workspace.path().join("bridge-private");
    let config = ServiceConfig::from_pairs([
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
    ])
    .expect("private config loads");

    (workspace, config, private_root)
}

fn reference_workload_crate() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("service crate has repo root")
        .parent()
        .expect("repo root has preestablished parent")
        .join("reference-workload/crates/refwork-script")
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    fs::metadata(path)
        .expect("metadata reads")
        .permissions()
        .mode()
        & 0o777
}
