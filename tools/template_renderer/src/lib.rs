mod destination;
mod manifest;
mod render;
pub mod repository_validation;

pub use destination::validate_destination;
pub use manifest::{
    ComponentManifest, ComponentPackageManifest, FrameworkDependency, ManifestCatalog,
    TemplateManifest,
};
pub use render::{RenderPlan, RenderRequest, RenderResult, plan, plan_snapshot, publish, render};

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
}

impl RendererError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            kind: RendererErrorKind::Rendering,
            message: message.into(),
        }
    }

    pub(crate) fn with_kind(kind: RendererErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(crate) fn classified(mut self, kind: RendererErrorKind) -> Self {
        self.kind = kind;
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
}

impl Display for RendererError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RendererError {}
