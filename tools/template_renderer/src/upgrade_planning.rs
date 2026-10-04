use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    path::Path,
};

use application_manifest::{
    APPLICATION_MANIFEST_SCHEMA, ApplicationComposition, ApplicationManifest,
    ApplicationUpgradeState, InstalledModule, SourceOwnershipClass,
};
use application_mutator::{
    CargoDependency, CargoDependencySection, ChangeOperation, ChangePlan, FileCreation,
    ManagedFileRetirement, PlannedFileChange, PreconditionSummary, StructuredEditErrorKind,
    StructuredEditOutcome, StructuredFileEdit, plan_cargo_dependency,
    plan_cargo_dependency_transition,
};
use serde::Serialize;

use crate::{
    AuthenticatedManagedSource, AuthenticatedUpgradeBoundary, CompositionRequest,
    FrameworkDependency, ManagedIntegrationTransition, ManagedIntegrationTransitionKind,
    ManifestCatalog, ResolvedComposition, UpgradeAuthenticationDiagnosticKind,
    UpgradeAuthenticationError, UpgradeManifestTransition, UpgradeReleaseIdentity,
};

pub const UPGRADE_PLAN_SUMMARY_SCHEMA: u32 = 1;
pub const UPGRADE_PLANNING_DIAGNOSTIC_SCHEMA: u32 = 1;
const HISTORICAL_MIGRATION_ROOT: &str = "crates/infrastructure/migrations/";
const APPLICATION_MANIFEST_PATH: &str = "hegira.toml";
const APPLICATION_MANIFEST_INTEGRATION: &str = "application-manifest";
const FRAMEWORK_DEPENDENCIES_INTEGRATION: &str = "framework-dependencies";

impl ManifestCatalog {
    /// Validate the target manifest contract without constructing a mutation plan.
    pub fn validate_upgrade_transition(
        &self,
        boundary: &AuthenticatedUpgradeBoundary,
    ) -> Result<(), UpgradePlanningError> {
        validated_target_manifest(self, boundary).map(|_| ())
    }
}

fn validated_target_manifest(
    catalog: &ManifestCatalog,
    boundary: &AuthenticatedUpgradeBoundary,
) -> Result<(ResolvedComposition, ApplicationManifest), UpgradePlanningError> {
    let graph = resolve_target_composition(catalog, boundary)?;
    reject_component_removal(boundary, &graph)?;
    target_framework_dependencies(catalog, &graph)?;
    let target = target_manifest(boundary, &graph)?;
    if manifest_transitions(boundary.application(), &target) != boundary.edge().manifest_transitions
    {
        return Err(planning_error(
            UpgradePlanningErrorKind::Incompatible,
            "manifest-transitions",
        ));
    }
    Ok((graph, target))
}

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
    manifest_transitions: Vec<UpgradeManifestTransition>,
    framework_dependencies: Vec<String>,
    target_components: Vec<String>,
    target_modules: Vec<String>,
}

impl UpgradePlan {
    pub fn from_authenticated(
        catalog: &ManifestCatalog,
        boundary: AuthenticatedUpgradeBoundary,
    ) -> Result<Self, UpgradePlanningError> {
        let (graph, target_manifest) = validated_target_manifest(catalog, &boundary)?;
        let dependencies = target_framework_dependencies(catalog, &graph)?;
        let manifest_transitions = manifest_transitions(boundary.application(), &target_manifest);

        let mut changes = Vec::new();
        let mut owners = BTreeMap::new();
        let mut structured_paths = BTreeSet::new();
        plan_framework_dependencies(
            &boundary,
            &dependencies,
            &mut changes,
            &mut owners,
            &mut structured_paths,
        )?;
        plan_application_manifest(
            &boundary,
            &target_manifest,
            &mut changes,
            &mut owners,
            &mut structured_paths,
        )?;

        for managed in boundary.managed_sources() {
            if structured_paths.contains(&managed.transition().path) {
                continue;
            }
            let transition = managed.transition();
            let ownership = match transition.kind {
                ManagedIntegrationTransitionKind::Create
                | ManagedIntegrationTransitionKind::Edit => {
                    &boundary.edge().composition.target_ownership
                }
                ManagedIntegrationTransitionKind::Retire => {
                    &boundary.edge().composition.source_ownership
                }
            };
            let change =
                plan_transition(ownership, transition, managed.source(), managed.target())?;
            insert_owner(&mut owners, transition)?;
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
            manifest_transitions,
            framework_dependencies: dependencies
                .iter()
                .map(|dependency| dependency.name.clone())
                .collect(),
            target_components: graph
                .components
                .iter()
                .map(|component| component.id.clone())
                .collect(),
            target_modules: graph
                .modules
                .iter()
                .map(|module| module.id.clone())
                .collect(),
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
            manifest_transitions: self.manifest_transitions.clone(),
            framework_dependencies: self.framework_dependencies.clone(),
            target_components: self.target_components.clone(),
            target_modules: self.target_modules.clone(),
            changes,
        }
    }
}

fn resolve_target_composition(
    catalog: &ManifestCatalog,
    boundary: &AuthenticatedUpgradeBoundary,
) -> Result<ResolvedComposition, UpgradePlanningError> {
    let edge = boundary.edge();
    let request = CompositionRequest::new(
        edge.target.framework.clone(),
        edge.target.package.clone(),
        edge.composition.target.components.clone(),
    )
    .with_recorded_state(
        edge.composition
            .target
            .modules
            .iter()
            .map(|module| InstalledModule {
                id: module.clone(),
                version: edge.target.package.version.clone(),
            }),
        edge.composition.target.capabilities.iter().copied(),
    );
    catalog
        .resolve_composition(&request)
        .map_err(|_| planning_error(UpgradePlanningErrorKind::Incompatible, "target-composition"))
}

fn reject_component_removal(
    boundary: &AuthenticatedUpgradeBoundary,
    graph: &ResolvedComposition,
) -> Result<(), UpgradePlanningError> {
    let target_components = graph
        .components
        .iter()
        .map(|component| component.id.as_str())
        .collect::<BTreeSet<_>>();
    let target_modules = graph
        .modules
        .iter()
        .map(|module| module.id.as_str())
        .collect::<BTreeSet<_>>();
    let source = &boundary.edge().composition.source;
    if source
        .components
        .iter()
        .any(|component| !target_components.contains(component.as_str()))
        || source
            .modules
            .iter()
            .any(|module| !target_modules.contains(module.as_str()))
    {
        return Err(planning_error(
            UpgradePlanningErrorKind::Unsupported,
            "component-removal",
        ));
    }
    Ok(())
}

fn target_framework_dependencies(
    catalog: &ManifestCatalog,
    graph: &ResolvedComposition,
) -> Result<Vec<FrameworkDependency>, UpgradePlanningError> {
    let mut dependencies = BTreeMap::new();
    for component in catalog
        .components_for(graph)
        .map_err(|_| planning_error(UpgradePlanningErrorKind::Incompatible, "target-components"))?
    {
        for dependency in component.framework_dependencies.iter().chain(
            component
                .installation
                .iter()
                .flat_map(|installation| &installation.framework_dependencies),
        ) {
            let key = (
                dependency.manifest.to_string_lossy().into_owned(),
                dependency.name.clone(),
            );
            if dependencies
                .insert(key.clone(), dependency.clone())
                .is_some_and(|existing| existing != *dependency)
            {
                return Err(planning_error(
                    UpgradePlanningErrorKind::Incompatible,
                    format!("framework-dependency.{}", dependency.name),
                ));
            }
        }
    }
    Ok(dependencies.into_values().collect())
}

fn target_manifest(
    boundary: &AuthenticatedUpgradeBoundary,
    graph: &ResolvedComposition,
) -> Result<ApplicationManifest, UpgradePlanningError> {
    let edge = boundary.edge();
    let mut manifest = boundary.application().clone();
    manifest.schema = APPLICATION_MANIFEST_SCHEMA;
    manifest.framework = edge.target.framework.clone();
    manifest.composition = Some(ApplicationComposition {
        package: edge.target.package.clone(),
        components: graph.installed_components().collect(),
        modules: graph.modules.clone(),
        capabilities: graph.capabilities.clone(),
    });
    manifest.upgrade = Some(ApplicationUpgradeState {
        framework: edge.target.framework.clone(),
        package: edge.target.package.clone(),
        ownership: edge.composition.target_ownership.clone(),
    });
    manifest
        .to_toml()
        .map_err(|_| planning_error(UpgradePlanningErrorKind::Incompatible, "target-manifest"))?;
    Ok(manifest)
}

fn manifest_transitions(
    source: &ApplicationManifest,
    target: &ApplicationManifest,
) -> Vec<UpgradeManifestTransition> {
    let mut transitions = Vec::new();
    if source.schema != target.schema {
        transitions.push(UpgradeManifestTransition::Schema);
    }
    if source.framework != target.framework {
        transitions.push(UpgradeManifestTransition::Framework);
    }
    let source_composition = source.composition.as_ref();
    let target_composition = target.composition.as_ref();
    if source_composition.map(|composition| &composition.package)
        != target_composition.map(|composition| &composition.package)
    {
        transitions.push(UpgradeManifestTransition::Package);
    }
    if source_composition.map(|composition| &composition.components)
        != target_composition.map(|composition| &composition.components)
    {
        transitions.push(UpgradeManifestTransition::Components);
    }
    if source_composition.map(|composition| &composition.modules)
        != target_composition.map(|composition| &composition.modules)
    {
        transitions.push(UpgradeManifestTransition::Modules);
    }
    if source_composition.map(|composition| &composition.capabilities)
        != target_composition.map(|composition| &composition.capabilities)
    {
        transitions.push(UpgradeManifestTransition::Capabilities);
    }
    if source.upgrade.as_ref().map(|upgrade| &upgrade.ownership)
        != target.upgrade.as_ref().map(|upgrade| &upgrade.ownership)
    {
        transitions.push(UpgradeManifestTransition::Ownership);
    }
    transitions.sort();
    transitions
}

fn plan_application_manifest(
    boundary: &AuthenticatedUpgradeBoundary,
    target: &ApplicationManifest,
    changes: &mut Vec<PlannedFileChange>,
    owners: &mut BTreeMap<String, UpgradeChangeOwner>,
    structured_paths: &mut BTreeSet<String>,
) -> Result<(), UpgradePlanningError> {
    let managed = required_managed_source(
        boundary,
        APPLICATION_MANIFEST_PATH,
        APPLICATION_MANIFEST_INTEGRATION,
    )?;
    if managed.transition().kind != ManagedIntegrationTransitionKind::Edit
        || managed.source() != Some(boundary.manifest_source())
    {
        return Err(planning_error(
            UpgradePlanningErrorKind::Incompatible,
            "application-manifest",
        ));
    }
    let target = target
        .to_toml()
        .map_err(|_| planning_error(UpgradePlanningErrorKind::Incompatible, "target-manifest"))?;
    let change = StructuredFileEdit::new(
        APPLICATION_MANIFEST_PATH,
        boundary.manifest_source(),
        target.into_bytes(),
    )
    .map(PlannedFileChange::from)
    .map_err(plan_error)?;
    insert_owner(owners, managed.transition())?;
    structured_paths.insert(APPLICATION_MANIFEST_PATH.to_owned());
    changes.push(change);
    Ok(())
}

fn reject_removed_framework_dependencies(
    source: &[u8],
    repository: &str,
    version: &str,
    targets: &[&FrameworkDependency],
) -> Result<(), UpgradePlanningError> {
    let source = std::str::from_utf8(source).map_err(|_| {
        planning_error(UpgradePlanningErrorKind::Conflict, "framework-dependencies")
    })?;
    let document = source.parse::<toml::Table>().map_err(|_| {
        planning_error(UpgradePlanningErrorKind::Conflict, "framework-dependencies")
    })?;
    let dependencies = document
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            planning_error(UpgradePlanningErrorKind::Conflict, "framework-dependencies")
        })?;
    let mut source_names = BTreeSet::new();
    for (name, declaration) in dependencies {
        let Some(declaration) = declaration.as_table() else {
            continue;
        };
        let repository_matches = declaration
            .get("git")
            .and_then(toml::Value::as_str)
            .is_some_and(|value| value == repository);
        let version_matches = declaration
            .get("tag")
            .and_then(toml::Value::as_str)
            .is_some_and(|value| value == version);
        if repository_matches != version_matches {
            return Err(planning_error(
                UpgradePlanningErrorKind::Conflict,
                format!("framework-dependency.{name}"),
            ));
        }
        if repository_matches {
            source_names.insert(name.as_str());
        }
    }
    let target_names = targets
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect::<BTreeSet<_>>();
    if !source_names.is_subset(&target_names) {
        return Err(planning_error(
            UpgradePlanningErrorKind::Unsupported,
            "framework-dependency-removal",
        ));
    }
    Ok(())
}

fn plan_framework_dependencies(
    boundary: &AuthenticatedUpgradeBoundary,
    dependencies: &[FrameworkDependency],
    changes: &mut Vec<PlannedFileChange>,
    owners: &mut BTreeMap<String, UpgradeChangeOwner>,
    structured_paths: &mut BTreeSet<String>,
) -> Result<(), UpgradePlanningError> {
    let mut by_manifest = BTreeMap::<String, Vec<&FrameworkDependency>>::new();
    for dependency in dependencies {
        by_manifest
            .entry(dependency.manifest.to_string_lossy().into_owned())
            .or_default()
            .push(dependency);
    }
    for (path, manifest_dependencies) in by_manifest {
        let managed = required_managed_source(boundary, &path, FRAMEWORK_DEPENDENCIES_INTEGRATION)?;
        if managed.transition().kind != ManagedIntegrationTransitionKind::Edit {
            return Err(planning_error(
                UpgradePlanningErrorKind::Incompatible,
                "framework-dependencies",
            ));
        }
        let original = required_source(managed.source())?;
        reject_removed_framework_dependencies(
            original,
            &boundary.edge().source.framework.repository,
            &boundary.edge().source.framework.version,
            &manifest_dependencies,
        )?;
        let mut current = original.to_vec();
        for dependency in manifest_dependencies {
            let default_features = dependency.default_features.unwrap_or(true);
            let source = CargoDependency::framework_release(
                &dependency.name,
                &boundary.edge().source.framework.repository,
                &boundary.edge().source.framework.version,
                default_features,
                false,
                std::iter::empty::<String>(),
            );
            let target = CargoDependency::framework_release(
                &dependency.name,
                &boundary.edge().target.framework.repository,
                &boundary.edge().target.framework.version,
                default_features,
                false,
                std::iter::empty::<String>(),
            );
            let outcome = match plan_cargo_dependency_transition(
                &path,
                &current,
                CargoDependencySection::WorkspaceDependencies,
                &source,
                &target,
            ) {
                Ok(outcome) => outcome,
                Err(error) if error.kind() == StructuredEditErrorKind::TomlConflict => {
                    plan_cargo_dependency(
                        &path,
                        &current,
                        CargoDependencySection::WorkspaceDependencies,
                        &target,
                    )
                    .map_err(structured_edit_error)?
                }
                Err(error) => return Err(structured_edit_error(error)),
            };
            current = match outcome {
                StructuredEditOutcome::Planned { edit, .. } => edit.resulting_content().to_vec(),
                StructuredEditOutcome::AlreadyPresent { .. } => {
                    return Err(planning_error(
                        UpgradePlanningErrorKind::Conflict,
                        "framework-dependencies",
                    ));
                }
            };
        }
        let change = StructuredFileEdit::new(&path, original, current)
            .map(PlannedFileChange::from)
            .map_err(plan_error)?;
        insert_owner(owners, managed.transition())?;
        structured_paths.insert(path);
        changes.push(change);
    }
    Ok(())
}

fn required_managed_source<'a>(
    boundary: &'a AuthenticatedUpgradeBoundary,
    path: &str,
    integration: &str,
) -> Result<&'a AuthenticatedManagedSource, UpgradePlanningError> {
    let mut matches = boundary.managed_sources().iter().filter(|managed| {
        managed.transition().path == path && managed.transition().integration == integration
    });
    let source = matches.next().ok_or_else(|| {
        planning_error(
            UpgradePlanningErrorKind::Incompatible,
            integration.to_owned(),
        )
    })?;
    if matches.next().is_some() {
        return Err(planning_error(
            UpgradePlanningErrorKind::Conflict,
            integration.to_owned(),
        ));
    }
    Ok(source)
}

fn insert_owner(
    owners: &mut BTreeMap<String, UpgradeChangeOwner>,
    transition: &ManagedIntegrationTransition,
) -> Result<(), UpgradePlanningError> {
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
    Ok(())
}

fn structured_edit_error(error: application_mutator::StructuredEditError) -> UpgradePlanningError {
    planning_error(
        UpgradePlanningErrorKind::Conflict,
        error.path().map_or("structured-edit", |path| path.as_str()),
    )
}
impl ManifestCatalog {
    pub fn plan_upgrade(
        &self,
        application_root: impl AsRef<Path>,
    ) -> Result<UpgradePlan, UpgradePlanningError> {
        let boundary = self
            .authenticate_upgrade_source(application_root)
            .map_err(UpgradePlanningError::from)?;
        UpgradePlan::from_authenticated(self, boundary)
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
    pub manifest_transitions: Vec<UpgradeManifestTransition>,
    pub framework_dependencies: Vec<String>,
    pub target_components: Vec<String>,
    pub target_modules: Vec<String>,
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
            target_source_component: None,
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
