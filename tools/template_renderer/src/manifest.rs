use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;

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
}

impl ManifestCatalog {
    pub fn load(repository_root: impl AsRef<Path>, template_id: &str) -> Result<Self> {
        validate_identifier(template_id, "template")?;

        let repository_root = canonical_directory(repository_root.as_ref(), "repository root")?;
        let templates_root =
            canonical_directory(&repository_root.join("templates"), "templates root")?;
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

        let catalog = Self {
            repository_root,
            templates_root,
            template,
            components,
        };
        catalog.resolve_components()?;
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
