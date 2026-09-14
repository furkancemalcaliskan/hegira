use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
};

use serde::Serialize;

use crate::{
    ChangeOperation, ChangePath, ChangePlan, ChangePlanError, FileCreation, PlannedFileChange,
    PreconditionSummary, StructuredFileEdit,
};

pub const COMPONENT_INSTALLATION_SUMMARY_SCHEMA: u32 = 1;

/// Canonical application-owned locations that an additive component may
/// contribute to. Ownership is deliberately closed instead of accepting a
/// package-supplied destination root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplicationFileOwner {
    WorkspaceManifest,
    ApplicationManifest,
    Configuration,
    Server,
    Web,
    DomainShared,
    Domain,
    ApplicationContracts,
    Application,
    Infrastructure,
    Presentation,
}

impl ApplicationFileOwner {
    fn owns(self, path: &ChangePath) -> bool {
        let path = path.as_str();
        match self {
            Self::WorkspaceManifest => path == "Cargo.toml",
            Self::ApplicationManifest => path == "hegira.toml",
            Self::Configuration => below(path, "config"),
            Self::Server => below(path, "apps/server"),
            Self::Web => below(path, "apps/web"),
            Self::DomainShared => below(path, "crates/domain_shared"),
            Self::Domain => below(path, "crates/domain"),
            Self::ApplicationContracts => below(path, "crates/application_contracts"),
            Self::Application => below(path, "crates/application"),
            Self::Infrastructure => below(path, "crates/infrastructure"),
            Self::Presentation => below(path, "crates/presentation"),
        }
    }
}

fn below(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|remainder| remainder.starts_with('/'))
}

pub struct ComponentArtifact {
    owner: ApplicationFileOwner,
    creation: FileCreation,
}

impl std::fmt::Debug for ComponentArtifact {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComponentArtifact")
            .field("owner", &self.owner)
            .field("path", &self.creation.path())
            .finish_non_exhaustive()
    }
}

impl ComponentArtifact {
    pub fn new(
        owner: ApplicationFileOwner,
        path: impl AsRef<std::path::Path>,
        content: impl Into<Vec<u8>>,
    ) -> Result<Self, ComponentInstallationError> {
        let creation =
            FileCreation::new(path, content).map_err(ComponentInstallationError::plan)?;
        validate_ownership(owner, creation.path())?;
        Ok(Self { owner, creation })
    }
}

pub struct ComponentIntegration {
    owner: ApplicationFileOwner,
    edit: StructuredFileEdit,
}

impl std::fmt::Debug for ComponentIntegration {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComponentIntegration")
            .field("owner", &self.owner)
            .field("path", &self.edit.path())
            .finish_non_exhaustive()
    }
}

impl ComponentIntegration {
    pub fn new(
        owner: ApplicationFileOwner,
        edit: StructuredFileEdit,
    ) -> Result<Self, ComponentInstallationError> {
        validate_ownership(owner, edit.path())?;
        if is_historical_migration(edit.path()) {
            return Err(ComponentInstallationError::at_path(
                ComponentInstallationErrorKind::HistoricalMigrationEdit,
                edit.path().clone(),
                "component installation cannot edit historical application migrations",
            ));
        }
        Ok(Self { owner, edit })
    }
}

pub enum ComponentContribution {
    Artifact(ComponentArtifact),
    Integration(ComponentIntegration),
}

impl From<ComponentArtifact> for ComponentContribution {
    fn from(artifact: ComponentArtifact) -> Self {
        Self::Artifact(artifact)
    }
}

impl From<ComponentIntegration> for ComponentContribution {
    fn from(integration: ComponentIntegration) -> Self {
        Self::Integration(integration)
    }
}

impl ComponentContribution {
    fn into_parts(self) -> (ApplicationFileOwner, PlannedFileChange) {
        match self {
            Self::Artifact(artifact) => (artifact.owner, artifact.creation.into()),
            Self::Integration(integration) => (integration.owner, integration.edit.into()),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ComponentInstallationPlan {
    component: String,
    owners: BTreeMap<ChangePath, ApplicationFileOwner>,
    changes: ChangePlan,
}

impl std::fmt::Debug for ComponentInstallationPlan {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComponentInstallationPlan")
            .field("summary", &self.summary())
            .finish()
    }
}

impl ComponentInstallationPlan {
    pub fn component(&self) -> &str {
        &self.component
    }

    pub fn changes(&self) -> &ChangePlan {
        &self.changes
    }

    pub fn owner(&self, path: &ChangePath) -> Option<ApplicationFileOwner> {
        self.owners.get(path).copied()
    }

    pub fn summary(&self) -> ComponentInstallationSummary {
        let changes = self.changes.summary();
        ComponentInstallationSummary {
            schema: COMPONENT_INSTALLATION_SUMMARY_SCHEMA,
            component: self.component.clone(),
            changes: changes
                .changes
                .into_iter()
                .map(|change| OwnedChangeSummary {
                    owner: self.owners[&ChangePath::new(&change.path)
                        .expect("validated plan paths remain valid")],
                    path: change.path,
                    operation: change.operation,
                    precondition: change.precondition,
                    result_sha256: change.result_sha256,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComponentInstallationSummary {
    pub schema: u32,
    pub component: String,
    pub changes: Vec<OwnedChangeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OwnedChangeSummary {
    pub owner: ApplicationFileOwner,
    pub path: String,
    pub operation: ChangeOperation,
    pub precondition: PreconditionSummary,
    pub result_sha256: String,
}

pub fn plan_component_installation<C, I, S>(
    component: C,
    installed_components: I,
    contributions: impl IntoIterator<Item = ComponentContribution>,
) -> Result<ComponentInstallationPlan, ComponentInstallationError>
where
    C: AsRef<str>,
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let component = validate_component_id(component.as_ref())?;
    let mut installed = BTreeSet::new();
    for installed_component in installed_components {
        let installed_component = validate_component_id(installed_component.as_ref())?;
        if !installed.insert(installed_component.clone()) {
            return Err(ComponentInstallationError::new(
                ComponentInstallationErrorKind::DuplicateInstalledComponent,
                None,
                format!("installed component `{installed_component}` is declared more than once"),
            ));
        }
    }
    if installed.contains(&component) {
        return Err(ComponentInstallationError::new(
            ComponentInstallationErrorKind::AlreadyInstalled,
            None,
            format!("component `{component}` is already installed"),
        ));
    }

    let mut owners = BTreeMap::new();
    let mut changes = Vec::new();
    for contribution in contributions {
        let (owner, change) = contribution.into_parts();
        let path = change.path().clone();
        if owners.insert(path.clone(), owner).is_some() {
            return Err(ComponentInstallationError::at_path(
                ComponentInstallationErrorKind::ConflictingContribution,
                path,
                "component installation contains multiple contributions for one path",
            ));
        }
        changes.push(change);
    }
    if changes.is_empty() {
        return Err(ComponentInstallationError::new(
            ComponentInstallationErrorKind::EmptyInstallation,
            None,
            "component installation must contain at least one contribution",
        ));
    }

    let changes = ChangePlan::new(changes).map_err(ComponentInstallationError::plan)?;
    Ok(ComponentInstallationPlan {
        component,
        owners,
        changes,
    })
}

fn validate_component_id(component: &str) -> Result<String, ComponentInstallationError> {
    let valid = !component.is_empty()
        && component.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'-' | b'_'))
        })
        && component
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric);
    if !valid {
        return Err(ComponentInstallationError::new(
            ComponentInstallationErrorKind::InvalidComponent,
            None,
            "component identities must start with a lowercase ASCII letter or digit and contain only lowercase ASCII letters, digits, hyphens, or underscores",
        ));
    }
    Ok(component.to_owned())
}

fn validate_ownership(
    owner: ApplicationFileOwner,
    path: &ChangePath,
) -> Result<(), ComponentInstallationError> {
    if owner.owns(path) {
        return Ok(());
    }
    Err(ComponentInstallationError::at_path(
        ComponentInstallationErrorKind::InvalidOwnership,
        path.clone(),
        "component contribution path is outside its canonical application owner",
    ))
}

fn is_historical_migration(path: &ChangePath) -> bool {
    path.as_str()
        .starts_with("crates/infrastructure/migrations/")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentInstallationErrorKind {
    InvalidComponent,
    DuplicateInstalledComponent,
    AlreadyInstalled,
    InvalidOwnership,
    HistoricalMigrationEdit,
    ConflictingContribution,
    EmptyInstallation,
    InvalidChangePlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentInstallationError {
    kind: ComponentInstallationErrorKind,
    path: Option<ChangePath>,
    message: String,
}

impl ComponentInstallationError {
    pub fn kind(&self) -> ComponentInstallationErrorKind {
        self.kind
    }

    pub fn path(&self) -> Option<&ChangePath> {
        self.path.as_ref()
    }

    fn new(
        kind: ComponentInstallationErrorKind,
        path: Option<ChangePath>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path,
            message: message.into(),
        }
    }

    fn at_path(
        kind: ComponentInstallationErrorKind,
        path: ChangePath,
        message: impl Into<String>,
    ) -> Self {
        Self::new(kind, Some(path), message)
    }

    fn plan(error: ChangePlanError) -> Self {
        Self::new(
            ComponentInstallationErrorKind::InvalidChangePlan,
            error.path().cloned(),
            error.to_string(),
        )
    }
}

impl Display for ComponentInstallationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ComponentInstallationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContentDigest, FilePrecondition};

    fn artifact(owner: ApplicationFileOwner, path: &str, content: &str) -> ComponentContribution {
        ComponentArtifact::new(owner, path, content.as_bytes().to_vec())
            .unwrap()
            .into()
    }

    fn integration(
        owner: ApplicationFileOwner,
        path: &str,
        observed: &str,
        result: &str,
    ) -> ComponentContribution {
        ComponentIntegration::new(
            owner,
            StructuredFileEdit::new(path, observed.as_bytes(), result.as_bytes().to_vec()).unwrap(),
        )
        .unwrap()
        .into()
    }

    #[test]
    fn additive_plans_are_owned_ordered_and_content_redacted() {
        let first = plan_component_installation(
            "identity",
            ["layered-base"],
            [
                integration(
                    ApplicationFileOwner::WorkspaceManifest,
                    "Cargo.toml",
                    "secret original",
                    "secret result",
                ),
                artifact(
                    ApplicationFileOwner::Domain,
                    "crates/domain/src/identity.rs",
                    "secret source",
                ),
            ],
        )
        .unwrap();
        let second = plan_component_installation(
            "identity",
            ["layered-base"],
            [
                artifact(
                    ApplicationFileOwner::Domain,
                    "crates/domain/src/identity.rs",
                    "secret source",
                ),
                integration(
                    ApplicationFileOwner::WorkspaceManifest,
                    "Cargo.toml",
                    "secret original",
                    "secret result",
                ),
            ],
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.component(), "identity");
        assert_eq!(first.changes().changes().len(), 2);
        assert_eq!(
            first.changes().changes()[0].precondition(),
            FilePrecondition::MatchesDigest(ContentDigest::calculate(b"secret original"))
        );
        assert_eq!(
            first.changes().changes()[1].precondition(),
            FilePrecondition::Absent
        );
        assert_eq!(first.summary(), second.summary());
        let summary = serde_json::to_string(&first.summary()).unwrap();
        assert!(!summary.contains("secret original"));
        assert!(!summary.contains("secret result"));
        assert!(!summary.contains("secret source"));
        assert!(summary.contains("matches-digest"));
        let debug = format!("{first:?}");
        assert!(!debug.contains("secret original"));
        assert!(!debug.contains("secret result"));
        assert!(!debug.contains("secret source"));
        assert_eq!(
            first.summary().schema,
            COMPONENT_INSTALLATION_SUMMARY_SCHEMA
        );
    }

    #[test]
    fn canonical_owners_reject_cross_layer_and_unsafe_paths() {
        let cross_layer = ComponentArtifact::new(
            ApplicationFileOwner::Domain,
            "apps/server/src/identity.rs",
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            cross_layer.kind(),
            ComponentInstallationErrorKind::InvalidOwnership
        );

        let unsafe_path = ComponentArtifact::new(
            ApplicationFileOwner::Domain,
            "crates/domain/../outside.rs",
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(
            unsafe_path.kind(),
            ComponentInstallationErrorKind::InvalidChangePlan
        );
    }

    #[test]
    fn installed_empty_and_conflicting_requests_fail_before_a_plan_exists() {
        let installed = plan_component_installation(
            "identity",
            ["identity"],
            [artifact(
                ApplicationFileOwner::Domain,
                "crates/domain/src/identity.rs",
                "source",
            )],
        )
        .unwrap_err();
        assert_eq!(
            installed.kind(),
            ComponentInstallationErrorKind::AlreadyInstalled
        );

        let empty =
            plan_component_installation("identity", std::iter::empty::<&str>(), std::iter::empty())
                .unwrap_err();
        assert_eq!(
            empty.kind(),
            ComponentInstallationErrorKind::EmptyInstallation
        );

        let conflicting = plan_component_installation(
            "identity",
            std::iter::empty::<&str>(),
            [
                artifact(
                    ApplicationFileOwner::Domain,
                    "crates/domain/src/identity.rs",
                    "one",
                ),
                artifact(
                    ApplicationFileOwner::Domain,
                    "crates/domain/src/identity.rs",
                    "two",
                ),
            ],
        )
        .unwrap_err();
        assert_eq!(
            conflicting.kind(),
            ComponentInstallationErrorKind::ConflictingContribution
        );
    }

    #[test]
    fn historical_migrations_are_append_only() {
        let edit = StructuredFileEdit::new(
            "crates/infrastructure/migrations/sqlite/001_existing.sql",
            b"old",
            b"changed".to_vec(),
        )
        .unwrap();
        let error =
            ComponentIntegration::new(ApplicationFileOwner::Infrastructure, edit).unwrap_err();
        assert_eq!(
            error.kind(),
            ComponentInstallationErrorKind::HistoricalMigrationEdit
        );

        let creation = ComponentArtifact::new(
            ApplicationFileOwner::Infrastructure,
            "crates/infrastructure/migrations/sqlite/002_identity.sql",
            b"CREATE TABLE identity;".to_vec(),
        )
        .unwrap();
        let plan = plan_component_installation(
            "identity",
            std::iter::empty::<&str>(),
            [ComponentContribution::from(creation)],
        )
        .unwrap();
        assert_eq!(
            plan.changes().changes()[0].operation(),
            ChangeOperation::Create
        );
    }

    #[test]
    fn component_identities_are_validated_for_targets_and_installed_state() {
        for component in ["", "Identity", "identity/escape", "-identity", "identité"] {
            let error = plan_component_installation(
                component,
                std::iter::empty::<&str>(),
                [artifact(
                    ApplicationFileOwner::Domain,
                    "crates/domain/src/identity.rs",
                    "source",
                )],
            )
            .unwrap_err();
            assert_eq!(
                error.kind(),
                ComponentInstallationErrorKind::InvalidComponent
            );
        }

        let error = plan_component_installation(
            "identity",
            ["invalid/component"],
            [artifact(
                ApplicationFileOwner::Domain,
                "crates/domain/src/identity.rs",
                "source",
            )],
        )
        .unwrap_err();
        assert_eq!(
            error.kind(),
            ComponentInstallationErrorKind::InvalidComponent
        );

        let duplicate = plan_component_installation(
            "identity",
            ["layered-base", "layered-base"],
            [artifact(
                ApplicationFileOwner::Domain,
                "crates/domain/src/identity.rs",
                "source",
            )],
        )
        .unwrap_err();
        assert_eq!(
            duplicate.kind(),
            ComponentInstallationErrorKind::DuplicateInstalledComponent
        );
    }
}
