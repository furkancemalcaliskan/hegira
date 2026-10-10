#[path = "support/upgrade_preservation.rs"]
mod support;

use std::{fs, path::Path};
use template_renderer::{ManifestCatalog, UpgradePlanningErrorKind};
use upgrade_test_support::{BaselineComposition, BaselineDatabase, BaselineRequest};

fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

struct TestDirectory(std::path::PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "hegira-upgrade-preservation-{}-{name}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn customized_released_applications_preserve_product_source_and_history() {
    let parent = TestDirectory::new("matrix");
    let mut snapshots = std::collections::BTreeMap::new();
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let request = BaselineRequest::new(composition, database);
            let application = parent.0.join(request.id());
            support::customize(repository(), &application, request);
            let product = support::product_fingerprint(&application);
            snapshots.insert(request.id(), product.clone());
            support::upgrade_and_verify_with(repository(), &application, request, |application| {
                let catalog = ManifestCatalog::load(repository(), "layered").unwrap();
                let plan = catalog.plan_upgrade(application).unwrap();
                application_mutator::publish_change_plan(application, plan.change_plan()).unwrap();
            });
            assert_eq!(product, support::product_fingerprint(&application));
        }
    }
    let snapshot = snapshots
        .into_iter()
        .map(|(id, digest)| format!("{id} {digest}\n"))
        .collect::<String>();
    assert_eq!(
        snapshot,
        include_str!("snapshots/customized-v0.6.0.txt"),
        "review released product-source fixtures before changing their fingerprints"
    );
}

#[test]
fn historical_upgrade_preserves_customized_application_documentation_without_adoption() {
    use upgrade_test_support::BaselineCatalog;
    let parent = TestDirectory::new("documentation");
    let baselines = BaselineCatalog::from_repository(repository()).unwrap();
    let catalog = ManifestCatalog::load(repository(), "layered").unwrap();
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let request = BaselineRequest::new(composition, database);
            let application = parent.0.join(request.id());
            baselines
                .snapshot(request)
                .unwrap()
                .materialize(&application)
                .unwrap();
            fs::create_dir_all(application.join("docs")).unwrap();
            let paths = [
                "AGENTS.md",
                "CLAUDE.md",
                ".cursor/rules/application.mdc",
                "README.md",
                "docs/architecture.md",
                "docs/development.md",
                "docs/ownership.md",
            ];
            fs::create_dir_all(application.join(".cursor/rules")).unwrap();
            for path in paths {
                fs::write(
                    application.join(path),
                    format!("# Owner documentation\n\n{} / {path}\n", request.id()),
                )
                .unwrap();
            }
            let before = support::fingerprints(&application);
            let plan = catalog.plan_upgrade(&application).unwrap();
            assert_eq!(
                plan.change_plan()
                    .changes()
                    .iter()
                    .map(|change| change.path().as_str())
                    .collect::<Vec<_>>(),
                support::MANAGED_FILES
            );
            application_mutator::publish_change_plan(&application, plan.change_plan()).unwrap();
            let after = support::fingerprints(&application);
            for path in paths {
                assert_eq!(before[Path::new(path)], after[Path::new(path)]);
            }
            let manifest =
                application_manifest::ApplicationManifest::read(application.join("hegira.toml"))
                    .unwrap();
            assert!(
                manifest
                    .upgrade
                    .unwrap()
                    .ownership
                    .claims
                    .iter()
                    .all(|claim| !paths.contains(&claim.path.as_str()))
            );
        }
    }
}

#[test]
fn conflicting_managed_source_blocks_customized_applications_before_publication() {
    let parent = TestDirectory::new("conflicts");
    let catalog = ManifestCatalog::load(repository(), "layered").unwrap();
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let request = BaselineRequest::new(composition, database);
            let application = parent.0.join(request.id());
            support::customize(repository(), &application, request);
            for path in ["Cargo.lock", "Cargo.toml"] {
                let original = fs::read(application.join(path)).unwrap();
                let mut changed = original.clone();
                changed.extend_from_slice(b"\n# user-modified release-managed source\n");
                fs::write(application.join(path), changed).unwrap();
                let before = support::fingerprints(&application);
                let error = catalog.plan_upgrade(&application).unwrap_err();
                assert_eq!(error.diagnostic().kind, UpgradePlanningErrorKind::Blocked);
                assert_eq!(before, support::fingerprints(&application));
                assert!(!application.join(".hegira-mutation.lock").exists());
                fs::write(application.join(path), original).unwrap();
            }
            // A previously reviewed plan must also reject a late managed edit.
            let plan = catalog.plan_upgrade(&application).unwrap();
            fs::write(application.join("Cargo.lock"), "# changed after planning\n").unwrap();
            let before = support::fingerprints(&application);
            assert!(
                application_mutator::publish_change_plan(&application, plan.change_plan()).is_err()
            );
            assert_eq!(before, support::fingerprints(&application));
        }
    }
}
