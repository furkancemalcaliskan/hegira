//! Test-only customization of authenticated released applications.
//! Never reconstruct the baseline from current templates or change its release identity.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use application_manifest::ApplicationManifest;
use application_mutator::{
    CargoDependency, CargoDependencySection, ChangePlan, PlannedFileChange, StructuredEditOutcome,
    StructuredFileEdit, plan_cargo_dependency, plan_toml_array_string, publish_change_plan,
};
use resource_generator::{
    ArtifactNamespace, HttpLayerSources, InwardLayerSources, LayeredNamingInput, MigrationIdentity,
    MinimalHttpLayerSources, MinimalWebLayerSources, PersistenceLayerSources, ResourceFieldInput,
    ResourceSpecification, ResourceSpecificationInput, SelectedDatabase, SpecificationErrorKind,
    WebLayerSources, plan_application_migration, plan_inward_resource_layers, plan_resource_http,
    plan_resource_http_minimal, plan_resource_persistence, plan_resource_web,
    plan_resource_web_minimal,
};
use sha2::{Digest, Sha256};
use template_renderer::ManifestCatalog;
use upgrade_test_support::{
    BaselineCatalog, BaselineComposition, BaselineDatabase, BaselineRequest,
};

pub const MANAGED_FILES: [&str; 3] = ["Cargo.lock", "Cargo.toml", "hegira.toml"];
const LAYER_ROOTS: [&str; 6] = [
    "crates/domain/src/lib.rs",
    "crates/application_contracts/src/lib.rs",
    "crates/application/src/lib.rs",
    "crates/infrastructure/src/lib.rs",
    "crates/presentation/src/lib.rs",
    "apps/web/src/lib.rs",
];

pub fn customize(repository: &Path, application: &Path, request: BaselineRequest) {
    BaselineCatalog::from_repository(repository)
        .unwrap()
        .snapshot(request)
        .unwrap()
        .materialize(application)
        .unwrap();
    let historical = history_fingerprints(application);
    let manifest = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
    let namespace = ArtifactNamespace::new(
        &manifest.application,
        manifest.installed_component_ids(),
        std::iter::empty::<&str>(),
    )
    .unwrap();
    let specification = ResourceSpecification::resolve(
        ResourceSpecificationInput::new(
            LayeredNamingInput::new("UpgradeNote"),
            [
                ResourceFieldInput::new("title", "string", false),
                ResourceFieldInput::new("active", "bool", false),
                ResourceFieldInput::new("sequence", "i64", false),
                ResourceFieldInput::new("external_id", "uuid", true),
                ResourceFieldInput::new("observed_at", "datetime", true),
            ],
        ),
        &namespace,
        &manifest,
    );
    if request.composition == BaselineComposition::Minimal {
        assert_eq!(
            specification.unwrap_err().kind(),
            SpecificationErrorKind::MissingCapability
        );
    } else {
        generate_resource(application, &specification.unwrap(), request.composition);
        append(
            application,
            "crates/domain/src/upgrade_note.rs",
            "\npub fn accepts_title(title: &str) -> bool { !title.trim().is_empty() && title.len() <= 120 }\n",
        );
    }
    for path in LAYER_ROOTS {
        append(
            application,
            path,
            "\npub fn application_owned_note_limit() -> usize { 120 }\n",
        );
    }
    let database = match request.database {
        BaselineDatabase::Sqlite => SelectedDatabase::Sqlite,
        BaselineDatabase::Postgres => SelectedDatabase::Postgres,
    };
    let migration = plan_application_migration(
        application,
        database,
        MigrationIdentity::new("note_review_index").unwrap(),
    )
    .unwrap();
    publish_change_plan(application, migration.plan()).unwrap();
    // Product-owned SQL, not executed by these source-preservation checks.
    fs::write(application.join(migration.path()),
        "CREATE TABLE application_note_reviews (id BIGINT PRIMARY KEY, reviewed BOOLEAN NOT NULL);\n").unwrap();
    for path in ["config/development.yaml", "config/sqlite.yaml"] {
        let content = fs::read_to_string(application.join(path)).unwrap();
        assert!(content.contains("request_timeout_seconds: 30"));
        fs::write(
            application.join(path),
            content.replace("request_timeout_seconds: 30", "request_timeout_seconds: 47"),
        )
        .unwrap();
    }
    let customized_history = history_fingerprints(application);
    for (path, digest) in historical {
        assert_eq!(
            customized_history.get(&path),
            Some(&digest),
            "{}",
            path.display()
        );
    }
    assert!(customized_history.len() > 1);
    assert_eq!(
        ApplicationManifest::read(application.join("hegira.toml")).unwrap(),
        manifest
    );
}

pub fn upgrade_and_verify_with(
    repository: &Path,
    application: &Path,
    request: BaselineRequest,
    apply: impl FnOnce(&Path),
) {
    let before = fingerprints(application);
    let history = history_fingerprints(application);
    let source = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
    let catalog = ManifestCatalog::load(repository, "layered").unwrap();
    let first = catalog.plan_upgrade(application).unwrap();
    let second = catalog.plan_upgrade(application).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        before,
        fingerprints(application),
        "planning must be read-only"
    );
    assert_eq!(
        first
            .change_plan()
            .changes()
            .iter()
            .map(|change| change.path().as_str())
            .collect::<Vec<_>>(),
        MANAGED_FILES
    );
    let summary = serde_json::to_string(&first.summary()).unwrap();
    for product_content in [
        "accepts_title",
        "application_owned_note_limit",
        "note_review_index",
        "request_timeout_seconds",
        "application_note_reviews",
    ] {
        assert!(!summary.contains(product_content));
    }
    apply(application);
    let after = fingerprints(application);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (path, digest) in &before {
        if !MANAGED_FILES
            .iter()
            .any(|managed| path == Path::new(managed))
        {
            assert_eq!(
                after.get(path),
                Some(digest),
                "{}: {}",
                request.id(),
                path.display()
            );
        }
    }
    assert_eq!(history, history_fingerprints(application));
    let target = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
    assert_eq!(target.framework.version, "v0.7.0");
    assert_eq!(source.application, target.application);
    assert_eq!(source.selection, target.selection);
    assert_eq!(
        source.installed_component_ids(),
        target.installed_component_ids()
    );
    assert_eq!(
        source.composition.as_ref().unwrap().capabilities,
        target.composition.as_ref().unwrap().capabilities
    );
    assert_eq!(
        target.composition.as_ref().unwrap().modules.is_empty(),
        request.composition == BaselineComposition::Minimal
    );
    for path in LAYER_ROOTS {
        let generated = application
            .join(Path::new(path).parent().unwrap())
            .join("upgrade_note.rs");
        assert_eq!(
            generated.exists(),
            request.composition != BaselineComposition::Minimal
        );
    }
    assert!(!application.join(".hegira-mutation.lock").exists());
}

pub fn fingerprints(root: &Path) -> BTreeMap<PathBuf, String> {
    fn visit(root: &Path, directory: &Path, output: &mut BTreeMap<PathBuf, String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink());
            if kind.is_dir() {
                visit(root, &entry.path(), output);
            } else {
                assert!(kind.is_file());
                output.insert(
                    entry.path().strip_prefix(root).unwrap().to_path_buf(),
                    format!("{:x}", Sha256::digest(fs::read(entry.path()).unwrap())),
                );
            }
        }
    }
    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}

pub fn product_fingerprint(root: &Path) -> String {
    let mut tree = Sha256::new();
    for (path, digest) in fingerprints(root) {
        if MANAGED_FILES
            .iter()
            .any(|managed| path == Path::new(managed))
        {
            continue;
        }
        let normalized = path
            .components()
            .map(|part| part.as_os_str().to_str().unwrap())
            .collect::<Vec<_>>()
            .join("/");
        tree.update(normalized.as_bytes());
        tree.update(b"\0");
        tree.update(digest.as_bytes());
        tree.update(b"\n");
    }
    format!("sha256:{tree:x}", tree = tree.finalize())
}

fn history_fingerprints(root: &Path) -> BTreeMap<PathBuf, String> {
    fingerprints(root)
        .into_iter()
        .filter(|(path, _)| path.starts_with("crates/infrastructure/migrations"))
        .collect()
}

fn append(root: &Path, path: &str, addition: &str) {
    let mut content = fs::read(root.join(path)).unwrap();
    content.extend_from_slice(addition.as_bytes());
    fs::write(root.join(path), content).unwrap();
}

fn planned_content<'a>(plan: &'a ChangePlan, path: &str) -> &'a [u8] {
    plan.changes()
        .iter()
        .find(|change| change.path().as_str() == path)
        .unwrap()
        .resulting_content()
        .unwrap()
}

fn generate_resource(root: &Path, spec: &ResourceSpecification, composition: BaselineComposition) {
    let read = |path: &str| fs::read(root.join(path)).unwrap();
    let domain = read(LAYER_ROOTS[0]);
    let contracts = read(LAYER_ROOTS[1]);
    let application = read(LAYER_ROOTS[2]);
    let infrastructure = read(LAYER_ROOTS[3]);
    let presentation = read(LAYER_ROOTS[4]);
    let web = read(LAYER_ROOTS[5]);
    let services = read("crates/infrastructure/src/identity/services.rs");
    let server = read("apps/server/src/server.rs");
    let routes = read("apps/web/src/routes.rs");
    let inward = plan_inward_resource_layers(
        spec,
        InwardLayerSources {
            domain_root: &domain,
            application_contracts_root: &contracts,
            application_root: &application,
        },
    )
    .unwrap();
    let persistence = plan_resource_persistence(
        root,
        spec,
        PersistenceLayerSources {
            infrastructure_root: &infrastructure,
        },
    )
    .unwrap();
    let http_sources = HttpLayerSources {
        presentation_root: &presentation,
        infrastructure_resource: planned_content(
            persistence.plan(),
            "crates/infrastructure/src/upgrade_note.rs",
        ),
        infrastructure_services: &services,
        server_source: &server,
    };
    let http = if composition == BaselineComposition::IdentityAdded {
        plan_resource_http_minimal(
            spec,
            MinimalHttpLayerSources {
                common: http_sources,
                identity_runtime: &read("apps/server/src/identity_runtime.rs"),
            },
        )
    } else {
        plan_resource_http(spec, http_sources)
    }
    .unwrap();
    let server_source = planned_content(http.plan(), "apps/server/src/server.rs");
    let generated_web = if composition == BaselineComposition::IdentityAdded {
        plan_resource_web_minimal(
            spec,
            MinimalWebLayerSources {
                web_root: &web,
                routes: &routes,
                dashboard: &read("apps/web/src/dashboard.rs"),
                server_source,
            },
        )
    } else {
        plan_resource_web(
            spec,
            WebLayerSources {
                web_root: &web,
                routes: &routes,
                navigation: &read("apps/web/src/app/navigation.rs"),
                i18n: &read("apps/web/src/shared/i18n/mod.rs"),
                sidebar: &read("apps/web/src/app/sidebar.rs"),
                server_source,
            },
        )
    }
    .unwrap();
    let mut plans = vec![
        inward.into_plan(),
        persistence.into_plan(),
        http.into_plan(),
        generated_web.into_plan(),
    ];
    if composition == BaselineComposition::IdentityAdded {
        let mut changes = Vec::new();
        for (path, dependencies, features) in [
            (
                "crates/infrastructure/Cargo.toml",
                &["app_application", "app_application_contracts", "app_domain"][..],
                &[][..],
            ),
            (
                "apps/web/Cargo.toml",
                &[
                    "app_application_contracts",
                    "chrono",
                    "leptos_support",
                    "uuid",
                ][..],
                &[
                    ("hydrate", "leptos_support/hydrate"),
                    ("ssr", "leptos_support/ssr"),
                ][..],
            ),
        ] {
            let observed = read(path);
            let mut content = observed.clone();
            for name in dependencies {
                if let StructuredEditOutcome::Planned { edit, .. } = plan_cargo_dependency(
                    path,
                    &content,
                    CargoDependencySection::Dependencies,
                    &CargoDependency::workspace(*name, false, std::iter::empty::<&str>()),
                )
                .unwrap()
                {
                    content = edit.resulting_content().to_vec();
                }
            }
            for (feature, selection) in features {
                if let StructuredEditOutcome::Planned { edit, .. } =
                    plan_toml_array_string(path, &content, &["features"], feature, selection)
                        .unwrap()
                {
                    content = edit.resulting_content().to_vec();
                }
            }
            if content != observed {
                changes.push(PlannedFileChange::from(
                    StructuredFileEdit::new(path, &observed, content).unwrap(),
                ));
            }
        }
        plans.push(ChangePlan::new(changes).unwrap());
    }
    let plan = ChangePlan::compose(plans).unwrap();
    publish_change_plan(root, &plan).unwrap();
}
