//! Repository-only compile fixtures upgraded through the public CLI.
#[path = "../tests/support/upgrade_preservation.rs"]
mod support;
#[path = "../../hegira_cli/tests/support/upgrade_matrix.rs"]
mod upgrade_matrix;

use application_manifest::ApplicationManifest;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};
use template_renderer::ManifestCatalog;
use upgrade_test_support::{BaselineComposition, BaselineDatabase, BaselineRequest};

fn main() {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    assert!(
        arguments.len() == 3 || (arguments.len() == 4 && arguments[3] == "--production"),
        "usage: upgrade_preservation <repository-root> <new-output> <hegira-binary> [--production]"
    );
    let production = arguments.len() == 4;
    let repository = fs::canonicalize(&arguments[0]).unwrap();
    let output = PathBuf::from(&arguments[1]);
    let binary = fs::canonicalize(&arguments[2]).unwrap();
    fs::create_dir(&output).expect("validation output must be new");
    let catalog = ManifestCatalog::load(&repository, "layered").unwrap();
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let request = BaselineRequest::new(composition, database);
            let source = output.join(format!("{}-source", request.id()));
            support::customize(&repository, &source, request);
            let history = production
                .then(|| snapshot_database_history(&repository, &source, request, &output));
            let product = support::product_fingerprint(&source);
            support::upgrade_and_verify_with(&repository, &source, request, |application| {
                upgrade_matrix::verify(&binary, application);
            });
            assert_eq!(product, support::product_fingerprint(&source));
            let verified = support::fingerprints(&source);
            let staged = output.join(request.id());
            fs::create_dir(&staged).unwrap();
            for path in verified.keys() {
                let destination = staged.join(path);
                fs::create_dir_all(destination.parent().unwrap()).unwrap();
                fs::copy(source.join(path), destination).unwrap();
            }
            assert_eq!(verified, support::fingerprints(&staged));
            if let Some(history) = history {
                prepare_lifecycle(&binary, &staged, &history, request);
            }
            stage_dependencies(&catalog, &repository, &staged, production);
            // Local resolution belongs only to the separate disposable compile copy.
            fs::remove_file(staged.join("Cargo.lock")).unwrap();
            let staged_files = support::fingerprints(&staged);
            for (path, digest) in &verified {
                if path != Path::new("Cargo.toml")
                    && path != Path::new("Cargo.lock")
                    && !(production
                        && [
                            "apps/server/Cargo.toml",
                            "Dockerfile",
                            "crates/infrastructure/migrations/.hegira-generator.toml",
                        ]
                        .iter()
                        .any(|allowed| path == Path::new(allowed)))
                {
                    assert_eq!(staged_files.get(path), Some(digest), "{}", path.display());
                }
            }
            assert_eq!(verified, support::fingerprints(&source));
            println!("verified customized upgrade: {}", request.id());
        }
    }
}

fn snapshot_database_history(
    repository: &Path,
    source: &Path,
    request: BaselineRequest,
    output: &Path,
) -> PathBuf {
    use sha2::{Digest, Sha256};
    let inventory: serde_json::Value = serde_json::from_str(include_str!(
        "../../../test-fixtures/upgrade-lifecycle/v0.6.0-identity-migrations.json"
    ))
    .unwrap();
    assert_eq!(inventory["schema"], 1);
    assert_eq!(inventory["release"], upgrade_test_support::RELEASE_TAG);
    assert_eq!(inventory["commit"], upgrade_test_support::RELEASE_COMMIT);
    let history = output.join(format!("{}-v060-migrations", request.id()));
    fs::create_dir(&history).unwrap();
    let database = request.database.as_str();
    for entry in
        fs::read_dir(source.join(format!("crates/infrastructure/migrations/{database}"))).unwrap()
    {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "sql") {
            assert!(entry.file_type().unwrap().is_file());
            fs::copy(entry.path(), history.join(entry.file_name())).unwrap();
        }
    }
    if request.composition != BaselineComposition::Minimal {
        let prefix = format!("modules/identity/sqlx/migrations/{database}/");
        for (path, digest) in inventory["files"].as_object().unwrap() {
            if let Some(name) = path.strip_prefix(&prefix) {
                let bytes = fs::read(repository.join(path)).unwrap();
                assert_eq!(
                    format!("{:x}", Sha256::digest(&bytes)),
                    digest.as_str().unwrap(),
                    "released Identity SQL must be explicitly reviewed before changing the upgrade baseline: {path}"
                );
                let destination = history.join(name);
                assert!(!destination.exists(), "migration source collision");
                fs::write(destination, bytes).unwrap();
            }
        }
    }
    history
}

fn prepare_lifecycle(binary: &Path, staged: &Path, history: &Path, request: BaselineRequest) {
    let target = staged.join(".hegira-validation/v060-migrations");
    fs::create_dir_all(&target).unwrap();
    for entry in fs::read_dir(history).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
    }
    let output = std::process::Command::new(binary)
        .args(["generate", "migration", "upgrade_complete", "--json"])
        .current_dir(staged)
        .env_clear()
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["outcome"], "applied");
    let migrations = staged.join(format!(
        "crates/infrastructure/migrations/{}",
        request.database.as_str()
    ));
    let migration = fs::read_dir(migrations)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with("_upgrade_complete.sql")
        })
        .unwrap();
    fs::write(
        migration,
        "CREATE TABLE upgrade_completion (id BIGINT PRIMARY KEY);\n",
    )
    .unwrap();
    fs::create_dir_all(staged.join("apps/server/tests")).unwrap();
    fs::write(
        staged.join("apps/server/tests/upgrade_lifecycle.rs"),
        include_str!("../../../scripts/upgrade-database-contract.rs"),
    )
    .unwrap();
    // Provider selection is confined to this disposable Docker build copy.
    let dockerfile = staged.join("Dockerfile");
    let content = fs::read_to_string(&dockerfile).unwrap();
    assert_eq!(content.matches("--bin-features ssr,db-postgres").count(), 1);
    let mut content = content.replace(
        "--bin-features ssr,db-postgres",
        &format!("--bin-features ssr,db-{}", request.database.as_str()),
    );
    if request.composition != BaselineComposition::Minimal {
        fs::create_dir_all(staged.join("apps/server/src/bin")).unwrap();
        fs::write(staged.join("apps/server/src/bin/upgrade_bff_path.rs"),
            "use leptos::server_fn::ServerFn;\nfn main() { print!(\"{}\", app_web::upgrade_note::CreateUpgradeNoteResource::PATH); }\n").unwrap();
        let runtime = "FROM debian:bookworm-slim AS runtime";
        assert_eq!(content.matches(runtime).count(), 1);
        content = content.replace(runtime, &format!(
            "RUN cargo run --locked --release -p app_server --bin upgrade_bff_path --no-default-features --features ssr,db-{} > /app/upgrade-bff-path\n\n{runtime}",
            request.database.as_str()
        ));
        content.push_str("\nCOPY --from=builder /app/upgrade-bff-path /app/upgrade-bff-path\n");
    }
    fs::write(dockerfile, content).unwrap();
    // Validation-owned test dependencies; never change verified public output.
    let path = staged.join("apps/server/Cargo.toml");
    let mut manifest: toml::Table = fs::read_to_string(&path).unwrap().parse().unwrap();
    let dependencies = manifest
        .entry("dev-dependencies")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .unwrap();
    dependencies.insert(
        "sqlx".to_owned(),
        toml::Value::Table(toml::Table::from_iter([(
            "workspace".to_owned(),
            toml::Value::Boolean(true),
        )])),
    );
    fs::write(path, toml::to_string(&manifest).unwrap()).unwrap();
}

fn stage_dependencies(
    catalog: &ManifestCatalog,
    repository: &Path,
    application: &Path,
    production: bool,
) {
    let manifest = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
    // The default graph declares the same framework primitives and Identity
    // adapters used by the closed six-profile fixture matrix. Patch only names
    // actually present in this verified application; minimal stays module-free.
    let components = catalog.resolve_components().unwrap();
    let mut dependencies = BTreeMap::new();
    for component in components {
        for dependency in component.framework_dependencies.iter().chain(
            component
                .installation
                .as_ref()
                .into_iter()
                .flat_map(|unit| &unit.framework_dependencies),
        ) {
            assert_eq!(dependency.manifest, Path::new("Cargo.toml"));
            dependencies.insert(dependency.name.clone(), dependency.path.clone());
        }
    }
    let path = application.join("Cargo.toml");
    let mut cargo: toml::Table = fs::read_to_string(&path).unwrap().parse().unwrap();
    let workspace = cargo.get_mut("workspace").unwrap().as_table_mut().unwrap();
    if production {
        assert!(!workspace.contains_key("exclude"));
        workspace.insert(
            "exclude".to_owned(),
            toml::Value::Array(vec![toml::Value::String(
                ".hegira-validation/framework".to_owned(),
            )]),
        );
    }
    let table = workspace
        .get_mut("dependencies")
        .unwrap()
        .as_table_mut()
        .unwrap();
    for (name, relative) in dependencies {
        let Some(dependency) = table.get_mut(&name) else {
            continue;
        };
        let dependency = dependency.as_table_mut().unwrap();
        assert_eq!(
            dependency.get("git").unwrap().as_str(),
            Some(manifest.framework.repository.as_str())
        );
        assert_eq!(dependency.get("tag").unwrap().as_str(), Some("v0.7.0"));
        assert!(!dependency.contains_key("path"));
        dependency.remove("git");
        dependency.remove("tag");
        let source = fs::canonicalize(repository.join(&relative)).unwrap();
        assert!(source.starts_with(repository));
        assert!(source.join("Cargo.toml").is_file());
        dependency.insert(
            "path".to_owned(),
            toml::Value::String(if production {
                Path::new(".hegira-validation/framework")
                    .join(relative)
                    .to_str()
                    .unwrap()
                    .to_owned()
            } else {
                source.to_str().unwrap().to_owned()
            }),
        );
    }
    assert!(table.values().all(
        |dependency| dependency.get("git").and_then(toml::Value::as_str)
            != Some(manifest.framework.repository.as_str())
    ));
    fs::write(path, toml::to_string(&cargo).unwrap()).unwrap();
}
