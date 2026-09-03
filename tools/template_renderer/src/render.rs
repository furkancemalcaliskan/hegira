use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
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
    let plan = plan(request)?;
    publish(&request.output, plan)
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
    publish_atomically(output, &plan)
        .map_err(|error| error.classified(RendererErrorKind::Output))?;
    Ok(RenderResult {
        output: absolute_path(output)
            .map_err(|error| error.classified(RendererErrorKind::Output))?,
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
