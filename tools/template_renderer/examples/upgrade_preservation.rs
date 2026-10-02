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
    assert_eq!(
        arguments.len(),
        3,
        "usage: upgrade_preservation <repository-root> <new-output> <hegira-binary>"
    );
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
            stage_dependencies(&catalog, &repository, &staged);
            // Local resolution belongs only to the separate disposable compile copy.
            fs::remove_file(staged.join("Cargo.lock")).unwrap();
            let staged_files = support::fingerprints(&staged);
            for (path, digest) in &verified {
                if path != Path::new("Cargo.toml") && path != Path::new("Cargo.lock") {
                    assert_eq!(staged_files.get(path), Some(digest));
                }
            }
            assert_eq!(verified, support::fingerprints(&source));
            println!("verified customized upgrade: {}", request.id());
        }
    }
}

fn stage_dependencies(catalog: &ManifestCatalog, repository: &Path, application: &Path) {
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
        let source = fs::canonicalize(repository.join(relative)).unwrap();
        assert!(source.starts_with(repository));
        assert!(source.join("Cargo.toml").is_file());
        dependency.insert(
            "path".to_owned(),
            toml::Value::String(source.to_str().unwrap().to_owned()),
        );
    }
    assert!(table.values().all(
        |dependency| dependency.get("git").and_then(toml::Value::as_str)
            != Some(manifest.framework.repository.as_str())
    ));
    fs::write(path, toml::to_string(&cargo).unwrap()).unwrap();
}
