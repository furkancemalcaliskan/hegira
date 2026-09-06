use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use application_manifest::FrameworkContract;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{RendererError, Result};

const MANIFEST_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateManifest {
    pub schema: u32,
    pub id: String,
    pub components: Vec<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentPackageManifest {
    pub schema: u32,
    pub id: String,
    pub version: String,
    pub framework: FrameworkContract,
    pub templates: Vec<String>,
    pub components: Vec<String>,
    pub content_digest: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentManifest {
    pub schema: u32,
    pub id: String,
    pub source: PathBuf,
    pub include: Vec<PathBuf>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub framework_dependencies: Vec<FrameworkDependency>,
    #[serde(skip)]
    pub(crate) manifest_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkDependency {
    pub manifest: PathBuf,
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub default_features: Option<bool>,
}

#[derive(Debug)]
pub struct ManifestCatalog {
    repository_root: PathBuf,
    templates_root: PathBuf,
    template: TemplateManifest,
    components: BTreeMap<String, ComponentManifest>,
    package: Option<ComponentPackageManifest>,
}

impl ManifestCatalog {
    pub fn load(repository_root: impl AsRef<Path>, template_id: &str) -> Result<Self> {
        Self::load_internal(repository_root.as_ref(), template_id, true)
    }

    pub fn calculate_package_digest(
        repository_root: impl AsRef<Path>,
        template_id: &str,
    ) -> Result<String> {
        let catalog = Self::load_internal(repository_root.as_ref(), template_id, false)?;
        catalog.calculate_package_content_digest()?.ok_or_else(|| {
            RendererError::new("templates root does not contain a component package manifest")
        })
    }

    fn load_internal(
        repository_root: &Path,
        template_id: &str,
        validate_content_digest: bool,
    ) -> Result<Self> {
        validate_identifier(template_id, "template")?;

        let repository_root = canonical_directory(repository_root, "repository root")?;
        let templates_root =
            canonical_directory(&repository_root.join("templates"), "templates root")?;
        let package_path = templates_root.join("package.toml");
        let package = if package_path.is_file() {
            let package: ComponentPackageManifest =
                read_manifest(&package_path, "component package")?;
            validate_package(&package, &package_path)?;
            if !package
                .templates
                .iter()
                .any(|template| template == template_id)
            {
                return Err(RendererError::new(format!(
                    "component package does not contain template {template_id}"
                )));
            }
            Some(package)
        } else {
            None
        };
        let template_path = templates_root
            .join("applications")
            .join(template_id)
            .join("template.toml");
        let template: TemplateManifest = read_manifest(&template_path, "template")?;
        validate_schema(template.schema, &template_path)?;
        validate_identifier(&template.id, "template")?;
        if template.id != template_id {
            return Err(RendererError::new(format!(
                "template id mismatch in {}: expected {template_id}, found {}",
                template_path.display(),
                template.id
            )));
        }
        if template.components.is_empty() {
            return Err(RendererError::new(format!(
                "template {} selects no components",
                template.id
            )));
        }
        for component in &template.components {
            validate_identifier(component, "component")?;
        }
        for variable in template.variables.keys() {
            validate_variable(variable)?;
        }

        let components_directory = templates_root.join("components");
        let mut component_paths = fs::read_dir(&components_directory)
            .map_err(|error| {
                RendererError::new(format!(
                    "failed to read component manifests from {}: {error}",
                    components_directory.display()
                ))
            })?
            .map(|entry| {
                entry.map(|entry| entry.path()).map_err(|error| {
                    RendererError::new(format!("failed to read component manifest entry: {error}"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        component_paths.sort();

        let mut components = BTreeMap::new();
        for component_path in component_paths {
            let metadata = fs::symlink_metadata(&component_path).map_err(|error| {
                RendererError::new(format!(
                    "failed to inspect component manifest {}: {error}",
                    component_path.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(RendererError::new(format!(
                    "component manifest may not be a symbolic link: {}",
                    component_path.display()
                )));
            }
            if !metadata.is_file()
                || component_path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    != Some("toml")
            {
                continue;
            }

            let mut component: ComponentManifest = read_manifest(&component_path, "component")?;
            validate_schema(component.schema, &component_path)?;
            validate_component(&component)?;
            component.manifest_path = component_path.clone();
            if components.insert(component.id.clone(), component).is_some() {
                return Err(RendererError::new(format!(
                    "duplicate component id in {}",
                    component_path.display()
                )));
            }
        }

        if let Some(package) = &package {
            let packaged = package.components.iter().cloned().collect::<BTreeSet<_>>();
            let discovered = components.keys().cloned().collect::<BTreeSet<_>>();
            if packaged != discovered {
                return Err(RendererError::new(format!(
                    "component package declares {packaged:?}, but contains {discovered:?}"
                )));
            }
        }

        let catalog = Self {
            repository_root,
            templates_root,
            template,
            components,
            package,
        };
        if validate_content_digest {
            catalog.validate_package_content()?;
        }
        Ok(catalog)
    }

    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    pub fn templates_root(&self) -> &Path {
        &self.templates_root
    }

    pub fn template(&self) -> &TemplateManifest {
        &self.template
    }

    pub fn package(&self) -> Option<&ComponentPackageManifest> {
        self.package.as_ref()
    }

    fn validate_package_content(&self) -> Result<()> {
        let Some(package) = &self.package else {
            return Ok(());
        };
        let actual = self
            .calculate_package_content_digest()?
            .expect("package presence was checked");
        if actual != package.content_digest {
            return Err(RendererError::new(format!(
                "component package content digest mismatch: expected {}, calculated {actual}",
                package.content_digest
            )));
        }
        Ok(())
    }

    fn calculate_package_content_digest(&self) -> Result<Option<String>> {
        if self.package.is_none() {
            return Ok(None);
        }
        let mut entries = BTreeMap::new();
        let template_path = self
            .templates_root
            .join("applications")
            .join(&self.template.id)
            .join("template.toml");
        insert_package_entry(
            &mut entries,
            format!("applications/{}/template.toml", self.template.id),
            fs::read(&template_path).map_err(|error| {
                RendererError::new(format!(
                    "failed to read packaged template manifest: {error}"
                ))
            })?,
        )?;

        for component in self.components.values() {
            insert_package_entry(
                &mut entries,
                format!("components/{}.toml", component.id),
                fs::read(&component.manifest_path).map_err(|error| {
                    RendererError::new(format!(
                        "failed to read packaged component manifest: {error}"
                    ))
                })?,
            )?;
            let source_root = component.source_root(&self.templates_root)?;
            let mut includes = component.include.clone();
            includes.sort();
            for include in includes {
                collect_package_entries(
                    component,
                    &source_root,
                    &source_root.join(include),
                    &mut entries,
                )?;
            }
        }

        Ok(Some(content_digest(&entries)))
    }

    pub fn resolve_components(&self) -> Result<Vec<&ComponentManifest>> {
        let mut selected = BTreeSet::new();
        let mut temporary = BTreeSet::new();
        let mut resolved = Vec::new();
        let mut roots = self.template.components.clone();
        roots.sort();

        for component in roots {
            self.visit_component(&component, &mut temporary, &mut selected, &mut resolved)?;
        }

        for component in &resolved {
            let mut conflicts = component.conflicts.clone();
            conflicts.sort();
            for conflict in conflicts {
                if selected.contains(&conflict) {
                    return Err(RendererError::new(format!(
                        "component {} conflicts with selected component {conflict}",
                        component.id
                    )));
                }
            }
        }

        Ok(resolved)
    }

    fn visit_component<'a>(
        &'a self,
        id: &str,
        temporary: &mut BTreeSet<String>,
        selected: &mut BTreeSet<String>,
        resolved: &mut Vec<&'a ComponentManifest>,
    ) -> Result<()> {
        if selected.contains(id) {
            return Ok(());
        }
        if !temporary.insert(id.to_string()) {
            return Err(RendererError::new(format!(
                "component requirement cycle includes {id}"
            )));
        }

        let component = self.components.get(id).ok_or_else(|| {
            RendererError::new(format!("selected component does not exist: {id}"))
        })?;
        let mut requirements = component.requires.clone();
        requirements.sort();
        for requirement in requirements {
            self.visit_component(&requirement, temporary, selected, resolved)?;
        }

        temporary.remove(id);
        selected.insert(id.to_string());
        resolved.push(component);
        Ok(())
    }
}

fn collect_package_entries(
    component: &ComponentManifest,
    source_root: &Path,
    candidate: &Path,
    entries: &mut BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(candidate).map_err(|error| {
        RendererError::new(format!(
            "failed to inspect packaged component input: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(RendererError::new(
            "packaged component input may not be a symbolic link",
        ));
    }
    if metadata.is_dir() {
        let mut children = fs::read_dir(candidate)
            .map_err(|error| {
                RendererError::new(format!(
                    "failed to read packaged component directory: {error}"
                ))
            })?
            .map(|entry| {
                entry.map(|entry| entry.path()).map_err(|error| {
                    RendererError::new(format!("failed to read packaged component entry: {error}"))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            collect_package_entries(component, source_root, &child, entries)?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(RendererError::new(
            "packaged component input is not a regular file or directory",
        ));
    }
    let relative = candidate
        .strip_prefix(source_root)
        .map_err(|_| RendererError::new("packaged component input escapes its source root"))?;
    let relative = relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => Ok(value.to_string_lossy()),
            _ => Err(RendererError::new(
                "packaged component input contains an invalid path component",
            )),
        })
        .collect::<Result<Vec<_>>>()?
        .join("/");
    let package_path = format!("sources/{}/{relative}", component.id);
    let bytes = fs::read(candidate).map_err(|error| {
        RendererError::new(format!("failed to read packaged component input: {error}"))
    })?;
    insert_package_entry(entries, package_path, bytes)
}

fn insert_package_entry(
    entries: &mut BTreeMap<String, Vec<u8>>,
    path: String,
    bytes: Vec<u8>,
) -> Result<()> {
    if entries.insert(path.clone(), bytes).is_some() {
        return Err(RendererError::new(format!(
            "duplicate component package entry: {}",
            path
        )));
    }
    Ok(())
}

fn content_digest(entries: &BTreeMap<String, Vec<u8>>) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in entries {
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn validate_package(package: &ComponentPackageManifest, path: &Path) -> Result<()> {
    validate_schema(package.schema, path)?;
    validate_identifier(&package.id, "component package")?;
    package.framework.validate().map_err(|error| {
        RendererError::new(format!("invalid component package framework: {error}"))
    })?;
    if package.version != package.framework.version {
        return Err(RendererError::new(format!(
            "component package version {} does not match framework version {}",
            package.version, package.framework.version
        )));
    }
    validate_sorted_identifiers(&package.templates, "package template")?;
    validate_sorted_identifiers(&package.components, "package component")?;
    let digest = package
        .content_digest
        .strip_prefix("sha256:")
        .unwrap_or_default();
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(RendererError::new(
            "component package content digest must be lowercase sha256",
        ));
    }
    Ok(())
}

fn validate_sorted_identifiers(values: &[String], kind: &str) -> Result<()> {
    if values.is_empty() {
        return Err(RendererError::new(format!(
            "component package declares no {kind}s"
        )));
    }
    for value in values {
        validate_identifier(value, kind)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(RendererError::new(format!(
            "component package {kind}s must be sorted and unique"
        )));
    }
    Ok(())
}

impl ComponentManifest {
    pub(crate) fn source_root(&self, templates_root: &Path) -> Result<PathBuf> {
        let source = canonical_directory(&templates_root.join(&self.source), "component source")?;
        ensure_inside(templates_root, &source, "component source")?;
        Ok(source)
    }
}

fn read_manifest<T>(path: &Path, kind: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let source = fs::read_to_string(path).map_err(|error| {
        RendererError::new(format!(
            "failed to read {kind} manifest {}: {error}",
            path.display()
        ))
    })?;
    toml::from_str(&source).map_err(|error| {
        RendererError::new(format!(
            "invalid {kind} manifest {}: {error}",
            path.display()
        ))
    })
}

fn validate_component(component: &ComponentManifest) -> Result<()> {
    validate_identifier(&component.id, "component")?;
    validate_relative_path(&component.source, "component source")?;
    if component.include.is_empty() {
        return Err(RendererError::new(format!(
            "component {} includes no files",
            component.id
        )));
    }
    for include in &component.include {
        validate_relative_path(include, "component include")?;
    }
    for requirement in &component.requires {
        validate_identifier(requirement, "component requirement")?;
    }
    for conflict in &component.conflicts {
        validate_identifier(conflict, "component conflict")?;
    }
    for dependency in &component.framework_dependencies {
        validate_identifier(&dependency.name, "framework dependency")?;
        validate_relative_path(&dependency.manifest, "framework dependency manifest")?;
        validate_relative_path(&dependency.path, "framework dependency path")?;
    }
    Ok(())
}

fn validate_schema(schema: u32, path: &Path) -> Result<()> {
    if schema != MANIFEST_SCHEMA {
        return Err(RendererError::new(format!(
            "unsupported manifest schema {schema} in {}; expected {MANIFEST_SCHEMA}",
            path.display()
        )));
    }
    Ok(())
}

pub(crate) fn validate_identifier(value: &str, kind: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.bytes().enumerate().all(|(index, character)| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || (index > 0 && matches!(character, b'-' | b'_'))
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric);
    if !valid {
        return Err(RendererError::new(format!(
            "invalid {kind} identifier: {value}"
        )));
    }
    Ok(())
}

pub(crate) fn validate_variable(value: &str) -> Result<()> {
    validate_identifier(value, "template variable")
}

pub(crate) fn validate_relative_path(path: &Path, kind: &str) -> Result<()> {
    let valid = !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !valid {
        return Err(RendererError::new(format!(
            "{kind} must be a non-traversing relative path: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(crate) fn ensure_inside(parent: &Path, candidate: &Path, kind: &str) -> Result<()> {
    if !candidate.starts_with(parent) {
        return Err(RendererError::new(format!(
            "{kind} escapes {}: {}",
            parent.display(),
            candidate.display()
        )));
    }
    Ok(())
}

fn canonical_directory(path: &Path, kind: &str) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        RendererError::new(format!(
            "failed to resolve {kind} {}: {error}",
            path.display()
        ))
    })?;
    if !canonical.is_dir() {
        return Err(RendererError::new(format!(
            "{kind} is not a directory: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}
