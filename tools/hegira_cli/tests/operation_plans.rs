use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use application_manifest::{ApplicationManifest, DatabaseAdapter};
use hegira_cli::{
    ApplicationContextRequest, CliExit,
    operations::{
        DatabaseOperation, OperationEffect, OperationErrorKind, OperationIntent,
        OperationPrerequisite, OperationProgram, OperationRequest, OperationStep, RuntimeProfile,
        plan_application_operation,
    },
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hegira-operation-plan-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn application(&self, composition: &str, database: &str) -> PathBuf {
        let root = self.0.join(format!("{composition}-{database}"));
        let mut args = vec![
            "new",
            "operation-app",
            "--destination",
            root.to_str().unwrap(),
            "--database",
            database,
        ];
        if composition != "default" {
            args.extend(["--composition", "minimal"]);
        }
        run(&self.0, &args);
        if composition == "identity-added" {
            run(&root, &["component", "add", "identity"]);
        }
        root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, args: &[&str]) {
    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(args)
        .env_clear()
        .env("PATH", "")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn request(root: &Path, intent: OperationIntent) -> OperationRequest {
    OperationRequest {
        application: ApplicationContextRequest::discover_from(root),
        intent,
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                result.insert(path.strip_prefix(root).unwrap().to_path_buf(), Vec::new());
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn six_compositions_have_deterministic_read_only_plans_for_every_intent() {
    let fixture = Fixture::new();
    for composition in ["default", "minimal", "identity-added"] {
        for database in ["sqlite", "postgres"] {
            let root = fixture.application(composition, database);
            let before = tree(&root);
            let provider = if database == "sqlite" {
                DatabaseAdapter::Sqlite
            } else {
                DatabaseAdapter::Postgres
            };
            let profile = if database == "sqlite" {
                RuntimeProfile::Sqlite
            } else {
                RuntimeProfile::Development
            };
            for intent in [
                OperationIntent::Develop,
                OperationIntent::Check,
                OperationIntent::Test,
                OperationIntent::ReleaseBuild,
                OperationIntent::DatabaseStatus { profile },
                OperationIntent::DatabaseMigrate { profile },
            ] {
                let plan =
                    plan_application_operation(&repository(), &request(&root, intent)).unwrap();
                let repeated =
                    plan_application_operation(&repository(), &request(&root, intent)).unwrap();
                let explicit = plan_application_operation(
                    &repository(),
                    &OperationRequest {
                        application: ApplicationContextRequest::explicit(
                            root.join("apps/web/src"),
                            "../../..",
                        ),
                        intent,
                    },
                )
                .unwrap();
                assert_eq!(plan, repeated);
                assert_eq!(plan, explicit);
                assert_eq!(plan.to_json().unwrap(), repeated.to_json().unwrap());
                assert_eq!(plan.render_human(), repeated.render_human());
                assert_eq!(plan.application_root(), root);
                let summary = plan.summary();
                assert_eq!(summary.output_schema, 1);
                assert_eq!(summary.intent, intent);
                assert_eq!(summary.database, provider);
                assert!(
                    summary.policy.explicit_intent_required
                        && summary.policy.trusted_source_required
                );
                assert_eq!(
                    summary.capabilities.len(),
                    if composition == "minimal" { 0 } else { 2 }
                );
                assert_eq!(
                    summary.modules.len(),
                    if composition == "minimal" { 0 } else { 1 }
                );
                for output in [
                    plan.to_json().unwrap(),
                    plan.render_human(),
                    format!("{plan:?}"),
                ] {
                    assert!(!output.contains(root.to_str().unwrap()));
                    assert!(!output.contains(repository().to_str().unwrap()));
                }
                for step in &summary.steps {
                    if let OperationStep::Tool {
                        program,
                        arguments,
                        environment,
                    } = step
                    {
                        assert_eq!(*program, OperationProgram::Cargo);
                        assert!(!arguments.iter().any(|arg| arg == "install"
                            || arg == "update"
                            || arg == "--ignored"
                            || arg == "--all-features"));
                        assert!(arguments.iter().any(|arg| arg.contains("--locked")));
                        assert!(environment.keys().all(|key| key == "APP_ENV"));
                    }
                }
            }
            assert_eq!(
                tree(&root),
                before,
                "planning must not even create a target/cache directory"
            );
        }
    }
}

#[test]
fn features_profiles_and_locked_arguments_match_the_current_application_contract() {
    let fixture = Fixture::new();
    for database in ["sqlite", "postgres"] {
        let root = fixture.application("minimal", database);
        let plan =
            plan_application_operation(&repository(), &request(&root, OperationIntent::Check))
                .unwrap();
        let OperationStep::Tool {
            arguments,
            environment,
            ..
        } = &plan.summary().steps[0]
        else {
            panic!("native check required")
        };
        assert_eq!(
            arguments,
            &vec![
                "check",
                "--locked",
                "--workspace",
                "--all-targets",
                "--no-default-features",
                "--features",
                &format!("app_server/ssr,app_server/db-{database}")
            ]
        );
        assert!(environment.is_empty());
        let OperationStep::Tool { arguments, .. } = &plan.summary().steps[1] else {
            panic!("hydration check required")
        };
        assert_eq!(
            arguments,
            &vec![
                "check",
                "--locked",
                "-p",
                "app_server",
                "--no-default-features",
                "--features",
                "hydrate",
                "--target",
                "wasm32-unknown-unknown"
            ]
        );
        let dev =
            plan_application_operation(&repository(), &request(&root, OperationIntent::Develop))
                .unwrap();
        let OperationStep::Tool {
            arguments,
            environment,
            ..
        } = &dev.summary().steps[0]
        else {
            panic!("watch required")
        };
        assert_eq!(&arguments[..2], ["leptos", "watch"]);
        assert!(arguments.contains(&format!("ssr,db-{database}")));
        assert!(arguments.contains(&"--bin-cargo-args=--locked".to_owned()));
        assert!(arguments.contains(&"--lib-cargo-args=--locked".to_owned()));
        assert_eq!(
            environment["APP_ENV"],
            if database == "sqlite" {
                "sqlite"
            } else {
                "development"
            }
        );
        let build = plan_application_operation(
            &repository(),
            &request(&root, OperationIntent::ReleaseBuild),
        )
        .unwrap();
        let OperationStep::Tool {
            arguments,
            environment,
            ..
        } = &build.summary().steps[0]
        else {
            panic!("build required")
        };
        assert_eq!(&arguments[..2], ["leptos", "build"]);
        assert!(arguments.contains(&"--release".to_owned()));
        assert!(environment.is_empty());
        assert!(
            build
                .summary()
                .prerequisites
                .contains(&OperationPrerequisite::CargoLeptos { version: "0.3.7" })
        );
    }
}

#[test]
fn schema_one_summary_has_a_closed_reviewed_json_shape() {
    let fixture = Fixture::new();
    let root = fixture.application("minimal", "sqlite");
    let plan =
        plan_application_operation(&repository(), &request(&root, OperationIntent::Check)).unwrap();
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let expected = serde_json::json!({
        "output_schema": 1,
        "application": "operation-app",
        "framework_version": version,
        "package": {"id": "hegira-canonical", "version": version},
        "components": [
            {"id": "layered-base", "version": version},
            {"id": "layered-leptos-minimal", "version": version}
        ],
        "modules": [], "capabilities": [],
        "database": "sqlite", "client": "leptos",
        "intent": {"operation": "check"},
        "effect": "compilation", "working_directory": "application-root",
        "policy": {
            "explicit_intent_required": true, "trusted_source_required": true,
            "production_migration_approval_required": false
        },
        "prerequisites": [
            {"kind": "application-rust-toolchain", "path": "rust-toolchain.toml"},
            {"kind": "cargo"},
            {"kind": "cargo-lockfile", "path": "Cargo.lock"},
            {"kind": "wasm-target", "target": "wasm32-unknown-unknown"}
        ],
        "steps": [
            {"kind": "tool", "program": "cargo", "environment": {}, "arguments": [
                "check", "--locked", "--workspace", "--all-targets", "--no-default-features",
                "--features", "app_server/ssr,app_server/db-sqlite"
            ]},
            {"kind": "tool", "program": "cargo", "environment": {}, "arguments": [
                "check", "--locked", "-p", "app_server", "--no-default-features",
                "--features", "hydrate", "--target", "wasm32-unknown-unknown"
            ]}
        ]
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&plan.to_json().unwrap()).unwrap(),
        expected
    );
    for intent in [
        OperationIntent::Develop,
        OperationIntent::Check,
        OperationIntent::Test,
        OperationIntent::ReleaseBuild,
        OperationIntent::DatabaseStatus {
            profile: RuntimeProfile::Sqlite,
        },
        OperationIntent::DatabaseMigrate {
            profile: RuntimeProfile::Production,
        },
    ] {
        assert_eq!(
            serde_json::from_str::<OperationIntent>(&serde_json::to_string(&intent).unwrap())
                .unwrap(),
            intent
        );
    }
}

#[test]
fn database_intents_declare_isolated_application_requirements_and_approval() {
    let fixture = Fixture::new();
    let root = fixture.application("default", "postgres");
    for (intent, effect, operation, approval) in [
        (
            OperationIntent::DatabaseStatus {
                profile: RuntimeProfile::Production,
            },
            OperationEffect::DatabaseRead,
            DatabaseOperation::Status,
            false,
        ),
        (
            OperationIntent::DatabaseMigrate {
                profile: RuntimeProfile::Production,
            },
            OperationEffect::DatabaseWrite,
            DatabaseOperation::Migrate,
            true,
        ),
    ] {
        let plan = plan_application_operation(&repository(), &request(&root, intent)).unwrap();
        assert_eq!(plan.summary().effect, effect);
        assert_eq!(
            plan.summary().policy.production_migration_approval_required,
            approval
        );
        assert_eq!(
            plan.summary().steps,
            vec![OperationStep::ApplicationDatabase {
                operation,
                profile: RuntimeProfile::Production,
                database: DatabaseAdapter::Postgres
            }]
        );
        assert!(
            plan.summary()
                .prerequisites
                .contains(&OperationPrerequisite::ApplicationDatabaseEntryPoint)
        );
        assert!(plan.render_human().contains("not an available executable"));
    }
    let error = plan_application_operation(
        &repository(),
        &request(
            &root,
            OperationIntent::DatabaseMigrate {
                profile: RuntimeProfile::Sqlite,
            },
        ),
    )
    .unwrap_err();
    assert_eq!(error.code, "runtime-profile");
    assert_eq!(error.exit(), CliExit::Validation);
}

#[test]
fn test_profile_requirements_respect_default_versus_minimal_template_origin() {
    let fixture = Fixture::new();
    for composition in ["default", "minimal", "identity-added"] {
        for database in ["sqlite", "postgres"] {
            let root = fixture.application(composition, database);
            let result = plan_application_operation(
                &repository(),
                &request(
                    &root,
                    OperationIntent::DatabaseStatus {
                        profile: RuntimeProfile::Test,
                    },
                ),
            );
            let compatible = if composition == "default" {
                database == "postgres"
            } else {
                database == "sqlite"
            };
            assert_eq!(result.is_ok(), compatible);
        }
    }
}

#[test]
fn summaries_and_diagnostics_do_not_read_or_echo_runtime_configuration() {
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let sentinel = "private-runtime-input-".repeat(4);
    fs::write(root.join("config/sqlite.yaml"), &sentinel).unwrap();
    let before = tree(&root);
    let plan = plan_application_operation(&repository(), &request(&root, OperationIntent::Develop))
        .unwrap();
    assert!(!plan.to_json().unwrap().contains(&sentinel));
    assert!(!plan.render_human().contains(&sentinel));
    assert_eq!(before, tree(&root));
    fs::write(root.join("hegira.toml"), format!("schema = '{sentinel}'")).unwrap();
    let error = plan_application_operation(&repository(), &request(&root, OperationIntent::Check))
        .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::Validation);
    assert!(!serde_json::to_string(&error).unwrap().contains(&sentinel));
    assert!(!error.to_string().contains(&sentinel));
    assert_eq!(error.output_schema, 1);
}

#[test]
fn unsupported_release_graph_capabilities_and_missing_roots_fail_before_planning() {
    let fixture = Fixture::new();
    let missing =
        plan_application_operation(&repository(), &request(&fixture.0, OperationIntent::Check))
            .unwrap_err();
    assert_eq!(missing.code, "application-context");
    let root = fixture.application("default", "sqlite");
    let path = root.join("hegira.toml");
    let original = fs::read_to_string(&path).unwrap();
    let current = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(&path, original.replace(&current, "v0.1.0")).unwrap();
    let error = plan_application_operation(&repository(), &request(&root, OperationIntent::Check))
        .unwrap_err();
    assert_eq!(error.code, "application-compatibility");
    assert_eq!(error.exit(), CliExit::Conflict);
    // Invalid capability state must fail at the existing manifest boundary,
    // not be made valid by relaxing its consistency rules for this fixture.
    let source = original.replace("\"authorization\"", "\"unsupported-capability\"");
    fs::write(&path, source).unwrap();
    let error = plan_application_operation(&repository(), &request(&root, OperationIntent::Check))
        .unwrap_err();
    assert_eq!(error.code, "application-context");
    let mut manifest = ApplicationManifest::from_toml(&original).unwrap();
    // A valid known additive component still conflicts with the default
    // Identity root in the authenticated composition graph.
    manifest.composition.as_mut().unwrap().components.push(
        application_manifest::InstalledComponent {
            id: application_manifest::IDENTITY_COMPONENT.to_owned(),
            version: current,
        },
    );
    fs::write(&path, manifest.to_toml().unwrap()).unwrap();
    let error = plan_application_operation(&repository(), &request(&root, OperationIntent::Check))
        .unwrap_err();
    assert_eq!(error.code, "application-composition");
    fs::write(&path, &original).unwrap();
    let error = plan_application_operation(
        &fixture.0.join("absent-source"),
        &request(&root, OperationIntent::Check),
    )
    .unwrap_err();
    assert_eq!(error.code, "component-package");
    assert_eq!(error.exit(), CliExit::Internal);
}

#[test]
fn closed_intents_reject_unknown_operations_profiles_and_arguments() {
    for input in [
        r#"{"operation":"deploy"}"#,
        r#"{"operation":"check","arguments":["install"]}"#,
        r#"{"operation":"database-status","profile":"custom"}"#,
        r#"{"operation":"database-migrate"}"#,
    ] {
        assert!(serde_json::from_str::<OperationIntent>(input).is_err());
    }
    assert_eq!(
        serde_json::from_str::<OperationIntent>(r#"{"operation":"check"}"#).unwrap(),
        OperationIntent::Check
    );
}

#[cfg(unix)]
#[test]
fn symlinked_and_ambiguous_roots_are_rejected_without_mutation() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let root = fixture.application("default", "sqlite");
    let alias = fixture.0.join("alias");
    symlink(&root, &alias).unwrap();
    let error = plan_application_operation(&repository(), &request(&alias, OperationIntent::Check))
        .unwrap_err();
    assert_eq!(error.code, "application-context");
    let nested = root.join("apps/nested");
    fs::create_dir(&nested).unwrap();
    fs::copy(root.join("hegira.toml"), nested.join("hegira.toml")).unwrap();
    let error =
        plan_application_operation(&repository(), &request(&nested, OperationIntent::Check))
            .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::Conflict);
    fs::remove_file(nested.join("hegira.toml")).unwrap();
    let config = root.join("config");
    fs::rename(&config, root.join("original-config")).unwrap();
    symlink(root.join("original-config"), &config).unwrap();
    let error = plan_application_operation(&repository(), &request(&root, OperationIntent::Check))
        .unwrap_err();
    assert_eq!(error.code, "application-context");
}
