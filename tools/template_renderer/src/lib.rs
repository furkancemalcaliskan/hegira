mod composition;
mod destination;
mod manifest;
mod package_source;
mod render;
pub mod repository_validation;
mod upgrade;
mod upgrade_authentication;

pub use destination::validate_destination;
pub use manifest::{
    ComponentInstallationContribution, ComponentInstallationManifest, ComponentManifest,
    ComponentPackageManifest, FrameworkDependency, ManifestCatalog, TemplateManifest,
};
pub use render::{RenderPlan, RenderRequest, RenderResult, plan, plan_snapshot, publish, render};
pub use upgrade::{
    ManagedIntegrationTransition, ManagedIntegrationTransitionKind, ResolvedUpgradeEdge,
    UPGRADE_EDGE_SCHEMA, UpgradeCompositionState, UpgradeCompositionTransition,
    UpgradeEdgeDiagnostic, UpgradeEdgeDiagnosticKind, UpgradeEdgeError, UpgradeEdgeManifest,
    UpgradeEdgeRequest, UpgradeGraphDiagnostic, UpgradeGraphDiagnosticKind,
    UpgradeManifestTransition, UpgradeReleaseIdentity,
};
pub use upgrade_authentication::{
    AuthenticatedManagedSource, AuthenticatedUpgradeBoundary, UPGRADE_AUTHENTICATION_SCHEMA,
    UpgradeAuthenticationDiagnostic, UpgradeAuthenticationDiagnosticKind,
    UpgradeAuthenticationError,
};

use std::fmt::{Display, Formatter};
use std::path::Path;

pub type Result<T> = std::result::Result<T, RendererError>;

pub fn validate_project_identity(name: &str) -> Result<()> {
    application_manifest::validate_application_name(name).map_err(|_| RendererError::with_kind(
        RendererErrorKind::Variables,
        "application identity must be 1–64 lowercase ASCII letters, digits and single internal hyphens, start with a letter, and not be a reserved Rust/Cargo/device name",
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererErrorKind {
    Catalog,
    ComponentResolution,
    Variables,
    Collision,
    Safety,
    Rendering,
    ApplicationManifest,
    Output,
    Conflict,
    RepositoryValidation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererError {
    kind: RendererErrorKind,
    message: String,
    upgrade_graph_diagnostic: Option<UpgradeGraphDiagnostic>,
}

impl RendererError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            kind: RendererErrorKind::Rendering,
            message: message.into(),
            upgrade_graph_diagnostic: None,
        }
    }

    pub(crate) fn with_kind(kind: RendererErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            upgrade_graph_diagnostic: None,
        }
    }

    pub(crate) fn with_upgrade_graph(diagnostic: UpgradeGraphDiagnostic) -> Self {
        Self {
            kind: RendererErrorKind::Rendering,
            message: diagnostic.to_string(),
            upgrade_graph_diagnostic: Some(diagnostic),
        }
    }

    pub(crate) fn classified(mut self, kind: RendererErrorKind) -> Self {
        if self.kind == RendererErrorKind::Rendering {
            self.kind = kind;
        }
        self
    }

    pub(crate) fn redacted_path(mut self, path: &Path, replacement: &str) -> Self {
        if path.is_absolute() {
            let path = path.to_string_lossy();
            if !path.is_empty() {
                self.message = self.message.replace(path.as_ref(), replacement);
            }
        }
        self
    }

    pub fn kind(&self) -> RendererErrorKind {
        self.kind
    }

    pub fn upgrade_graph_diagnostic(&self) -> Option<&UpgradeGraphDiagnostic> {
        self.upgrade_graph_diagnostic.as_ref()
    }
}

impl Display for RendererError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RendererError {}
pub use composition::{
    COMPOSITION_GRAPH_SCHEMA, CompositionDiagnostic, CompositionDiagnosticKind, CompositionError,
    CompositionRequest, ResolvedComponent, ResolvedComposition,
};
