use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use application_manifest::ApplicationManifest;

use crate::{
    ComponentManifest, ComponentPackageManifest, ManifestCatalog, RendererError, RendererErrorKind,
    Result, manifest::validate_variable,
};

#[derive(Debug)]
pub struct RenderRequest {
    pub repository_root: PathBuf,
    pub template: String,
    pub output: PathBuf,
    pub variables: BTreeMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RenderResult {
    pub output: PathBuf,
    pub package: Option<ComponentPackageManifest>,
    pub components: Vec<String>,
    pub files: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct RenderPlan {
    pub(crate) package: Option<ComponentPackageManifest>,
    pub(crate) components: Vec<String>,
    pub(crate) files: BTreeMap<PathBuf, PlannedFile>,
}

#[derive(Debug)]
pub(crate) struct PlannedFile {
    pub(crate) bytes: Vec<u8>,
    pub(crate) owner: String,
}

pub fn render(request: &RenderRequest) -> Result<RenderResult> {
    let output = crate::destination::checked_path(&request.output)?;
    crate::validate_destination(&output)?;
    let plan = plan(request)?;
    publish(&output, plan)
}

pub fn plan_snapshot(request: &RenderRequest) -> Result<String> {
    let plan = plan(request)?;
    let mut snapshot = String::new();
    if let Some(package) = &plan.package {
        snapshot.push_str(&format!(
            "package={}@{}\nframework={}#{}\ndigest={}\n",
            package.id,
            package.version,
            package.framework.repository,
            package.framework.version,
            package.content_digest
        ));
    }
    snapshot.push_str(&format!("components={}\n", plan.components.join(",")));
    for (path, file) in plan.files {
        snapshot.push_str(&format!(
            "{:016x} {}\n",
            fnv1a64(&file.bytes),
            path.display()
        ));
    }
    Ok(snapshot)
}

pub fn plan(request: &RenderRequest) -> Result<RenderPlan> {
    let sensitive_root = fs::canonicalize(&request.repository_root)
        .unwrap_or_else(|_| request.repository_root.clone());
    build_plan(request).map_err(|error| error.redacted_path(&sensitive_root, "<repository-root>"))
}

fn build_plan(request: &RenderRequest) -> Result<RenderPlan> {
    let catalog = ManifestCatalog::load(&request.repository_root, &request.template)
        .map_err(|error| error.classified(RendererErrorKind::Catalog))?;
    let components = catalog
        .resolve_components()
        .map_err(|error| error.classified(RendererErrorKind::ComponentResolution))?;
    let variables = resolve_variables(&catalog, &request.variables)?;
    let mut files = BTreeMap::new();

    for component in &components {
        collect_component_files(component, catalog.templates_root(), &variables, &mut files)?;
    }

    validate_application_manifest(&components, &files)?;

    reject_repository_path_leaks(catalog.repository_root(), &files)?;
    Ok(RenderPlan {
        package: catalog.package().cloned(),
        components: components
            .into_iter()
            .map(|component| component.id.clone())
            .collect(),
        files,
    })
}

impl RenderPlan {
    pub fn package(&self) -> Option<&ComponentPackageManifest> {
        self.package.as_ref()
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn files(&self) -> impl ExactSizeIterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }
}

pub fn publish(output: &Path, plan: RenderPlan) -> Result<RenderResult> {
    let output = crate::destination::checked_path(output)?;
    crate::destination::platform::publish(&output, &plan)?;
    Ok(RenderResult {
        output,
        package: plan.package,
        components: plan.components,
        files: plan.files.into_keys().collect(),
    })
}

fn validate_application_manifest(
    components: &[&ComponentManifest],
    files: &BTreeMap<PathBuf, PlannedFile>,
) -> Result<()> {
    let Some(planned) = files.get(Path::new("hegira.toml")) else {
        return Ok(());
    };
    let source = std::str::from_utf8(&planned.bytes).map_err(|_| {
        RendererError::with_kind(
            RendererErrorKind::ApplicationManifest,
            "generated application manifest is not UTF-8",
        )
    })?;
    let manifest = ApplicationManifest::from_toml(source).map_err(|error| {
        RendererError::with_kind(
            RendererErrorKind::ApplicationManifest,
            format!("invalid generated application manifest: {error}"),
        )
    })?;
    manifest
        .validate_rendered_components(components.iter().map(|component| component.id.as_str()))
        .map_err(|error| {
            RendererError::with_kind(
                RendererErrorKind::ApplicationManifest,
                format!("invalid generated application manifest: {error}"),
            )
        })
}

fn resolve_variables(
    catalog: &ManifestCatalog,
    overrides: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut variables = catalog.template().variables.clone();
    for (name, value) in overrides {
        validate_variable(name).map_err(|error| error.classified(RendererErrorKind::Variables))?;
        if !variables.contains_key(name) {
            return Err(RendererError::with_kind(
                RendererErrorKind::Variables,
                format!("template variable override is not declared: {name}"),
            ));
        }
        variables.insert(name.clone(), value.clone());
    }
    if let Some(package) = catalog.package() {
        for (name, value) in [
            ("framework_repository", &package.framework.repository),
            ("framework_version", &package.framework.version),
        ] {
            if variables.insert(name.to_string(), value.clone()).is_some() {
                return Err(RendererError::with_kind(
                    RendererErrorKind::Catalog,
                    format!("reserved package variable is declared by the template: {name}"),
                ));
            }
        }
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
        return Err(RendererError::with_kind(
            RendererErrorKind::Collision,
            format!(
                "output collision at {} between components {} and {}",
                output.display(),
                existing.owner,
                component.id
            ),
        ));
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
            return Err(RendererError::with_kind(
                RendererErrorKind::Variables,
                format!("unmatched template token in {}", path.display()),
            ));
        }
        output.push_str(prefix);
        let token = &remaining[start + 2..];
        let end = token.find("}}").ok_or_else(|| {
            RendererError::with_kind(
                RendererErrorKind::Variables,
                format!("unmatched template token in {}", path.display()),
            )
        })?;
        let name = &token[..end];
        validate_variable(name).map_err(|error| error.classified(RendererErrorKind::Variables))?;
        let value = variables.get(name).ok_or_else(|| {
            RendererError::with_kind(
                RendererErrorKind::Variables,
                format!(
                    "missing template variable {name} required by {}",
                    path.display()
                ),
            )
        })?;
        output.push_str(value);
        remaining = &token[end + 2..];
    }
    if remaining.contains("}}") {
        return Err(RendererError::with_kind(
            RendererErrorKind::Variables,
            format!("unmatched template token in {}", path.display()),
        ));
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
            return Err(RendererError::with_kind(
                RendererErrorKind::Safety,
                format!(
                    "rendered output contains the repository-local path in {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
