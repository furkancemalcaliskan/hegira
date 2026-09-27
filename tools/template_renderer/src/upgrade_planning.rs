use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    path::Path,
};

use application_manifest::SourceOwnershipClass;
use application_mutator::{
    ChangeOperation, ChangePlan, FileCreation, ManagedFileRetirement, PlannedFileChange,
    PreconditionSummary, StructuredFileEdit,
};
use serde::Serialize;

use crate::{
    AuthenticatedUpgradeBoundary, ManagedIntegrationTransition, ManagedIntegrationTransitionKind,
    ManifestCatalog, UpgradeAuthenticationDiagnosticKind, UpgradeAuthenticationError,
    UpgradeReleaseIdentity,
};

pub const UPGRADE_PLAN_SUMMARY_SCHEMA: u32 = 1;
pub const UPGRADE_PLANNING_DIAGNOSTIC_SCHEMA: u32 = 1;
const HISTORICAL_MIGRATION_ROOT: &str = "crates/infrastructure/migrations/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpgradePlanningErrorKind {
    Blocked,
    Unsupported,
    Incompatible,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradePlanningDiagnostic {
    pub schema: u32,
    pub kind: UpgradePlanningErrorKind,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradePlanningError {
    diagnostic: UpgradePlanningDiagnostic,
}

impl UpgradePlanningError {
    pub fn diagnostic(&self) -> &UpgradePlanningDiagnostic {
        &self.diagnostic
    }
}

impl Display for UpgradePlanningError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {}",
            planning_kind_name(self.diagnostic.kind),
            self.diagnostic.subject
        )
    }
}

impl std::error::Error for UpgradePlanningError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpgradeChangeOwner {
    component: String,
    integration: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradePlan {
    edge_id: String,
    source: UpgradeReleaseIdentity,
    target: UpgradeReleaseIdentity,
    source_package_digest: String,
    source_baseline_digest: String,
    change_plan: ChangePlan,
    owners: BTreeMap<String, UpgradeChangeOwner>,
}

impl UpgradePlan {
    pub fn from_authenticated(
        boundary: AuthenticatedUpgradeBoundary,
    ) -> Result<Self, UpgradePlanningError> {
        let mut changes = Vec::new();
        let mut owners = BTreeMap::new();
        for managed in boundary.managed_sources() {
            let transition = managed.transition();
            let change = plan_transition(
                &boundary.edge().composition.source_ownership,
                transition,
                managed.source(),
                managed.target(),
            )?;

            if owners
                .insert(
                    transition.path.clone(),
                    UpgradeChangeOwner {
                        component: transition.component.clone(),
                        integration: transition.integration.clone(),
                    },
                )
                .is_some()
            {
                return Err(planning_error(
                    UpgradePlanningErrorKind::Conflict,
                    transition.path.clone(),
                ));
            }
            changes.push(change);
        }
        let change_plan = ChangePlan::new(changes).map_err(plan_error)?;

        Ok(Self {
            edge_id: boundary.edge().id.clone(),
            source: boundary.edge().source.clone(),
            target: boundary.edge().target.clone(),
            source_package_digest: boundary.source_package_digest().to_owned(),
            source_baseline_digest: boundary.source_baseline_digest().to_owned(),
            change_plan,
            owners,
        })
    }

    pub fn edge_id(&self) -> &str {
        &self.edge_id
    }

    pub fn source(&self) -> &UpgradeReleaseIdentity {
        &self.source
    }

    pub fn target(&self) -> &UpgradeReleaseIdentity {
        &self.target
    }

    pub fn change_plan(&self) -> &ChangePlan {
        &self.change_plan
    }

    pub fn summary(&self) -> UpgradePlanSummary {
        let changes = self
            .change_plan
            .summary()
            .changes
            .into_iter()
            .map(|change| {
                let owner = self
                    .owners
                    .get(&change.path)
                    .expect("a validated upgrade change has one authenticated owner");
                UpgradePlannedChangeSummary {
                    path: change.path,
                    operation: change.operation,
                    component: owner.component.clone(),
                    integration: owner.integration.clone(),
                    ownership: SourceOwnershipClass::ManagedIntegration,
                    precondition: change.precondition,
                    result: change
                        .result_sha256
                        .map_or(UpgradeResultSummary::Absent, |sha256| {
                            UpgradeResultSummary::Present { sha256 }
                        }),
                }
            })
            .collect();
        UpgradePlanSummary {
            schema: UPGRADE_PLAN_SUMMARY_SCHEMA,
            edge: self.edge_id.clone(),
            source: UpgradeReleaseSummary::from(&self.source),
            target: UpgradeReleaseSummary::from(&self.target),
            source_package_digest: self.source_package_digest.clone(),
            source_baseline_digest: self.source_baseline_digest.clone(),
            changes,
        }
    }
}

impl ManifestCatalog {
    pub fn plan_upgrade(
        &self,
        application_root: impl AsRef<Path>,
    ) -> Result<UpgradePlan, UpgradePlanningError> {
        let boundary = self
            .authenticate_upgrade_source(application_root)
            .map_err(UpgradePlanningError::from)?;
        UpgradePlan::from_authenticated(boundary)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeReleaseSummary {
    pub framework_repository: String,
    pub framework_version: String,
    pub package: String,
    pub package_version: String,
}

impl From<&UpgradeReleaseIdentity> for UpgradeReleaseSummary {
    fn from(identity: &UpgradeReleaseIdentity) -> Self {
        Self {
            framework_repository: identity.framework.repository.clone(),
            framework_version: identity.framework.version.clone(),
            package: identity.package.id.clone(),
            package_version: identity.package.version.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradePlanSummary {
    pub schema: u32,
    pub edge: String,
    pub source: UpgradeReleaseSummary,
    pub target: UpgradeReleaseSummary,
    pub source_package_digest: String,
    pub source_baseline_digest: String,
    pub changes: Vec<UpgradePlannedChangeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradePlannedChangeSummary {
    pub path: String,
    pub operation: ChangeOperation,
    pub component: String,
    pub integration: String,
    pub ownership: SourceOwnershipClass,
    pub precondition: PreconditionSummary,
    pub result: UpgradeResultSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum UpgradeResultSummary {
    Absent,
    Present { sha256: String },
}

impl From<UpgradeAuthenticationError> for UpgradePlanningError {
    fn from(source: UpgradeAuthenticationError) -> Self {
        let diagnostic = source.diagnostic();
        let kind = match diagnostic.kind {
            UpgradeAuthenticationDiagnosticKind::UnsupportedRelease
            | UpgradeAuthenticationDiagnosticKind::UnsupportedComposition => {
                UpgradePlanningErrorKind::Unsupported
            }
            UpgradeAuthenticationDiagnosticKind::InvalidManifest
            | UpgradeAuthenticationDiagnosticKind::PackageContract => {
                UpgradePlanningErrorKind::Incompatible
            }
            UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot
            | UpgradeAuthenticationDiagnosticKind::OwnershipMismatch
            | UpgradeAuthenticationDiagnosticKind::MissingManagedSource
            | UpgradeAuthenticationDiagnosticKind::UnexpectedManagedSource
            | UpgradeAuthenticationDiagnosticKind::UnsupportedSourceType
            | UpgradeAuthenticationDiagnosticKind::SourceDigestMismatch
            | UpgradeAuthenticationDiagnosticKind::SourceLimitExceeded => {
                UpgradePlanningErrorKind::Blocked
            }
        };
        planning_error(kind, diagnostic.subject.clone())
    }
}

fn plan_transition(
    ownership: &application_manifest::SourceOwnership,
    transition: &ManagedIntegrationTransition,
    source: Option<&[u8]>,
    target: Option<&[u8]>,
) -> Result<PlannedFileChange, UpgradePlanningError> {
    if matches!(
        transition.kind,
        ManagedIntegrationTransitionKind::Edit | ManagedIntegrationTransitionKind::Retire
    ) && transition.path.starts_with(HISTORICAL_MIGRATION_ROOT)
    {
        return Err(planning_error(
            UpgradePlanningErrorKind::Incompatible,
            "immutable-history",
        ));
    }

    match transition.kind {
        ManagedIntegrationTransitionKind::Create => {
            FileCreation::new(&transition.path, required_target(target)?.to_vec())
                .map(PlannedFileChange::from)
        }
        ManagedIntegrationTransitionKind::Edit => StructuredFileEdit::new(
            &transition.path,
            required_source(source)?,
            required_target(target)?.to_vec(),
        )
        .map(PlannedFileChange::from),
        ManagedIntegrationTransitionKind::Retire => ManagedFileRetirement::new(
            ownership,
            &transition.path,
            &transition.integration,
            required_source(source)?,
        )
        .map(PlannedFileChange::from),
    }
    .map_err(plan_error)
}

fn required_source(source: Option<&[u8]>) -> Result<&[u8], UpgradePlanningError> {
    source.ok_or_else(|| {
        planning_error(
            UpgradePlanningErrorKind::Incompatible,
            "managed-integration.source",
        )
    })
}

fn required_target(target: Option<&[u8]>) -> Result<&[u8], UpgradePlanningError> {
    target.ok_or_else(|| {
        planning_error(
            UpgradePlanningErrorKind::Incompatible,
            "managed-integration.target",
        )
    })
}

fn plan_error(error: application_mutator::ChangePlanError) -> UpgradePlanningError {
    planning_error(
        UpgradePlanningErrorKind::Conflict,
        error.path().map_or("change-plan", |path| path.as_str()),
    )
}

fn planning_error(
    kind: UpgradePlanningErrorKind,
    subject: impl Into<String>,
) -> UpgradePlanningError {
    UpgradePlanningError {
        diagnostic: UpgradePlanningDiagnostic {
            schema: UPGRADE_PLANNING_DIAGNOSTIC_SCHEMA,
            kind,
            subject: subject.into(),
        },
    }
}

fn planning_kind_name(kind: UpgradePlanningErrorKind) -> &'static str {
    match kind {
        UpgradePlanningErrorKind::Blocked => "upgrade-blocked",
        UpgradePlanningErrorKind::Unsupported => "upgrade-unsupported",
        UpgradePlanningErrorKind::Incompatible => "upgrade-incompatible",
        UpgradePlanningErrorKind::Conflict => "upgrade-conflict",
    }
}

#[cfg(test)]
mod tests {
    use application_manifest::{SourceOwnership, SourceOwnershipClaim};

    use super::*;

    #[test]
    fn create_edit_and_retire_have_exact_preconditions_and_results() {
        let ownership = ownership("managed.rs", "managed-source");

        let creation = plan_transition(
            &ownership,
            &transition(ManagedIntegrationTransitionKind::Create, "managed.rs"),
            None,
            Some(b"created"),
        )
        .unwrap();
        assert_eq!(creation.operation(), ChangeOperation::Create);
        assert_eq!(
            creation.precondition(),
            application_mutator::FilePrecondition::Absent
        );
        assert_eq!(
            creation.result_digest(),
            Some(application_mutator::ContentDigest::calculate(b"created"))
        );

        let edit = plan_transition(
            &ownership,
            &transition(ManagedIntegrationTransitionKind::Edit, "managed.rs"),
            Some(b"source"),
            Some(b"target"),
        )
        .unwrap();
        assert_eq!(edit.operation(), ChangeOperation::Edit);
        assert_eq!(
            edit.precondition(),
            application_mutator::FilePrecondition::MatchesDigest(
                application_mutator::ContentDigest::calculate(b"source")
            )
        );
        assert_eq!(
            edit.result_digest(),
            Some(application_mutator::ContentDigest::calculate(b"target"))
        );

        let retirement = plan_transition(
            &ownership,
            &transition(ManagedIntegrationTransitionKind::Retire, "managed.rs"),
            Some(b"source"),
            None,
        )
        .unwrap();
        assert_eq!(retirement.operation(), ChangeOperation::Retire);
        assert_eq!(retirement.result_digest(), None);
    }

    #[test]
    fn historical_migrations_are_never_edited_or_retired() {
        let path = "crates/infrastructure/migrations/sqlite/001_history.sql";
        let ownership = ownership(path, "managed-source");
        for kind in [
            ManagedIntegrationTransitionKind::Edit,
            ManagedIntegrationTransitionKind::Retire,
        ] {
            let target = (kind == ManagedIntegrationTransitionKind::Edit).then_some(&b"target"[..]);
            let error =
                plan_transition(&ownership, &transition(kind, path), Some(b"source"), target)
                    .unwrap_err();
            assert_eq!(
                error.diagnostic().kind,
                UpgradePlanningErrorKind::Incompatible
            );
            assert_eq!(error.diagnostic().subject, "immutable-history");
        }
    }

    fn transition(
        kind: ManagedIntegrationTransitionKind,
        path: &str,
    ) -> ManagedIntegrationTransition {
        ManagedIntegrationTransition {
            component: "layered-base".to_owned(),
            path: path.to_owned(),
            integration: "managed-source".to_owned(),
            kind,
            source_sha256: None,
            target_sha256: None,
        }
    }

    fn ownership(path: &str, integration: &str) -> SourceOwnership {
        SourceOwnership {
            default: SourceOwnershipClass::ApplicationOwned,
            claims: vec![SourceOwnershipClaim {
                path: path.to_owned(),
                class: SourceOwnershipClass::ManagedIntegration,
                integration: Some(integration.to_owned()),
            }],
        }
    }
}
