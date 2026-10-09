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

#[test]
fn public_source_upgrades_never_connect_to_database_or_run_seed() {
    use std::net::TcpListener;
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let fixture = Fixture::new();
            let root = fixture.baseline(composition, database);
            let sentinel = fixture.0.join("database.sqlite3");
            let sentinel_bytes = b"application-owner database bytes; never open or seed";
            fs::write(&sentinel, sentinel_bytes).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let postgres = format!(
                "postgres://owner@{}/application",
                listener.local_addr().unwrap()
            );
            let sqlite = format!("sqlite://{}?mode=rwc", sentinel.display());
            let url = match database {
                BaselineDatabase::Sqlite => &sqlite,
                BaselineDatabase::Postgres => &postgres,
            };
            let before = tree(&root);
            for args in [
                vec!["upgrade", "status", "--json"],
                vec!["upgrade", "--dry-run", "--json"],
                vec!["upgrade", "--json"],
            ] {
                let output = Command::new(env!("CARGO_BIN_EXE_hegira"))
                    .args(&args)
                    .current_dir(&root)
                    .env_clear()
                    .env("PATH", "")
                    .env("APP_ENV", "production")
                    .env("APP__DATABASE__URL", url)
                    .env("DATABASE_URL", url)
                    .env("APP__DATABASE__AUTO_MIGRATE", "true")
                    .env("APP__STARTUP__ENSURE_DATABASE", "true")
                    .env("APP__STARTUP__SEED_IDENTITY", "true")
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert!(output.stderr.is_empty());
                assert_eq!(fs::read(&sentinel).unwrap(), sentinel_bytes);
                assert_eq!(
                    listener.accept().unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock
                );
                assert!(!String::from_utf8_lossy(&output.stdout).contains(url));
            }
            let after = tree(&root);
            assert_eq!(
                before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>()
            );
            let changed: Vec<_> = before
                .iter()
                .filter(|(path, bytes)| after.get(*path) != Some(*bytes))
                .map(|(path, _)| path.to_str().unwrap())
                .collect();
            assert_eq!(changed, ["Cargo.lock", "Cargo.toml", "hegira.toml"]);
            assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
        }
    }
}

#[path = "support/upgrade_matrix.rs"]
mod upgrade_matrix;
#[path = "support/upgrade_schema.rs"]
mod upgrade_schema;

#[test]
fn public_upgrade_matrix_retains_composition_and_capability_contracts() {
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let fixture = Fixture::new();
            let root = fixture.baseline(composition, database);
            upgrade_matrix::verify(Path::new(env!("CARGO_BIN_EXE_hegira")), &root);
        }
    }
}

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
    upgrade_schema::assert_output(&report, false);
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
    assert!(preview(&root, &[], 3)["plan"].is_null());
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
    assert!(preview(&root, &[], 4)["plan"].is_null());
    fs::remove_file(&marker).unwrap();
    fs::remove_file(root.join("Cargo.lock")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("Cargo.lock")).unwrap();
    status(&root, "conflict", 4);
    assert!(preview(&root, &[], 4)["plan"].is_null());
    assert_eq!(
        fs::read_to_string(outside).unwrap(),
        "managed-private-content\n"
    );
}

#[test]
fn invalid_and_future_manifests_have_redacted_machine_outcomes() {
    let fixture = Fixture::new();
    status(&fixture.0, "invalid-input", 3);
    assert!(preview(&fixture.0, &[], 3)["plan"].is_null());
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    fs::write(
        root.join("hegira.toml"),
        "schema = 99\nprivate = 'private-runtime-value'\n",
    )
    .unwrap();
    status(&root, "unsupported", 3);
    assert!(preview(&root, &[], 3)["plan"].is_null());
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

#[derive(Debug, PartialEq, Eq)]
struct EntryMetadata {
    length: u64,
    modified: std::time::SystemTime,
    readonly: bool,
    #[cfg(unix)]
    identity: (u64, u64, u32, i64, i64),
}

fn entry_metadata(root: &Path) -> BTreeMap<PathBuf, EntryMetadata> {
    std::iter::once(PathBuf::new())
        .chain(tree(root).into_keys())
        .map(|relative| {
            let metadata = fs::symlink_metadata(root.join(&relative)).unwrap();
            let fingerprint = EntryMetadata {
                length: metadata.len(),
                modified: metadata.modified().unwrap(),
                readonly: metadata.permissions().readonly(),
                #[cfg(unix)]
                identity: {
                    use std::os::unix::fs::MetadataExt;
                    (
                        metadata.dev(),
                        metadata.ino(),
                        metadata.mode(),
                        metadata.ctime(),
                        metadata.ctime_nsec(),
                    )
                },
            };
            (relative, fingerprint)
        })
        .collect()
}

fn preview(root: &Path, extra: &[&str], expected: i32) -> serde_json::Value {
    let before = tree(root);
    let metadata = entry_metadata(root);
    let mut args = vec!["upgrade", "--dry-run", "--json"];
    args.extend_from_slice(extra);
    let first = run(root, &args);
    let second = run(root, &args);
    assert_eq!(
        first.status.code(),
        Some(expected),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let json = String::from_utf8(first.stdout).unwrap();
    assert!(!json.contains(root.to_str().unwrap()));
    assert!(!json.contains("private-runtime-value"));
    assert!(!json.contains("managed-private-content"));
    assert_eq!(tree(root), before);
    assert_eq!(entry_metadata(root), metadata);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    upgrade_schema::assert_output(&value, true);
    assert_eq!(value["output_schema"], 1);
    assert_eq!(value["mode"], "dry-run");
    value
}

#[test]
fn dry_run_exposes_the_exact_typed_plan_for_every_released_profile_without_writes() {
    let fixture = Fixture::new();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let catalog = template_renderer::ManifestCatalog::load(repository, "layered").unwrap();
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let root = fixture.baseline(composition, database);
            fs::write(root.join("config/production.yaml"), "private-runtime-value").unwrap();
            let report = preview(&root, &[], 0);
            assert_eq!(report["outcome"], "planned");
            let typed = catalog.plan_upgrade(&root).unwrap();
            assert_eq!(
                report["plan"],
                serde_json::to_value(typed.summary()).unwrap()
            );
            assert_eq!(report["plan"]["changes"].as_array().unwrap().len(), 3);
            assert_eq!(
                report["preserved_boundaries"],
                serde_json::json!(["application-owned", "generated-once", "immutable-history"])
            );
            assert_eq!(preview(&root, &["--target", "v0.7.0"], 0), report);
            assert_eq!(preview(&root.join("apps/web/src"), &[], 0), report);
            let explicit = run(
                &fixture.0,
                &[
                    "upgrade",
                    "--dry-run",
                    "--application-root",
                    root.to_str().unwrap(),
                    "--json",
                ],
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&explicit.stdout).unwrap(),
                report
            );
            let human = run(&root, &["upgrade", "--dry-run"]);
            assert!(human.status.success());
            let human = String::from_utf8(human.stdout).unwrap();
            for change in report["plan"]["changes"].as_array().unwrap() {
                assert!(human.contains(&format!(
                    "{} {}",
                    change["operation"].as_str().unwrap(),
                    change["path"].as_str().unwrap()
                )));
                assert!(
                    human.contains(&format!("owner={}", change["component"].as_str().unwrap()))
                );
                assert!(human.contains(&format!(
                    "integration={}",
                    change["integration"].as_str().unwrap()
                )));
                assert!(human.contains("ownership=managed-integration"));
                for field in ["precondition", "result"] {
                    for value in change[field].as_object().unwrap().values() {
                        assert!(human.contains(value.as_str().unwrap()));
                    }
                }
            }
            for field in ["edge", "source_package_digest", "source_baseline_digest"] {
                assert!(human.contains(report["plan"][field].as_str().unwrap()));
            }
            assert!(!human.contains("private-runtime-value"));
            assert!(!human.contains(root.to_str().unwrap()));
        }
    }
}

#[test]
fn dry_run_rejects_skipped_targets_conflicts_and_recovery_without_a_plan() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    for target in ["v0.8.0", "v0.6.0", "0.7.0", "https://private-runtime-value"] {
        let output = preview(&root, &["--target", target], 3);
        assert_eq!(output["assessment"]["status"], "unsupported");
        assert!(output["plan"].is_null());
        assert_eq!(output["outcome"], "unavailable");
        let before = tree(&root);
        let applied = run(&root, &["upgrade", "--target", target, "--json"]);
        assert_eq!(applied.status.code(), Some(3));
        let applied: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
        assert!(applied["plan"].is_null());
        assert!(applied.get("receipt").is_none());
        assert_eq!(before, tree(&root));
    }
    fs::write(root.join("Cargo.lock"), "managed-private-content").unwrap();
    assert!(preview(&root, &[], 4)["plan"].is_null());
    fs::write(root.join(".hegira-mutation.lock"), "private-runtime-value").unwrap();
    let output = preview(&root, &[], 4);
    assert_eq!(output["assessment"]["status"], "recovery-blocked");
    assert!(output["plan"].is_null());
}

#[test]
fn current_release_preview_is_a_no_op_and_repeated_apply_has_no_direct_edge() {
    let fixture = Fixture::new();
    let root = fixture.0.join("current");
    assert!(
        run(
            &fixture.0,
            &[
                "new",
                "preview-app",
                "--destination",
                root.to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    let output = preview(&root, &[], 0);
    assert_eq!(output["outcome"], "no-upgrade");
    assert!(output["plan"].is_null());
    assert!(preview(&root, &["--target", "v0.7.0"], 3)["plan"].is_null());
    for args in [
        vec!["upgrade", "--dry-run", "status"],
        vec!["upgrade", "--apply"],
    ] {
        assert_eq!(run(&root, &args).status.code(), Some(2));
    }
    for args in [
        vec!["upgrade"],
        vec!["upgrade", "--json"],
        vec!["upgrade", "--target", "v0.7.0"],
    ] {
        assert_eq!(run(&root, &args).status.code(), Some(3));
    }
}

#[test]
fn public_apply_uses_the_preview_plan_and_preserves_every_unmanaged_byte() {
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let fixture = Fixture::new();
            let root = fixture.baseline(composition, database);
            fs::write(root.join("config/production.yaml"), "private-runtime-value").unwrap();
            fs::write(
                root.join("crates/domain/src/product-owned.rs"),
                "managed-private-content",
            )
            .unwrap();
            let before = tree(&root);
            let original = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
            let planned = preview(&root, &[], 0);
            let applied = run(
                &root.join("apps/web/src"),
                &["upgrade", "--target", "v0.7.0", "--json"],
            );
            assert!(
                applied.status.success(),
                "{}",
                String::from_utf8_lossy(&applied.stdout)
            );
            assert!(applied.stderr.is_empty());
            let text = String::from_utf8(applied.stdout).unwrap();
            for private in [
                "private-runtime-value",
                "managed-private-content",
                root.to_str().unwrap(),
            ] {
                assert!(!text.contains(private));
            }
            let applied: serde_json::Value = serde_json::from_str(&text).unwrap();
            upgrade_schema::assert_output(&applied, true);
            assert_eq!(applied["output_schema"], 1);
            assert_eq!(applied["mode"], "apply");
            assert_eq!(applied["outcome"], "applied");
            assert_eq!(applied["plan"], planned["plan"]);
            assert_eq!(applied["receipt"]["changed_files"], 3);
            assert_eq!(applied["receipt"]["target"], applied["plan"]["target"]);
            assert_eq!(applied["next_steps"].as_array().unwrap().len(), 3);
            let current = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
            assert_eq!(current.schema, 3);
            assert_eq!(current.framework.version, "v0.7.0");
            assert_eq!(current.selection, original.selection);
            let current_composition = current.composition.unwrap();
            let original_composition = original.composition.unwrap();
            assert_eq!(current_composition.package.version, "v0.7.0");
            assert_eq!(
                current_composition.capabilities,
                original_composition.capabilities
            );
            assert!(
                current_composition
                    .components
                    .iter()
                    .all(|component| component.version == "v0.7.0")
            );
            assert!(
                current_composition
                    .modules
                    .iter()
                    .all(|module| module.version == "v0.7.0")
            );
            let after = tree(&root);
            assert_eq!(
                before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>()
            );
            let changed = before
                .iter()
                .filter(|(path, bytes)| after.get(*path) != Some(*bytes))
                .map(|(path, _)| path.to_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(changed, ["Cargo.lock", "Cargo.toml", "hegira.toml"]);
            let metadata = entry_metadata(&root);
            let repeated = run(&root, &["upgrade", "--json"]);
            assert_eq!(repeated.status.code(), Some(3));
            let repeated: serde_json::Value = serde_json::from_slice(&repeated.stdout).unwrap();
            assert_eq!(
                repeated["assessment"]["diagnostics"][0]["code"],
                "direct-edge"
            );
            assert!(repeated["plan"].is_null());
            assert!(repeated.get("receipt").is_none());
            assert_eq!(after, tree(&root));
            assert_eq!(metadata, entry_metadata(&root));
        }
    }
}

#[test]
fn public_apply_has_deterministic_receipts_and_explicit_owner_operations() {
    let mut first = None;
    for _ in 0..2 {
        let fixture = Fixture::new();
        let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
        let output = run(
            &fixture.0,
            &[
                "upgrade",
                "--application-root",
                root.to_str().unwrap(),
                "--json",
            ],
        );
        assert!(output.status.success());
        if let Some(previous) = &first {
            assert_eq!(previous, &output.stdout);
        }
        first = Some(output.stdout);
    }
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    let human = run(&root, &["upgrade"]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("Applied receipt: 3 managed files"));
    assert!(human.contains("do not regenerate the lockfile blindly"));
    assert!(human.contains("application-owned database migrations"));
    assert!(human.contains("native/hydration checks"));
    assert!(!human.contains("No application files were changed"));
}

#[test]
fn public_apply_conflicts_and_recovery_never_issue_a_success_receipt() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    for (path, value) in [
        ("Cargo.lock", "managed-private-content"),
        (".hegira-mutation.lock", "private-runtime-value"),
    ] {
        fs::write(root.join(path), value).unwrap();
        let before = tree(&root);
        let output = run(&root, &["upgrade", "--json"]);
        assert_eq!(output.status.code(), Some(4));
        let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output["outcome"], "unavailable");
        assert!(output["plan"].is_null());
        assert!(output.get("receipt").is_none());
        assert_eq!(before, tree(&root));
    }
}

#[test]
fn concurrent_public_apply_cannot_issue_two_receipts() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    let mut children = Vec::new();
    for _ in 0..2 {
        children.push(
            Command::new(env!("CARGO_BIN_EXE_hegira"))
                .args(["upgrade", "--json"])
                .current_dir(&root)
                .env_clear()
                .env("PATH", "")
                .env("HOME", &root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let results = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    for output in results {
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if output.status.success() {
            assert_eq!(value["outcome"], "applied");
            assert_eq!(value["receipt"]["changed_files"], 3);
        } else {
            assert!(matches!(output.status.code(), Some(3 | 4)));
            assert_eq!(value["outcome"], "unavailable");
            assert!(value.get("receipt").is_none());
        }
    }
    assert_eq!(
        ApplicationManifest::read(root.join("hegira.toml"))
            .unwrap()
            .framework
            .version,
        "v0.7.0"
    );
    assert!(!root.join(".hegira-mutation.lock").exists());
}

#[test]
fn reviewed_upgrade_snapshots_cover_successes_and_failures() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    let original = fs::read_to_string(root.join("hegira.toml")).unwrap();
    let lock = fs::read(root.join("Cargo.lock")).unwrap();
    let mut snapshots = BTreeMap::new();
    let mut capture = |name: &str, args: &[&str], expected: i32| {
        let human = run(&root, args);
        assert_eq!(human.status.code(), Some(expected));
        assert!(human.stderr.is_empty());
        let mut json_args = args.to_vec();
        json_args.push("--json");
        let json = run(&root, &json_args);
        assert_eq!(json.status.code(), Some(expected));
        assert!(json.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        upgrade_schema::assert_output(&value, args.get(1) != Some(&"status"));
        snapshots.insert(
            format!("{name}.txt"),
            String::from_utf8(human.stdout).unwrap(),
        );
        snapshots.insert(
            format!("{name}.json"),
            String::from_utf8(json.stdout).unwrap(),
        );
    };
    capture("ready", &["upgrade", "status"], 0);
    capture("planned", &["upgrade", "--dry-run"], 0);
    fs::write(
        root.join("hegira.toml"),
        original.replace("v0.6.0", "v0.4.0"),
    )
    .unwrap();
    capture("unsupported", &["upgrade", "status"], 3);
    fs::write(root.join("hegira.toml"), format!("{original}\n[[composition.components]]\nid = \"custom-component\"\nversion = \"v0.6.0\"\n")).unwrap();
    capture("incompatible", &["upgrade", "status"], 3);
    fs::write(
        root.join("hegira.toml"),
        "schema = 'private-runtime-value'\n",
    )
    .unwrap();
    capture("invalid", &["upgrade", "status"], 3);
    fs::write(root.join("hegira.toml"), &original).unwrap();
    fs::write(root.join("Cargo.lock"), "managed-private-content").unwrap();
    capture("conflict", &["upgrade", "--dry-run"], 4);
    fs::write(root.join("Cargo.lock"), lock).unwrap();
    fs::write(root.join(".hegira-mutation.lock"), "private-runtime-value").unwrap();
    capture("recovery", &["upgrade"], 4);
    fs::remove_file(root.join(".hegira-mutation.lock")).unwrap();
    // Apply human and JSON to separate immutable baseline copies.
    let other_fixture = Fixture::new();
    let other = other_fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    for (name, output) in [
        ("applied.txt", run(&root, &["upgrade"])),
        ("applied.json", run(&other, &["upgrade", "--json"])),
    ] {
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        snapshots.insert(name.to_owned(), String::from_utf8(output.stdout).unwrap());
    }
    for (name, args, exit) in [
        ("no-upgrade", vec!["upgrade", "status"], 0),
        ("repeated", vec!["upgrade"], 3),
    ] {
        let human = run(&root, &args);
        let mut args = args;
        args.push("--json");
        let json = run(&root, &args);
        assert_eq!(human.status.code(), Some(exit));
        assert_eq!(json.status.code(), Some(exit));
        snapshots.insert(
            format!("{name}.txt"),
            String::from_utf8(human.stdout).unwrap(),
        );
        snapshots.insert(
            format!("{name}.json"),
            String::from_utf8(json.stdout).unwrap(),
        );
    }
    let snapshot_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/upgrade");
    for actual in snapshots.values() {
        for forbidden in [
            "private-runtime-value",
            "managed-private-content",
            root.to_str().unwrap(),
        ] {
            assert!(
                !actual.contains(forbidden),
                "snapshot output must remain content-redacted"
            );
        }
    }
    let differences = snapshots
        .iter()
        .filter(|(name, actual)| {
            fs::read_to_string(snapshot_dir.join(name)).as_ref().ok() != Some(actual)
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    if !differences.is_empty() {
        println!(
            "UPGRADE_SNAPSHOT_REVIEW={}",
            serde_json::to_string(&snapshots).unwrap()
        );
    }
    assert!(
        differences.is_empty(),
        "Review upgrade human/JSON output explicitly before changing snapshots: {differences:?}"
    );
}

#[test]
fn upgrade_requests_ignore_stdin_and_keep_the_same_noninteractive_contract() {
    use std::io::Write;
    for args in [
        vec!["upgrade", "status", "--json"],
        vec!["upgrade", "--dry-run", "--json"],
        vec!["upgrade", "--json"],
    ] {
        let fixture = Fixture::new();
        let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
        let other_fixture = Fixture::new();
        let other = other_fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
        let expected = run(&root, &args);
        let mut child = Command::new(env!("CARGO_BIN_EXE_hegira"))
            .args(&args)
            .current_dir(&other)
            .env_clear()
            .env("PATH", "")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"private-runtime-value\ninteractive answers are not upgrade authority\n")
            .unwrap();
        let actual = child.wait_with_output().unwrap();
        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
    }
    let fixture = Fixture::new();
    let usage = run(&fixture.0, &["upgrade", "status", "--unknown"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert!(!usage.stderr.is_empty());
}

#[test]
fn filesystem_creation_order_does_not_change_readiness_plans_or_receipts() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Default, BaselineDatabase::Sqlite);
    let other = fixture.0.join("reverse-order");
    fs::create_dir(&other).unwrap();
    for (path, bytes) in tree(&root).into_iter().rev() {
        let original = root.join(&path);
        let target = other.join(path);
        if original.is_dir() {
            fs::create_dir_all(target).unwrap();
        } else {
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
        }
    }
    for args in [
        vec!["upgrade", "status", "--json"],
        vec!["upgrade", "--dry-run", "--json"],
        vec!["upgrade", "--json"],
    ] {
        let first = run(&root, &args);
        let second = run(&other, &args);
        assert!(first.status.success());
        assert_eq!(first.status.code(), second.status.code());
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(first.stderr, second.stderr);
    }
}

#[test]
fn preview_matches_the_plan_consumed_by_existing_atomic_publication() {
    let fixture = Fixture::new();
    let root = fixture.baseline(BaselineComposition::Minimal, BaselineDatabase::Sqlite);
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let report = preview(&root, &[], 0);
    let catalog = template_renderer::ManifestCatalog::load(repository, "layered").unwrap();
    let plan = catalog.plan_upgrade(&root).unwrap();
    assert_eq!(
        report["plan"],
        serde_json::to_value(plan.summary()).unwrap()
    );
    // This fixture-only publication uses the same typed plan, not JSON replay
    // or a public upgrade apply command.
    application_mutator::publish_change_plan(&root, plan.change_plan()).unwrap();
    assert_eq!(
        ApplicationManifest::read(root.join("hegira.toml"))
            .unwrap()
            .framework
            .version,
        "v0.7.0"
    );
    assert_eq!(
        report["plan"],
        serde_json::to_value(plan.summary()).unwrap()
    );
}
