#![cfg(target_os = "linux")]

use std::{
    collections::BTreeMap,
    fs,
    os::unix::{
        fs::{MetadataExt, symlink},
        net::UnixListener,
    },
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::{Value, json};

const RELEASE: &str = "target/hegira/release-build";
const MARKER: &str = ".hegira-release-build.json";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hegira-artifacts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn application(&self, composition: &str, provider: &str) -> PathBuf {
        let root = self.0.join(format!("{composition}-{provider}"));
        let mut args = vec![
            "new",
            "artifact-app",
            "--destination",
            root.to_str().unwrap(),
            "--database",
            provider,
        ];
        if composition != "default" {
            args.extend(["--composition", "minimal"]);
        }
        assert!(run(&self.0, &args).status.success());
        if composition == "identity-added" {
            assert!(
                run(&root, &["component", "add", "identity"])
                    .status
                    .success()
            );
        }
        root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(args)
        .env_clear()
        .env("PATH", "")
        .current_dir(root)
        .output()
        .unwrap()
}

fn status(root: &Path, exit: i32) -> Value {
    let output = run(root, &["artifacts", "status", "--json"]);
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["output_schema"], 1);
    value
}

// Inode, mode, length, modification/change time and link identity must be unchanged.
// Access times are intentionally excluded: these are read-only diagnostics.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u64>> {
    fn visit(root: &Path, path: &Path, tree: &mut BTreeMap<PathBuf, Vec<u64>>) {
        let m = fs::symlink_metadata(path).unwrap();
        tree.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            vec![
                m.dev(),
                m.ino(),
                m.mode() as u64,
                m.len(),
                m.mtime() as u64,
                m.mtime_nsec() as u64,
                m.ctime() as u64,
                m.ctime_nsec() as u64,
                m.nlink(),
            ],
        );
        if m.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), tree);
            }
        }
    }
    let mut tree = BTreeMap::new();
    visit(root, root, &mut tree);
    tree
}

fn owned_release(root: &Path) -> PathBuf {
    let release = root.join(RELEASE);
    fs::create_dir_all(&release).unwrap();
    fs::write(
        release.join(MARKER),
        serde_json::to_vec(&json!({
            "output_schema": 1, "owner": "hegira-release-build", "application": "artifact-app"
        }))
        .unwrap(),
    )
    .unwrap();
    release
}

#[test]
fn all_six_compositions_report_empty_trees_without_tools_or_writes() {
    let fixture = Fixture::new();
    for composition in ["default", "minimal", "identity-added"] {
        for provider in ["sqlite", "postgres"] {
            let root = fixture.application(composition, provider);
            let before = snapshot(&root);
            let value = status(&root, 0);
            assert_eq!(value["application"], "artifact-app");
            assert_eq!(value["operation_activity"], "idle");
            assert_eq!(value["budget_bytes"], Value::Null);
            assert_eq!(value["budget_status"], "not-enforced");
            assert_eq!(value["regions"].as_array().unwrap().len(), 3);
            for region in value["regions"].as_array().unwrap() {
                assert_eq!(region["status"], "missing");
                assert_eq!(region["logical_bytes"], 0);
                assert_eq!(region["cleanup_eligible"], false);
            }
            assert_eq!(value, status(&root, 0));
            for args in [
                vec!["artifacts", "cache-status", "--json"],
                vec!["artifacts", "prune", "--dry-run", "--json"],
            ] {
                let output = run(&root, &args);
                assert!(output.status.success());
                let cache: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(cache["allocated_bytes"], 0);
                assert_eq!(cache["caches"], json!([]));
            }
            let explicit = run(
                &root.join("apps/web"),
                &[
                    "artifacts",
                    "status",
                    "--application-root",
                    "../..",
                    "--json",
                ],
            );
            assert_eq!(
                serde_json::from_slice::<Value>(&explicit.stdout).unwrap(),
                value
            );
            assert_eq!(snapshot(&root), before);
        }
    }
}

#[test]
fn mixed_output_is_counted_once_per_region_including_sparse_and_hidden_files() {
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let release = owned_release(&root);
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::create_dir_all(root.join("target/release")).unwrap();
    fs::create_dir_all(root.join("target/hegira/unclaimed")).unwrap();
    fs::write(root.join("target/debug/developer"), b"1234567").unwrap();
    fs::hard_link(
        root.join("target/debug/developer"),
        root.join("target/release/same-file"),
    )
    .unwrap();
    fs::write(root.join("target/hegira/unclaimed/.hidden"), b"abc").unwrap();
    let sparse = fs::File::create(release.join("sparse")).unwrap();
    sparse.set_len(32 * 1024 * 1024 * 1024).unwrap();
    fs::hard_link(release.join("sparse"), release.join("sparse-alias")).unwrap();
    let before = snapshot(&root);
    let value = status(&root, 0);
    assert_eq!(value["regions"][0]["path"], "target");
    assert_eq!(
        value["regions"][0]["excluded_paths"],
        json!([RELEASE, "target/hegira/validation"])
    );
    assert_eq!(value["regions"][0]["owner"], "developer-or-unclaimed");
    assert_eq!(value["regions"][0]["logical_bytes"], 10);
    assert_eq!(value["regions"][0]["unique_files"], 2);
    let marker_size = fs::metadata(release.join(MARKER)).unwrap().len();
    assert_eq!(value["regions"][1]["owner"], "hegira-release-build");
    assert_eq!(
        value["regions"][1]["logical_bytes"],
        32_u64 * 1024 * 1024 * 1024 + marker_size
    );
    assert_eq!(value["regions"][1]["unique_files"], 2);
    assert_eq!(
        value["regions"][1]["allocated_bytes"],
        fs::metadata(release.join(MARKER)).unwrap().blocks() * 512
            + sparse.metadata().unwrap().blocks() * 512
    );
    assert_eq!(value, status(&root, 0));
    let human = run(&root, &["artifacts", "status"]);
    assert!(human.status.success());
    let text = String::from_utf8(human.stdout).unwrap();
    for expected in [
        "read-only",
        "developer-or-unclaimed",
        "hegira-release-build",
        "Budget: not enforced",
        "cleanup eligible: false",
        "Docker",
        "logical bytes",
    ] {
        assert!(text.contains(expected), "missing {expected}");
    }
    assert_eq!(snapshot(&root), before);
}

#[test]
fn existing_application_operation_is_observed_without_waiting_or_changing_its_lock() {
    use rustix::fs::{FlockOperation, flock};
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    owned_release(&root);
    let lock = fs::File::open(&root).unwrap();
    flock(&lock, FlockOperation::NonBlockingLockExclusive).unwrap();
    let before = snapshot(&root);
    let value = status(&root, 0);
    assert_eq!(value["operation_activity"], "active");
    assert_eq!(value["regions"][1]["cleanup_eligible"], false);
    let independent = fs::File::open(&root).unwrap();
    assert_eq!(
        flock(&independent, FlockOperation::NonBlockingLockExclusive),
        Err(rustix::io::Errno::WOULDBLOCK)
    );
    assert_eq!(snapshot(&root), before);
    drop(lock);
    assert_eq!(status(&root, 0)["operation_activity"], "idle");
}

#[test]
fn symlinks_at_each_output_boundary_never_claim_or_measure_external_output() {
    let fixture = Fixture::new();
    let external = fixture.0.join("external-secret-directory");
    fs::create_dir(&external).unwrap();
    fs::write(
        external.join("secret-token"),
        b"sensitive bytes must never be printed",
    )
    .unwrap();
    for (index, boundary) in ["target", "target/hegira", RELEASE, "target/debug"]
        .iter()
        .enumerate()
    {
        let root = fixture.application(
            if index % 2 == 0 { "default" } else { "minimal" },
            if index < 2 { "sqlite" } else { "postgres" },
        );
        let link = root.join(boundary);
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        symlink(&external, &link).unwrap();
        let before = snapshot(&fixture.0);
        let value = status(&root, 3);
        for region in value["regions"].as_array().unwrap() {
            assert_eq!(region["logical_bytes"], 0);
            assert_ne!(region["owner"], "hegira-release-build");
        }
        let text = value.to_string();
        for secret in [
            "external-secret-directory",
            "secret-token",
            "sensitive bytes",
            fixture.0.to_str().unwrap(),
        ] {
            assert!(!text.contains(secret));
        }
        assert_eq!(snapshot(&fixture.0), before);
    }
}

#[test]
fn malformed_missing_foreign_and_symlinked_claims_are_unclaimed_and_preserved() {
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let release = owned_release(&root);
    for source in [
        "",
        "secret-from-invalid-claim",
        "{\"output_schema\":1,\"owner\":\"hegira-release-build\",\"application\":\"foreign-app\"}",
        &"x".repeat(4097),
    ] {
        fs::write(release.join(MARKER), source).unwrap();
        let before = snapshot(&root);
        let value = status(&root, 3);
        assert_eq!(value["regions"][1]["owner"], "unclaimed");
        assert!(!value.to_string().contains("secret-from-invalid-claim"));
        assert_eq!(snapshot(&root), before);
    }
    fs::remove_file(release.join(MARKER)).unwrap();
    let before = snapshot(&root);
    assert_eq!(status(&root, 3)["regions"][1]["owner"], "unclaimed");
    assert_eq!(snapshot(&root), before);
    let external = fixture.0.join("claim");
    fs::write(&external, b"never follow me").unwrap();
    symlink(&external, release.join(MARKER)).unwrap();
    let before = snapshot(&fixture.0);
    assert_eq!(status(&root, 3)["regions"][1]["owner"], "unclaimed");
    assert_eq!(snapshot(&fixture.0), before);
}

#[test]
fn external_hard_links_and_special_files_invalidate_claims_without_reading_contents() {
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let release = owned_release(&root);
    let external = fixture.0.join("secret-hard-link");
    fs::write(&external, b"secret file contents").unwrap();
    fs::hard_link(&external, release.join("linked")).unwrap();
    let before = snapshot(&fixture.0);
    let value = status(&root, 3);
    assert_eq!(value["regions"][1]["owner"], "unclaimed");
    assert!(
        value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "artifact-external-hard-link")
    );
    assert!(!value.to_string().contains("secret file contents"));
    assert_eq!(snapshot(&fixture.0), before);
    fs::remove_file(release.join("linked")).unwrap();
    let socket = UnixListener::bind(release.join("socket")).unwrap();
    let value = status(&root, 3);
    assert_eq!(value["regions"][1]["owner"], "unclaimed");
    assert_eq!(value["regions"][1]["skipped_entries"], 1);
    drop(socket);
}

#[test]
fn traversal_depth_is_bounded_and_never_labels_partial_measurements_as_owned() {
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let release = owned_release(&root);
    let mut nested = release;
    for _ in 0..70 {
        nested = nested.join("nested");
    }
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("hidden-data"), b"not measured").unwrap();
    let before = snapshot(&root);
    let value = status(&root, 3);
    assert_eq!(value["regions"][1]["owner"], "unclaimed");
    assert_eq!(value["regions"][1]["status"], "incomplete");
    assert_eq!(value["diagnostics"][0]["code"], "artifact-scan-limit");
    assert_eq!(snapshot(&root), before);
}

#[test]
fn replaced_output_is_reauthenticated_and_external_overrides_are_not_scanned() {
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let release = owned_release(&root);
    assert_eq!(
        status(&root, 0)["regions"][1]["owner"],
        "hegira-release-build"
    );
    let moved = fixture.0.join("moved-output");
    fs::rename(&release, &moved).unwrap();
    fs::write(moved.join("secret-name"), b"external-secret").unwrap();
    symlink(&moved, &release).unwrap();
    let before = snapshot(&fixture.0);
    let value = status(&root, 3);
    assert_ne!(value["regions"][1]["owner"], "hegira-release-build");
    assert_eq!(value["regions"][1]["logical_bytes"], 0);
    assert_eq!(snapshot(&fixture.0), before);

    fs::remove_file(&release).unwrap();
    fs::write(root.join("config/sqlite.yaml"), "invalid runtime secret: [").unwrap();
    let before = snapshot(&fixture.0);
    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(["artifacts", "status", "--json"])
        .env_clear()
        .env("PATH", "")
        .env("CARGO_TARGET_DIR", &moved)
        .env("APP__DATABASE__URL", "secret-database-url")
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["regions"][0]["logical_bytes"], 0);
    for secret in [
        "external-secret",
        "invalid runtime secret",
        "secret-database-url",
        "secret-name",
    ] {
        assert!(!String::from_utf8_lossy(&result.stdout).contains(secret));
    }
    assert_eq!(snapshot(&fixture.0), before);
}

#[test]
fn fifo_claims_and_non_directory_release_roots_fail_without_blocking() {
    use rustix::fs::{CWD, FileType, Mode, mknodat};
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let release = owned_release(&root);
    fs::remove_file(release.join(MARKER)).unwrap();
    mknodat(
        CWD,
        release.join(MARKER),
        FileType::Fifo,
        Mode::RUSR | Mode::WUSR,
        0,
    )
    .unwrap();
    let before = snapshot(&root);
    let value = status(&root, 3);
    assert_eq!(value["regions"][1]["owner"], "unclaimed");
    assert_eq!(value["regions"][1]["skipped_entries"], 1);
    assert_eq!(snapshot(&root), before);
    fs::remove_file(release.join(MARKER)).unwrap();
    fs::remove_dir(&release).unwrap();
    fs::write(&release, b"not a directory").unwrap();
    let before = snapshot(&root);
    assert_eq!(status(&root, 3)["regions"][1]["status"], "incomplete");
    assert_eq!(snapshot(&root), before);
}

#[test]
fn help_invalid_modes_and_missing_application_have_stable_process_outcomes() {
    let fixture = Fixture::new();
    let help = run(&fixture.0, &["artifacts", "status", "--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("--application-root"));
    assert!(text.contains("--json"));
    for arguments in [
        vec!["artifacts"],
        vec!["artifacts", "prune"],
        vec!["artifacts", "status", "--execute"],
        vec!["artifacts", "status", "--budget", "1"],
    ] {
        assert_eq!(run(&fixture.0, &arguments).status.code(), Some(2));
    }
    let value = status(&fixture.0, 3);
    assert_eq!(value["diagnostics"][0]["code"], "application-context");
    let human = run(&fixture.0, &["artifacts", "status"]);
    assert_eq!(human.status.code(), Some(3));
    assert!(human.stdout.is_empty());
    assert!(!human.stderr.is_empty());
}
