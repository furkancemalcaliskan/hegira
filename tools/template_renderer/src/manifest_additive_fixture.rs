//! Explicit synthetic target plans over immutable v0.7.0 inputs.
//! These are compatibility probes, never a bundled or executable release edge.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{
    ManagedIntegrationTransition, ManagedIntegrationTransitionKind, ManifestCatalog,
    UpgradeCompositionState, UpgradeCompositionTransition, UpgradeEdgeManifest,
    UpgradeManifestTransition, UpgradeReleaseIdentity,
};
use application_manifest::{ApplicationManifest, SourceOwnershipClaim, SourceOwnershipClass};
use application_mutator::ContentDigest;
use upgrade_test_support::{BaselineCatalog, BaselineRequest, BaselineVersion};

pub const CREATES: [&str; 2] = [
    "apps/server/src/bin/app_database.rs",
    "crates/infrastructure/src/database_operations.rs",
];
pub const EDITS: [&str; 3] = ["Cargo.lock", "Cargo.toml", "hegira.toml"];
pub const SECRET: &str = "private-application-sentinel-393";
pub const EDGE: &str = "synthetic-additive-compatibility";
pub const GOVERNANCE: [&str; 12] = [
    "README.md",
    "AGENTS.md",
    "CLAUDE.md",
    ".cursor/rules/application.mdc",
    "docs/architecture.md",
    "docs/development.md",
    "docs/ownership.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    ".github/workflows/quality.yml",
    ".github/workflows/repository-policy.yml",
    "scripts/ci.sh",
    "scripts/repository-policy.sh",
];

static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

pub struct Fixture {
    pub root: PathBuf,
    pub baselines: BaselineCatalog,
    pub edge: UpgradeEdgeManifest,
}

impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hegira-additive-upgrade-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        copy_target(&repository().join("templates"), &root.join("templates"));
        for template in ["layered", "layered-minimal"] {
            let path = root
                .join("templates/applications")
                .join(template)
                .join("Cargo.lock");
            let bytes = fs::read(&path).unwrap();
            fs::write(
                path,
                [
                    bytes.as_slice(),
                    b"\n# synthetic compatibility target; never resolved\n",
                ]
                .concat(),
            )
            .unwrap();
        }
        // Discard only the copied package's edge: canonical source is never changed.
        fs::remove_dir_all(root.join("templates/upgrades")).unwrap();
        fs::create_dir(root.join("templates/upgrades")).unwrap();
        let baselines =
            BaselineCatalog::from_repository_release(repository(), BaselineVersion::V0_7_0)
                .unwrap();
        let mut compositions = Vec::new();
        for request in baselines.requests() {
            let snapshot = baselines.snapshot(request).unwrap();
            let manifest = ApplicationManifest::from_toml(
                std::str::from_utf8(snapshot.file("hegira.toml").unwrap().bytes()).unwrap(),
            )
            .unwrap();
            let composition = manifest.composition.as_ref().unwrap();
            let state = UpgradeCompositionState {
                database: *manifest.selection.databases.first().unwrap(),
                client: *manifest.selection.clients.first().unwrap(),
                components: composition
                    .components
                    .iter()
                    .map(|item| item.id.clone())
                    .collect(),
                modules: composition
                    .modules
                    .iter()
                    .map(|item| item.id.clone())
                    .collect(),
                capabilities: composition.capabilities.iter().copied().collect(),
            };
            let source_ownership = manifest.upgrade.as_ref().unwrap().ownership.clone();
            let mut target_ownership = source_ownership.clone();
            for (path, integration) in CREATES
                .into_iter()
                .zip(["database-entry-point", "database-operations"])
            {
                assert!(
                    snapshot.file(path).is_none(),
                    "released source must lack {path}"
                );
                target_ownership.claims.push(SourceOwnershipClaim {
                    path: path.to_owned(),
                    class: SourceOwnershipClass::ManagedIntegration,
                    integration: Some(integration.to_owned()),
                });
            }
            let adapter = if state
                .components
                .iter()
                .any(|id| id == "layered-leptos-identity")
            {
                "layered-leptos-identity"
            } else {
                "layered-leptos-minimal"
            };
            let mut managed_integrations = Vec::new();
            for (path, integration, component) in [
                ("Cargo.lock", "framework-lockfile", adapter),
                ("Cargo.toml", "framework-dependencies", adapter),
                ("hegira.toml", "application-manifest", "layered-base"),
                (CREATES[0], "database-entry-point", adapter),
                (CREATES[1], "database-operations", adapter),
            ] {
                let source = snapshot.file(path);
                let source_path = if component == "layered-leptos-minimal" {
                    "layered-minimal"
                } else {
                    "layered"
                };
                let target = fs::read(
                    root.join("templates/applications")
                        .join(source_path)
                        .join(path),
                )
                .unwrap();
                managed_integrations.push(ManagedIntegrationTransition {
                    component: component.to_owned(),
                    path: path.to_owned(),
                    integration: integration.to_owned(),
                    kind: if source.is_some() {
                        ManagedIntegrationTransitionKind::Edit
                    } else {
                        ManagedIntegrationTransitionKind::Create
                    },
                    source_sha256: source.map(|file| digest(file.bytes())),
                    target_source_component: None,
                    target_sha256: Some(digest(&target)),
                });
            }
            compositions.push(UpgradeCompositionTransition {
                id: request.id(),
                source_baseline_digest: snapshot.tree_digest().to_owned(),
                source_ownership,
                target_ownership,
                source: state.clone(),
                target: state,
                manifest_transitions: None,
                managed_integrations,
            });
        }
        let release = |version: &str| UpgradeReleaseIdentity {
            framework: application_manifest::FrameworkContract {
                repository: baselines.release().framework_repository.clone(),
                version: version.to_owned(),
            },
            package: application_manifest::PackageIdentity {
                id: baselines.release().package_id.clone(),
                version: version.to_owned(),
            },
        };
        let edge = UpgradeEdgeManifest {
            schema: 1,
            id: EDGE.to_owned(),
            source_package_digest: baselines.release().package_digest.clone(),
            source: release("v0.7.0"),
            target: release("v0.8.0"),
            compositions,
            manifest_transitions: vec![
                UpgradeManifestTransition::Framework,
                UpgradeManifestTransition::Package,
                UpgradeManifestTransition::Components,
                UpgradeManifestTransition::Modules,
                UpgradeManifestTransition::Ownership,
            ],
            managed_integrations: Vec::new(),
        };
        let mut fixture = Self {
            root,
            baselines,
            edge,
        };
        // Minimal has no modules whose versions could change.
        for composition in &mut fixture.edge.compositions {
            if composition.source.modules.is_empty() {
                composition.manifest_transitions = Some(
                    fixture
                        .edge
                        .manifest_transitions
                        .iter()
                        .copied()
                        .filter(|item| *item != UpgradeManifestTransition::Modules)
                        .collect(),
                );
            }
        }
        fixture.prepare_package();
        fixture
    }

    fn prepare_package(&self) {
        let path = self.root.join("templates/package.toml");
        let mut package: toml::Table = fs::read_to_string(&path).unwrap().parse().unwrap();
        package.insert("upgrade_edges".to_owned(), toml::Value::Array(Vec::new()));
        fs::write(&path, toml::to_string(&package).unwrap()).unwrap();
        let digest = ManifestCatalog::calculate_package_digest(&self.root, "layered").unwrap();
        package.insert("content_digest".to_owned(), digest.into());
        fs::write(path, toml::to_string(&package).unwrap()).unwrap();
    }

    pub fn try_catalog(&self) -> crate::Result<ManifestCatalog> {
        // First load a fully authenticated CURRENT package through the production
        // loader. Its compiled-release guard must never accept a future package.
        let mut catalog = ManifestCatalog::load(&self.root, "layered")?;
        // Unit-test-only assembly supplies a hypothetical release and exact edge.
        // No production constructor, bypass flag, bundled edge, tag or lock changes.
        let package = catalog.package.as_mut().unwrap();
        package.version = "v0.8.0".to_owned();
        package.framework.version = "v0.8.0".to_owned();
        for component in catalog.components.values_mut() {
            component.version = Some("v0.8.0".to_owned());
        }
        catalog.upgrade_edges = vec![self.edge.clone()];
        catalog.validate_upgrade_graph()?;
        Ok(catalog)
    }

    pub fn catalog(&self) -> ManifestCatalog {
        self.try_catalog().unwrap()
    }

    pub fn application(&self, request: BaselineRequest, case: &str) -> PathBuf {
        let root = self.root.join(format!("{}-{case}", request.id()));
        self.baselines
            .snapshot(request)
            .unwrap()
            .materialize(&root)
            .unwrap();
        // File plans require existing real parents. This is explicit fixture
        // setup, not an inferred source-upgrade directory-creation capability.
        fs::create_dir(root.join("apps/server/src/bin")).unwrap();
        for path in GOVERNANCE.into_iter().chain([
            "crates/domain/src/product_owned.rs",
            "crates/infrastructure/migrations/20990101000000_owner_history.sql",
        ]) {
            fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            fs::write(
                root.join(path),
                format!("{SECRET}\n{} / {path}\n", request.id()),
            )
            .unwrap();
        }
        root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", ContentDigest::calculate(bytes))
}

pub fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                visit(root, &entry.path(), out);
            } else {
                let bytes = if kind.is_symlink() {
                    fs::read_link(entry.path())
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec()
                } else {
                    fs::read(entry.path()).unwrap()
                };
                out.insert(entry.path().strip_prefix(root).unwrap().to_owned(), bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

fn copy_target(source: &Path, target: &Path) {
    fs::create_dir(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_target(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}
