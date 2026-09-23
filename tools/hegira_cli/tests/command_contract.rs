use std::{
    ffi::OsString,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn hegira(arguments: &[&str]) -> Output {
    let environment = TestDirectory::new("process-environment");
    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(arguments)
        .env_clear()
        .env("HOME", environment.path())
        .env("USERPROFILE", environment.path())
        .env("XDG_CONFIG_HOME", environment.path())
        .env("PATH", "")
        .current_dir(environment.path())
        .output()
        .expect("hegira command should run");
    assert_eq!(
        fs::read_dir(environment.path()).unwrap().count(),
        0,
        "CLI must not write working-directory or user-home state"
    );
    result
}

fn hegira_at(working_directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(arguments)
        .env_clear()
        .env("HOME", working_directory)
        .env("USERPROFILE", working_directory)
        .env("XDG_CONFIG_HOME", working_directory)
        .env("PATH", "")
        .current_dir(working_directory)
        .output()
        .expect("hegira command should run")
}

#[test]
fn top_level_help_is_human_readable_output() {
    let result = hegira(&["--help"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("help should be UTF-8");
    assert!(output.starts_with("Create and maintain Hegira applications"));
    assert!(output.contains("Usage: hegira <COMMAND>"));
    assert!(output.contains("new"));
    assert!(output.contains("inspect"));
    assert!(output.contains("doctor"));
    assert!(output.contains("component"));
    assert!(output.contains("generate"));
}

#[test]
fn component_add_help_exposes_reviewable_mutation_options() {
    let result = hegira(&["component", "add", "--help"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("help should be UTF-8");
    assert!(output.contains("Usage: hegira component add"));
    assert!(output.contains("<COMPONENT>"));
    assert!(output.contains("--application-root"));
    assert!(output.contains("--dry-run"));
    assert!(output.contains("--json"));
}

#[test]
fn repeated_component_add_is_a_stable_non_destructive_conflict() {
    let output = TestDirectory::new("component-already-installed");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "component-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let before = output_tree(&application);

    let result = hegira_at(
        &application,
        &["component", "add", "layered-leptos-identity"],
    );

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("already installed"));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn identity_installation_is_reviewable_atomic_and_repeated_add_is_safe() {
    for database in ["sqlite", "postgres"] {
        let output = TestDirectory::new(&format!("identity-install-{database}"));
        let application = output.path().join("application");
        let created = hegira(&[
            "new",
            "component-app",
            "--destination",
            path_argument(&application),
            "--composition",
            "minimal",
            "--database",
            database,
        ]);
        assert!(created.status.success(), "{:?}", created.stderr);
        let before = output_tree(&application);

        let dry_run = hegira_at(
            &application,
            &["component", "add", "identity", "--dry-run", "--json"],
        );
        assert!(dry_run.status.success(), "{:?}", dry_run.stderr);
        let dry_run: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
        assert_eq!(dry_run["mode"], "dry-run");
        assert_eq!(dry_run["outcome"], "planned");
        assert_eq!(output_tree(&application), before);

        let applied = hegira_at(&application, &["component", "add", "identity", "--json"]);
        assert!(applied.status.success(), "{:?}", applied.stderr);
        let applied: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
        assert_eq!(applied["outcome"], "applied");
        assert_eq!(dry_run["plan"], applied["plan"]);

        let manifest = fs::read_to_string(application.join("hegira.toml")).unwrap();
        assert!(manifest.contains("id = \"identity\""));
        assert!(manifest.contains("\"authentication\""));
        assert!(manifest.contains("\"authorization\""));
        let operations =
            fs::read_to_string(application.join("crates/infrastructure/src/operations.rs"))
                .unwrap();
        assert!(operations.contains(&format!(
            "identity_sqlx::identity::migrations::{database}_migration_source()"
        )));
        let routes = fs::read_to_string(application.join("apps/web/src/routes.rs")).unwrap();
        assert!(routes.contains("<identity_leptos::identity::routes::IdentityRoutes/>"));
        assert!(!routes.contains("<Route path=StaticSegment(\"\") view=DashboardRoute/>"));

        let after = output_tree(&application);
        let repeated = hegira_at(&application, &["component", "add", "identity"]);
        assert_eq!(repeated.status.code(), Some(4));
        assert!(String::from_utf8_lossy(&repeated.stderr).contains("already installed"));
        assert_eq!(output_tree(&application), after);
    }
}

#[test]
fn identity_installation_preserves_an_occupied_application_artifact() {
    let output = TestDirectory::new("identity-install-occupied");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "component-app",
        "--destination",
        path_argument(&application),
        "--composition",
        "minimal",
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    fs::write(
        application.join("apps/server/src/identity_runtime.rs"),
        "// owned by app\n",
    )
    .unwrap();
    let before = output_tree(&application);

    let result = hegira_at(&application, &["component", "add", "identity"]);
    assert_eq!(result.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&result.stderr).contains("identity_runtime.rs"));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn unknown_component_fails_validation_without_application_writes() {
    let output = TestDirectory::new("component-unknown");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "component-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let before = output_tree(&application);

    let result = hegira_at(
        &application,
        &[
            "component",
            "add",
            "unknown-component",
            "--dry-run",
            "--json",
        ],
    );

    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("missing-component"));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn component_without_bundled_contributions_cannot_publish_partial_state() {
    let output = TestDirectory::new("component-without-contributions");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "component-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        application.join("hegira.toml"),
        format!(
            r#"schema = 2
application = "component-app"

[framework]
repository = "https://github.com/furkancemalcaliskan/hegira.git"
version = "{version}"

[selection]
databases = ["sqlite"]
clients = ["leptos"]

[composition]
capabilities = []

[composition.package]
id = "hegira-canonical"
version = "{version}"

[[composition.components]]
id = "layered-base"
version = "{version}"
"#
        ),
    )
    .unwrap();
    let before = output_tree(&application);

    let result = hegira_at(
        output.path(),
        &[
            "component",
            "add",
            "layered-leptos-identity",
            "--application-root",
            path_argument(&application),
            "--dry-run",
            "--json",
        ],
    );

    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("does not declare additive installation contributions")
    );
    assert_eq!(output_tree(&application), before);
}

#[test]
fn migration_help_exposes_reviewable_mutation_options() {
    let result = hegira(&["generate", "migration", "--help"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("help should be UTF-8");
    assert!(output.contains("Usage: hegira generate migration"));
    assert!(output.contains("--application-root"));
    assert!(output.contains("--dry-run"));
    assert!(output.contains("--json"));
}

#[test]
fn resource_help_exposes_typed_fields_and_reviewable_mutation_options() {
    let result = hegira(&["generate", "resource", "--help"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("help should be UTF-8");
    assert!(output.contains("Usage: hegira generate resource"));
    assert!(output.contains("--field <NAME:TYPE>"));
    assert!(output.contains("--plural"));
    assert!(output.contains("--application-root"));
    assert!(output.contains("--dry-run"));
    assert!(output.contains("--json"));
}

#[test]
fn complete_resource_dry_run_and_apply_share_one_atomic_plan() {
    let output = TestDirectory::new("complete-resource");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "resource-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let before = output_tree(&application);
    let arguments = [
        "generate",
        "resource",
        "OrderItem",
        "--field",
        "name:string",
        "--field",
        "published_at:datetime?",
        "--json",
    ];

    let dry_run = hegira_at(
        &application,
        &[
            "generate",
            "resource",
            "OrderItem",
            "--field",
            "name:string",
            "--field",
            "published_at:datetime?",
            "--dry-run",
            "--json",
        ],
    );
    assert!(dry_run.status.success(), "{:?}", dry_run.stderr);
    assert!(dry_run.stderr.is_empty());
    let dry_run: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(dry_run["mode"], "dry-run");
    assert_eq!(dry_run["outcome"], "planned");
    assert_eq!(dry_run["changed_files"], 20);
    assert_eq!(output_tree(&application), before);

    let applied = hegira_at(&application, &arguments);
    assert!(applied.status.success(), "{:?}", applied.stderr);
    assert!(applied.stderr.is_empty());
    let applied: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(applied["mode"], "apply");
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(dry_run["plan"], applied["plan"]);
    for path in [
        "crates/domain/src/order_item.rs",
        "crates/application_contracts/src/order_item.rs",
        "crates/application/src/order_item.rs",
        "crates/infrastructure/src/order_item.rs",
        "crates/presentation/src/order_item.rs",
        "apps/web/src/order_item.rs",
        "crates/infrastructure/migrations/sqlite/1000000_order_item.sql",
    ] {
        assert!(application.join(path).is_file(), "missing {path}");
    }
    let infrastructure =
        fs::read_to_string(application.join("crates/infrastructure/src/order_item.rs")).unwrap();
    assert!(infrastructure.contains("OrderItemAuthorizationAdapter"));
    let server = fs::read_to_string(application.join("apps/server/src/server.rs")).unwrap();
    assert!(server.contains("app_presentation::order_item::bearer_api_routes"));
    assert!(server.contains("app_web::order_item::OrderItemLeptosServices"));

    let after = output_tree(&application);
    let repeated = hegira_at(
        &application,
        &[
            "generate",
            "resource",
            "OrderItem",
            "--field",
            "name:string",
        ],
    );
    assert_eq!(repeated.status.code(), Some(4));
    assert!(repeated.stdout.is_empty());
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("collides"));
    assert_eq!(output_tree(&application), after);
}

#[test]
fn minimal_resource_generation_fails_before_writes_with_stable_capability_diagnostics() {
    for database in ["sqlite", "postgres"] {
        let output = TestDirectory::new(&format!("resource-capabilities-{database}"));
        let application = output.path().join("application");
        let created = hegira(&[
            "new",
            "resource-app",
            "--destination",
            path_argument(&application),
            "--composition",
            "minimal",
            "--database",
            database,
        ]);
        assert!(created.status.success(), "{:?}", created.stderr);
        let before = output_tree(&application);

        let human = hegira_at(
            &application,
            &[
                "generate",
                "resource",
                "OrderItem",
                "--field",
                "name:string",
            ],
        );
        assert_eq!(human.status.code(), Some(3));
        assert!(human.stdout.is_empty());
        let diagnostic = String::from_utf8(human.stderr).unwrap();
        assert!(diagnostic.contains("missing: authentication, authorization"));
        assert!(diagnostic.contains("hegira component add identity"));
        assert_eq!(output_tree(&application), before);

        let json_arguments = [
            "generate",
            "resource",
            "OrderItem",
            "--field",
            "name:string",
            "--dry-run",
            "--json",
        ];
        let first = hegira_at(&application, &json_arguments);
        let second = hegira_at(&application, &json_arguments);
        assert_eq!(first.status.code(), Some(3));
        assert_eq!(first.stdout, second.stdout);
        assert!(first.stdout.is_empty());
        assert_eq!(first.stderr, second.stderr);
        let diagnostic: serde_json::Value = serde_json::from_slice(&first.stderr).unwrap();
        assert_eq!(diagnostic["output_schema"], 1);
        assert_eq!(diagnostic["code"], "missing-capabilities");
        assert_eq!(
            diagnostic["required"],
            serde_json::json!(["authentication", "authorization"])
        );
        assert_eq!(diagnostic["missing"], diagnostic["required"]);
        assert!(
            diagnostic["hint"]
                .as_str()
                .unwrap()
                .contains("hegira component add identity")
        );
        assert_eq!(output_tree(&application), before);
    }
}

#[test]
fn identity_added_minimal_resource_uses_its_own_web_shell_and_atomic_plan() {
    for database in ["sqlite", "postgres"] {
        let output = TestDirectory::new(&format!("minimal-resource-{database}"));
        let application = output.path().join("application");
        let created = hegira(&[
            "new",
            "resource-app",
            "--destination",
            path_argument(&application),
            "--composition",
            "minimal",
            "--database",
            database,
        ]);
        assert!(created.status.success(), "{:?}", created.stderr);
        let installed = hegira_at(&application, &["component", "add", "identity"]);
        assert!(installed.status.success(), "{:?}", installed.stderr);
        let before = output_tree(&application);
        let arguments = [
            "generate",
            "resource",
            "OrderItem",
            "--field",
            "name:string",
            "--json",
        ];
        let preview = hegira_at(
            &application,
            &[
                "generate",
                "resource",
                "OrderItem",
                "--field",
                "name:string",
                "--dry-run",
                "--json",
            ],
        );
        assert!(preview.status.success(), "{:?}", preview.stderr);
        assert_eq!(output_tree(&application), before);
        let applied = hegira_at(&application, &arguments);
        assert!(applied.status.success(), "{:?}", applied.stderr);
        let preview: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
        let applied: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
        assert_eq!(preview["plan"], applied["plan"]);
        let web = fs::read_to_string(application.join("apps/web/src/order_item.rs")).unwrap();
        assert!(web.contains("MinimalI18n"));
        assert!(web.contains("<RequirePermission permission=PermissionName(LIST_PERMISSION)>"));
        assert!(!web.contains("app::layout"));
        let dashboard = fs::read_to_string(application.join("apps/web/src/dashboard.rs")).unwrap();
        assert!(dashboard.contains("href=\"/order-items\""));
        assert!(dashboard.contains("LIST_PERMISSION"));
        let routes = fs::read_to_string(application.join("apps/web/src/routes.rs")).unwrap();
        assert!(routes.contains("StaticSegment(\"order-items\")"));
        let infrastructure_manifest =
            fs::read_to_string(application.join("crates/infrastructure/Cargo.toml")).unwrap();
        assert!(infrastructure_manifest.contains("app_application = { workspace = true }"));
        assert!(infrastructure_manifest.contains("app_domain = { workspace = true }"));
        let web_manifest = fs::read_to_string(application.join("apps/web/Cargo.toml")).unwrap();
        assert!(web_manifest.contains("app_application_contracts = { workspace = true }"));
        assert!(web_manifest.contains("leptos_support = { workspace = true }"));
        let runtime =
            fs::read_to_string(application.join("apps/server/src/identity_runtime.rs")).unwrap();
        assert!(runtime.contains("pub fn services(&self) -> &AppServices"));
        assert!(runtime.contains("app_presentation::order_item::openapi_document()"));
        let after = output_tree(&application);
        let repeated = hegira_at(&application, &arguments);
        assert_eq!(repeated.status.code(), Some(4));
        assert_eq!(output_tree(&application), after);
    }
}

#[test]
fn identity_added_minimal_resource_missing_integration_fails_without_partial_writes() {
    let output = TestDirectory::new("minimal-resource-missing-integration");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "resource-app",
        "--destination",
        path_argument(&application),
        "--composition",
        "minimal",
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let installed = hegira_at(&application, &["component", "add", "identity"]);
    assert!(installed.status.success(), "{:?}", installed.stderr);
    fs::remove_file(application.join("apps/web/src/dashboard.rs")).unwrap();
    let before = output_tree(&application);
    let result = hegira_at(
        &application,
        &[
            "generate",
            "resource",
            "OrderItem",
            "--field",
            "name:string",
        ],
    );
    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("apps/web/src/dashboard.rs"));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn invalid_resource_fields_fail_before_application_writes() {
    let output = TestDirectory::new("invalid-resource-field");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "resource-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let before = output_tree(&application);

    let result = hegira_at(
        &application,
        &["generate", "resource", "Order", "--field", "total:decimal"],
    );

    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported scalar"));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn partial_resource_conflicts_do_not_publish_any_other_layer() {
    let output = TestDirectory::new("partial-resource-conflict");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "resource-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    fs::write(
        application.join("crates/application/src/order.rs"),
        "preserved partial resource",
    )
    .unwrap();
    let before = output_tree(&application);

    let result = hegira_at(
        &application,
        &["generate", "resource", "Order", "--field", "name:string"],
    );

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert_eq!(output_tree(&application), before);
    assert!(!application.join("crates/domain/src/order.rs").exists());
}

#[test]
fn sqlite_migration_dry_run_and_apply_share_one_append_only_plan() {
    let output = TestDirectory::new("sqlite-migration");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "migration-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let historical = fs::read(
        application
            .join("crates/infrastructure/migrations/sqlite/009_retire_catalog_persistence.sql"),
    )
    .unwrap();

    let dry_run = hegira_at(
        &application,
        &["generate", "migration", "add_orders", "--dry-run", "--json"],
    );
    assert!(dry_run.status.success(), "{:?}", dry_run.stderr);
    assert!(dry_run.stderr.is_empty());
    let dry_run: serde_json::Value = serde_json::from_slice(&dry_run.stdout).unwrap();
    assert_eq!(dry_run["mode"], "dry-run");
    assert_eq!(dry_run["outcome"], "planned");
    assert_eq!(dry_run["changed_files"], 2);
    let migration =
        application.join("crates/infrastructure/migrations/sqlite/1000000_add_orders.sql");
    let state = application.join("crates/infrastructure/migrations/.hegira-generator.toml");
    assert!(!migration.exists());
    assert!(!state.exists());

    let applied = hegira_at(
        &application,
        &["generate", "migration", "add_orders", "--json"],
    );
    assert!(applied.status.success(), "{:?}", applied.stderr);
    let applied: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(dry_run["plan"], applied["plan"]);
    assert_eq!(applied["outcome"], "applied");
    assert!(migration.is_file());
    assert!(state.is_file());
    assert_eq!(
        fs::read(
            application
                .join("crates/infrastructure/migrations/sqlite/009_retire_catalog_persistence.sql")
        )
        .unwrap(),
        historical
    );

    let repeated = hegira_at(&application, &["generate", "migration", "add_orders"]);
    assert_eq!(repeated.status.code(), Some(4));
    assert!(repeated.stdout.is_empty());
    assert!(
        String::from_utf8(repeated.stderr)
            .unwrap()
            .contains("already exists at version 1000000")
    );
}

#[test]
fn postgres_manifest_selects_only_the_postgres_migration_history() {
    let output = TestDirectory::new("postgres-migration");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "postgres-migration-app",
        "--destination",
        path_argument(&application),
        "--database",
        "postgres",
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);

    let result = hegira_at(
        output.path(),
        &[
            "generate",
            "migration",
            "add_orders",
            "--application-root",
            path_argument(&application),
        ],
    );

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(
        application
            .join("crates/infrastructure/migrations/postgres/1000000_add_orders.sql")
            .is_file()
    );
    assert!(
        !application
            .join("crates/infrastructure/migrations/sqlite/1000000_add_orders.sql")
            .exists()
    );
    let source = fs::read_to_string(
        application.join("crates/infrastructure/migrations/postgres/1000000_add_orders.sql"),
    )
    .unwrap();
    assert!(source.contains("Application-owned PostgreSQL migration"));
    assert!(!source.contains("DATABASE_URL"));
}

#[test]
fn invalid_migration_identity_fails_before_application_writes() {
    let output = TestDirectory::new("invalid-migration");
    let application = output.path().join("application");
    let created = hegira(&[
        "new",
        "migration-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let before = output_tree(&application);

    let result = hegira_at(&application, &["generate", "migration", "../drop_table"]);

    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert_eq!(output_tree(&application), before);
}

#[test]
fn version_is_human_readable_output() {
    let result = hegira(&["--version"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    assert_eq!(
        String::from_utf8(result.stdout).expect("version should be UTF-8"),
        format!("hegira {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn new_help_succeeds_without_accessing_global_configuration() {
    let result = hegira(&["new", "--help"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("help should be UTF-8");
    assert!(output.contains("Usage: hegira new"));
}

#[test]
fn invalid_usage_is_reported_only_on_standard_error() {
    let result = hegira(&["unknown"]);

    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    let diagnostic = String::from_utf8(result.stderr).expect("diagnostic should be UTF-8");
    assert!(diagnostic.contains("unrecognized subcommand 'unknown'"));
    assert!(diagnostic.contains("Usage: hegira <COMMAND>"));
    assert!(!diagnostic.contains("stack backtrace"));
}

#[test]
fn new_requires_an_application_name_and_destination() {
    let result = hegira(&["new"]);

    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    let diagnostic = String::from_utf8(result.stderr).expect("diagnostic should be UTF-8");
    assert!(
        diagnostic.contains("non-interactive application creation requires a name and destination")
    );
    assert!(diagnostic.contains("provide `hegira new <NAME> --destination <PATH>`"));
}

#[test]
fn interactive_defaults_match_explicit_default_generation() {
    let output = TestDirectory::new("interactive-defaults");
    let guided_destination = output.path().join("guided");
    let explicit_destination = output.path().join("explicit");
    let input = format!("guided-app\n{}\n\n\n\n\n", guided_destination.display());

    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], &input);
    assert_eq!(exit, 0, "{diagnostics}");
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Application name: "));
    assert!(stdout.contains("Database [sqlite] (sqlite/postgres): "));
    assert!(stdout.contains("Application summary:"));
    assert!(stdout.contains("Database: sqlite"));

    let result = hegira(&[
        "new",
        "guided-app",
        "--destination",
        path_argument(&explicit_destination),
    ]);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(
        output_tree(&guided_destination),
        output_tree(&explicit_destination)
    );
}

#[test]
fn interactive_selection_reprompts_and_maps_to_supported_values() {
    let output = TestDirectory::new("interactive-postgres");
    let destination = output.path().join("ApplicationOutput");
    let input = format!(
        "guided-postgres\n{}\nmysql\nPostgres\nunknown\nLeptos\nother\nIdentity\nyes\n",
        destination.display()
    );

    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], &input);

    assert_eq!(exit, 0, "{diagnostics}");
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Please choose `sqlite` or `postgres`."));
    assert!(stdout.contains("The currently supported client is `leptos`."));
    assert!(stdout.contains("Please choose `identity` or `minimal`."));
    assert!(stdout.contains("Destination: "));
    assert!(stdout.contains("Database: postgres"));
    let manifest = fs::read_to_string(destination.join("hegira.toml"))
        .expect("application manifest should exist");
    assert!(manifest.contains("databases = [\"postgres\"]"));
}

#[test]
fn interactive_cancellation_leaves_no_output() {
    let output = TestDirectory::new("interactive-cancel");
    let destination = output.path().join("cancelled");
    let input = format!("cancelled-app\n{}\n\n\n\nn\n", destination.display());

    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], &input);

    assert_eq!(exit, 0);
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Application summary:"));
    assert!(stdout.contains("Cancelled; no files were written."));
    assert!(!destination.exists());
}

#[test]
fn interactive_end_of_input_cancels_safely() {
    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], "");

    assert_eq!(exit, 0);
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Application name: "));
    assert!(stdout.contains("Cancelled; no files were written."));
}

#[test]
fn new_generates_the_default_layered_application_without_prompts() {
    let output = TestDirectory::new("default");
    let destination = output.path().join("application");
    let result = hegira(&[
        "new",
        "my-application",
        "--destination",
        path_argument(&destination),
    ]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("output should be UTF-8");
    assert!(stdout.contains("Created my-application"));
    assert!(stdout.contains("APP_ENV=sqlite cargo leptos watch"));
    assert!(stdout.contains("--bin-cargo-args=--locked --lib-cargo-args=--locked"));

    let manifest = fs::read_to_string(destination.join("hegira.toml"))
        .expect("application manifest should exist");
    assert!(manifest.contains("application = \"my-application\""));
    assert!(manifest.contains("databases = [\"sqlite\"]"));
    assert!(manifest.contains("clients = [\"leptos\"]"));
    assert!(manifest.contains("\"layered-leptos-identity\""));

    let server_manifest = fs::read_to_string(destination.join("apps/server/Cargo.toml"))
        .expect("server manifest should exist");
    let workspace_manifest = fs::read_to_string(destination.join("Cargo.toml"))
        .expect("workspace manifest should exist");
    assert!(server_manifest.contains("default = [\"db-sqlite\"]"));
    assert!(workspace_manifest.contains("git = \"https://github.com/"));
    assert!(workspace_manifest.contains("tag = \"v0.5.0\""));
    assert!(!workspace_manifest.contains(repository_root().to_string_lossy().as_ref()));
    let lockfile = fs::read_to_string(destination.join("Cargo.lock"))
        .expect("application lockfile should exist");
    assert!(lockfile.contains("?tag=v0.5.0#"));
}

#[test]
fn new_represents_an_explicit_postgres_selection_consistently() {
    let output = TestDirectory::new("postgres");
    let destination = output.path().join("application");
    let result = hegira(&[
        "new",
        "postgres-app",
        "--destination",
        path_argument(&destination),
        "--database",
        "postgres",
        "--client",
        "leptos",
        "--component",
        "identity",
    ]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("output should be UTF-8");
    assert!(stdout.contains("APP_ENV=development cargo leptos watch"));
    assert!(stdout.contains("--bin-features ssr,db-postgres"));
    assert!(stdout.contains("--bin-cargo-args=--locked --lib-cargo-args=--locked"));

    let manifest = fs::read_to_string(destination.join("hegira.toml"))
        .expect("application manifest should exist");
    assert!(manifest.contains("databases = [\"postgres\"]"));
    let server_manifest = fs::read_to_string(destination.join("apps/server/Cargo.toml"))
        .expect("server manifest should exist");
    let web_manifest = fs::read_to_string(destination.join("apps/web/Cargo.toml"))
        .expect("web manifest should exist");
    assert!(server_manifest.contains("default = [\"db-postgres\"]"));
    assert!(web_manifest.contains("default = [\"db-postgres\"]"));
}

#[test]
fn identical_requests_create_byte_equivalent_output_trees() {
    let output = TestDirectory::new("deterministic");
    let first = output.path().join("first");
    let second = output.path().join("second");

    for destination in [&first, &second] {
        let result = hegira(&[
            "new",
            "deterministic-app",
            "--destination",
            path_argument(destination),
        ]);
        assert!(result.status.success(), "{:?}", result.stderr);
    }

    assert_eq!(output_tree(&first), output_tree(&second));
}

#[test]
fn existing_destination_is_a_conflict_and_is_not_modified() {
    let output = TestDirectory::new("conflict");
    let destination = output.path().join("application");
    fs::create_dir(&destination).expect("destination fixture should exist");
    fs::write(destination.join("preserved.txt"), "preserved").expect("sentinel should be written");

    let result = hegira(&[
        "new",
        "my-application",
        "--destination",
        path_argument(&destination),
    ]);

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("destination already exists"));
    assert_eq!(
        fs::read_to_string(destination.join("preserved.txt")).expect("sentinel should remain"),
        "preserved"
    );
}

#[test]
fn inspect_reports_the_discovered_application_without_writing() {
    let root = TestDirectory::new("inspect-human");
    let application = root.path().join("application");
    let created = hegira(&[
        "new",
        "inspection-app",
        "--destination",
        path_argument(&application),
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let runtime_secret = "inspection-runtime-secret-must-not-appear";
    fs::write(
        application.join("config/development.yaml"),
        format!("private_value: {runtime_secret}\n"),
    )
    .unwrap();
    let before = output_tree(&application);

    let result = hegira_at(&application.join("apps/web/src"), &["inspect"]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("inspection should be UTF-8");
    assert!(output.contains("Application: inspection-app\n"));
    assert!(output.contains(&format!(
        "Root: {}\n",
        fs::canonicalize(&application).unwrap().display()
    )));
    assert!(output.contains("Manifest schema: 2\n"));
    assert!(output.contains("Framework: https://github.com/furkancemalcaliskan/hegira.git"));
    assert!(output.contains("Composition status: compatible\n"));
    assert!(output.contains("Component package: hegira-canonical @ v0.5.0\n"));
    assert!(
        output.contains("Components: layered-base @ v0.5.0, layered-leptos-identity @ v0.5.0\n")
    );
    assert!(output.contains("Modules: identity @ v0.5.0\n"));
    assert!(output.contains("Capabilities: authentication, authorization\n"));
    assert!(output.contains("Databases: sqlite\n"));
    assert!(output.contains("Clients: leptos\n"));
    assert!(output.ends_with("Mutation compatibility: compatible\n"));
    assert!(!output.contains(runtime_secret));
    assert!(!output.contains(repository_root().to_string_lossy().as_ref()));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn doctor_reports_default_minimal_and_installed_identity_without_application_writes() {
    for composition in ["identity", "minimal"] {
        let root = TestDirectory::new(&format!("doctor-{composition}"));
        let application = root.path().join("application");
        let created = hegira(&[
            "new",
            "doctor-app",
            "--destination",
            path_argument(&application),
            "--composition",
            composition,
        ]);
        assert!(created.status.success(), "{:?}", created.stderr);
        let before = output_tree(&application);

        let first = hegira_at(&application, &["doctor", "--json"]);
        let second = hegira_at(&application, &["doctor", "--json"]);
        assert_eq!(first.status.code(), Some(0));
        assert!(first.stderr.is_empty());
        assert_eq!(first.stdout, second.stdout);
        let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
        assert_eq!(report["output_schema"], 1);
        assert_eq!(report["status"], "warning");
        assert_eq!(report["checks"][0]["code"], "manifest");
        assert_eq!(report["checks"][0]["status"], "pass");
        assert_eq!(report["checks"][1]["code"], "composition");
        assert_eq!(report["checks"][1]["status"], "pass");
        assert_eq!(report["checks"][3]["code"], "managed-integrations");
        assert_eq!(report["checks"][3]["status"], "pass");
        assert_eq!(report["checks"][6]["code"], "cargo");
        assert_eq!(report["checks"][6]["status"], "warning");
        assert!(!String::from_utf8_lossy(&first.stdout).contains(path_argument(&application)));
        assert_eq!(output_tree(&application), before);

        let human = hegira_at(&application, &["doctor"]);
        assert_eq!(human.status.code(), Some(0));
        assert!(human.stderr.is_empty());
        let human = String::from_utf8(human.stdout).unwrap();
        assert!(human.contains("[PASS] managed-integrations"));
        assert!(human.contains("[WARN] cargo-leptos"));
        assert!(human.contains("Doctor status: warning"));
        assert_eq!(output_tree(&application), before);

        if composition == "minimal" {
            let installed = hegira_at(&application, &["component", "add", "identity"]);
            assert!(installed.status.success(), "{:?}", installed.stderr);
            let installed_tree = output_tree(&application);
            let report = hegira_at(&application, &["doctor", "--json"]);
            assert_eq!(report.status.code(), Some(0));
            let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
            assert_eq!(report["checks"][3]["status"], "pass");
            assert_eq!(output_tree(&application), installed_tree);
        }
    }
}

#[test]
fn doctor_reports_recovery_and_integration_failures_without_disclosing_source() {
    let root = TestDirectory::new("doctor-failure");
    let application = root.path().join("application");
    assert!(
        hegira(&[
            "new",
            "doctor-app",
            "--destination",
            path_argument(&application)
        ])
        .status
        .success()
    );

    let server_path = application.join("apps/server/src/server.rs");
    let server = fs::read_to_string(&server_path).unwrap();
    let secret = "doctor-source-secret-must-not-appear";
    fs::write(
        &server_path,
        server.replace("identity_http::bearer_api_routes", secret),
    )
    .unwrap();
    fs::write(application.join(".hegira-mutation.lock"), secret).unwrap();
    let before = output_tree(&application);

    let result = hegira_at(&application, &["doctor", "--json"]);
    assert_eq!(result.status.code(), Some(3));
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report["status"], "failure");
    assert_eq!(report["checks"][2]["code"], "recovery-marker");
    assert_eq!(report["checks"][2]["status"], "failure");
    assert_eq!(report["checks"][3]["code"], "managed-integrations");
    assert_eq!(report["checks"][3]["status"], "failure");
    assert!(!output.contains(secret));
    assert!(!output.contains(path_argument(&application)));
    assert_eq!(output_tree(&application), before);

    let human = hegira_at(&application, &["doctor"]);
    assert_eq!(human.status.code(), Some(3));
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("[FAIL] recovery-marker:"));
    assert!(human.contains("[FAIL] managed-integrations:"));
    assert!(human.ends_with("Doctor status: failure\n"));
    assert!(!human.contains(secret));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn doctor_describes_postgres_without_probing_a_database() {
    let root = TestDirectory::new("doctor-postgres");
    let application = root.path().join("application");
    let created = hegira(&[
        "new",
        "doctor-app",
        "--destination",
        path_argument(&application),
        "--database",
        "postgres",
    ]);
    assert!(created.status.success(), "{:?}", created.stderr);
    let before = output_tree(&application);

    let result = hegira_at(&application, &["doctor", "--json"]);
    assert_eq!(result.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["checks"][4]["code"], "database-provider");
    assert_eq!(report["checks"][4]["status"], "warning");
    assert!(
        report["checks"][4]["message"]
            .as_str()
            .unwrap()
            .contains("not probed")
    );
    assert_eq!(output_tree(&application), before);
}

#[cfg(unix)]
#[test]
fn doctor_rejects_symlinked_managed_source_without_reading_its_target() {
    use std::os::unix::fs::symlink;

    let root = TestDirectory::new("doctor-symlink");
    let application = root.path().join("application");
    assert!(
        hegira(&[
            "new",
            "doctor-app",
            "--destination",
            path_argument(&application)
        ])
        .status
        .success()
    );
    let secret = "doctor-symlink-target-secret-must-not-appear";
    let secret_path = root.path().join("secret.txt");
    fs::write(&secret_path, secret).unwrap();
    let routes = application.join("apps/web/src/routes.rs");
    fs::remove_file(&routes).unwrap();
    symlink(&secret_path, &routes).unwrap();

    let result = hegira_at(&application, &["doctor", "--json"]);
    assert_eq!(result.status.code(), Some(3));
    let output = String::from_utf8(result.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report["checks"][3]["status"], "failure");
    assert!(!output.contains(secret));
    assert!(!output.contains(path_argument(&secret_path)));
    assert_eq!(fs::read_to_string(&secret_path).unwrap(), secret);
}

#[cfg(unix)]
fn doctor_with_stubbed_tools(
    fixture: &TestDirectory,
    application: &Path,
    wasm_installed: bool,
    arguments: &[&str],
) -> Output {
    use std::os::unix::fs::PermissionsExt;

    let tools = fixture.path().join("tools");
    let home = fixture.path().join("home");
    fs::create_dir_all(&tools).unwrap();
    fs::create_dir_all(&home).unwrap();
    for binary in ["rustc", "cargo", "cargo-leptos", "node", "npm"] {
        let path = tools.join(binary);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let rustup = tools.join("rustup");
    let target = if wasm_installed {
        "wasm32-unknown-unknown"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    fs::write(
        &rustup,
        format!(
            "#!/bin/sh\nif [ -n \"${{APP__SECURITY__JWT_SECRET:-}}\" ]; then exit 77; fi\nprintf '%s\\n' '{target}'\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&rustup, fs::Permissions::from_mode(0o700)).unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(arguments)
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("PATH", &tools)
        .env(
            "APP__SECURITY__JWT_SECRET",
            "doctor-env-secret-must-not-appear",
        )
        .current_dir(application)
        .output()
        .expect("doctor should run with isolated tool fixtures");
    assert_eq!(fs::read_dir(home).unwrap().count(), 0);
    result
}

#[cfg(unix)]
#[test]
fn doctor_json_schema_order_and_success_are_stable_with_stubbed_prerequisites() {
    let fixture = TestDirectory::new("doctor-contract-pass");
    let application = fixture.path().join("application");
    assert!(
        hegira(&[
            "new",
            "doctor-app",
            "--destination",
            path_argument(&application),
        ])
        .status
        .success()
    );
    let config = application.join("config/development.yaml");
    let secret = "doctor-config-secret-must-not-appear";
    fs::write(&config, secret).unwrap();
    let before = output_tree(&application);

    let first = doctor_with_stubbed_tools(&fixture, &application, true, &["doctor", "--json"]);
    let second = doctor_with_stubbed_tools(&fixture, &application, true, &["doctor", "--json"]);
    let explicit = doctor_with_stubbed_tools(
        &fixture,
        &application,
        true,
        &[
            "doctor",
            "--application-root",
            path_argument(&application),
            "--json",
        ],
    );
    assert_eq!(first.status.code(), Some(0));
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stdout, explicit.stdout);
    let output = String::from_utf8(first.stdout).unwrap();
    assert!(
        output
            .starts_with("{\n  \"output_schema\": 1,\n  \"status\": \"pass\",\n  \"checks\": [\n")
    );
    let report: serde_json::Value = serde_json::from_str(&output).unwrap();
    let codes: Vec<_> = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|check| check["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "manifest",
            "composition",
            "recovery-marker",
            "managed-integrations",
            "database-provider",
            "rust-toolchain",
            "cargo",
            "cargo-leptos",
            "node",
            "npm",
            "wasm-target",
        ]
    );
    assert!(
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|check| check["status"] == "pass")
    );
    let first_check = output.split("\"code\": \"manifest\"").nth(1).unwrap();
    assert!(first_check.starts_with(",\n      \"status\": \"pass\",\n      \"message\": "));
    assert!(first_check.contains("\"action\": null"));
    for forbidden in [
        secret,
        "doctor-env-secret-must-not-appear",
        path_argument(&application),
    ] {
        assert!(!output.contains(forbidden));
    }

    let human = doctor_with_stubbed_tools(&fixture, &application, true, &["doctor"]);
    assert_eq!(human.status.code(), Some(0));
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.starts_with("[PASS] manifest: Application manifest is valid"));
    assert!(human.contains("[PASS] wasm-target:"));
    assert!(human.ends_with("Doctor status: pass\n"));
    assert_eq!(output_tree(&application), before);
}

#[cfg(unix)]
#[test]
fn doctor_warnings_remain_non_blocking_with_a_missing_wasm_target() {
    let fixture = TestDirectory::new("doctor-contract-warning");
    let application = fixture.path().join("application");
    assert!(
        hegira(&[
            "new",
            "doctor-app",
            "--destination",
            path_argument(&application),
        ])
        .status
        .success()
    );
    let before = output_tree(&application);

    let result = doctor_with_stubbed_tools(&fixture, &application, false, &["doctor", "--json"]);
    assert_eq!(result.status.code(), Some(0));
    assert!(result.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["status"], "warning");
    assert_eq!(report["checks"][10]["code"], "wasm-target");
    assert_eq!(report["checks"][10]["status"], "warning");
    assert!(
        report["checks"].as_array().unwrap()[..10]
            .iter()
            .all(|check| check["status"] == "pass")
    );
    assert_eq!(output_tree(&application), before);
}

#[cfg(unix)]
#[test]
fn doctor_incompatible_manifest_is_a_redacted_blocking_json_result() {
    let fixture = TestDirectory::new("doctor-contract-incompatible");
    let application = fixture.path().join("application");
    assert!(
        hegira(&[
            "new",
            "doctor-app",
            "--destination",
            path_argument(&application),
        ])
        .status
        .success()
    );
    let manifest_path = application.join("hegira.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    fs::write(&manifest_path, manifest.replace("v0.5.0", "v9.9.9")).unwrap();
    let before = output_tree(&application);

    let first = doctor_with_stubbed_tools(&fixture, &application, true, &["doctor", "--json"]);
    let second = doctor_with_stubbed_tools(&fixture, &application, true, &["doctor", "--json"]);
    assert_eq!(first.status.code(), Some(3));
    assert!(first.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let output = String::from_utf8(first.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(report["status"], "failure");
    assert_eq!(report["checks"][0]["status"], "failure");
    assert_eq!(report["checks"][1]["status"], "failure");
    assert!(!output.contains(path_argument(&application)));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn inspect_json_is_versioned_deterministic_and_matches_explicit_resolution() {
    let root = TestDirectory::new("inspect-json");
    let application = root.path().join("application");
    assert!(
        hegira(&[
            "new",
            "json-app",
            "--destination",
            path_argument(&application),
        ])
        .status
        .success()
    );
    let before = output_tree(&application);

    let discovered = hegira_at(&application.join("crates/domain"), &["inspect", "--json"]);
    let explicit = hegira_at(
        root.path(),
        &[
            "inspect",
            "--application-root",
            path_argument(&application),
            "--json",
        ],
    );

    assert!(discovered.status.success(), "{:?}", discovered.stderr);
    assert!(explicit.status.success(), "{:?}", explicit.stderr);
    assert_eq!(discovered.stdout, explicit.stdout);
    let document: serde_json::Value =
        serde_json::from_slice(&discovered.stdout).expect("inspection JSON should parse");
    assert_eq!(document["output_schema"], 2);
    assert_eq!(document["manifest"]["application"], "json-app");
    assert_eq!(document["manifest"]["schema"], 2);
    assert_eq!(
        document["manifest"]["composition"]["package"]["id"],
        "hegira-canonical"
    );
    assert_eq!(
        document["manifest"]["composition"]["modules"][0]["id"],
        "identity"
    );
    assert_eq!(document["manifest"]["selection"]["databases"][0], "sqlite");
    assert_eq!(document["manifest"]["selection"]["clients"][0], "leptos");
    assert_eq!(document["composition"]["status"], "compatible");
    assert_eq!(
        document["composition"]["components"][0]["id"],
        "layered-base"
    );
    assert_eq!(
        document["composition"]["components"][0]["version"],
        "v0.5.0"
    );
    assert_eq!(document["composition"]["modules"][0]["id"], "identity");
    assert_eq!(document["composition"]["databases"][0], "sqlite");
    assert_eq!(document["composition"]["clients"][0], "leptos");
    assert_eq!(
        document["composition"]["capabilities"],
        serde_json::json!(["authentication", "authorization"])
    );
    assert_eq!(
        document["composition"]["diagnostics"],
        serde_json::json!([])
    );
    assert_eq!(document["mutation_compatibility"]["status"], "compatible");
    assert_eq!(output_tree(&application), before);
}

#[test]
fn inspect_reports_unresolved_composition_without_mutating_the_application() {
    let root = TestDirectory::new("inspect-composition-conflict");
    let application = root.path().join("application");
    assert!(
        hegira(&[
            "new",
            "conflict-app",
            "--destination",
            path_argument(&application),
        ])
        .status
        .success()
    );
    let manifest_path = application.join("hegira.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    fs::write(
        &manifest_path,
        format!(
            "{manifest}\n[[composition.components]]\nid = \"unavailable-component\"\nversion = \"{version}\"\n"
        ),
    )
    .unwrap();
    let before = output_tree(&application);

    let human = hegira_at(&application, &["inspect"]);
    let json = hegira_at(&application, &["inspect", "--json"]);

    assert!(human.status.success(), "{:?}", human.stderr);
    assert!(json.status.success(), "{:?}", json.stderr);
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("Composition status: unresolved\n"));
    assert!(human.contains("Composition diagnostics:\n"));
    assert!(human.contains("missing-component: unavailable-component"));
    let document: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(document["composition"]["status"], "unresolved");
    assert_eq!(
        document["composition"]["diagnostics"][0]["kind"],
        "missing-component"
    );
    assert_eq!(document["mutation_compatibility"]["status"], "unsupported");
    assert_eq!(output_tree(&application), before);
}

#[test]
fn inspect_reports_unsupported_releases_without_mutating_or_failing() {
    let root = TestDirectory::new("inspect-unsupported");
    let application = root.path().join("application");
    assert!(
        hegira(&[
            "new",
            "old-app",
            "--destination",
            path_argument(&application),
        ])
        .status
        .success()
    );
    let manifest_path = application.join("hegira.toml");
    let current = format!("v{}", env!("CARGO_PKG_VERSION"));
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    fs::write(&manifest_path, manifest.replace(&current, "v0.1.0")).unwrap();
    let before = output_tree(&application);

    let result = hegira_at(&application, &["inspect"]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    let output = String::from_utf8_lossy(&result.stdout);
    assert!(output.contains("Composition status: unresolved\n"));
    assert!(output.contains("framework-version-mismatch: framework.version"));
    assert!(output.contains("Mutation compatibility: unsupported (framework.version is v0.1.0"));
    assert_eq!(output_tree(&application), before);
}

#[test]
fn inspect_missing_root_is_a_validation_failure_without_global_state() {
    let root = TestDirectory::new("inspect-missing");
    let result = hegira_at(root.path(), &["inspect"]);

    assert_eq!(result.status.code(), Some(3));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no hegira.toml"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

fn path_argument(path: &Path) -> &str {
    path.to_str().expect("test path should be UTF-8")
}

#[test]
fn adversarial_identities_fail_before_creating_parent_or_output() {
    let root = TestDirectory::new("invalid-identities");
    let destination = root.path().join("missing/application");
    for name in [
        "../escape",
        "/absolute",
        "BadName",
        "naïve",
        "app\nname",
        "app\u{1b}name",
        "con",
        "aux",
        "com1",
        "type",
        "self",
        "gen",
        "target",
        "a--b",
    ] {
        let result = hegira(&["new", name, "--destination", path_argument(&destination)]);
        assert_eq!(
            result.status.code(),
            Some(3),
            "{name:?}: {:?}",
            result.stderr
        );
        assert!(result.stdout.is_empty());
        assert!(!root.path().join("missing").exists());
    }
}

#[test]
fn adversarial_destinations_leave_user_data_unchanged() {
    let root = TestDirectory::new("invalid-destinations");
    fs::write(root.path().join("sentinel"), "preserved").unwrap();
    for path in [
        "child/../escape",
        "missing/application",
        "CON",
        "aux",
        "com1",
        "foo.",
        "foo/.",
        "naïve",
        "app\nname",
        "app\\name",
    ] {
        let destination = root.path().join(path);
        let result = hegira(&[
            "new",
            "safe-app",
            "--destination",
            path_argument(&destination),
        ]);
        assert_eq!(
            result.status.code(),
            Some(3),
            "{path:?}: {:?}",
            result.stderr
        );
        assert!(result.stdout.is_empty());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    assert_eq!(
        fs::read_to_string(root.path().join("sentinel")).unwrap(),
        "preserved"
    );
}

#[test]
fn interactive_identity_is_validated_without_silent_normalization() {
    let root = TestDirectory::new("interactive-invalid");
    let destination = root.path().join("application");
    let input = format!(" bad-name \n{}\n\n\n\ny\n", destination.display());
    let (exit, _, _) = interactive_hegira(&["new"], &input);
    assert_eq!(exit, 3);
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn non_utf8_destination_is_rejected_before_writing() {
    use std::os::unix::ffi::OsStringExt;
    let root = TestDirectory::new("non-utf8");
    let destination = root.path().join(OsString::from_vec(vec![b'a', 0xff]));
    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .env_clear()
        .env("HOME", root.path())
        .current_dir(root.path())
        .args(["new", "safe-app", "--destination"])
        .arg(destination)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn explicit_sibling_destination_still_works() {
    let root = TestDirectory::new("sibling");
    let cwd = root.path().join("caller");
    fs::create_dir(&cwd).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .env_clear()
        .env("HOME", root.path())
        .current_dir(cwd)
        .args(["new", "sibling-app", "--destination", "../application"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(root.path().join("application/hegira.toml").is_file());
}

#[test]
fn provider_snapshots_and_interactive_requests_match() {
    for (database, expected) in [
        ("sqlite", 10208568440786142037_u64),
        ("postgres", 5398016588184618970_u64),
    ] {
        let root = TestDirectory::new(database);
        let explicit = root.path().join("explicit");
        let guided = root.path().join("guided");
        let result = hegira(&[
            "new",
            "snapshot-app",
            "--destination",
            path_argument(&explicit),
            "--database",
            database,
            "--client",
            "leptos",
            "--component",
            "identity",
        ]);
        assert!(result.status.success(), "{:?}", result.stderr);
        let input = format!(
            "snapshot-app\n{}\n{database}\nleptos\nidentity\ny\n",
            guided.display()
        );
        let (exit, _, diagnostics) = interactive_hegira(&["new"], &input);
        assert_eq!(exit, 0, "{diagnostics}");
        let tree = output_tree(&explicit);
        assert_eq!(tree, output_tree(&guided));
        let manifest = fs::read_to_string(explicit.join("hegira.toml")).unwrap();
        assert!(manifest.contains("application = \"snapshot-app\""));
        assert!(manifest.contains(&format!("databases = [\"{database}\"]")));
        assert!(manifest.contains("clients = [\"leptos\"]"));
        assert!(manifest.contains("\"layered-leptos-identity\""));
        let workspace = fs::read_to_string(explicit.join("Cargo.toml")).unwrap();
        assert!(workspace.contains("tag = \"v0.5.0\""));
        assert!(!workspace.contains(repository_root().to_str().unwrap()));
        assert!(!explicit.join(".git").exists());
        assert!(!explicit.join("target").exists());
        // A committed fingerprint of every path and byte, including binary assets.
        // This is a regression snapshot, not a cryptographic integrity check.
        let mut fingerprint = 0xcbf29ce484222325_u64;
        for (path, bytes) in &tree {
            for part in [path.to_str().unwrap().as_bytes(), bytes.as_slice()] {
                for byte in (part.len() as u64).to_le_bytes().iter().chain(part) {
                    fingerprint = (fingerprint ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
                }
            }
        }
        assert_eq!(
            fingerprint, expected,
            "review {database} output before updating its snapshot"
        );
    }
}

#[test]
fn explicit_minimal_composition_is_module_free_and_deterministic() {
    let root = TestDirectory::new("minimal-composition");
    let destination = root.path().join("application");
    let result = hegira(&[
        "new",
        "minimal-app",
        "--destination",
        path_argument(&destination),
        "--database",
        "sqlite",
        "--client",
        "leptos",
        "--composition",
        "minimal",
    ]);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("no authentication or authorization capability")
    );

    let manifest = fs::read_to_string(destination.join("hegira.toml")).unwrap();
    assert!(manifest.contains("id = \"layered-base\""));
    assert!(manifest.contains("id = \"layered-leptos-minimal\""));
    assert!(!manifest.contains("id = \"identity\""));
    let manifest = application_manifest::ApplicationManifest::from_toml(&manifest).unwrap();
    let composition = manifest.composition.unwrap();
    assert!(composition.modules.is_empty());
    assert!(composition.capabilities.is_empty());

    let workspace = fs::read_to_string(destination.join("Cargo.toml")).unwrap();
    assert!(!workspace.contains("identity_"));
    let server = fs::read_to_string(destination.join("apps/server/src/server.rs")).unwrap();
    assert!(!server.contains("identity_"));
    assert!(!server.contains("/api/auth"));
}

#[test]
fn unsupported_selections_are_usage_errors_without_output() {
    let root = TestDirectory::new("unsupported");
    let destination = root.path().join("application");
    for extra in [
        vec!["--database", "mysql"],
        vec!["--client", "sveltekit"],
        vec!["--component", "catalog"],
        vec!["--force"],
        vec!["--framework-root", "/tmp"],
    ] {
        let mut args = vec![
            "new",
            "safe-app",
            "--destination",
            path_argument(&destination),
        ];
        args.extend(extra);
        let result = hegira(&args);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).starts_with("error:"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn empty_directory_and_file_are_conflicts_without_staging() {
    let root = TestDirectory::new("existing-entries");
    let directory = root.path().join("empty");
    let file = root.path().join("file");
    fs::create_dir(&directory).unwrap();
    fs::write(&file, "preserved").unwrap();
    for destination in [&directory, &file] {
        let result = hegira(&[
            "new",
            "safe-app",
            "--destination",
            path_argument(destination),
        ]);
        assert_eq!(result.status.code(), Some(4));
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .starts_with("error: destination already exists")
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
    assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    assert_eq!(fs::read_to_string(file).unwrap(), "preserved");
}

#[test]
fn end_of_input_at_each_prompt_leaves_no_staging_or_application() {
    let root = TestDirectory::new("cancel-prompts");
    let destination = root.path().join("application");
    let answers = [
        "cancel-app",
        path_argument(&destination),
        "sqlite",
        "leptos",
        "identity",
    ];
    for count in 0..=answers.len() {
        let input = if count == 0 {
            String::new()
        } else {
            format!("{}\n", answers[..count].join("\n"))
        };
        let (exit, stdout, stderr) = interactive_hegira(&["new"], &input);
        assert_eq!(exit, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("Cancelled; no files were written."));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn real_write_failure_cleans_staging_and_allows_retry() {
    let root = TestDirectory::new("write-failure");
    let destination = root.path().join("application");
    fs::write(root.path().join("sentinel"), "preserved").unwrap();
    // Limit only the child process. Ignore SIGXFSZ so write returns EFBIG,
    // exercising normal renderer cleanup instead of killing the process.
    let result = Command::new("/bin/bash")
        .env_clear()
        .env("HOME", root.path())
        .env("PATH", "")
        .current_dir(root.path())
        .args([
            "-c",
            "ulimit -f 1; trap '' XFSZ; exec \"$@\"",
            "write-failure",
        ])
        .arg(env!("CARGO_BIN_EXE_hegira"))
        .args([
            "new",
            "safe-app",
            "--destination",
            path_argument(&destination),
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3), "{:?}", result.stderr);
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("failed to write rendered file"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(root.path().join("sentinel")).unwrap(),
        "preserved"
    );
    assert!(
        hegira(&[
            "new",
            "safe-app",
            "--destination",
            path_argument(&destination)
        ])
        .status
        .success()
    );
}

#[cfg(unix)]
#[test]
fn migration_write_failure_is_reported_without_partial_application_changes() {
    use std::os::unix::fs::PermissionsExt;

    let root = TestDirectory::new("migration-write-failure");
    let application = root.path().join("application");
    assert!(
        hegira(&[
            "new",
            "migration-write-test",
            "--destination",
            path_argument(&application),
        ])
        .status
        .success()
    );
    let migration_directory = application.join("crates/infrastructure/migrations/sqlite");
    let original_permissions = fs::metadata(&migration_directory).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_mode(0o500);
    fs::set_permissions(&migration_directory, read_only).unwrap();
    let before = output_tree(&application);

    let failed = hegira_at(&application, &["generate", "migration", "write_failure"]);

    fs::set_permissions(&migration_directory, original_permissions).unwrap();
    assert_eq!(failed.status.code(), Some(1), "{:?}", failed.stderr);
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("cannot create a private staged"));
    assert_eq!(output_tree(&application), before);
    assert!(!application.join(".hegira-mutation.lock").exists());
    assert!(
        hegira_at(&application, &["generate", "migration", "write_failure"])
            .status
            .success()
    );
}

fn interactive_hegira(arguments: &[&str], input: &str) -> (u8, String, String) {
    let arguments = std::iter::once(OsString::from("hegira"))
        .chain(arguments.iter().map(OsString::from))
        .collect::<Vec<_>>();
    let mut input = Cursor::new(input.as_bytes());
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let exit =
        hegira_cli::run_interactive_from(arguments, &mut input, &mut output, &mut diagnostics);
    (
        exit.code(),
        String::from_utf8(output).expect("interactive output should be UTF-8"),
        String::from_utf8(diagnostics).expect("interactive diagnostics should be UTF-8"),
    )
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI should live under tools")
        .to_path_buf()
}

fn output_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries = fs::read_dir(directory)
            .expect("output directory should be readable")
            .map(|entry| entry.expect("output entry should be readable").path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                collect(root, &entry, files);
            } else {
                files.push((
                    entry
                        .strip_prefix(root)
                        .expect("output should remain under root")
                        .to_path_buf(),
                    fs::read(&entry).expect("output file should be readable"),
                ));
            }
        }
    }

    let mut files = Vec::new();
    collect(root, root, &mut files);
    files
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "hegira-cli-{name}-{}-{sequence}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
