use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
};

use serde::Serialize;

use crate::{
    CargoDependency, CargoDependencySection, ChangeOperation, ChangePath, ChangePlan,
    ChangePlanError, FileCreation, PlannedFileChange, PreconditionSummary, StructuredEditError,
    StructuredEditKind, StructuredEditOutcome, StructuredFileEdit, plan_cargo_dependency,
    plan_rust_managed_entry, plan_rust_module, plan_toml_array_string,
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

impl std::fmt::Debug for ComponentContribution {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Artifact(artifact) => artifact.fmt(formatter),
            Self::Integration(integration) => integration.fmt(formatter),
        }
    }
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
    fn owner(&self) -> ApplicationFileOwner {
        match self {
            Self::Artifact(artifact) => artifact.owner,
            Self::Integration(integration) => integration.owner,
        }
    }

    fn path(&self) -> &ChangePath {
        match self {
            Self::Artifact(artifact) => artifact.creation.path(),
            Self::Integration(integration) => integration.edit.path(),
        }
    }

    fn into_parts(self) -> (ApplicationFileOwner, PlannedFileChange) {
        match self {
            Self::Artifact(artifact) => (artifact.owner, artifact.creation.into()),
            Self::Integration(integration) => (integration.owner, integration.edit.into()),
        }
    }
}

/// Collapse an ordered series of edits into one contribution per path.
///
/// Each later edit must have observed the exact result of the preceding
/// contribution. The first absent or digest precondition is retained while the
/// final bytes and digest become the published result.
pub fn compose_component_contributions(
    contributions: impl IntoIterator<Item = ComponentContribution>,
) -> Result<Vec<ComponentContribution>, ComponentInstallationError> {
    let mut composed = BTreeMap::<ChangePath, ComponentContribution>::new();
    for contribution in contributions {
        let path = contribution.path().clone();
        let Some(previous) = composed.remove(&path) else {
            composed.insert(path, contribution);
            continue;
        };
        if previous.owner() != contribution.owner() {
            return Err(ComponentInstallationError::at_path(
                ComponentInstallationErrorKind::InvalidOwnership,
                path,
                "component contribution sequence changes the canonical owner of one path",
            ));
        }
        let chained = match (previous, contribution) {
            (
                ComponentContribution::Artifact(mut artifact),
                ComponentContribution::Integration(integration),
            ) if integration.edit.original_digest == artifact.creation.result_digest => {
                artifact.creation.content = integration.edit.content;
                artifact.creation.result_digest = integration.edit.result_digest;
                ComponentContribution::Artifact(artifact)
            }
            (
                ComponentContribution::Integration(mut previous),
                ComponentContribution::Integration(next),
            ) if next.edit.original_digest == previous.edit.result_digest => {
                previous.edit.content = next.edit.content;
                previous.edit.result_digest = next.edit.result_digest;
                ComponentContribution::Integration(previous)
            }
            _ => {
                return Err(ComponentInstallationError::at_path(
                    ComponentInstallationErrorKind::ConflictingContribution,
                    path,
                    "component contribution sequence does not preserve the preceding result digest",
                ));
            }
        };
        composed.insert(path, chained);
    }
    Ok(composed.into_values().collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentRustModuleTarget {
    DomainShared,
    Domain,
    ApplicationContracts,
    Application,
    Infrastructure,
    Presentation,
    Server,
    Web,
}

impl ComponentRustModuleTarget {
    fn owner_and_path(self) -> (ApplicationFileOwner, &'static str) {
        match self {
            Self::DomainShared => (
                ApplicationFileOwner::DomainShared,
                "crates/domain_shared/src/lib.rs",
            ),
            Self::Domain => (ApplicationFileOwner::Domain, "crates/domain/src/lib.rs"),
            Self::ApplicationContracts => (
                ApplicationFileOwner::ApplicationContracts,
                "crates/application_contracts/src/lib.rs",
            ),
            Self::Application => (
                ApplicationFileOwner::Application,
                "crates/application/src/lib.rs",
            ),
            Self::Infrastructure => (
                ApplicationFileOwner::Infrastructure,
                "crates/infrastructure/src/lib.rs",
            ),
            Self::Presentation => (
                ApplicationFileOwner::Presentation,
                "crates/presentation/src/lib.rs",
            ),
            Self::Server => (ApplicationFileOwner::Server, "apps/server/src/lib.rs"),
            Self::Web => (ApplicationFileOwner::Web, "apps/web/src/lib.rs"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentCargoManifest {
    Workspace,
    DomainShared,
    Domain,
    ApplicationContracts,
    Application,
    Infrastructure,
    Presentation,
    Server,
    Web,
}

impl ComponentCargoManifest {
    fn owner_and_path(self) -> (ApplicationFileOwner, &'static str) {
        match self {
            Self::Workspace => (ApplicationFileOwner::WorkspaceManifest, "Cargo.toml"),
            Self::DomainShared => (
                ApplicationFileOwner::DomainShared,
                "crates/domain_shared/Cargo.toml",
            ),
            Self::Domain => (ApplicationFileOwner::Domain, "crates/domain/Cargo.toml"),
            Self::ApplicationContracts => (
                ApplicationFileOwner::ApplicationContracts,
                "crates/application_contracts/Cargo.toml",
            ),
            Self::Application => (
                ApplicationFileOwner::Application,
                "crates/application/Cargo.toml",
            ),
            Self::Infrastructure => (
                ApplicationFileOwner::Infrastructure,
                "crates/infrastructure/Cargo.toml",
            ),
            Self::Presentation => (
                ApplicationFileOwner::Presentation,
                "crates/presentation/Cargo.toml",
            ),
            Self::Server => (ApplicationFileOwner::Server, "apps/server/Cargo.toml"),
            Self::Web => (ApplicationFileOwner::Web, "apps/web/Cargo.toml"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentManagedRustTarget {
    InfrastructureConfigurationFields,
    InfrastructurePostgresMigrationSources,
    InfrastructureSqliteMigrationSources,
    ServerBearerRoutes,
    ServerOpenApiDocuments,
    ServerLeptosContexts,
    WebNativeRoutes,
    WebSplitRoutes,
    WebNavigationItems,
}

impl ComponentManagedRustTarget {
    fn integration(self) -> (ApplicationFileOwner, &'static str, &'static str) {
        match self {
            Self::InfrastructureConfigurationFields => (
                ApplicationFileOwner::Infrastructure,
                "crates/infrastructure/src/config.rs",
                "module-config-fields",
            ),
            Self::InfrastructurePostgresMigrationSources => (
                ApplicationFileOwner::Infrastructure,
                "crates/infrastructure/src/operations.rs",
                "module-migrations-postgres",
            ),
            Self::InfrastructureSqliteMigrationSources => (
                ApplicationFileOwner::Infrastructure,
                "crates/infrastructure/src/operations.rs",
                "module-migrations-sqlite",
            ),
            Self::ServerBearerRoutes => (
                ApplicationFileOwner::Server,
                "apps/server/src/server.rs",
                "resource-bearer-routes",
            ),
            Self::ServerOpenApiDocuments => (
                ApplicationFileOwner::Server,
                "apps/server/src/server.rs",
                "resource-openapi-documents",
            ),
            Self::ServerLeptosContexts => (
                ApplicationFileOwner::Server,
                "apps/server/src/server.rs",
                "resource-leptos-contexts",
            ),
            Self::WebNativeRoutes => (
                ApplicationFileOwner::Web,
                "apps/web/src/routes.rs",
                "resource-routes-native",
            ),
            Self::WebSplitRoutes => (
                ApplicationFileOwner::Web,
                "apps/web/src/routes.rs",
                "resource-routes-split",
            ),
            Self::WebNavigationItems => (
                ApplicationFileOwner::Web,
                "apps/web/src/app/navigation.rs",
                "nav-items",
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentConfigurationTarget {
    Capabilities,
}

impl ComponentConfigurationTarget {
    fn integration(
        self,
    ) -> (
        ApplicationFileOwner,
        &'static str,
        &'static [&'static str],
        &'static str,
    ) {
        match self {
            Self::Capabilities => (
                ApplicationFileOwner::ApplicationManifest,
                "hegira.toml",
                &["composition"],
                "capabilities",
            ),
        }
    }
}

#[derive(Debug)]
pub enum ComponentEditOutcome {
    Planned(ComponentContribution),
    AlreadyPresent {
        owner: ApplicationFileOwner,
        path: ChangePath,
        kind: StructuredEditKind,
    },
}

pub fn plan_component_rust_module(
    target: ComponentRustModuleTarget,
    observed_source: &[u8],
    module: &str,
) -> Result<ComponentEditOutcome, ComponentEditError> {
    let (owner, path) = target.owner_and_path();
    component_edit_outcome(owner, plan_rust_module(path, observed_source, module)?)
}

pub fn plan_component_managed_rust_entry(
    target: ComponentManagedRustTarget,
    observed_source: &[u8],
    key: &str,
    entry: &str,
) -> Result<ComponentEditOutcome, ComponentEditError> {
    let (owner, path, block) = target.integration();
    component_edit_outcome(
        owner,
        plan_rust_managed_entry(path, observed_source, block, key, entry)?,
    )
}

pub fn plan_component_cargo_dependency(
    manifest: ComponentCargoManifest,
    observed_source: &[u8],
    section: CargoDependencySection,
    dependency: &CargoDependency,
) -> Result<ComponentEditOutcome, ComponentEditError> {
    let (owner, path) = manifest.owner_and_path();
    if matches!(manifest, ComponentCargoManifest::Workspace)
        != matches!(section, CargoDependencySection::WorkspaceDependencies)
    {
        return Err(ComponentEditError::UnsupportedCargoSection);
    }
    component_edit_outcome(
        owner,
        plan_cargo_dependency(path, observed_source, section, dependency)?,
    )
}

pub fn plan_component_cargo_feature(
    manifest: ComponentCargoManifest,
    observed_source: &[u8],
    feature: &str,
    selection: &str,
) -> Result<ComponentEditOutcome, ComponentEditError> {
    if matches!(manifest, ComponentCargoManifest::Workspace) {
        return Err(ComponentEditError::UnsupportedCargoSection);
    }
    let (owner, path) = manifest.owner_and_path();
    component_edit_outcome(
        owner,
        plan_toml_array_string(path, observed_source, &["features"], feature, selection)?,
    )
}

pub fn plan_component_configuration_entry(
    target: ComponentConfigurationTarget,
    observed_source: &[u8],
    entry: &str,
) -> Result<ComponentEditOutcome, ComponentEditError> {
    let (owner, path, table, key) = target.integration();
    component_edit_outcome(
        owner,
        plan_toml_array_string(path, observed_source, table, key, entry)?,
    )
}

fn component_edit_outcome(
    owner: ApplicationFileOwner,
    outcome: StructuredEditOutcome,
) -> Result<ComponentEditOutcome, ComponentEditError> {
    match outcome {
        StructuredEditOutcome::Planned { edit, .. } => Ok(ComponentEditOutcome::Planned(
            ComponentIntegration::new(owner, edit)?.into(),
        )),
        StructuredEditOutcome::AlreadyPresent { kind, path } => {
            Ok(ComponentEditOutcome::AlreadyPresent { owner, path, kind })
        }
    }
}

#[derive(Debug)]
pub enum ComponentEditError {
    Structured(StructuredEditError),
    Installation(ComponentInstallationError),
    UnsupportedCargoSection,
}

impl ComponentEditError {
    pub fn structured_kind(&self) -> Option<crate::StructuredEditErrorKind> {
        match self {
            Self::Structured(error) => Some(error.kind()),
            Self::Installation(_) | Self::UnsupportedCargoSection => None,
        }
    }
}

impl Display for ComponentEditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structured(error) => error.fmt(formatter),
            Self::Installation(error) => error.fmt(formatter),
            Self::UnsupportedCargoSection => formatter.write_str(
                "workspace dependencies belong only in the workspace manifest; package manifests must use their local dependency sections",
            ),
        }
    }
}

impl std::error::Error for ComponentEditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Structured(error) => Some(error),
            Self::Installation(error) => Some(error),
            Self::UnsupportedCargoSection => None,
        }
    }
}

impl From<StructuredEditError> for ComponentEditError {
    fn from(error: StructuredEditError) -> Self {
        Self::Structured(error)
    }
}

impl From<ComponentInstallationError> for ComponentEditError {
    fn from(error: ComponentInstallationError) -> Self {
        Self::Installation(error)
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

    fn planned_component(outcome: ComponentEditOutcome) -> ComponentContribution {
        match outcome {
            ComponentEditOutcome::Planned(contribution) => contribution,
            ComponentEditOutcome::AlreadyPresent { .. } => {
                panic!("expected a planned component edit")
            }
        }
    }

    fn resulting_content(contribution: &ComponentContribution) -> &[u8] {
        match contribution {
            ComponentContribution::Artifact(artifact) => &artifact.creation.content,
            ComponentContribution::Integration(integration) => &integration.edit.content,
        }
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

    #[test]
    fn cargo_dependencies_and_features_compose_into_one_digest_guarded_edit() {
        let original =
            b"[dependencies]\nserde.workspace = true\n\n[features]\nssr = [\"serde/derive\"]\n";
        let dependency =
            CargoDependency::workspace("identity_http", true, std::iter::empty::<&str>());
        let dependency_edit = planned_component(
            plan_component_cargo_dependency(
                ComponentCargoManifest::Server,
                original,
                CargoDependencySection::Dependencies,
                &dependency,
            )
            .unwrap(),
        );
        let feature_edit = planned_component(
            plan_component_cargo_feature(
                ComponentCargoManifest::Server,
                resulting_content(&dependency_edit),
                "ssr",
                "dep:identity_http",
            )
            .unwrap(),
        );
        let contributions =
            compose_component_contributions([dependency_edit, feature_edit]).unwrap();
        let plan =
            plan_component_installation("identity", ["layered-base"], contributions).unwrap();

        assert_eq!(plan.changes().changes().len(), 1);
        assert_eq!(
            plan.changes().changes()[0].precondition(),
            FilePrecondition::MatchesDigest(ContentDigest::calculate(original))
        );
        let result = std::str::from_utf8(plan.changes().changes()[0].resulting_content()).unwrap();
        assert!(result.contains("identity_http = { workspace = true, optional = true }"));
        assert!(result.contains("\"dep:identity_http\""));
        assert!(matches!(
            plan_component_cargo_feature(
                ComponentCargoManifest::Server,
                result.as_bytes(),
                "ssr",
                "dep:identity_http",
            )
            .unwrap(),
            ComponentEditOutcome::AlreadyPresent {
                kind: StructuredEditKind::TomlArrayString,
                ..
            }
        ));
    }

    #[test]
    fn module_route_migration_and_configuration_targets_are_explicit() {
        let module_source = format!(
            "{}\n{}\n",
            crate::RUST_MODULES_START,
            crate::RUST_MODULES_END
        );
        let module = plan_component_rust_module(
            ComponentRustModuleTarget::Domain,
            module_source.as_bytes(),
            "identity",
        )
        .unwrap();
        let module = planned_component(module);
        assert_eq!(module.path().as_str(), "crates/domain/src/lib.rs");

        let migrations = b"fn sources() {\n    // hegira:module-migrations-sqlite\n    // hegira:module-migrations-sqlite:end\n}\n";
        let migration = planned_component(
            plan_component_managed_rust_entry(
                ComponentManagedRustTarget::InfrastructureSqliteMigrationSources,
                migrations,
                "identity",
                "identity_sqlx::identity::migrations::sqlite_migration_source(),",
            )
            .unwrap(),
        );
        assert_eq!(
            migration.path().as_str(),
            "crates/infrastructure/src/operations.rs"
        );

        let routes = b"let routes = Router::new()\n    // hegira:resource-bearer-routes\n    // hegira:resource-bearer-routes:end\n;\n";
        let route = planned_component(
            plan_component_managed_rust_entry(
                ComponentManagedRustTarget::ServerBearerRoutes,
                routes,
                "identity",
                ".merge(identity_routes())",
            )
            .unwrap(),
        );
        assert_eq!(route.path().as_str(), "apps/server/src/server.rs");

        let manifest = b"[composition]\ncapabilities = []\n";
        let configuration = planned_component(
            plan_component_configuration_entry(
                ComponentConfigurationTarget::Capabilities,
                manifest,
                "authentication",
            )
            .unwrap(),
        );
        assert_eq!(configuration.path().as_str(), "hegira.toml");
    }

    #[test]
    fn malformed_points_stale_sequences_and_wrong_cargo_targets_fail_closed() {
        let missing = plan_component_managed_rust_entry(
            ComponentManagedRustTarget::InfrastructureConfigurationFields,
            b"pub struct AppConfig;\n",
            "identity",
            "pub identity: IdentityConfig,",
        )
        .unwrap_err();
        assert_eq!(
            missing.structured_kind(),
            Some(crate::StructuredEditErrorKind::MissingIntegrationPoint)
        );

        let dependency =
            CargoDependency::workspace("identity_http", false, std::iter::empty::<&str>());
        assert!(matches!(
            plan_component_cargo_dependency(
                ComponentCargoManifest::Workspace,
                b"[workspace.dependencies]\n",
                CargoDependencySection::Dependencies,
                &dependency,
            ),
            Err(ComponentEditError::UnsupportedCargoSection)
        ));

        let first = integration(
            ApplicationFileOwner::Domain,
            "crates/domain/src/lib.rs",
            "original",
            "first",
        );
        let stale = integration(
            ApplicationFileOwner::Domain,
            "crates/domain/src/lib.rs",
            "other",
            "second",
        );
        let error = compose_component_contributions([first, stale]).unwrap_err();
        assert_eq!(
            error.kind(),
            ComponentInstallationErrorKind::ConflictingContribution
        );
    }

    #[test]
    fn canonical_template_exposes_the_typed_component_integration_points() {
        let application = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../templates/applications/layered");

        let operations =
            std::fs::read(application.join("crates/infrastructure/src/operations.rs")).unwrap();
        for target in [
            ComponentManagedRustTarget::InfrastructurePostgresMigrationSources,
            ComponentManagedRustTarget::InfrastructureSqliteMigrationSources,
        ] {
            assert!(matches!(
                plan_component_managed_rust_entry(
                    target,
                    &operations,
                    "identity",
                    match target {
                        ComponentManagedRustTarget::InfrastructurePostgresMigrationSources =>
                            "identity_sqlx::identity::migrations::postgres_migration_source(),",
                        ComponentManagedRustTarget::InfrastructureSqliteMigrationSources =>
                            "identity_sqlx::identity::migrations::sqlite_migration_source(),",
                        _ => unreachable!(),
                    },
                )
                .unwrap(),
                ComponentEditOutcome::AlreadyPresent {
                    kind: StructuredEditKind::RustManagedEntry,
                    ..
                }
            ));
        }

        let config =
            std::fs::read(application.join("crates/infrastructure/src/config.rs")).unwrap();
        assert!(matches!(
            plan_component_managed_rust_entry(
                ComponentManagedRustTarget::InfrastructureConfigurationFields,
                &config,
                "probe",
                "pub probe: ProbeConfig,",
            )
            .unwrap(),
            ComponentEditOutcome::Planned(_)
        ));

        let infrastructure_manifest =
            std::fs::read(application.join("crates/infrastructure/Cargo.toml")).unwrap();
        let dependency =
            CargoDependency::workspace("identity_sqlx", false, std::iter::empty::<&str>());
        assert!(matches!(
            plan_component_cargo_dependency(
                ComponentCargoManifest::Infrastructure,
                &infrastructure_manifest,
                CargoDependencySection::Dependencies,
                &dependency,
            )
            .unwrap(),
            ComponentEditOutcome::AlreadyPresent {
                kind: StructuredEditKind::CargoDependency,
                ..
            }
        ));

        let server_manifest = std::fs::read(application.join("apps/server/Cargo.toml")).unwrap();
        assert!(matches!(
            plan_component_cargo_feature(
                ComponentCargoManifest::Server,
                &server_manifest,
                "db-sqlite",
                "identity_sqlx/db-sqlite",
            )
            .unwrap(),
            ComponentEditOutcome::AlreadyPresent {
                kind: StructuredEditKind::TomlArrayString,
                ..
            }
        ));
    }
}
