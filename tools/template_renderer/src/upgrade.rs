use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    path::Path,
};

use application_manifest::{
    ApplicationCapability, ClientAdapter, DatabaseAdapter, FrameworkContract, PackageIdentity,
};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{
    ComponentManifest, ComponentPackageManifest, RendererError, Result,
    manifest::{validate_identifier, validate_relative_path},
};

pub const UPGRADE_EDGE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeReleaseIdentity {
    pub framework: FrameworkContract,
    pub package: PackageIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeCompositionState {
    pub database: DatabaseAdapter,
    pub client: ClientAdapter,
    pub components: Vec<String>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<ApplicationCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeCompositionTransition {
    pub id: String,
    pub source: UpgradeCompositionState,
    pub target: UpgradeCompositionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpgradeManifestTransition {
    Schema,
    Framework,
    Package,
    Components,
    Modules,
    Capabilities,
    Ownership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedIntegrationTransitionKind {
    Create,
    Edit,
    Retire,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedIntegrationTransition {
    pub component: String,
    pub path: String,
    pub integration: String,
    pub kind: ManagedIntegrationTransitionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeEdgeManifest {
    pub schema: u32,
    pub id: String,
    pub source: UpgradeReleaseIdentity,
    pub target: UpgradeReleaseIdentity,
    pub compositions: Vec<UpgradeCompositionTransition>,
    #[serde(default)]
    pub manifest_transitions: Vec<UpgradeManifestTransition>,
    #[serde(default)]
    pub managed_integrations: Vec<ManagedIntegrationTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeEdgeRequest {
    pub source: UpgradeReleaseIdentity,
    pub composition: UpgradeCompositionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedUpgradeEdge {
    pub schema: u32,
    pub id: String,
    pub source: UpgradeReleaseIdentity,
    pub target: UpgradeReleaseIdentity,
    pub composition: UpgradeCompositionTransition,
    pub manifest_transitions: Vec<UpgradeManifestTransition>,
    pub managed_integrations: Vec<ManagedIntegrationTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpgradeEdgeDiagnosticKind {
    UnsupportedRelease,
    UnsupportedComposition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeEdgeDiagnostic {
    pub kind: UpgradeEdgeDiagnosticKind,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeEdgeError {
    diagnostic: UpgradeEdgeDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SourceCompositionKey {
    framework_repository: String,
    framework_version: String,
    package_id: String,
    database: DatabaseAdapter,
    client: ClientAdapter,
    components: Vec<String>,
    modules: Vec<String>,
    capabilities: Vec<ApplicationCapability>,
}

impl UpgradeEdgeError {
    pub fn diagnostic(&self) -> &UpgradeEdgeDiagnostic {
        &self.diagnostic
    }
}

impl Display for UpgradeEdgeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let kind = match self.diagnostic.kind {
            UpgradeEdgeDiagnosticKind::UnsupportedRelease => "unsupported-release",
            UpgradeEdgeDiagnosticKind::UnsupportedComposition => "unsupported-composition",
        };
        write!(formatter, "{kind}: {}", self.diagnostic.subject)
    }
}

impl std::error::Error for UpgradeEdgeError {}

pub(crate) fn validate_upgrade_graph(
    package: &ComponentPackageManifest,
    components: &BTreeMap<String, ComponentManifest>,
    component_paths: &BTreeSet<(String, String)>,
    edges: &mut [UpgradeEdgeManifest],
) -> Result<()> {
    edges.sort_by(|left, right| left.id.cmp(&right.id));
    let mut ids = BTreeSet::new();
    let mut release_pairs = BTreeSet::new();
    let mut source_states = BTreeSet::new();

    for edge in edges.iter_mut() {
        if edge.schema != UPGRADE_EDGE_SCHEMA {
            return Err(RendererError::new(format!(
                "unsupported upgrade edge schema {}",
                edge.schema
            )));
        }
        validate_graph_identifier(&edge.id, "upgrade edge")?;
        if !ids.insert(edge.id.clone()) {
            return Err(RendererError::new("duplicate upgrade edge id"));
        }
        validate_release_identity(package, &edge.source, false)?;
        validate_release_identity(package, &edge.target, true)?;
        validate_direct_release(&edge.source, &edge.target)?;

        let release_pair = (
            edge.source.framework.version.clone(),
            edge.target.framework.version.clone(),
        );
        if !release_pairs.insert(release_pair) {
            return Err(RendererError::new("duplicate upgrade release edge"));
        }
        if edge.compositions.is_empty() {
            return Err(RendererError::new(format!(
                "upgrade edge {} declares no composition transitions",
                edge.id
            )));
        }

        edge.compositions
            .sort_by(|left, right| left.id.cmp(&right.id));
        let mut composition_ids = BTreeSet::new();
        for transition in &edge.compositions {
            validate_graph_identifier(&transition.id, "upgrade composition")?;
            if !composition_ids.insert(transition.id.clone()) {
                return Err(RendererError::new("duplicate upgrade composition id"));
            }
            validate_composition(package, components, &transition.source, false)?;
            validate_composition(package, components, &transition.target, true)?;
            if transition.source.database != transition.target.database
                || transition.source.client != transition.target.client
            {
                return Err(RendererError::new(
                    "upgrade edge cannot switch database or client adapters",
                ));
            }
            let source_key = composition_key(&edge.source, &transition.source);
            if !source_states.insert(source_key) {
                return Err(RendererError::new(
                    "upgrade graph maps one source composition more than once",
                ));
            }
        }

        sort_unique(&mut edge.manifest_transitions, "manifest transition")?;
        edge.managed_integrations.sort();
        let mut managed_integrations = BTreeSet::new();
        for transition in &edge.managed_integrations {
            validate_graph_identifier(&transition.component, "managed integration component")?;
            validate_graph_identifier(&transition.integration, "managed integration")?;
            validate_graph_path(Path::new(&transition.path), "managed integration path")?;
            if !component_paths.contains(&(transition.component.clone(), transition.path.clone())) {
                return Err(RendererError::new(
                    "upgrade edge references a graph-undeclared managed integration path",
                ));
            }
            if !managed_integrations.insert((
                transition.component.clone(),
                transition.path.clone(),
                transition.integration.clone(),
            )) {
                return Err(RendererError::new(
                    "upgrade edge contains conflicting managed integration transitions",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn resolve_upgrade_edge(
    edges: &[UpgradeEdgeManifest],
    request: &UpgradeEdgeRequest,
) -> std::result::Result<ResolvedUpgradeEdge, UpgradeEdgeError> {
    let matching_release = edges
        .iter()
        .find(|edge| edge.source == request.source)
        .ok_or_else(|| UpgradeEdgeError {
            diagnostic: UpgradeEdgeDiagnostic {
                kind: UpgradeEdgeDiagnosticKind::UnsupportedRelease,
                subject: request.source.framework.version.clone(),
            },
        })?;
    let composition = matching_release
        .compositions
        .iter()
        .find(|transition| transition.source == request.composition)
        .ok_or_else(|| UpgradeEdgeError {
            diagnostic: UpgradeEdgeDiagnostic {
                kind: UpgradeEdgeDiagnosticKind::UnsupportedComposition,
                subject: matching_release.id.clone(),
            },
        })?;

    Ok(ResolvedUpgradeEdge {
        schema: UPGRADE_EDGE_SCHEMA,
        id: matching_release.id.clone(),
        source: matching_release.source.clone(),
        target: matching_release.target.clone(),
        composition: composition.clone(),
        manifest_transitions: matching_release.manifest_transitions.clone(),
        managed_integrations: matching_release.managed_integrations.clone(),
    })
}

fn validate_release_identity(
    package: &ComponentPackageManifest,
    release: &UpgradeReleaseIdentity,
    target: bool,
) -> Result<()> {
    release
        .framework
        .validate()
        .map_err(|_| RendererError::new("upgrade edge contains an invalid framework repository"))?;
    if release.framework.repository != package.framework.repository
        || release.package.id != package.id
        || release.framework.version != release.package.version
    {
        return Err(RendererError::new(
            "upgrade edge release identity does not match the package contract",
        ));
    }
    parse_release(&release.framework.version)?;
    if target
        && (release.framework.version != package.framework.version
            || release.package.version != package.version)
    {
        return Err(RendererError::new(
            "upgrade edge target does not match the authenticated package release",
        ));
    }
    Ok(())
}

fn validate_direct_release(
    source: &UpgradeReleaseIdentity,
    target: &UpgradeReleaseIdentity,
) -> Result<()> {
    let source = parse_release(&source.framework.version)?;
    let target = parse_release(&target.framework.version)?;
    let direct_patch = target.major == source.major
        && target.minor == source.minor
        && source.patch.checked_add(1) == Some(target.patch);
    let direct_minor = target.major == source.major
        && source.minor.checked_add(1) == Some(target.minor)
        && target.patch == 0;
    let direct_major =
        source.major.checked_add(1) == Some(target.major) && target.minor == 0 && target.patch == 0;
    let direct = direct_patch || direct_minor || direct_major;
    if !direct {
        return Err(RendererError::new(
            "upgrade edge must describe one direct forward SemVer release",
        ));
    }
    Ok(())
}

fn parse_release(value: &str) -> Result<Version> {
    let version = value
        .strip_prefix('v')
        .ok_or_else(|| RendererError::new("upgrade release must be a stable v-prefixed SemVer"))?;
    let version = Version::parse(version)
        .map_err(|_| RendererError::new("upgrade release must be a stable v-prefixed SemVer"))?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(RendererError::new(
            "upgrade release must be a stable v-prefixed SemVer",
        ));
    }
    Ok(version)
}

fn validate_composition(
    package: &ComponentPackageManifest,
    components: &BTreeMap<String, ComponentManifest>,
    state: &UpgradeCompositionState,
    target: bool,
) -> Result<()> {
    validate_sorted_members(&state.components, "upgrade component", false)?;
    validate_sorted_members(&state.modules, "upgrade module", true)?;
    validate_sorted_capabilities(&state.capabilities)?;

    for component in &state.components {
        let manifest = components.get(component).ok_or_else(|| {
            RendererError::new("upgrade composition references a graph-undeclared component")
        })?;
        if target
            && let Some(installation) = &manifest.installation
            && (!installation.databases.contains(&state.database)
                || !installation.clients.contains(&state.client))
        {
            return Err(RendererError::new(
                "upgrade target composition uses an unsupported component adapter",
            ));
        }
    }
    if state
        .modules
        .iter()
        .any(|module| !package.modules.contains(module))
    {
        return Err(RendererError::new(
            "upgrade composition references a graph-undeclared module",
        ));
    }
    let declared_capabilities = components
        .values()
        .flat_map(|component| component.provides_capabilities.iter().copied())
        .collect::<BTreeSet<_>>();
    if state
        .capabilities
        .iter()
        .any(|capability| !declared_capabilities.contains(capability))
    {
        return Err(RendererError::new(
            "upgrade composition references a graph-undeclared capability",
        ));
    }
    if target {
        let request = crate::CompositionRequest::new(
            package.framework.clone(),
            PackageIdentity {
                id: package.id.clone(),
                version: package.version.clone(),
            },
            state.components.clone(),
        );
        let resolved = crate::composition::resolve(package, components, &request)
            .map_err(|_| RendererError::new("upgrade target composition is not resolvable"))?;
        let resolved_components = resolved
            .components
            .iter()
            .map(|component| component.id.clone())
            .collect::<BTreeSet<_>>();
        let declared_components = state.components.iter().cloned().collect::<BTreeSet<_>>();
        let resolved_modules = resolved
            .modules
            .iter()
            .map(|module| module.id.clone())
            .collect::<Vec<_>>();
        let resolved_capabilities = resolved.capabilities.iter().copied().collect::<Vec<_>>();
        if resolved_components != declared_components
            || resolved_modules != state.modules
            || resolved_capabilities != state.capabilities
        {
            return Err(RendererError::new(
                "upgrade target composition does not match the resolved component graph",
            ));
        }
    }
    Ok(())
}

fn validate_sorted_members(values: &[String], kind: &str, allow_empty: bool) -> Result<()> {
    if values.is_empty() && !allow_empty {
        return Err(RendererError::new(format!("upgrade declares no {kind}s")));
    }
    for value in values {
        validate_graph_identifier(value, kind)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(RendererError::new(format!(
            "upgrade {kind}s must be sorted and unique"
        )));
    }
    Ok(())
}

fn validate_sorted_capabilities(values: &[ApplicationCapability]) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(RendererError::new(
            "upgrade capabilities must be sorted and unique",
        ));
    }
    Ok(())
}

fn sort_unique<T: Ord>(values: &mut [T], kind: &str) -> Result<()> {
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(RendererError::new(format!(
            "upgrade edge contains a duplicate {kind}"
        )));
    }
    Ok(())
}

fn validate_graph_identifier(value: &str, kind: &str) -> Result<()> {
    validate_identifier(value, kind)
        .map_err(|_| RendererError::new(format!("upgrade graph contains an invalid {kind}")))
}

fn validate_graph_path(path: &Path, kind: &str) -> Result<()> {
    validate_relative_path(path, kind)
        .map_err(|_| RendererError::new(format!("upgrade graph contains an invalid {kind}")))
}

fn composition_key(
    release: &UpgradeReleaseIdentity,
    state: &UpgradeCompositionState,
) -> SourceCompositionKey {
    SourceCompositionKey {
        framework_repository: release.framework.repository.clone(),
        framework_version: release.framework.version.clone(),
        package_id: release.package.id.clone(),
        database: state.database,
        client: state.client,
        components: state.components.clone(),
        modules: state.modules.clone(),
        capabilities: state.capabilities.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_release_rejects_downgrades_and_skips() {
        for (source, target) in [("v0.7.0", "v0.6.0"), ("v0.5.0", "v0.7.0")] {
            let source = release(source);
            let target = release(target);
            assert!(validate_direct_release(&source, &target).is_err());
        }
        assert!(validate_direct_release(&release("v0.6.0"), &release("v0.7.0")).is_ok());
    }

    fn release(version: &str) -> UpgradeReleaseIdentity {
        UpgradeReleaseIdentity {
            framework: FrameworkContract {
                repository: application_manifest::HEGIRA_FRAMEWORK_REPOSITORY.to_owned(),
                version: version.to_owned(),
            },
            package: PackageIdentity {
                id: application_manifest::HEGIRA_COMPONENT_PACKAGE.to_owned(),
                version: version.to_owned(),
            },
        }
    }
}
