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
            support::upgrade_and_verify(repository(), &application, request);
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
