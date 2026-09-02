use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use application_manifest::ApplicationManifest;

use crate::{
    ComponentManifest, ManifestCatalog, RendererError, Result,
    manifest::{ensure_inside, validate_variable},
};

#[derive(Debug)]
pub struct RenderRequest {
    pub repository_root: PathBuf,
    pub template: String,
    pub output: PathBuf,
    pub variables: BTreeMap<String, String>,
    pub framework_root: Option<PathBuf>,
    pub framework_path: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RenderResult {
    pub output: PathBuf,
    pub components: Vec<String>,
    pub files: Vec<PathBuf>,
}

#[derive(Debug)]
struct RenderPlan {
    components: Vec<String>,
    files: BTreeMap<PathBuf, PlannedFile>,
}

#[derive(Debug)]
struct PlannedFile {
    bytes: Vec<u8>,
    owner: String,
}

pub fn render(request: &RenderRequest) -> Result<RenderResult> {
    let plan = build_plan(request)?;
    publish_atomically(&request.output, &plan)?;
    Ok(RenderResult {
        output: absolute_path(&request.output)?,
        components: plan.components,
        files: plan.files.into_keys().collect(),
    })
}

pub fn plan_snapshot(request: &RenderRequest) -> Result<String> {
    let plan = build_plan(request)?;
    let mut snapshot = format!("components={}\n", plan.components.join(","));
    for (path, file) in plan.files {
        snapshot.push_str(&format!(
            "{:016x} {}\n",
            fnv1a64(&file.bytes),
            path.display()
        ));
    }
    Ok(snapshot)
}

fn build_plan(request: &RenderRequest) -> Result<RenderPlan> {
    let catalog = ManifestCatalog::load(&request.repository_root, &request.template)?;
    let components = catalog.resolve_components()?;
    let variables = resolve_variables(&catalog, &request.variables)?;
    let mut files = BTreeMap::new();

    for component in &components {
        collect_component_files(component, catalog.templates_root(), &variables, &mut files)?;
    }

    validate_application_manifest(&components, &files)?;

    reject_repository_path_leaks(catalog.repository_root(), &files)?;
    if let Some(framework_root) = &request.framework_root {
        let framework_path = request.framework_path.as_deref().unwrap_or(framework_root);
        apply_framework_patches(framework_root, framework_path, &components, &mut files)?;
    } else if request.framework_path.is_some() {
        return Err(RendererError::new("framework_path requires framework_root"));
    }

    Ok(RenderPlan {
        components: components
            .into_iter()
            .map(|component| component.id.clone())
            .collect(),
        files,
    })
}

fn validate_application_manifest(
    components: &[&ComponentManifest],
    files: &BTreeMap<PathBuf, PlannedFile>,
) -> Result<()> {
    let Some(planned) = files.get(Path::new("hegira.toml")) else {
        return Ok(());
    };
    let source = std::str::from_utf8(&planned.bytes)
        .map_err(|_| RendererError::new("generated application manifest is not UTF-8"))?;
    let manifest = ApplicationManifest::from_toml(source).map_err(|error| {
        RendererError::new(format!("invalid generated application manifest: {error}"))
    })?;
    manifest
        .validate_rendered_components(components.iter().map(|component| component.id.as_str()))
        .map_err(|error| {
            RendererError::new(format!("invalid generated application manifest: {error}"))
        })
}

fn resolve_variables(
    catalog: &ManifestCatalog,
    overrides: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut variables = catalog.template().variables.clone();
    for (name, value) in overrides {
        validate_variable(name)?;
        if !variables.contains_key(name) {
            return Err(RendererError::new(format!(
                "template variable override is not declared: {name}"
            )));
        }
        variables.insert(name.clone(), value.clone());
    }
    Ok(variables)
}

fn collect_component_files(
    component: &ComponentManifest,
    templates_root: &Path,
    variables: &BTreeMap<String, String>,
    files: &mut BTreeMap<PathBuf, PlannedFile>,
) -> Result<()> {
    let source_root = component.source_root(templates_root)?;
    let mut includes = component.include.clone();
    includes.sort();

    for include in includes {
        let candidate = source_root.join(&include);
        let metadata = fs::symlink_metadata(&candidate).map_err(|error| {
            RendererError::new(format!(
                "failed to inspect component input {}: {error}",
                candidate.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(RendererError::new(format!(
                "component input may not be a symbolic link: {}",
                candidate.display()
            )));
        }
        if metadata.is_dir() {
            collect_directory(component, &source_root, &candidate, variables, files)?;
        } else if metadata.is_file() {
            collect_file(component, &source_root, &candidate, variables, files)?;
        } else {
            return Err(RendererError::new(format!(
                "component input is not a regular file or directory: {}",
                candidate.display()
            )));
        }
    }
    Ok(())
}

fn collect_directory(
    component: &ComponentManifest,
    source_root: &Path,
    directory: &Path,
    variables: &BTreeMap<String, String>,
    files: &mut BTreeMap<PathBuf, PlannedFile>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| {
            RendererError::new(format!(
                "failed to read component directory {}: {error}",
                directory.display()
            ))
        })?
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|error| {
                RendererError::new(format!(
                    "failed to read entry in {}: {error}",
                    directory.display()
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    entries.sort();

    for entry in entries {
        let metadata = fs::symlink_metadata(&entry).map_err(|error| {
            RendererError::new(format!(
                "failed to inspect component input {}: {error}",
                entry.display()
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(RendererError::new(format!(
                "component input may not be a symbolic link: {}",
                entry.display()
            )));
        }
        if metadata.is_dir() {
            collect_directory(component, source_root, &entry, variables, files)?;
        } else if metadata.is_file() {
            collect_file(component, source_root, &entry, variables, files)?;
        } else {
            return Err(RendererError::new(format!(
                "component input is not a regular file or directory: {}",
                entry.display()
            )));
        }
    }
    Ok(())
}

fn collect_file(
    component: &ComponentManifest,
    source_root: &Path,
    source: &Path,
    variables: &BTreeMap<String, String>,
    files: &mut BTreeMap<PathBuf, PlannedFile>,
) -> Result<()> {
    let output = source
        .strip_prefix(source_root)
        .map_err(|_| {
            RendererError::new(format!(
                "component input escapes source root: {}",
                source.display()
            ))
        })?
        .to_path_buf();
    let bytes = fs::read(source).map_err(|error| {
        RendererError::new(format!(
            "failed to read component input {}: {error}",
            source.display()
        ))
    })?;
    let bytes = substitute_variables(&output, bytes, variables)?;
    if let Some(existing) = files.insert(
        output.clone(),
        PlannedFile {
            bytes,
            owner: component.id.clone(),
        },
    ) {
        return Err(RendererError::new(format!(
            "output collision at {} between components {} and {}",
            output.display(),
            existing.owner,
            component.id
        )));
    }
    Ok(())
}

fn substitute_variables(
    path: &Path,
    bytes: Vec<u8>,
    variables: &BTreeMap<String, String>,
) -> Result<Vec<u8>> {
    let source = match String::from_utf8(bytes) {
        Ok(source) => source,
        Err(error) => return Ok(error.into_bytes()),
    };
    if !source.contains("{{") && !source.contains("}}") {
        return Ok(source.into_bytes());
    }

    let mut output = String::with_capacity(source.len());
    let mut remaining = source.as_str();
    while let Some(start) = remaining.find("{{") {
        let prefix = &remaining[..start];
        if prefix.contains("}}") {
            return Err(RendererError::new(format!(
                "unmatched template token in {}",
                path.display()
            )));
        }
        output.push_str(prefix);
        let token = &remaining[start + 2..];
        let end = token.find("}}").ok_or_else(|| {
            RendererError::new(format!("unmatched template token in {}", path.display()))
        })?;
        let name = &token[..end];
        validate_variable(name)?;
        let value = variables.get(name).ok_or_else(|| {
            RendererError::new(format!(
                "missing template variable {name} required by {}",
                path.display()
            ))
        })?;
        output.push_str(value);
        remaining = &token[end + 2..];
    }
    if remaining.contains("}}") {
        return Err(RendererError::new(format!(
            "unmatched template token in {}",
            path.display()
        )));
    }
    output.push_str(remaining);
    Ok(output.into_bytes())
}

fn reject_repository_path_leaks(
    repository_root: &Path,
    files: &BTreeMap<PathBuf, PlannedFile>,
) -> Result<()> {
    let repository_root = repository_root.to_string_lossy();
    for (path, file) in files {
        if let Ok(content) = std::str::from_utf8(&file.bytes)
            && content.contains(repository_root.as_ref())
        {
            return Err(RendererError::new(format!(
                "rendered output contains the repository-local path in {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn apply_framework_patches(
    framework_root: &Path,
    framework_path: &Path,
    components: &[&ComponentManifest],
    files: &mut BTreeMap<PathBuf, PlannedFile>,
) -> Result<()> {
    let framework_root = fs::canonicalize(framework_root).map_err(|error| {
        RendererError::new(format!(
            "failed to resolve framework root {}: {error}",
            framework_root.display()
        ))
    })?;
    if !framework_root.is_dir() {
        return Err(RendererError::new(format!(
            "framework root is not a directory: {}",
            framework_root.display()
        )));
    }
    validate_framework_path(framework_path)?;

    for component in components {
        for dependency in &component.framework_dependencies {
            let dependency_root =
                fs::canonicalize(framework_root.join(&dependency.path)).map_err(|error| {
                    RendererError::new(format!(
                        "failed to resolve framework dependency {}: {error}",
                        dependency.path.display()
                    ))
                })?;
            ensure_inside(&framework_root, &dependency_root, "framework dependency")?;
            if !dependency_root.join("Cargo.toml").is_file() {
                return Err(RendererError::new(format!(
                    "framework dependency has no Cargo.toml: {}",
                    dependency_root.display()
                )));
            }

            let planned = files.get_mut(&dependency.manifest).ok_or_else(|| {
                RendererError::new(format!(
                    "framework dependency patch targets missing output: {}",
                    dependency.manifest.display()
                ))
            })?;
            patch_dependency(planned, dependency, &framework_path.join(&dependency.path))?;
        }
    }
    Ok(())
}

fn validate_framework_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(RendererError::new("framework_path may not be empty"));
    }
    if path.is_absolute() {
        return Ok(());
    }
    for component in path.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(RendererError::new(format!(
                "relative framework_path contains an unsafe component: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn patch_dependency(
    planned: &mut PlannedFile,
    dependency: &crate::FrameworkDependency,
    dependency_root: &Path,
) -> Result<()> {
    let content = std::str::from_utf8(&planned.bytes).map_err(|_| {
        RendererError::new(format!(
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
        return Err(RendererError::new(format!(
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

fn publish_atomically(output: &Path, plan: &RenderPlan) -> Result<()> {
    let output = absolute_path(output)?;
    if fs::symlink_metadata(&output).is_ok() {
        return Err(RendererError::new(format!(
            "render output already exists: {}",
            output.display()
        )));
    }
    let parent = output.parent().ok_or_else(|| {
        RendererError::new(format!("render output has no parent: {}", output.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        RendererError::new(format!(
            "failed to create render output parent {}: {error}",
            parent.display()
        ))
    })?;

    let temporary = create_temporary_directory(parent, &output)?;
    let mut guard = TemporaryDirectory::new(temporary.clone());
    for (path, file) in &plan.files {
        let target = temporary.join(path);
        if let Some(directory) = target.parent() {
            fs::create_dir_all(directory).map_err(|error| {
                RendererError::new(format!(
                    "failed to create rendered directory {}: {error}",
                    directory.display()
                ))
            })?;
        }
        fs::write(&target, &file.bytes).map_err(|error| {
            RendererError::new(format!(
                "failed to write rendered file {}: {error}",
                target.display()
            ))
        })?;
    }

    fs::rename(&temporary, &output).map_err(|error| {
        RendererError::new(format!(
            "failed to publish rendered output {}: {error}",
            output.display()
        ))
    })?;
    guard.disarm();
    Ok(())
}

fn create_temporary_directory(parent: &Path, output: &Path) -> Result<PathBuf> {
    let output_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("application");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RendererError::new(format!("system clock error: {error}")))?
        .as_nanos();

    for attempt in 0..32_u8 {
        let candidate = parent.join(format!(
            ".{output_name}.hegira-render-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(RendererError::new(format!(
                    "failed to create render staging directory {}: {error}",
                    candidate.display()
                )));
            }
        }
    }
    Err(RendererError::new(
        "failed to allocate a unique render staging directory",
    ))
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|directory| directory.join(path))
        .map_err(|error| {
            RendererError::new(format!("failed to resolve current directory: {error}"))
        })
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

struct TemporaryDirectory {
    path: PathBuf,
    armed: bool,
}

impl TemporaryDirectory {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
