use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use application_manifest::{ApplicationManifest, ApplicationUpgradeState};
use upgrade_test_support::{
    BaselineCatalog, BaselineComposition, BaselineDatabase, BaselineRequest,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hegira-upgrade-status-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn baseline(&self, composition: BaselineComposition, database: BaselineDatabase) -> PathBuf {
        let request = BaselineRequest::new(composition, database);
        let root = self.0.join(request.id());
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        BaselineCatalog::from_repository(repository)
            .unwrap()
            .snapshot(request)
            .unwrap()
            .materialize(&root)
            .unwrap();
        root
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(arguments)
        .current_dir(root)
        .env_clear()
        .env("PATH", "")
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root)
        .output()
        .unwrap()
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                files.insert(path.strip_prefix(root).unwrap().to_path_buf(), Vec::new());
                visit(root, &path, files);
            } else if kind.is_symlink() {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read_link(&path)
                        .unwrap()
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec(),
                );
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn status(root: &Path, expected: &str, exit: i32) -> serde_json::Value {
    let before = tree(root);
    let first = run(root, &["upgrade", "status", "--json"]);
    let second = run(root, &["upgrade", "status", "--json"]);
    assert_eq!(
        first.status.code(),
        Some(exit),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["output_schema"], 1);
    assert_eq!(report["status"], expected);
    let human = run(root, &["upgrade", "status"]);
    assert_eq!(human.status.code(), Some(exit));
    assert!(human.stderr.is_empty());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains(&format!("Upgrade status: {expected}")));
    for diagnostic in report["diagnostics"].as_array().unwrap() {
        assert!(human.contains(diagnostic["code"].as_str().unwrap()));
        assert!(human.contains(diagnostic["message"].as_str().unwrap()));
    }
    let json = String::from_utf8(first.stdout).unwrap();
    for forbidden in [
        "private-runtime-value",
        "managed-private-content",
        root.to_str().unwrap(),
    ] {
        assert!(!json.contains(forbidden));
        assert!(!human.contains(forbidden));
    }
    assert_eq!(
        before,
        tree(root),
        "readiness must not write application or home state"
    );
    report
}

#[test]
fn all_released_compositions_report_one_direct_target_without_side_effects() {
    let fixture = Fixture::new();
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let root = fixture.baseline(composition, database);
            fs::write(
                root.join("config/production.yaml"),
                "private-runtime-value\n",
            )
            .unwrap();
            let report = status(&root, "ready", 0);
            assert_eq!(report["source"]["framework_version"], "v0.6.0");
            assert_eq!(report["target"]["framework_version"], "v0.7.0");
            assert_eq!(report["target"]["package"]["version"], "v0.7.0");
            assert_eq!(report["managed_boundaries"], "pass");
            assert_eq!(report["recovery"], "pass");
            assert!(report["diagnostics"].as_array().unwrap().is_empty());
            assert_eq!(
                report["composition"]["modules"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                composition == BaselineComposition::Minimal
            );
            let descendant = root.join("apps/web/src");
            let discovered = run(&descendant, &["upgrade", "status", "--json"]);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&discovered.stdout).unwrap(),
                report
            );
            let explicit = run(
                &fixture.0,
                &[
                    "upgrade",
                    "status",
                    "--application-root",
                    root.to_str().unwrap(),
                    "--json",
                ],
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&explicit.stdout).unwrap(),
                report
            );
            assert_eq!(
                ApplicationManifest::read(root.join("hegira.toml"))
                    .unwrap()
                    .schema,
                2
            );
        }
    }
}

#[test]
fn current_compositions_report_no_upgrade_and_canonical_output_order() {
    let fixture = Fixture::new();
    for (composition, added) in [("identity", false), ("minimal", false), ("minimal", true)] {
        for database in ["sqlite", "postgres"] {
            let root = fixture.0.join(format!("{composition}-{added}-{database}"));
            let created = run(
                &fixture.0,
                &[
                    "new",
                    "status-app",
                    "--destination",
                    root.to_str().unwrap(),
                    "--composition",
                    composition,
                    "--database",
                    database,
                ],
            );
            assert!(created.status.success());
            if added {
                assert!(
                    run(&root, &["component", "add", "identity"])
                        .status
                        .success()
                );
            }
            let report = status(&root, "no-upgrade", 0);
            assert!(report["target"].is_null());
            assert_eq!(report["managed_boundaries"], "not-applicable");
            let mut manifest = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
            manifest.composition.as_mut().unwrap().components.reverse();
            fs::write(root.join("hegira.toml"), manifest.to_toml().unwrap()).unwrap();
            assert_eq!(status(&root, "no-upgrade", 0), report);
        }
    }
}

#[test]
fn unsupported_release_incompatible_composition_and_managed_conflict_are_distinct() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    let manifest_path = root.join("hegira.toml");
    let original = fs::read_to_string(&manifest_path).unwrap();
    fs::write(&manifest_path, original.replace("v0.6.0", "v0.4.0")).unwrap();
    let unsupported = status(&root, "unsupported", 3);
    assert!(unsupported["target"].is_null());
    fs::write(&manifest_path, original.replace("v0.6.0", "v0.7.0")).unwrap();
    assert_eq!(
        status(&root, "incompatible", 3)["diagnostics"][0]["code"],
        "current-manifest"
    );
    fs::write(&manifest_path, &original).unwrap();
    fs::write(&manifest_path, format!("{original}\n[[composition.components]]\nid = \"custom-component\"\nversion = \"v0.6.0\"\n")).unwrap();
    assert_eq!(
        status(&root, "incompatible", 3)["diagnostics"][0]["code"],
        "composition"
    );
    fs::write(&manifest_path, &original).unwrap();
    fs::write(root.join("Cargo.lock"), "managed-private-content\n").unwrap();
    assert_eq!(
        status(&root, "conflict", 4)["diagnostics"][0]["code"],
        "managed-source-digest"
    );
}

#[test]
fn forged_current_schema_cannot_bypass_the_released_manifest_digest() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let catalog = template_renderer::ManifestCatalog::load(repository, "layered").unwrap();
    let boundary = catalog.authenticate_upgrade_source(&root).unwrap();
    let mut manifest = boundary.application().clone();
    manifest.schema = 3;
    manifest.upgrade = Some(ApplicationUpgradeState {
        framework: manifest.framework.clone(),
        package: manifest.composition.as_ref().unwrap().package.clone(),
        ownership: boundary.edge().composition.source_ownership.clone(),
    });
    fs::write(root.join("hegira.toml"), manifest.to_toml().unwrap()).unwrap();
    assert_eq!(
        catalog
            .authenticate_upgrade_source(&root)
            .unwrap_err()
            .diagnostic()
            .kind,
        template_renderer::UpgradeAuthenticationDiagnosticKind::SourceDigestMismatch
    );
    assert_eq!(
        status(&root, "conflict", 4)["diagnostics"][0]["code"],
        "managed-source-digest"
    );
}

#[test]
fn recovery_entries_block_without_reading_or_removing_them() {
    for directory in [false, true] {
        let fixture = Fixture::new();
        let root = fixture.baseline(BaselineComposition::Minimal, BaselineDatabase::Postgres);
        let marker = root.join(application_mutator::MUTATION_MARKER);
        if directory {
            fs::create_dir(&marker).unwrap();
        } else {
            fs::write(&marker, "private-runtime-value\n").unwrap();
        }
        let report = status(&root, "recovery-blocked", 4);
        assert_eq!(report["recovery"], "blocked");
        assert_eq!(report["target"]["framework_version"], "v0.7.0");
        assert!(marker.exists());
    }
}

#[cfg(unix)]
#[test]
fn symlinked_recovery_and_managed_files_are_never_followed() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    let outside = fixture.0.join("outside");
    fs::write(&outside, "managed-private-content\n").unwrap();
    let marker = root.join(application_mutator::MUTATION_MARKER);
    std::os::unix::fs::symlink(&outside, &marker).unwrap();
    status(&root, "recovery-blocked", 4);
    fs::remove_file(&marker).unwrap();
    fs::remove_file(root.join("Cargo.lock")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("Cargo.lock")).unwrap();
    status(&root, "conflict", 4);
    assert_eq!(
        fs::read_to_string(outside).unwrap(),
        "managed-private-content\n"
    );
}

#[test]
fn invalid_and_future_manifests_have_redacted_machine_outcomes() {
    let fixture = Fixture::new();
    status(&fixture.0, "invalid-input", 3);
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    fs::write(
        root.join("hegira.toml"),
        "schema = 99\nprivate = 'private-runtime-value'\n",
    )
    .unwrap();
    status(&root, "unsupported", 3);
    fs::write(
        root.join("hegira.toml"),
        "schema = 'private-runtime-value'\n",
    )
    .unwrap();
    status(&root, "invalid-input", 3);
}

#[test]
fn help_exposes_only_readiness_and_unknown_upgrade_modes_are_usage_errors() {
    let fixture = Fixture::new();
    let help = run(&fixture.0, &["upgrade", "status", "--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.contains("--json"));
    assert!(text.contains("--application-root"));
    for mode in ["plan", "apply"] {
        assert_eq!(run(&fixture.0, &["upgrade", mode]).status.code(), Some(2));
    }
    assert!(tree(&fixture.0).is_empty());
}
