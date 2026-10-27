#[path = "manifest_additive_fixture.rs"]
mod support;

use std::{fs, path::Path};

use crate::{
    ManifestCatalog, UpgradeAuthenticationDiagnosticKind as Authentication,
    UpgradePlanningErrorKind, upgrade_recovery_pending,
};
use application_manifest::{ApplicationManifest, SourceOwnershipClass};
use application_mutator::{
    ChangeOperation, ContentDigest, FilePrecondition, MutationErrorKind, publish_change_plan,
};

use support::{CREATES, EDITS, Fixture, GOVERNANCE, SECRET, digest, files};

#[test]
fn synthetic_additive_edges_authenticate_all_six_released_states_and_preserve_owned_source() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog();
    // No synthetic edge is installed into the real package.
    let canonical = ManifestCatalog::load(support::repository(), "layered").unwrap();
    assert!(
        canonical
            .upgrade_edges()
            .iter()
            .all(|edge| edge.id != support::EDGE)
    );
    for request in fixture.baselines.requests() {
        let root = fixture.application(request, "success");
        let before = files(&root);
        let original = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
        let boundary = catalog.authenticate_upgrade_source(&root).unwrap();
        assert_eq!(
            boundary.source_package_digest(),
            fixture.baselines.release().package_digest
        );
        assert_eq!(
            boundary.source_baseline_digest(),
            fixture.baselines.snapshot(request).unwrap().tree_digest()
        );
        let first = catalog.plan_upgrade(&root).unwrap();
        assert_eq!(first, catalog.plan_upgrade(&root).unwrap());
        assert_eq!(before, files(&root));
        assert_eq!(
            first.change_plan().changes().len(),
            EDITS.len() + CREATES.len()
        );
        for change in first.change_plan().changes() {
            let path = Path::new(change.path().as_str());
            if CREATES.contains(&change.path().as_str()) {
                assert_eq!(change.operation(), ChangeOperation::Create);
                assert_eq!(change.precondition(), FilePrecondition::Absent);
            } else {
                assert!(EDITS.contains(&change.path().as_str()));
                assert_eq!(change.operation(), ChangeOperation::Edit);
                assert_eq!(
                    change.precondition(),
                    FilePrecondition::MatchesDigest(ContentDigest::calculate(&before[path]))
                );
            }
            assert_eq!(
                change.result_digest(),
                Some(ContentDigest::calculate(
                    change.resulting_content().unwrap()
                ))
            );
        }
        let summary = serde_json::to_string(&first.summary()).unwrap();
        assert!(summary.contains(support::EDGE));
        assert!(summary.contains("managed-integration"));
        for source in [SECRET, "[workspace]", "pub fn", "{{application_name}}"] {
            assert!(!summary.contains(source));
        }
        assert!(!summary.contains(&root.to_string_lossy().to_string()));
        let receipt = publish_change_plan(&root, first.change_plan()).unwrap();
        assert_eq!(receipt.changed_files(), 5);
        let after = files(&root);
        for (path, bytes) in &before {
            if !EDITS.contains(&path.to_str().unwrap()) {
                assert_eq!(bytes, &after[path], "{}", path.display());
            }
        }
        assert_eq!(after.len(), before.len() + CREATES.len());
        for change in first.change_plan().changes() {
            assert_eq!(
                after[Path::new(change.path().as_str())],
                change.resulting_content().unwrap()
            );
        }
        let target = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
        assert_eq!(target.application, original.application);
        assert_eq!(target.selection, original.selection);
        assert_eq!(
            target.installed_component_ids(),
            original.installed_component_ids()
        );
        for path in GOVERNANCE {
            let old_claims = &original.upgrade.as_ref().unwrap().ownership.claims;
            let new_claims = &target.upgrade.as_ref().unwrap().ownership.claims;
            assert_eq!(
                old_claims
                    .iter()
                    .filter(|claim| claim.path == path)
                    .collect::<Vec<_>>(),
                new_claims
                    .iter()
                    .filter(|claim| claim.path == path)
                    .collect::<Vec<_>>()
            );
        }
        assert!(!upgrade_recovery_pending(&root).unwrap());
        assert!(publish_change_plan(&root, first.change_plan()).is_err());
        assert_eq!(after, files(&root));
    }
}

#[test]
fn create_collisions_and_custom_managed_files_fail_before_publication_or_adoption() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog();
    for request in fixture.baselines.requests() {
        for (index, path) in CREATES.into_iter().chain(EDITS).enumerate() {
            let root = fixture.application(request, &format!("conflict-{index}"));
            let plan = catalog.plan_upgrade(&root).unwrap();
            fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            let original = fs::read(root.join(path)).ok();
            let occupied = if let Some(mut bytes) = original {
                bytes.extend_from_slice(format!("\n# {SECRET}\n").as_bytes());
                bytes
            } else {
                // Even identical target bytes do not authorize adopting an occupied path.
                plan.change_plan()
                    .changes()
                    .iter()
                    .find(|change| change.path().as_str() == path)
                    .unwrap()
                    .resulting_content()
                    .unwrap()
                    .to_vec()
            };
            fs::write(root.join(path), occupied).unwrap();
            let before = files(&root);
            let error = catalog.authenticate_upgrade_source(&root).unwrap_err();
            assert_eq!(
                error.diagnostic().kind,
                if CREATES.contains(&path) {
                    Authentication::UnexpectedManagedSource
                } else {
                    Authentication::SourceDigestMismatch
                }
            );
            assert!(!error.to_string().contains(SECRET));
            assert_eq!(
                catalog.plan_upgrade(&root).unwrap_err().diagnostic().kind,
                UpgradePlanningErrorKind::Blocked
            );
            assert_eq!(
                publish_change_plan(&root, plan.change_plan())
                    .unwrap_err()
                    .kind(),
                MutationErrorKind::PreconditionFailed
            );
            assert_eq!(before, files(&root));
            assert!(!upgrade_recovery_pending(&root).unwrap());
        }
    }
}

#[test]
fn edge_claims_and_target_bytes_require_exact_authenticated_authority() {
    let mut fixture = Fixture::new();
    let original = fixture.edge.clone();
    // A matching digest does not grant writes to generated-once, application-owned,
    // or immutable-history claims. Missing/mismatched claims also fail graph loading.
    for class in [
        SourceOwnershipClass::GeneratedOnce,
        SourceOwnershipClass::ApplicationOwned,
        SourceOwnershipClass::ImmutableHistory,
    ] {
        fixture.edge = original.clone();
        let claim = fixture.edge.compositions[0]
            .target_ownership
            .claims
            .iter_mut()
            .find(|claim| claim.path == CREATES[0])
            .unwrap();
        claim.class = class;
        claim.integration = None;
        assert!(fixture.try_catalog().is_err());
    }
    fixture.edge = original.clone();
    fixture.edge.compositions[0]
        .target_ownership
        .claims
        .retain(|claim| claim.path != CREATES[0]);
    assert!(fixture.try_catalog().is_err());
    fixture.edge = original.clone();
    fixture.edge.compositions[0].managed_integrations[0].target_sha256 =
        Some(digest(b"not the authenticated package target"));
    assert!(fixture.try_catalog().is_err());
    fixture.edge = original;
    // Snapshot authentication still applies before any synthetic test assembly.
    let target = fixture
        .root
        .join("templates/applications/layered/Cargo.lock");
    let bytes = fs::read(&target).unwrap();
    fs::write(
        target,
        [bytes.as_slice(), b"\n# unreviewed package alteration\n"].concat(),
    )
    .unwrap();
    assert!(fixture.try_catalog().is_err());
}

#[test]
fn local_ownership_disagreement_is_never_repaired_by_the_source_edge() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog();
    for request in fixture.baselines.requests() {
        let root = fixture.application(request, "local-ownership");
        let mut manifest = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
        manifest
            .upgrade
            .as_mut()
            .unwrap()
            .ownership
            .claims
            .retain(|claim| claim.path != "hegira.toml");
        fs::write(root.join("hegira.toml"), manifest.to_toml().unwrap()).unwrap();
        let before = files(&root);
        assert_eq!(
            catalog
                .authenticate_upgrade_source(&root)
                .unwrap_err()
                .diagnostic()
                .kind,
            Authentication::OwnershipMismatch
        );
        assert_eq!(before, files(&root));
        assert!(!upgrade_recovery_pending(&root).unwrap());
    }
}

#[test]
fn missing_parents_and_directory_collisions_do_not_grant_creation_authority() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog();
    for request in fixture.baselines.requests() {
        let root = fixture.application(request, "missing-parent");
        let plan = catalog.plan_upgrade(&root).unwrap();
        fs::remove_dir(root.join("apps/server/src/bin")).unwrap();
        let before = files(&root);
        assert!(catalog.authenticate_upgrade_source(&root).is_err());
        assert_eq!(
            publish_change_plan(&root, plan.change_plan())
                .unwrap_err()
                .kind(),
            MutationErrorKind::UnsafeRoot
        );
        assert_eq!(before, files(&root));
        assert!(!root.join("apps/server/src/bin").exists());
        assert!(!upgrade_recovery_pending(&root).unwrap());
        for (index, path) in CREATES.into_iter().enumerate() {
            let root = fixture.application(request, &format!("occupied-directory-{index}"));
            let plan = catalog.plan_upgrade(&root).unwrap();
            fs::create_dir(root.join(path)).unwrap();
            let before = files(&root);
            assert_eq!(
                catalog
                    .authenticate_upgrade_source(&root)
                    .unwrap_err()
                    .diagnostic()
                    .kind,
                Authentication::UnexpectedManagedSource
            );
            assert_eq!(
                publish_change_plan(&root, plan.change_plan())
                    .unwrap_err()
                    .kind(),
                MutationErrorKind::PreconditionFailed
            );
            assert!(root.join(path).is_dir());
            assert_eq!(before, files(&root));
            assert!(!upgrade_recovery_pending(&root).unwrap());
        }
    }
}

#[test]
fn synthetic_edge_cannot_adopt_an_unclaimed_existing_application_manifest() {
    use crate::{ManagedIntegrationTransition, ManagedIntegrationTransitionKind};
    use application_manifest::SourceOwnershipClaim;
    let mut fixture = Fixture::new();
    let path = "apps/server/Cargo.toml";
    for (composition, request) in fixture
        .edge
        .compositions
        .iter_mut()
        .zip(fixture.baselines.requests())
    {
        let source = fixture
            .baselines
            .snapshot(request)
            .unwrap()
            .file(path)
            .unwrap()
            .bytes();
        assert!(
            composition
                .source_ownership
                .claims
                .iter()
                .all(|claim| claim.path != path)
        );
        let claim = SourceOwnershipClaim {
            path: path.to_owned(),
            class: SourceOwnershipClass::ManagedIntegration,
            integration: Some("database-entry-point-manifest".to_owned()),
        };
        // Hypothetical edge declarations cannot rewrite the application's own
        // ownership contract, even when its bytes have a known matching digest.
        composition.source_ownership.claims.push(claim.clone());
        composition.target_ownership.claims.push(claim);
        let minimal = !composition
            .source
            .components
            .iter()
            .any(|id| id == "layered-leptos-identity");
        let (component, template) = if minimal {
            ("layered-leptos-minimal", "layered-minimal")
        } else {
            ("layered-leptos-identity", "layered")
        };
        let target = fs::read(
            fixture
                .root
                .join("templates/applications")
                .join(template)
                .join(path),
        )
        .unwrap();
        composition
            .managed_integrations
            .push(ManagedIntegrationTransition {
                component: component.to_owned(),
                path: path.to_owned(),
                integration: "database-entry-point-manifest".to_owned(),
                kind: ManagedIntegrationTransitionKind::Edit,
                source_sha256: Some(digest(source)),
                target_source_component: None,
                target_sha256: Some(digest(&target)),
            });
    }
    let catalog = fixture.catalog();
    for request in fixture.baselines.requests() {
        let root = fixture.application(request, "unclaimed-manifest");
        let before = files(&root);
        let error = catalog.authenticate_upgrade_source(&root).unwrap_err();
        assert_eq!(error.diagnostic().kind, Authentication::OwnershipMismatch);
        assert_eq!(error.diagnostic().subject, path);
        assert_eq!(before, files(&root));
        assert!(!upgrade_recovery_pending(&root).unwrap());
    }
}

#[test]
fn target_claims_cannot_adopt_occupied_agent_or_workflow_files() {
    use crate::{ManagedIntegrationTransition, ManagedIntegrationTransitionKind};
    use application_manifest::SourceOwnershipClaim;
    let mut fixture = Fixture::new();
    for composition in &mut fixture.edge.compositions {
        for path in ["AGENTS.md", ".github/workflows/quality.yml"] {
            let integration = "synthetic-governance-source";
            composition
                .target_ownership
                .claims
                .push(SourceOwnershipClaim {
                    path: path.to_owned(),
                    class: SourceOwnershipClass::ManagedIntegration,
                    integration: Some(integration.to_owned()),
                });
            let target = fs::read(
                fixture
                    .root
                    .join("templates/applications/layered")
                    .join(path),
            )
            .unwrap();
            composition
                .managed_integrations
                .push(ManagedIntegrationTransition {
                    component: "layered-base".to_owned(),
                    path: path.to_owned(),
                    integration: integration.to_owned(),
                    kind: ManagedIntegrationTransitionKind::Create,
                    source_sha256: None,
                    target_source_component: None,
                    target_sha256: Some(digest(&target)),
                });
        }
    }
    let catalog = fixture.catalog();
    for request in fixture.baselines.requests() {
        let root = fixture.application(request, "occupied-governance");
        let before = files(&root);
        let error = catalog.authenticate_upgrade_source(&root).unwrap_err();
        assert_eq!(
            error.diagnostic().kind,
            Authentication::UnexpectedManagedSource
        );
        assert!(
            ["AGENTS.md", ".github/workflows/quality.yml"]
                .contains(&error.diagnostic().subject.as_str())
        );
        assert!(!error.to_string().contains(SECRET));
        assert_eq!(before, files(&root));
        assert!(!upgrade_recovery_pending(&root).unwrap());
    }
}

#[cfg(unix)]
#[test]
fn managed_symlinks_and_replaced_parents_never_redirect_reads_or_publication() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let catalog = fixture.catalog();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), SECRET).unwrap();
    for request in fixture.baselines.requests() {
        for (index, path) in CREATES
            .into_iter()
            .chain(["Cargo.lock", "Cargo.toml", "hegira.toml"])
            .enumerate()
        {
            let root = fixture.application(request, &format!("symlink-{index}"));
            let plan = catalog.plan_upgrade(&root).unwrap();
            fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            if root.join(path).exists() {
                fs::remove_file(root.join(path)).unwrap();
            }
            symlink(outside.join("sentinel"), root.join(path)).unwrap();
            let before = files(&root);
            assert!(catalog.authenticate_upgrade_source(&root).is_err());
            assert!(publish_change_plan(&root, plan.change_plan()).is_err());
            assert_eq!(before, files(&root));
            assert!(root.join(path).is_symlink());
        }
        let root = fixture.application(request, "symlink-parent");
        let plan = catalog.plan_upgrade(&root).unwrap();
        fs::rename(
            root.join("apps/server/src"),
            root.join("apps/server/source-before-replacement"),
        )
        .unwrap();
        symlink(&outside, root.join("apps/server/src")).unwrap();
        let before = files(&root);
        assert!(catalog.authenticate_upgrade_source(&root).is_err());
        assert!(publish_change_plan(&root, plan.change_plan()).is_err());
        assert_eq!(before, files(&root));
        assert_eq!(files(&outside).len(), 1);
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            SECRET.as_bytes()
        );
    }
}

#[test]
fn pending_recovery_blocks_previously_planned_additions_without_consuming_the_marker() {
    let fixture = Fixture::new();
    let catalog = fixture.catalog();
    for request in fixture.baselines.requests() {
        let root = fixture.application(request, "recovery");
        let plan = catalog.plan_upgrade(&root).unwrap();
        fs::write(
            root.join(application_mutator::MUTATION_MARKER),
            "owner recovery state\n",
        )
        .unwrap();
        let before = files(&root);
        assert!(upgrade_recovery_pending(&root).unwrap());
        assert_eq!(
            publish_change_plan(&root, plan.change_plan())
                .unwrap_err()
                .kind(),
            MutationErrorKind::RecoveryRequired
        );
        assert_eq!(before, files(&root));
    }
}
