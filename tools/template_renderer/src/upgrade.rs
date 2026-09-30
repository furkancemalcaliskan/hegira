use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    path::Path,
};

use application_manifest::{
    ApplicationCapability, ClientAdapter, DatabaseAdapter, FrameworkContract, PackageIdentity,
    SourceOwnership, SourceOwnershipClass,
};
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{
    ComponentManifest, ComponentPackageManifest, RendererError, Result,
    manifest::{validate_identifier, validate_relative_path},
};

pub const UPGRADE_EDGE_SCHEMA: u32 = 1;
const MAX_UPGRADE_EDGES: usize = 16;
const MAX_COMPOSITIONS_PER_EDGE: usize = 64;
const MAX_MANAGED_INTEGRATIONS_PER_EDGE: usize = 512;
const MAX_COMPOSITION_MEMBERS: usize = 64;

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
    pub source_baseline_digest: String,
    pub source_ownership: SourceOwnership,
    pub target_ownership: SourceOwnership,
    pub source: UpgradeCompositionState,
    pub target: UpgradeCompositionState,
    #[serde(default)]
    pub manifest_transitions: Option<Vec<UpgradeManifestTransition>>,
    #[serde(default)]
    pub managed_integrations: Vec<ManagedIntegrationTransition>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_source_component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeEdgeManifest {
    pub schema: u32,
    pub id: String,
    pub source_package_digest: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpgradeGraphDiagnosticKind {
    InvalidManifest,
    InvalidSchema,
    InvalidIdentifier,
    InvalidReleaseIdentity,
    InvalidReleaseVersion,
    TargetReleaseMismatch,
    NonDirectRelease,
    DuplicateEdge,
    DuplicateReleaseEdge,
    EmptyCompositionSet,
    DuplicateComposition,
    NonCanonicalComposition,
    UndeclaredComponent,
    UndeclaredModule,
    UndeclaredCapability,
    UnsupportedAdapter,
    TargetCompositionMismatch,
    AdapterChange,
    AmbiguousSourceComposition,
    DuplicateManifestTransition,
    InvalidManagedPath,
    InvalidDigest,
    InvalidOwnership,
    UndeclaredManagedPath,
    ConflictingManagedTransition,
    LimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeGraphDiagnostic {
    pub kind: UpgradeGraphDiagnosticKind,
    pub subject: String,
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

impl Display for UpgradeGraphDiagnostic {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {}",
            graph_diagnostic_kind_name(self.kind),
            self.subject
        )
    }
}

pub(crate) fn graph_error(
    kind: UpgradeGraphDiagnosticKind,
    subject: impl Into<String>,
) -> RendererError {
    RendererError::with_upgrade_graph(UpgradeGraphDiagnostic {
        kind,
        subject: subject.into(),
    })
}

fn graph_diagnostic_kind_name(kind: UpgradeGraphDiagnosticKind) -> &'static str {
    match kind {
        UpgradeGraphDiagnosticKind::InvalidManifest => "invalid-manifest",
        UpgradeGraphDiagnosticKind::InvalidSchema => "invalid-schema",
        UpgradeGraphDiagnosticKind::InvalidIdentifier => "invalid-identifier",
        UpgradeGraphDiagnosticKind::InvalidReleaseIdentity => "invalid-release-identity",
        UpgradeGraphDiagnosticKind::InvalidReleaseVersion => "invalid-release-version",
        UpgradeGraphDiagnosticKind::TargetReleaseMismatch => "target-release-mismatch",
        UpgradeGraphDiagnosticKind::NonDirectRelease => "non-direct-release",
        UpgradeGraphDiagnosticKind::DuplicateEdge => "duplicate-edge",
        UpgradeGraphDiagnosticKind::DuplicateReleaseEdge => "duplicate-release-edge",
        UpgradeGraphDiagnosticKind::EmptyCompositionSet => "empty-composition-set",
        UpgradeGraphDiagnosticKind::DuplicateComposition => "duplicate-composition",
        UpgradeGraphDiagnosticKind::NonCanonicalComposition => "non-canonical-composition",
        UpgradeGraphDiagnosticKind::UndeclaredComponent => "undeclared-component",
        UpgradeGraphDiagnosticKind::UndeclaredModule => "undeclared-module",
        UpgradeGraphDiagnosticKind::UndeclaredCapability => "undeclared-capability",
        UpgradeGraphDiagnosticKind::UnsupportedAdapter => "unsupported-adapter",
        UpgradeGraphDiagnosticKind::TargetCompositionMismatch => "target-composition-mismatch",
        UpgradeGraphDiagnosticKind::AdapterChange => "adapter-change",
        UpgradeGraphDiagnosticKind::AmbiguousSourceComposition => "ambiguous-source-composition",
        UpgradeGraphDiagnosticKind::DuplicateManifestTransition => "duplicate-manifest-transition",
        UpgradeGraphDiagnosticKind::InvalidManagedPath => "invalid-managed-path",
        UpgradeGraphDiagnosticKind::InvalidDigest => "invalid-digest",
        UpgradeGraphDiagnosticKind::InvalidOwnership => "invalid-ownership",
        UpgradeGraphDiagnosticKind::UndeclaredManagedPath => "undeclared-managed-path",
        UpgradeGraphDiagnosticKind::ConflictingManagedTransition => {
            "conflicting-managed-transition"
        }
        UpgradeGraphDiagnosticKind::LimitExceeded => "limit-exceeded",
    }
}

pub(crate) fn validate_upgrade_graph(
    package: &ComponentPackageManifest,
    components: &BTreeMap<String, ComponentManifest>,
    component_paths: &BTreeMap<(String, String), String>,
    edges: &mut [UpgradeEdgeManifest],
) -> Result<()> {
    if edges.len() > MAX_UPGRADE_EDGES {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::LimitExceeded,
            "upgrade-edges",
        ));
    }
    edges.sort_by(|left, right| left.id.cmp(&right.id));
    let mut ids = BTreeSet::new();
    let mut release_pairs = BTreeSet::new();
    let mut source_states = BTreeSet::new();

    for edge in edges.iter_mut() {
        if edge.schema != UPGRADE_EDGE_SCHEMA {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::InvalidSchema,
                "upgrade-edge.schema",
            ));
        }
        validate_graph_identifier(&edge.id, "upgrade edge")?;
        if !ids.insert(edge.id.clone()) {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::DuplicateEdge,
                "upgrade-edge.id",
            ));
        }
        validate_release_identity(package, &edge.source, false)?;
        validate_release_identity(package, &edge.target, true)?;
        validate_direct_release(&edge.source, &edge.target)?;
        validate_sha256(
            &edge.source_package_digest,
            "upgrade-edge.source-package-digest",
        )?;

        let release_pair = (
            edge.source.framework.version.clone(),
            edge.target.framework.version.clone(),
        );
        if !release_pairs.insert(release_pair) {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::DuplicateReleaseEdge,
                "upgrade-edge.release",
            ));
        }
        if edge.compositions.is_empty() {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::EmptyCompositionSet,
                "upgrade-edge.compositions",
            ));
        }
        if edge.compositions.len() > MAX_COMPOSITIONS_PER_EDGE {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::LimitExceeded,
                "upgrade-edge.compositions",
            ));
        }

        let integration_count = edge.managed_integrations.len()
            + edge
                .compositions
                .iter()
                .map(|composition| composition.managed_integrations.len())
                .sum::<usize>();
        if integration_count > MAX_MANAGED_INTEGRATIONS_PER_EDGE {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::LimitExceeded,
                "upgrade-edge.managed-integrations",
            ));
        }
        edge.compositions
            .sort_by(|left, right| left.id.cmp(&right.id));
        let mut composition_ids = BTreeSet::new();
        for transition in &mut edge.compositions {
            transition.source_ownership.claims.sort_by(|left, right| {
                (&left.path, left.class, &left.integration).cmp(&(
                    &right.path,
                    right.class,
                    &right.integration,
                ))
            });
            transition.target_ownership.claims.sort_by(|left, right| {
                (&left.path, left.class, &left.integration).cmp(&(
                    &right.path,
                    right.class,
                    &right.integration,
                ))
            });
            transition.managed_integrations.sort();
            if let Some(manifest_transitions) = &mut transition.manifest_transitions {
                sort_unique(manifest_transitions)?;
            }
            validate_graph_identifier(&transition.id, "upgrade composition")?;
            if !composition_ids.insert(transition.id.clone()) {
                return Err(graph_error(
                    UpgradeGraphDiagnosticKind::DuplicateComposition,
                    "upgrade-composition.id",
                ));
            }
            validate_composition(package, components, &transition.source, false)?;
            validate_composition(package, components, &transition.target, true)?;
            validate_sha256(
                &transition.source_baseline_digest,
                "upgrade-composition.source-baseline-digest",
            )?;
            transition.source_ownership.validate().map_err(|_| {
                graph_error(
                    UpgradeGraphDiagnosticKind::InvalidOwnership,
                    "upgrade-composition.source-ownership",
                )
            })?;
            transition.target_ownership.validate().map_err(|_| {
                graph_error(
                    UpgradeGraphDiagnosticKind::InvalidOwnership,
                    "upgrade-composition.target-ownership",
                )
            })?;
            if transition.source.database != transition.target.database
                || transition.source.client != transition.target.client
            {
                return Err(graph_error(
                    UpgradeGraphDiagnosticKind::AdapterChange,
                    "upgrade-composition.adapters",
                ));
            }
            let source_key = composition_key(&edge.source, &transition.source);
            if !source_states.insert(source_key) {
                return Err(graph_error(
                    UpgradeGraphDiagnosticKind::AmbiguousSourceComposition,
                    "upgrade-composition.source",
                ));
            }
        }

        sort_unique(&mut edge.manifest_transitions)?;
        edge.managed_integrations.sort();
        validate_managed_integrations(&edge.managed_integrations, components, component_paths)?;

        for composition in &edge.compositions {
            let applicable_components = composition
                .source
                .components
                .iter()
                .chain(&composition.target.components)
                .collect::<BTreeSet<_>>();
            validate_managed_integrations(
                &composition.managed_integrations,
                components,
                component_paths,
            )?;
            if composition
                .managed_integrations
                .iter()
                .any(|transition| !applicable_components.contains(&transition.component))
            {
                return Err(graph_error(
                    UpgradeGraphDiagnosticKind::UndeclaredComponent,
                    "upgrade-composition.managed-integration.component",
                ));
            }
            let mut applicable_integrations = BTreeSet::new();
            for transition in edge
                .managed_integrations
                .iter()
                .chain(composition.managed_integrations.iter())
                .filter(|transition| applicable_components.contains(&transition.component))
            {
                if !applicable_integrations.insert((
                    &transition.component,
                    &transition.path,
                    &transition.integration,
                )) {
                    return Err(graph_error(
                        UpgradeGraphDiagnosticKind::ConflictingManagedTransition,
                        "managed-integration",
                    ));
                }
                let ownership = match transition.kind {
                    ManagedIntegrationTransitionKind::Create => &composition.target_ownership,
                    ManagedIntegrationTransitionKind::Edit => {
                        let source_declared =
                            ownership_declares(&composition.source_ownership, transition);
                        if !source_declared {
                            return Err(graph_error(
                                UpgradeGraphDiagnosticKind::InvalidOwnership,
                                "managed-integration.source-ownership",
                            ));
                        }
                        &composition.target_ownership
                    }
                    ManagedIntegrationTransitionKind::Retire => &composition.source_ownership,
                };
                let declared = ownership_declares(ownership, transition);
                if !declared {
                    return Err(graph_error(
                        UpgradeGraphDiagnosticKind::InvalidOwnership,
                        "managed-integration.ownership",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_managed_integrations(
    integrations: &[ManagedIntegrationTransition],
    components: &BTreeMap<String, ComponentManifest>,
    component_paths: &BTreeMap<(String, String), String>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for transition in integrations {
        validate_graph_identifier(&transition.component, "managed integration component")?;
        validate_graph_identifier(&transition.integration, "managed integration")?;
        validate_graph_path(Path::new(&transition.path), "managed integration path")?;
        if !components.contains_key(&transition.component) {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::UndeclaredComponent,
                "managed-integration.component",
            ));
        }
        if let Some(source_component) = &transition.target_source_component {
            validate_graph_identifier(source_component, "target source component")?;
            if !components.contains_key(source_component) {
                return Err(graph_error(
                    UpgradeGraphDiagnosticKind::UndeclaredComponent,
                    "managed-integration.target-source-component",
                ));
            }
            if transition.kind == ManagedIntegrationTransitionKind::Retire {
                return Err(graph_error(
                    UpgradeGraphDiagnosticKind::InvalidDigest,
                    "managed-integration.target-source-component",
                ));
            }
        }
        validate_transition_digests(transition, component_paths)?;
        if !seen.insert((
            &transition.component,
            &transition.path,
            &transition.integration,
        )) {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::ConflictingManagedTransition,
                "managed-integration",
            ));
        }
    }
    Ok(())
}
fn ownership_declares(
    ownership: &SourceOwnership,
    transition: &ManagedIntegrationTransition,
) -> bool {
    ownership.claims.iter().any(|claim| {
        claim.path == transition.path
            && claim.class == SourceOwnershipClass::ManagedIntegration
            && claim.integration.as_deref() == Some(&transition.integration)
    })
}

fn validate_transition_digests(
    transition: &ManagedIntegrationTransition,
    component_paths: &BTreeMap<(String, String), String>,
) -> Result<()> {
    let source_required = matches!(
        transition.kind,
        ManagedIntegrationTransitionKind::Edit | ManagedIntegrationTransitionKind::Retire
    );
    let target_required = matches!(
        transition.kind,
        ManagedIntegrationTransitionKind::Create | ManagedIntegrationTransitionKind::Edit
    );
    if source_required != transition.source_sha256.is_some()
        || target_required != transition.target_sha256.is_some()
    {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::InvalidDigest,
            "managed-integration.digest-contract",
        ));
    }
    if let Some(source) = &transition.source_sha256 {
        validate_sha256(source, "managed-integration.source-sha256")?;
    }
    if transition.kind == ManagedIntegrationTransitionKind::Edit
        && transition.source_sha256 == transition.target_sha256
    {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::InvalidDigest,
            "managed-integration.unchanged-edit",
        ));
    }
    if let Some(target) = &transition.target_sha256 {
        validate_sha256(target, "managed-integration.target-sha256")?;
        let target_component = transition
            .target_source_component
            .as_ref()
            .unwrap_or(&transition.component);
        let actual = component_paths
            .get(&(target_component.clone(), transition.path.clone()))
            .ok_or_else(|| {
                graph_error(
                    UpgradeGraphDiagnosticKind::UndeclaredManagedPath,
                    "managed-integration.path",
                )
            })?;
        if actual != target {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::InvalidDigest,
                "managed-integration.target-sha256",
            ));
        }
    }
    Ok(())
}

fn validate_sha256(value: &str, subject: &str) -> Result<()> {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::InvalidDigest,
            subject,
        ));
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::InvalidDigest,
            subject,
        ));
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
        manifest_transitions: composition
            .manifest_transitions
            .clone()
            .unwrap_or_else(|| matching_release.manifest_transitions.clone()),
        managed_integrations: matching_release
            .managed_integrations
            .iter()
            .chain(composition.managed_integrations.iter())
            .cloned()
            .collect(),
    })
}

fn validate_release_identity(
    package: &ComponentPackageManifest,
    release: &UpgradeReleaseIdentity,
    target: bool,
) -> Result<()> {
    parse_release(&release.framework.version)?;
    release.framework.validate().map_err(|_| {
        graph_error(
            UpgradeGraphDiagnosticKind::InvalidReleaseIdentity,
            "upgrade-release.framework",
        )
    })?;
    if release.framework.repository != package.framework.repository
        || release.package.id != package.id
        || release.framework.version != release.package.version
    {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::InvalidReleaseIdentity,
            "upgrade-release.identity",
        ));
    }
    if target
        && (release.framework.version != package.framework.version
            || release.package.version != package.version)
    {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::TargetReleaseMismatch,
            "upgrade-edge.target",
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
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::NonDirectRelease,
            "upgrade-edge.release",
        ));
    }
    Ok(())
}

fn parse_release(value: &str) -> Result<Version> {
    let version = value.strip_prefix('v').ok_or_else(|| {
        graph_error(
            UpgradeGraphDiagnosticKind::InvalidReleaseVersion,
            "upgrade-release.version",
        )
    })?;
    let version = Version::parse(version).map_err(|_| {
        graph_error(
            UpgradeGraphDiagnosticKind::InvalidReleaseVersion,
            "upgrade-release.version",
        )
    })?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::InvalidReleaseVersion,
            "upgrade-release.version",
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
    if state.components.len() > MAX_COMPOSITION_MEMBERS
        || state.modules.len() > MAX_COMPOSITION_MEMBERS
        || state.capabilities.len() > MAX_COMPOSITION_MEMBERS
    {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::LimitExceeded,
            "upgrade-composition.members",
        ));
    }
    validate_sorted_members(&state.components, "component", false)?;
    validate_sorted_members(&state.modules, "module", true)?;
    validate_sorted_capabilities(&state.capabilities)?;

    for component in &state.components {
        let manifest = components.get(component).ok_or_else(|| {
            graph_error(
                UpgradeGraphDiagnosticKind::UndeclaredComponent,
                "upgrade-composition.components",
            )
        })?;
        if target
            && let Some(installation) = &manifest.installation
            && (!installation.databases.contains(&state.database)
                || !installation.clients.contains(&state.client))
        {
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::UnsupportedAdapter,
                "upgrade-composition.adapters",
            ));
        }
    }
    if state
        .modules
        .iter()
        .any(|module| !package.modules.contains(module))
    {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::UndeclaredModule,
            "upgrade-composition.modules",
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
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::UndeclaredCapability,
            "upgrade-composition.capabilities",
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
        let resolved =
            crate::composition::resolve(package, components, &request).map_err(|_| {
                graph_error(
                    UpgradeGraphDiagnosticKind::TargetCompositionMismatch,
                    "upgrade-composition.target",
                )
            })?;
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
            return Err(graph_error(
                UpgradeGraphDiagnosticKind::TargetCompositionMismatch,
                "upgrade-composition.target",
            ));
        }
    }
    Ok(())
}

fn validate_sorted_members(values: &[String], kind: &str, allow_empty: bool) -> Result<()> {
    if values.is_empty() && !allow_empty {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::NonCanonicalComposition,
            format!("upgrade-composition.{kind}s"),
        ));
    }
    for value in values {
        validate_graph_identifier(value, kind)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::NonCanonicalComposition,
            format!("upgrade-composition.{kind}s"),
        ));
    }
    Ok(())
}

fn validate_sorted_capabilities(values: &[ApplicationCapability]) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::NonCanonicalComposition,
            "upgrade-composition.capabilities",
        ));
    }
    Ok(())
}

fn sort_unique<T: Ord>(values: &mut [T]) -> Result<()> {
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(graph_error(
            UpgradeGraphDiagnosticKind::DuplicateManifestTransition,
            "upgrade-edge.manifest-transitions",
        ));
    }
    Ok(())
}

fn validate_graph_identifier(value: &str, kind: &str) -> Result<()> {
    validate_identifier(value, kind).map_err(|_| {
        graph_error(
            UpgradeGraphDiagnosticKind::InvalidIdentifier,
            format!("upgrade-graph.{}", kind.replace(' ', "-")),
        )
    })
}

fn validate_graph_path(path: &Path, kind: &str) -> Result<()> {
    validate_relative_path(path, kind).map_err(|_| {
        graph_error(
            UpgradeGraphDiagnosticKind::InvalidManagedPath,
            format!("upgrade-graph.{}", kind.replace(' ', "-")),
        )
    })
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
    use std::path::PathBuf;

    use super::*;
    use crate::{ComponentInstallationManifest, FrameworkDependency};

    #[test]
    fn every_supported_composition_and_adapter_resolves_independently() {
        let mut edges = vec![edge()];
        validate(&mut edges, &components()).expect("canonical graph should validate");

        assert_eq!(edges[0].compositions.len(), 6);
        for transition in &edges[0].compositions {
            let resolved = resolve_upgrade_edge(
                &edges,
                &UpgradeEdgeRequest {
                    source: release("v0.5.0"),
                    composition: transition.source.clone(),
                },
            )
            .expect("every declared source state should resolve");
            assert_eq!(resolved.composition, *transition);
            assert_eq!(resolved.target, release("v0.6.0"));
        }
    }

    #[test]
    fn composition_specific_managed_digests_resolve_without_cross_profile_leakage() {
        let mut candidate = edge();
        let base = candidate
            .managed_integrations
            .iter()
            .position(|integration| integration.component == "base")
            .map(|index| candidate.managed_integrations.remove(index))
            .unwrap();
        for composition in &mut candidate.compositions {
            let mut integration = base.clone();
            integration.source_sha256 = Some(if composition.id.starts_with("minimal") {
                digest('c')
            } else {
                digest('b')
            });
            if composition.id.starts_with("minimal") {
                composition.manifest_transitions = Some(vec![UpgradeManifestTransition::Framework]);
            }
            composition.managed_integrations.push(integration);
        }
        let mut edges = vec![candidate];
        validate(&mut edges, &components()).unwrap();
        for composition in &edges[0].compositions {
            let resolved = resolve_upgrade_edge(
                &edges,
                &UpgradeEdgeRequest {
                    source: release("v0.5.0"),
                    composition: composition.source.clone(),
                },
            )
            .unwrap();
            assert_eq!(
                resolved.manifest_transitions,
                if composition.id.starts_with("minimal") {
                    vec![UpgradeManifestTransition::Framework]
                } else {
                    edges[0].manifest_transitions.clone()
                }
            );
            let expected = if composition.id.starts_with("minimal") {
                digest('c')
            } else {
                digest('b')
            };
            assert_eq!(
                resolved
                    .managed_integrations
                    .iter()
                    .find(|integration| integration.component == "base")
                    .unwrap()
                    .source_sha256,
                Some(expected)
            );
        }
        let duplicated = edges[0].managed_integrations[0].clone();
        let matching = edges[0]
            .compositions
            .iter_mut()
            .find(|composition| {
                composition
                    .source
                    .components
                    .contains(&duplicated.component)
            })
            .unwrap();
        matching.managed_integrations.push(duplicated);
        assert_eq!(
            error_for(edges, components()).kind,
            UpgradeGraphDiagnosticKind::ConflictingManagedTransition
        );
    }

    #[test]
    fn target_source_component_is_declared_and_path_bounded() {
        let missing_component = error_after(|edge, _| {
            edge.managed_integrations[1].target_source_component =
                Some("unknown-component".to_owned());
        });
        assert_eq!(
            missing_component.kind,
            UpgradeGraphDiagnosticKind::UndeclaredComponent
        );

        let undeclared_path = error_after(|edge, _| {
            edge.managed_integrations[1].target_source_component = Some("identity".to_owned());
        });
        assert_eq!(
            undeclared_path.kind,
            UpgradeGraphDiagnosticKind::UndeclaredManagedPath
        );

        let retirement = error_after(|edge, _| {
            let transition = &mut edge.managed_integrations[1];
            transition.kind = ManagedIntegrationTransitionKind::Retire;
            transition.target_sha256 = None;
            transition.target_source_component = Some("identity".to_owned());
        });
        assert_eq!(retirement.kind, UpgradeGraphDiagnosticKind::InvalidDigest);
    }

    #[test]
    fn declaration_permutations_produce_byte_identical_resolution() {
        let components = components();
        let mut canonical = vec![edge()];
        validate(&mut canonical, &components).unwrap();
        let expected = resolved_snapshots(&canonical);

        for seed in 0..64 {
            let mut candidate = vec![edge()];
            candidate[0]
                .compositions
                .sort_by_key(|transition| deterministic_order_key(transition.id.as_bytes(), seed));
            candidate[0].manifest_transitions.sort_by_key(|transition| {
                deterministic_order_key(format!("{transition:?}").as_bytes(), seed + 97)
            });
            candidate[0].managed_integrations.reverse();
            for composition in &mut candidate[0].compositions {
                composition.managed_integrations.reverse();
            }

            validate(&mut candidate, &components).unwrap();
            assert_eq!(candidate, canonical, "seed {seed}");
            assert_eq!(resolved_snapshots(&candidate), expected, "seed {seed}");
        }
    }

    #[test]
    fn invalid_graph_classes_return_stable_typed_redacted_diagnostics() {
        const SENSITIVE: &str = "credential-shaped-input-must-not-appear";
        let mut diagnostics = Vec::new();

        diagnostics.push(error_after(|edge, _| edge.schema = 99));
        diagnostics.push(error_after(|edge, _| edge.id = format!("{SENSITIVE}!")));
        diagnostics.push(error_for(vec![edge(), edge()], components()));
        diagnostics.push(error_after(|edge, _| {
            edge.source.package.id = "different-package".to_owned();
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.source.framework.version = "v0.5.0-alpha.1".to_owned();
            edge.source.package.version = "v0.5.0-alpha.1".to_owned();
        }));
        diagnostics.push(error_after(|edge, _| edge.target = release("v0.5.0")));
        diagnostics.push(error_for(
            {
                let first = edge();
                let mut second = edge();
                second.id = "same-release-other-id".to_owned();
                vec![first, second]
            },
            components(),
        ));
        diagnostics.push(error_after(|edge, _| edge.source = release("v0.7.0")));
        diagnostics.push(error_after(|edge, _| edge.source = release("v0.4.0")));
        diagnostics.push(error_after(|edge, _| edge.compositions.clear()));
        diagnostics.push(error_after(|edge, _| {
            let mut duplicate = edge.compositions[0].clone();
            duplicate.target.components = vec!["base".to_owned(), "minimal".to_owned()];
            edge.compositions.push(duplicate);
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.compositions[0].source.components = vec!["unknown".to_owned()];
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.compositions[0].source.components.reverse();
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.compositions[0].source.modules = vec!["unknown".to_owned()];
        }));
        diagnostics.push(error_after(|_, components| {
            components
                .values_mut()
                .for_each(|component| component.provides_capabilities.clear());
        }));
        diagnostics.push(error_after(|_, components| {
            components
                .get_mut("identity")
                .expect("identity component should exist")
                .installation = Some(ComponentInstallationManifest {
                module: "identity".to_owned(),
                databases: vec![DatabaseAdapter::Sqlite],
                clients: vec![ClientAdapter::Leptos],
                contributions: Vec::new(),
                framework_dependencies: Vec::new(),
            });
        }));
        diagnostics.push(error_after(|_, components| {
            components
                .get_mut("minimal")
                .expect("minimal component should exist")
                .requires = vec!["identity".to_owned()];
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.compositions[0].target.client = ClientAdapter::Leptos;
            edge.compositions[0].source.database = DatabaseAdapter::Sqlite;
            edge.compositions[0].target.database = DatabaseAdapter::Postgres;
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.compositions[0].target.capabilities.clear();
        }));
        diagnostics.push(error_after(|edge, _| {
            let mut ambiguous = edge.compositions[0].clone();
            ambiguous.id = "zz-ambiguous".to_owned();
            ambiguous.target = state("minimal", ambiguous.target.database);
            edge.compositions.push(ambiguous);
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.manifest_transitions
                .push(UpgradeManifestTransition::Schema);
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.managed_integrations[0].path = "../outside".to_owned();
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.managed_integrations[0].path = "unknown.rs".to_owned();
        }));
        diagnostics.push(error_after(|edge, _| {
            let mut conflicting = edge.managed_integrations[0].clone();
            conflicting.kind = ManagedIntegrationTransitionKind::Retire;
            conflicting.target_sha256 = None;
            edge.managed_integrations.push(conflicting);
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.compositions = (0..=MAX_COMPOSITIONS_PER_EDGE)
                .map(|index| {
                    let mut transition = edge.compositions[0].clone();
                    transition.id = format!("oversized-{index:03}");
                    transition
                })
                .collect();
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.compositions[0].source.components = (0..=MAX_COMPOSITION_MEMBERS)
                .map(|_| "base".to_owned())
                .collect();
        }));
        diagnostics.push(error_after(|edge, _| {
            edge.managed_integrations = (0..=MAX_MANAGED_INTEGRATIONS_PER_EDGE)
                .map(|index| ManagedIntegrationTransition {
                    component: "base".to_owned(),
                    path: "base.rs".to_owned(),
                    integration: format!("oversized-{index:03}"),
                    kind: ManagedIntegrationTransitionKind::Edit,
                    source_sha256: Some(digest('b')),
                    target_source_component: None,
                    target_sha256: Some(digest('a')),
                })
                .collect();
        }));
        diagnostics.push(error_for(
            (0..=MAX_UPGRADE_EDGES)
                .map(|index| {
                    let mut edge = edge();
                    edge.id = format!("oversized-{index:03}");
                    edge
                })
                .collect(),
            components(),
        ));

        let output = diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            output,
            include_str!("../tests/snapshots/upgrade-graph-invalid.txt").trim_end()
        );
        assert!(!output.contains(SENSITIVE));
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.subject.is_empty())
        );
    }

    #[test]
    fn invalid_composition_specific_managed_transitions_fail_closed() {
        let invalid_target = error_after(|edge, _| {
            let mut transition = edge.managed_integrations[1].clone();
            transition.target_sha256 = Some(digest('f'));
            edge.compositions[0].managed_integrations.push(transition);
        });
        assert_eq!(
            invalid_target.kind,
            UpgradeGraphDiagnosticKind::InvalidDigest
        );

        let undeclared_owner = error_after(|edge, _| {
            let transition = edge.managed_integrations.remove(1);
            edge.managed_integrations.clear();
            edge.compositions[0].target_ownership.claims.clear();
            edge.compositions[0].managed_integrations.push(transition);
        });
        assert_eq!(
            undeclared_owner.kind,
            UpgradeGraphDiagnosticKind::InvalidOwnership
        );
    }

    #[test]
    fn malformed_identifier_sizes_are_bounded_and_never_disclosed() {
        for length in [1, 64, 1_024, 4_096] {
            let sensitive = format!("{}!", "x".repeat(length));
            let diagnostic = error_after(|edge, _| edge.id = sensitive.clone());
            assert_eq!(
                diagnostic.kind,
                UpgradeGraphDiagnosticKind::InvalidIdentifier
            );
            assert!(!diagnostic.to_string().contains(&sensitive));
        }
    }

    #[test]
    fn source_package_baseline_ownership_and_file_digests_fail_closed() {
        let cases = [
            error_after(|edge, _| edge.source_package_digest = "sha256:not-a-digest".to_owned()),
            error_after(|edge, _| {
                edge.compositions[0].source_baseline_digest = "sha256:short".to_owned();
            }),
            error_after(|edge, _| {
                edge.compositions[0].source_ownership.claims.clear();
            }),
            error_after(|edge, _| {
                edge.managed_integrations[0].source_sha256 = None;
            }),
            error_after(|edge, _| {
                edge.managed_integrations[0].target_sha256 = Some(digest('f'));
            }),
            error_after(|edge, _| {
                edge.managed_integrations[0].target_sha256 =
                    edge.managed_integrations[0].source_sha256.clone();
            }),
        ];

        assert_eq!(cases[0].kind, UpgradeGraphDiagnosticKind::InvalidDigest);
        assert_eq!(cases[1].kind, UpgradeGraphDiagnosticKind::InvalidDigest);
        assert_eq!(cases[2].kind, UpgradeGraphDiagnosticKind::InvalidOwnership);
        assert_eq!(cases[3].kind, UpgradeGraphDiagnosticKind::InvalidDigest);
        assert_eq!(cases[4].kind, UpgradeGraphDiagnosticKind::InvalidDigest);
        assert_eq!(cases[5].kind, UpgradeGraphDiagnosticKind::InvalidDigest);
        assert!(cases.iter().all(|diagnostic| {
            !diagnostic.to_string().contains("not-a-digest")
                && !diagnostic.to_string().contains("sha256:short")
        }));
    }

    fn validate(
        edges: &mut [UpgradeEdgeManifest],
        components: &BTreeMap<String, ComponentManifest>,
    ) -> Result<()> {
        validate_upgrade_graph(&package(), components, &component_paths(), edges)
    }

    fn error_after(
        mutate: impl FnOnce(&mut UpgradeEdgeManifest, &mut BTreeMap<String, ComponentManifest>),
    ) -> UpgradeGraphDiagnostic {
        let mut edge = edge();
        let mut components = components();
        mutate(&mut edge, &mut components);
        error_for(vec![edge], components)
    }

    fn error_for(
        mut edges: Vec<UpgradeEdgeManifest>,
        components: BTreeMap<String, ComponentManifest>,
    ) -> UpgradeGraphDiagnostic {
        validate(&mut edges, &components)
            .expect_err("invalid graph should fail")
            .upgrade_graph_diagnostic()
            .expect("graph rejection should be typed")
            .clone()
    }

    fn resolved_snapshots(edges: &[UpgradeEdgeManifest]) -> Vec<String> {
        edges[0]
            .compositions
            .iter()
            .map(|transition| {
                let resolved = resolve_upgrade_edge(
                    edges,
                    &UpgradeEdgeRequest {
                        source: release("v0.5.0"),
                        composition: transition.source.clone(),
                    },
                )
                .unwrap();
                toml::to_string(&resolved).unwrap()
            })
            .collect()
    }

    fn deterministic_order_key(value: &[u8], seed: usize) -> u64 {
        value.iter().fold(seed as u64 + 1, |state, byte| {
            state
                .wrapping_mul(1_099_511_628_211)
                .wrapping_add(u64::from(*byte) + 1)
        })
    }

    fn package() -> ComponentPackageManifest {
        ComponentPackageManifest {
            schema: 3,
            id: application_manifest::HEGIRA_COMPONENT_PACKAGE.to_owned(),
            version: "v0.6.0".to_owned(),
            framework: release("v0.6.0").framework,
            templates: vec!["layered".to_owned()],
            components: vec![
                "base".to_owned(),
                "default".to_owned(),
                "identity".to_owned(),
                "minimal".to_owned(),
            ],
            modules: vec!["identity".to_owned()],
            upgrade_edges: vec![PathBuf::from("upgrades/v0-5-0-to-v0-6-0.toml")],
            content_digest: format!("sha256:{}", "0".repeat(64)),
        }
    }

    fn components() -> BTreeMap<String, ComponentManifest> {
        let base = component("base", &[], &[], &[]);
        let default = component(
            "default",
            &["base"],
            &["identity"],
            &[
                ApplicationCapability::Authentication,
                ApplicationCapability::Authorization,
            ],
        );
        let minimal = component("minimal", &["base"], &[], &[]);
        let mut identity = component(
            "identity",
            &["minimal"],
            &["identity"],
            &[
                ApplicationCapability::Authentication,
                ApplicationCapability::Authorization,
            ],
        );
        identity.conflicts = vec!["default".to_owned()];
        identity.installation = Some(ComponentInstallationManifest {
            module: "identity".to_owned(),
            databases: vec![DatabaseAdapter::Postgres, DatabaseAdapter::Sqlite],
            clients: vec![ClientAdapter::Leptos],
            contributions: Vec::new(),
            framework_dependencies: Vec::<FrameworkDependency>::new(),
        });
        [base, default, identity, minimal]
            .into_iter()
            .map(|component| (component.id.clone(), component))
            .collect()
    }

    fn component(
        id: &str,
        requires: &[&str],
        modules: &[&str],
        capabilities: &[ApplicationCapability],
    ) -> ComponentManifest {
        ComponentManifest {
            schema: 3,
            id: id.to_owned(),
            version: Some("v0.6.0".to_owned()),
            source: PathBuf::from("applications/layered"),
            include: vec![PathBuf::from(format!("{id}.rs"))],
            requires: requires.iter().map(|value| (*value).to_owned()).collect(),
            conflicts: Vec::new(),
            optional_dependencies: Vec::new(),
            modules: modules.iter().map(|value| (*value).to_owned()).collect(),
            provides_capabilities: capabilities.to_vec(),
            requires_capabilities: Vec::new(),
            framework_dependencies: Vec::new(),
            installation: None,
            manifest_path: PathBuf::new(),
        }
    }

    fn component_paths() -> BTreeMap<(String, String), String> {
        ["base", "default", "identity", "minimal"]
            .into_iter()
            .map(|component| {
                (
                    (component.to_owned(), format!("{component}.rs")),
                    digest('a'),
                )
            })
            .collect()
    }

    fn edge() -> UpgradeEdgeManifest {
        let mut compositions = Vec::new();
        for name in ["default", "minimal", "identity"] {
            for database in [DatabaseAdapter::Sqlite, DatabaseAdapter::Postgres] {
                let state = state(name, database);
                let mut claims = vec![managed_claim("base.rs", "application-manifest")];
                if state
                    .components
                    .iter()
                    .any(|component| component == "identity")
                {
                    claims.push(managed_claim("identity.rs", "identity-routes"));
                }
                compositions.push(UpgradeCompositionTransition {
                    id: format!("{name}-{}", database_name(database)),
                    source_baseline_digest: digest('c'),
                    source_ownership: SourceOwnership {
                        default: SourceOwnershipClass::ApplicationOwned,
                        claims: claims.clone(),
                    },
                    target_ownership: SourceOwnership {
                        default: SourceOwnershipClass::ApplicationOwned,
                        claims,
                    },
                    source: state.clone(),
                    target: state,
                    manifest_transitions: None,
                    managed_integrations: Vec::new(),
                });
            }
        }
        UpgradeEdgeManifest {
            schema: UPGRADE_EDGE_SCHEMA,
            id: "v0-5-0-to-v0-6-0".to_owned(),
            source_package_digest: digest('d'),
            source: release("v0.5.0"),
            target: release("v0.6.0"),
            compositions,
            manifest_transitions: vec![
                UpgradeManifestTransition::Ownership,
                UpgradeManifestTransition::Schema,
                UpgradeManifestTransition::Framework,
            ],
            managed_integrations: vec![
                ManagedIntegrationTransition {
                    component: "identity".to_owned(),
                    path: "identity.rs".to_owned(),
                    integration: "identity-routes".to_owned(),
                    kind: ManagedIntegrationTransitionKind::Edit,
                    source_sha256: Some(digest('b')),
                    target_source_component: None,
                    target_sha256: Some(digest('a')),
                },
                ManagedIntegrationTransition {
                    component: "base".to_owned(),
                    path: "base.rs".to_owned(),
                    integration: "application-manifest".to_owned(),
                    kind: ManagedIntegrationTransitionKind::Edit,
                    source_sha256: Some(digest('b')),
                    target_source_component: None,
                    target_sha256: Some(digest('a')),
                },
            ],
        }
    }

    fn managed_claim(path: &str, integration: &str) -> application_manifest::SourceOwnershipClaim {
        application_manifest::SourceOwnershipClaim {
            path: path.to_owned(),
            class: SourceOwnershipClass::ManagedIntegration,
            integration: Some(integration.to_owned()),
        }
    }

    fn digest(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn state(name: &str, database: DatabaseAdapter) -> UpgradeCompositionState {
        let (components, modules, capabilities) = match name {
            "default" => (
                vec!["base".to_owned(), "default".to_owned()],
                vec!["identity".to_owned()],
                vec![
                    ApplicationCapability::Authentication,
                    ApplicationCapability::Authorization,
                ],
            ),
            "minimal" => (
                vec!["base".to_owned(), "minimal".to_owned()],
                Vec::new(),
                Vec::new(),
            ),
            "identity" => (
                vec![
                    "base".to_owned(),
                    "identity".to_owned(),
                    "minimal".to_owned(),
                ],
                vec!["identity".to_owned()],
                vec![
                    ApplicationCapability::Authentication,
                    ApplicationCapability::Authorization,
                ],
            ),
            _ => panic!("unknown fixture composition"),
        };
        UpgradeCompositionState {
            database,
            client: ClientAdapter::Leptos,
            components,
            modules,
            capabilities,
        }
    }

    fn database_name(database: DatabaseAdapter) -> &'static str {
        match database {
            DatabaseAdapter::Postgres => "postgres",
            DatabaseAdapter::Sqlite => "sqlite",
        }
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
