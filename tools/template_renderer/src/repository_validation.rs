//! Repository-only rendering adapter.
//!
//! This module exists for Hegira's disposable integration checks. Applications
//! and the future public CLI must use the normal renderer, which preserves the
//! release-source dependencies declared by canonical components.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    FrameworkDependency, ManifestCatalog, RenderPlan, RenderRequest, RenderResult, RendererError,
    RendererErrorKind, Result, plan as core_plan, publish,
};

#[derive(Debug)]
pub struct RepositoryValidationRequest {
    pub render: RenderRequest,
    pub framework_root: PathBuf,
    pub framework_path: Option<PathBuf>,
}

pub fn render(request: &RepositoryValidationRequest) -> Result<RenderResult> {
    let plan = plan(request)?;
    publish(&request.render.output, plan)
}

pub fn plan(request: &RepositoryValidationRequest) -> Result<RenderPlan> {
    let mut render_plan = core_plan(&request.render)?;
    let catalog = ManifestCatalog::load(&request.render.repository_root, &request.render.template)
        .map_err(classify)?;
    let components = catalog.resolve_components().map_err(classify)?;
    let framework_root = fs::canonicalize(&request.framework_root)
        .map_err(|error| validation_error(format!("failed to resolve framework root: {error}")))?;
    if !framework_root.is_dir() {
        return Err(validation_error("framework root is not a directory"));
    }
    let framework_path = request.framework_path.as_deref().unwrap_or(&framework_root);
    validate_framework_path(framework_path)?;

    for component in components {
        for dependency in &component.framework_dependencies {
            let dependency_root =
                fs::canonicalize(framework_root.join(&dependency.path)).map_err(|error| {
                    validation_error(format!(
                        "failed to resolve framework dependency {}: {error}",
                        dependency.path.display()
                    ))
                })?;
            if !dependency_root.starts_with(&framework_root) {
                return Err(validation_error(
                    "framework dependency escapes framework root",
                ));
            }
            if !dependency_root.join("Cargo.toml").is_file() {
                return Err(validation_error(format!(
                    "framework dependency {} has no Cargo.toml",
                    dependency.path.display()
                )));
            }

            let planned = render_plan
                .files
                .get_mut(&dependency.manifest)
                .ok_or_else(|| {
                    validation_error(format!(
                        "framework dependency patch targets missing output: {}",
                        dependency.manifest.display()
                    ))
                })?;
            patch_dependency(planned, dependency, &framework_path.join(&dependency.path))?;
        }
    }

    Ok(render_plan)
}

fn validate_framework_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(validation_error("framework path may not be empty"));
    }
    if path.is_absolute() {
        return Ok(());
    }
    if path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(validation_error(
            "relative framework path contains an unsafe component",
        ));
    }
    Ok(())
}

fn patch_dependency(
    planned: &mut crate::render::PlannedFile,
    dependency: &FrameworkDependency,
    dependency_root: &Path,
) -> Result<()> {
    let content = std::str::from_utf8(&planned.bytes).map_err(|_| {
        validation_error(format!(
            "framework dependency manifest is not UTF-8: {}",
            dependency.manifest.display()
        ))
    })?;
    let prefix = format!("{} = {{ git = ", dependency.name);
    let mut matches = 0;
    let mut lines = Vec::new();
    let quoted_path =
        toml::Value::String(dependency_root.to_string_lossy().into_owned()).to_string();

    for line in content.lines() {
        if line.trim_start().starts_with(&prefix) {
            matches += 1;
            let indentation = &line[..line.len() - line.trim_start().len()];
            let default_features = dependency
                .default_features
                .map(|enabled| format!(", default-features = {enabled}"))
                .unwrap_or_default();
            lines.push(format!(
                "{indentation}{} = {{ path = {quoted_path}{default_features} }}",
                dependency.name
            ));
        } else {
            lines.push(line.to_string());
        }
    }

    if matches != 1 {
        return Err(validation_error(format!(
            "expected one git dependency named {} in {}; found {matches}",
            dependency.name,
            dependency.manifest.display()
        )));
    }
    let mut patched = lines.join("\n");
    if content.ends_with('\n') {
        patched.push('\n');
    }
    planned.bytes = patched.into_bytes();
    Ok(())
}

fn validation_error(message: impl Into<String>) -> RendererError {
    RendererError::with_kind(RendererErrorKind::RepositoryValidation, message)
}

fn classify(error: RendererError) -> RendererError {
    error.classified(RendererErrorKind::RepositoryValidation)
}
