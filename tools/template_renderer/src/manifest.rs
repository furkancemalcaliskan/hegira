use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use application_manifest::{
    ApplicationCapability, ClientAdapter, DatabaseAdapter, FrameworkContract,
    HEGIRA_COMPONENT_PACKAGE, HEGIRA_FRAMEWORK_REPOSITORY, PackageIdentity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{RendererError, Result, package_source::PackageSource};

const TEMPLATE_MANIFEST_SCHEMA: u32 = 1;
const COMPONENT_PACKAGE_MANIFEST_SCHEMA: u32 = 2;
const COMPONENT_MANIFEST_SCHEMA: u32 = 3;
const LEGACY_COMPONENT_MANIFEST_SCHEMA: u32 = 1;

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
    pub modules: Vec<String>,
    pub content_digest: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentManifest {
    pub schema: u32,
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    pub source: PathBuf,
    #[serde(default)]
    pub include: Vec<PathBuf>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub optional_dependencies: Vec<String>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub provides_capabilities: Vec<ApplicationCapability>,
    #[serde(default)]
    pub requires_capabilities: Vec<ApplicationCapability>,
    #[serde(default)]
    pub framework_dependencies: Vec<FrameworkDependency>,
    #[serde(default)]
    pub installation: Option<ComponentInstallationManifest>,
    #[serde(skip)]
    pub(crate) manifest_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkDependency {
    pub manifest: PathBuf,
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub default_features: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentInstallationManifest {
    pub module: String,
    pub databases: Vec<DatabaseAdapter>,
    pub clients: Vec<ClientAdapter>,
    pub contributions: Vec<ComponentInstallationContribution>,
    pub framework_dependencies: Vec<FrameworkDependency>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentInstallationContribution {
    AuthenticationSeed,
    BackgroundJobs,
    BearerApiRoutes,
    CapabilityPreflight,
    Configuration,
    CookieBffRoutes,
    LeptosNavigation,
    LeptosRoutes,
    Openapi,
    PostgresMigrationSource,
    SqliteMigrationSource,
}

#[derive(Debug)]
pub struct ManifestCatalog {
    repository_root: PathBuf,
    templates_root: PathBuf,
    source: PackageSource,
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
        let templates_root = repository_root.join("templates");
        let source = PackageSource::open(&templates_root)?;
        let package_path = Path::new("package.toml");
        let package = if let Some(bytes) = source.file(package_path) {
            let package: ComponentPackageManifest = read_manifest(bytes, "component package")?;
            validate_package(&package, package_path)?;
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
        let template_path = PathBuf::from("applications")
            .join(template_id)
            .join("template.toml");
        let template: TemplateManifest =
            read_required_manifest(&source, &template_path, "template")?;
        validate_schema(template.schema, TEMPLATE_MANIFEST_SCHEMA, &template_path)?;
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

        let mut component_paths = source
            .files_below(Path::new("components"))
            .map(|(path, _)| path.to_path_buf())
            .collect::<Vec<_>>();
        component_paths.sort();

        let mut components = BTreeMap::new();
        for component_path in component_paths {
            if component_path.parent() != Some(Path::new("components"))
                || component_path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    != Some("toml")
            {
                return Err(RendererError::new(
                    "component package contains an undeclared component manifest entry",
                ));
            }

            let mut component: ComponentManifest =
                read_required_manifest(&source, &component_path, "component")?;
            let expected_schema = if package.is_some() {
                COMPONENT_MANIFEST_SCHEMA
            } else {
                LEGACY_COMPONENT_MANIFEST_SCHEMA
            };
            validate_schema(component.schema, expected_schema, &component_path)?;
            validate_component(&component, package.as_ref())?;
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
            source,
            template,
            components,
            package,
        };
        catalog.validate_declared_package_files()?;
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
        let package = self.package.as_ref().expect("package presence was checked");
        for template in &package.templates {
            let path = PathBuf::from("applications")
                .join(template)
                .join("template.toml");
            insert_package_entry(
                &mut entries,
                path_to_package_key(&path)?,
                self.required_file(&path, "packaged template manifest")?
                    .to_vec(),
            )?;
        }

        for component in self.components.values() {
            let manifest_path = PathBuf::from("components").join(format!("{}.toml", component.id));
            insert_package_entry(
                &mut entries,
                format!("components/{}.toml", component.id),
                self.required_file(&manifest_path, "packaged component manifest")?
                    .to_vec(),
            )?;
            for (relative, bytes) in self.component_files(component)? {
                insert_package_entry(
                    &mut entries,
                    format!(
                        "sources/{}/{}",
                        component.id,
                        path_to_package_key(&relative)?
                    ),
                    bytes.to_vec(),
                )?;
            }
        }

        Ok(Some(content_digest(&entries)))
    }

    fn required_file(&self, path: &Path, kind: &str) -> Result<&[u8]> {
        self.source.file(path).ok_or_else(|| {
            RendererError::new(format!("component package is missing the declared {kind}"))
        })
    }

    pub(crate) fn component_files<'a>(
        &'a self,
        component: &ComponentManifest,
    ) -> Result<Vec<(PathBuf, &'a [u8])>> {
        let source_root = &component.source;
        let mut selected = BTreeMap::<PathBuf, &'a [u8]>::new();
        for include in &component.include {
            let declared = source_root.join(include);
            if let Some(bytes) = self.source.file(&declared) {
                let relative = declared.strip_prefix(source_root).map_err(|_| {
                    RendererError::new("component source declaration escapes its source root")
                })?;
                selected.insert(relative.to_path_buf(), bytes);
                continue;
            }
            let descendants = self
                .source
                .files_below(&declared)
                .filter(|(path, _)| *path != declared)
                .collect::<Vec<_>>();
            if descendants.is_empty() {
                return Err(RendererError::new(
                    "component include does not identify a declared package file or directory",
                ));
            }
            for (path, bytes) in descendants {
                let relative = path.strip_prefix(source_root).map_err(|_| {
                    RendererError::new("component source declaration escapes its source root")
                })?;
                selected.insert(relative.to_path_buf(), bytes);
            }
        }
        Ok(selected.into_iter().collect())
    }

    fn validate_declared_package_files(&self) -> Result<()> {
        let Some(package) = &self.package else {
            return Ok(());
        };
        let mut declared = BTreeSet::from([PathBuf::from("package.toml")]);
        for template in &package.templates {
            declared.insert(
                PathBuf::from("applications")
                    .join(template)
                    .join("template.toml"),
            );
        }
        for component in self.components.values() {
            declared.insert(PathBuf::from("components").join(format!("{}.toml", component.id)));
            let source_root = &component.source;
            for (relative, _) in self.component_files(component)? {
                declared.insert(source_root.join(relative));
            }
        }
        let observed = self
            .source
            .files()
            .map(|(path, _)| path.to_path_buf())
            .collect::<BTreeSet<_>>();
        if declared != observed {
            return Err(RendererError::new(
                "component package contains missing or undeclared files",
            ));
        }
        Ok(())
    }

    pub fn resolve_components(&self) -> Result<Vec<&ComponentManifest>> {
        if self.package.is_some() {
            let graph = self
                .resolve_component_roots(None)
                .map_err(|error| RendererError::new(error.to_string()))?;
            return self.components_for(&graph);
        }

        self.resolve_legacy_components()
    }

    pub fn resolve_composition(
        &self,
        request: &crate::CompositionRequest,
    ) -> std::result::Result<crate::ResolvedComposition, crate::CompositionError> {
        let Some(package) = &self.package else {
            return Err(crate::CompositionError::missing_package());
        };
        crate::composition::resolve(package, &self.components, request)
    }

    pub fn resolve_template_composition(
        &self,
    ) -> std::result::Result<crate::ResolvedComposition, crate::CompositionError> {
        self.resolve_component_roots(None)
    }

    /// Resolve caller-selected component roots against the package identity
    /// authenticated by this catalog. Callers may select composition roots,
    /// but cannot substitute the package or framework source contract.
    pub fn resolve_component_roots(
        &self,
        roots: Option<&[String]>,
    ) -> std::result::Result<crate::ResolvedComposition, crate::CompositionError> {
        let Some(package) = &self.package else {
            return Err(crate::CompositionError::missing_package());
        };
        let request = crate::CompositionRequest::new(
            package.framework.clone(),
            PackageIdentity {
                id: package.id.clone(),
                version: package.version.clone(),
            },
            roots
                .map(<[String]>::to_vec)
                .unwrap_or_else(|| self.template.components.clone()),
        );
        crate::composition::resolve(package, &self.components, &request)
    }

    pub(crate) fn components_for(
        &self,
        graph: &crate::ResolvedComposition,
    ) -> Result<Vec<&ComponentManifest>> {
        graph
            .components
            .iter()
            .map(|component| {
                self.components.get(&component.id).ok_or_else(|| {
                    RendererError::new(format!(
                        "resolved component does not exist: {}",
                        component.id
                    ))
                })
            })
            .collect()
    }

    fn resolve_legacy_components(&self) -> Result<Vec<&ComponentManifest>> {
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

fn path_to_package_key(path: &Path) -> Result<String> {
    let parts = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => Ok(value.to_string_lossy()),
            _ => Err(RendererError::new(
                "packaged component input contains an invalid path component",
            )),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(parts.join("/"))
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
    validate_schema(package.schema, COMPONENT_PACKAGE_MANIFEST_SCHEMA, path)?;
    validate_identifier(&package.id, "component package")?;
    if package.id != HEGIRA_COMPONENT_PACKAGE {
        return Err(RendererError::new(
            "component package identity does not match the bundled package",
        ));
    }
    package
        .framework
        .validate()
        .map_err(|_| RendererError::new("invalid framework repository in component package"))?;
    if package.framework.repository != HEGIRA_FRAMEWORK_REPOSITORY {
        return Err(RendererError::new(
            "component package framework source does not match the bundled release source",
        ));
    }
    if package.version != package.framework.version {
        return Err(RendererError::new(
            "component package version does not match framework version",
        ));
    }
    let bundled_version = format!("v{}", env!("CARGO_PKG_VERSION"));
    if package.version != bundled_version {
        return Err(RendererError::new(
            "component package version does not match the bundled framework release",
        ));
    }
    validate_sorted_identifiers(&package.templates, "package template", false)?;
    validate_sorted_identifiers(&package.components, "package component", false)?;
    validate_sorted_identifiers(&package.modules, "package module", true)?;
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

fn validate_sorted_identifiers(values: &[String], kind: &str, allow_empty: bool) -> Result<()> {
    if values.is_empty() && !allow_empty {
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

fn read_required_manifest<'a, T>(source: &'a PackageSource, path: &Path, kind: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = source.file(path).ok_or_else(|| {
        RendererError::new(format!("component package is missing the {kind} manifest"))
    })?;
    read_manifest(bytes, kind)
}

fn read_manifest<T>(bytes: &[u8], kind: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let source = std::str::from_utf8(bytes)
        .map_err(|_| RendererError::new(format!("invalid {kind} manifest encoding")))?;
    toml::from_str(source).map_err(|error| {
        let reason = if error.to_string().contains("unknown field") {
            ": unknown field"
        } else {
            ""
        };
        RendererError::new(format!("invalid {kind} manifest{reason}"))
    })
}

fn validate_component(
    component: &ComponentManifest,
    package: Option<&ComponentPackageManifest>,
) -> Result<()> {
    validate_identifier(&component.id, "component")?;
    match (package, &component.version) {
        (Some(_), Some(_)) => {}
        (Some(_), None) => {
            return Err(RendererError::new(format!(
                "packaged component {} declares no version",
                component.id
            )));
        }
        (None, None) => {}
        (None, Some(_)) => {
            return Err(RendererError::new(format!(
                "unpackaged legacy component {} cannot declare a version",
                component.id
            )));
        }
    }
    validate_relative_path(&component.source, "component source")?;
    if component.include.is_empty() && component.installation.is_none() {
        return Err(RendererError::new(format!(
            "rendered component {} includes no files",
            component.id
        )));
    }
    for include in &component.include {
        validate_relative_path(include, "component include")?;
    }
    validate_sorted_identifiers(&component.requires, "component requirement", true)?;
    validate_sorted_identifiers(&component.conflicts, "component conflict", true)?;
    validate_sorted_identifiers(
        &component.optional_dependencies,
        "component optional dependency",
        true,
    )?;
    validate_sorted_identifiers(&component.modules, "component module", true)?;
    validate_sorted_capabilities(
        &component.provides_capabilities,
        "provided component capability",
    )?;
    validate_sorted_capabilities(
        &component.requires_capabilities,
        "required component capability",
    )?;
    for relation in component
        .requires
        .iter()
        .chain(&component.optional_dependencies)
        .chain(&component.conflicts)
    {
        if relation == &component.id {
            return Err(RendererError::new(format!(
                "component {} cannot reference itself",
                component.id
            )));
        }
    }
    if component
        .requires
        .iter()
        .chain(&component.optional_dependencies)
        .any(|dependency| component.conflicts.contains(dependency))
    {
        return Err(RendererError::new(format!(
            "component {} cannot both depend on and conflict with the same component",
            component.id
        )));
    }
    for dependency in &component.framework_dependencies {
        validate_identifier(&dependency.name, "framework dependency")?;
        validate_relative_path(&dependency.manifest, "framework dependency manifest")?;
        validate_relative_path(&dependency.path, "framework dependency path")?;
    }
    if let Some(installation) = &component.installation {
        validate_installation(component, installation)?;
    }
    Ok(())
}

fn validate_installation(
    component: &ComponentManifest,
    installation: &ComponentInstallationManifest,
) -> Result<()> {
    validate_identifier(&installation.module, "installation module")?;
    if component.modules.as_slice() != [installation.module.as_str()] {
        return Err(RendererError::new(format!(
            "installable component {} must own exactly its declared module",
            component.id
        )));
    }
    if !component.include.is_empty() {
        return Err(RendererError::new(format!(
            "installable component {} cannot render or vendor application source",
            component.id
        )));
    }
    if component.requires.is_empty() {
        return Err(RendererError::new(format!(
            "installable component {} declares no compatible application base",
            component.id
        )));
    }
    validate_sorted_values(&installation.databases, "installation database adapter")?;
    validate_sorted_values(&installation.clients, "installation client adapter")?;
    validate_sorted_values(&installation.contributions, "installation contribution")?;
    if installation.framework_dependencies.is_empty() {
        return Err(RendererError::new(format!(
            "installable component {} declares no framework dependencies",
            component.id
        )));
    }
    for dependency in &installation.framework_dependencies {
        validate_identifier(&dependency.name, "installation framework dependency")?;
        validate_relative_path(
            &dependency.manifest,
            "installation framework dependency manifest",
        )?;
        validate_relative_path(&dependency.path, "installation framework dependency path")?;
    }
    if installation
        .framework_dependencies
        .windows(2)
        .any(|pair| pair[0].name >= pair[1].name)
    {
        return Err(RendererError::new(
            "installation framework dependencies must be sorted and unique",
        ));
    }
    if installation
        .framework_dependencies
        .iter()
        .any(|dependency| dependency.manifest != Path::new("Cargo.toml"))
    {
        return Err(RendererError::new(
            "installation framework dependencies must target the workspace manifest",
        ));
    }
    let dependency_paths = installation
        .framework_dependencies
        .iter()
        .map(|dependency| &dependency.path)
        .collect::<BTreeSet<_>>();
    if dependency_paths.len() != installation.framework_dependencies.len() {
        return Err(RendererError::new(
            "installation framework dependency paths must be unique",
        ));
    }
    for (database, contribution) in [
        (
            DatabaseAdapter::Postgres,
            ComponentInstallationContribution::PostgresMigrationSource,
        ),
        (
            DatabaseAdapter::Sqlite,
            ComponentInstallationContribution::SqliteMigrationSource,
        ),
    ] {
        if installation.databases.contains(&database)
            != installation.contributions.contains(&contribution)
        {
            return Err(RendererError::new(
                "installation database adapters and migration-source contributions must match",
            ));
        }
    }
    Ok(())
}

fn validate_sorted_values<T: Ord>(values: &[T], kind: &str) -> Result<()> {
    if values.is_empty() {
        return Err(RendererError::new(format!("component declares no {kind}s")));
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(RendererError::new(format!(
            "component {kind}s must be sorted and unique"
        )));
    }
    Ok(())
}

fn validate_schema(schema: u32, expected: u32, path: &Path) -> Result<()> {
    if schema != expected {
        return Err(RendererError::new(format!(
            "unsupported manifest schema {schema} in {}; expected {expected}",
            path.display()
        )));
    }
    Ok(())
}

fn validate_sorted_capabilities(values: &[ApplicationCapability], kind: &str) -> Result<()> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(RendererError::new(format!(
            "component {kind}s must be sorted and unique"
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

fn canonical_directory(path: &Path, kind: &str) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
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
