mod manifest;
mod render;

pub use manifest::{ComponentManifest, FrameworkDependency, ManifestCatalog, TemplateManifest};
pub use render::{RenderRequest, RenderResult, plan_snapshot, render};

use std::fmt::{Display, Formatter};

pub type Result<T> = std::result::Result<T, RendererError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererError(String);

impl RendererError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for RendererError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RendererError {}
