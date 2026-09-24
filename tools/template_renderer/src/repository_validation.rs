//! Repository-only rendering adapter.
//!
//! This module exists for Hegira's disposable integration checks. Applications
//! and the public CLI use the normal renderer, which preserves the
//! release-source dependencies declared by canonical components.

use std::{
    fs,
    path::{Path, PathBuf},
};

use application_manifest::ApplicationManifest;

use crate::{
    FrameworkDependency, ManifestCatalog, RenderPlan, RenderRequest, RenderResult, RendererError,
    RendererErrorKind, Result, plan as core_plan, publish, render::PlannedFile,
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
    patch_plan(request, core_plan(&request.render)?)
}

/// Stage an untouched CLI output for disposable repository validation.
/// Every source path and byte must match the requested canonical generation.
/// Declared dependencies are rewritten and an in-tree framework is excluded
/// from automatic workspace membership only in the new validation output.
pub fn stage_generated(
    request: &RepositoryValidationRequest,
    source: &Path,
) -> Result<RenderResult> {
    let mut generated = core_plan(&request.render)?;
    let mut files = std::collections::BTreeMap::new();
    read_generated_tree(source, source, &mut files)?;
    if files.len() != generated.files.len() {
        return Err(validation_error(
            "CLI output file set differs from the canonical request",
        ));
    }
    for (path, planned) in &mut generated.files {
        let bytes = files
            .remove(path)
            .ok_or_else(|| validation_error("CLI output is missing a canonical file"))?;
        if bytes != planned.bytes {
            return Err(validation_error(format!(
                "CLI output differs from the canonical request: {}",
                path.display()
            )));
        }
        planned.bytes = bytes;
    }
    let mut staged = patch_plan(request, generated)?;
    isolate_staged_framework(request, &mut staged)?;
    publish(&request.render.output, staged)
}

fn isolate_staged_framework(
    request: &RepositoryValidationRequest,
    staged: &mut RenderPlan,
) -> Result<()> {
    // In-tree path dependencies would otherwise become automatic workspace
    // members and enable official-module defaults during --workspace checks.
    if let Some(path) = &request.framework_path
        && path.is_relative()
    {
        let manifest = staged
            .files
            .get_mut(Path::new("Cargo.toml"))
            .ok_or_else(|| validation_error("generated workspace manifest is missing"))?;
        let content = std::str::from_utf8(&manifest.bytes)
            .map_err(|_| validation_error("generated workspace manifest is not UTF-8"))?;
        let parsed: toml::Value = toml::from_str(content)
            .map_err(|_| validation_error("generated workspace manifest is invalid"))?;
        if parsed
            .get("workspace")
            .and_then(|w| w.get("exclude"))
            .is_some()
            || content.matches("[workspace]\n").count() != 1
        {
            return Err(validation_error(
                "cannot safely isolate staged framework workspace",
            ));
        }
        let exclude = toml::Value::String(path.to_string_lossy().into_owned());
        manifest.bytes = content
            .replacen(
                "[workspace]\n",
                &format!("[workspace]\nexclude = [{exclude}]\n"),
                1,
            )
            .into_bytes();
    }
    Ok(())
}

/// Stage a CLI-created minimal application after the public CLI has installed
/// the bundled Identity component. The caller owns the disposable source and
/// must perform generation and installation before invoking this adapter.
pub fn stage_identity_added(
    request: &RepositoryValidationRequest,
    source: &Path,
) -> Result<RenderResult> {
    let mut generated = core_plan(&request.render)?;
    if generated.components != ["layered-base", "layered-leptos-minimal"] {
        return Err(validation_error(
            "Identity staging requires the minimal composition",
        ));
    }
    let mut files = std::collections::BTreeMap::new();
    read_generated_tree(source, source, &mut files)?;
    let manifest_bytes = files
        .get(Path::new("hegira.toml"))
        .ok_or_else(|| validation_error("installed application manifest is missing"))?;
    let manifest = ApplicationManifest::from_toml(
        std::str::from_utf8(manifest_bytes)
            .map_err(|_| validation_error("installed application manifest is not UTF-8"))?,
    )
    .map_err(|_| validation_error("installed application manifest is invalid"))?;
    let base_manifest = generated
        .files
        .get(Path::new("hegira.toml"))
        .ok_or_else(|| validation_error("minimal application manifest is missing"))?;
    let base_manifest = ApplicationManifest::from_toml(
        std::str::from_utf8(&base_manifest.bytes)
            .map_err(|_| validation_error("minimal application manifest is not UTF-8"))?,
    )
    .map_err(|_| validation_error("minimal application manifest is invalid"))?;
    if manifest.application != base_manifest.application
        || manifest.selection != base_manifest.selection
    {
        return Err(validation_error(
            "installed application changes the minimal application identity",
        ));
    }
    let catalog = ManifestCatalog::load(&request.render.repository_root, &request.render.template)
        .map_err(classify)?;
    let graph = catalog
        .resolve_component_roots(Some(&["identity".to_owned()]))
        .map_err(|_| validation_error("cannot resolve the installed Identity composition"))?;
    let mut expected_components = graph.installed_components().collect::<Vec<_>>();
    expected_components.sort_by(|left, right| left.id.cmp(&right.id));
    manifest
        .validate_rendered_components(
            graph
                .components
                .iter()
                .map(|component| component.id.as_str()),
        )
        .map_err(|_| {
            validation_error("installed component list differs from Identity composition")
        })?;
    if manifest.framework != graph.framework
        || manifest.composition.as_ref().is_none_or(|composition| {
            composition.package != graph.package
                || composition.components != expected_components
                || composition.modules != graph.modules
                || composition.capabilities != graph.capabilities
        })
    {
        return Err(validation_error(
            "installed Identity composition identity differs from package",
        ));
    }
    for (path, planned) in &mut generated.files {
        planned.bytes = files
            .remove(path)
            .ok_or_else(|| validation_error("installed application is missing a base file"))?;
    }
    for (path, bytes) in files {
        generated.files.insert(
            path,
            PlannedFile {
                bytes,
                owner: "repository-validation".to_owned(),
            },
        );
    }
    generated.components = graph
        .components
        .iter()
        .map(|component| component.id.clone())
        .collect();
    generated.composition = Some(graph);
    let mut staged = patch_plan(request, generated)?;
    isolate_staged_framework(request, &mut staged)?;
    publish(&request.render.output, staged)
}

fn read_generated_tree(
    root: &Path,
    path: &Path,
    files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| validation_error("cannot inspect CLI output"))?;
    if metadata.is_dir() {
        let mut empty = true;
        for entry in
            fs::read_dir(path).map_err(|_| validation_error("cannot read CLI output directory"))?
        {
            empty = false;
            let entry = entry.map_err(|_| validation_error("cannot read CLI output entry"))?;
            read_generated_tree(root, &entry.path(), files)?;
        }
        if empty {
            return Err(validation_error(
                "CLI output contains an unexpected empty directory",
            ));
        }
    } else if metadata.is_file() {
        files.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            fs::read(path).map_err(|_| validation_error("cannot read CLI output file"))?,
        );
    } else {
        return Err(validation_error(
            "CLI output must contain only real directories and files",
        ));
    }
    Ok(())
}

fn patch_plan(
    request: &RepositoryValidationRequest,
    mut render_plan: RenderPlan,
) -> Result<RenderPlan> {
    let catalog = ManifestCatalog::load(&request.render.repository_root, &request.render.template)
        .map_err(classify)?;
    let components = match render_plan.composition() {
        Some(composition) => catalog.components_for(composition).map_err(classify)?,
        None => catalog.resolve_components().map_err(classify)?,
    };
    let framework_root = fs::canonicalize(&request.framework_root)
        .map_err(|error| validation_error(format!("failed to resolve framework root: {error}")))?;
    if !framework_root.is_dir() {
        return Err(validation_error("framework root is not a directory"));
    }
    let framework_path = request.framework_path.as_deref().unwrap_or(&framework_root);
    validate_framework_path(framework_path)?;

    let mut dependencies = std::collections::BTreeMap::new();
    for component in components {
        let installation_dependencies = component
            .installation
            .as_ref()
            .into_iter()
            .flat_map(|installation| &installation.framework_dependencies);
        for dependency in component
            .framework_dependencies
            .iter()
            .chain(installation_dependencies)
        {
            let key = (dependency.manifest.clone(), dependency.name.clone());
            if let Some(previous) = dependencies.insert(key, dependency)
                && previous != dependency
            {
                return Err(validation_error(
                    "conflicting framework dependency declarations",
                ));
            }
        }
    }
    for dependency in dependencies.into_values() {
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

    render_plan
        .files
        .remove(Path::new("Cargo.lock"))
        .ok_or_else(|| validation_error("canonical application lockfile is missing"))?;

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
