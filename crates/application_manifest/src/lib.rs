//! Versioned identity contract for applications generated from Hegira source.
//!
//! `hegira.toml` records generation state only. Runtime configuration and
//! credentials belong to the generated application's environment profiles and
//! secret-management system.

use std::{
    collections::BTreeSet,
    fmt::{Display, Formatter},
    fs,
    path::Path,
};

use semver::Version;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

pub const APPLICATION_MANIFEST_SCHEMA: u32 = 2;
pub const LEGACY_APPLICATION_MANIFEST_SCHEMA: u32 = 1;
pub const HEGIRA_FRAMEWORK_REPOSITORY: &str = "https://github.com/furkancemalcaliskan/hegira.git";
pub const HEGIRA_COMPONENT_PACKAGE: &str = "hegira-canonical";
pub const LAYERED_BASE_COMPONENT: &str = "layered-base";
pub const LAYERED_LEPTOS_IDENTITY_COMPONENT: &str = "layered-leptos-identity";
pub const LAYERED_LEPTOS_MINIMAL_COMPONENT: &str = "layered-leptos-minimal";
pub const IDENTITY_COMPONENT: &str = "identity";
pub const IDENTITY_MODULE: &str = "identity";

const SUPPORTED_COMPONENTS: [&str; 4] = [
    LAYERED_BASE_COMPONENT,
    LAYERED_LEPTOS_IDENTITY_COMPONENT,
    LAYERED_LEPTOS_MINIMAL_COMPONENT,
    IDENTITY_COMPONENT,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationManifest {
    pub schema: u32,
    pub application: String,
    pub framework: FrameworkContract,
    pub selection: ApplicationSelection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<ApplicationComposition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkContract {
    pub repository: String,
    pub version: String,
}

impl FrameworkContract {
    pub fn validate(&self) -> Result<(), ManifestError> {
        validate_framework_contract(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationSelection {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub components: BTreeSet<String>,
    pub databases: BTreeSet<DatabaseAdapter>,
    pub clients: BTreeSet<ClientAdapter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationComposition {
    pub package: PackageIdentity,
    pub components: Vec<InstalledComponent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<InstalledModule>,
    #[serde(default)]
    pub capabilities: BTreeSet<ApplicationCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageIdentity {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledComponent {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledModule {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplicationCapability {
    Authentication,
    Authorization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseAdapter {
    Postgres,
    Sqlite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientAdapter {
    Leptos,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationCompatibilityPolicy {
    schema: u32,
    framework: FrameworkContract,
    package: PackageIdentity,
    components: BTreeSet<String>,
    modules: BTreeSet<String>,
    capabilities: BTreeSet<ApplicationCapability>,
    databases: BTreeSet<DatabaseAdapter>,
    clients: BTreeSet<ClientAdapter>,
}

impl MutationCompatibilityPolicy {
    pub fn for_current_release() -> Result<Self, ManifestError> {
        Self::for_framework_version(format!("v{}", env!("CARGO_PKG_VERSION")))
    }

    pub fn for_framework_version(version: impl Into<String>) -> Result<Self, ManifestError> {
        let version = version.into();
        let framework = FrameworkContract {
            repository: HEGIRA_FRAMEWORK_REPOSITORY.to_owned(),
            version: version.clone(),
        };
        framework.validate()?;

        Ok(Self {
            schema: APPLICATION_MANIFEST_SCHEMA,
            framework,
            package: PackageIdentity {
                id: HEGIRA_COMPONENT_PACKAGE.to_owned(),
                version,
            },
            components: SUPPORTED_COMPONENTS
                .map(str::to_owned)
                .into_iter()
                .collect(),
            modules: [IDENTITY_MODULE.to_owned()].into_iter().collect(),
            capabilities: [
                ApplicationCapability::Authentication,
                ApplicationCapability::Authorization,
            ]
            .into_iter()
            .collect(),
            databases: [DatabaseAdapter::Postgres, DatabaseAdapter::Sqlite]
                .into_iter()
                .collect(),
            clients: [ClientAdapter::Leptos].into_iter().collect(),
        })
    }

    pub fn framework_version(&self) -> &str {
        &self.framework.version
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MutationManifestField {
    Schema,
    FrameworkRepository,
    FrameworkVersion,
    CompositionPackage,
    CompositionComponents,
    CompositionModules,
    CompositionCapabilities,
    SelectionComponents,
    SelectionDatabases,
    SelectionClients,
}

impl Display for MutationManifestField {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Schema => "schema",
            Self::FrameworkRepository => "framework.repository",
            Self::FrameworkVersion => "framework.version",
            Self::CompositionPackage => "composition.package",
            Self::CompositionComponents => "composition.components",
            Self::CompositionModules => "composition.modules",
            Self::CompositionCapabilities => "composition.capabilities",
            Self::SelectionComponents => "selection.components",
            Self::SelectionDatabases => "selection.databases",
            Self::SelectionClients => "selection.clients",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationCompatibilityIssue {
    pub field: MutationManifestField,
    pub actual: String,
    pub expected: String,
}

impl Display for MutationCompatibilityIssue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} is {}; expected {}",
            self.field, self.actual, self.expected
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "issue", rename_all = "kebab-case")]
pub enum MutationCompatibility {
    Compatible,
    Incompatible(MutationCompatibilityIssue),
    Unsupported(MutationCompatibilityIssue),
}

#[derive(Deserialize)]
struct ManifestSchemaProbe {
    schema: u32,
}

pub fn assess_mutation_compatibility(
    source: &str,
    policy: &MutationCompatibilityPolicy,
) -> Result<MutationCompatibility, ManifestError> {
    let probe: ManifestSchemaProbe = toml::from_str(source).map_err(ManifestError::Parse)?;
    if probe.schema != policy.schema {
        return Ok(MutationCompatibility::Unsupported(
            MutationCompatibilityIssue {
                field: MutationManifestField::Schema,
                actual: probe.schema.to_string(),
                expected: policy.schema.to_string(),
            },
        ));
    }

    let manifest: ApplicationManifest = toml::from_str(source).map_err(ManifestError::Parse)?;
    manifest.mutation_compatibility(policy)
}

#[derive(Debug)]
pub enum ManifestError {
    Read(std::io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
    UnsupportedSchema(u32),
    InvalidApplicationName(String),
    InvalidFrameworkRepository(String),
    InvalidFrameworkVersion(String),
    InvalidComponent(String),
    InvalidModule(String),
    InvalidPackage(String),
    UnsupportedComponent(String),
    MissingComposition,
    LegacyManifestReadOnly,
    DuplicateIdentity {
        kind: &'static str,
        identity: String,
    },
    EmptyDatabaseSelection,
    EmptyClientSelection,
    IncompatibleSelection(String),
}

impl ApplicationManifest {
    pub fn from_toml(source: &str) -> Result<Self, ManifestError> {
        let mut manifest: Self = toml::from_str(source).map_err(ManifestError::Parse)?;
        manifest.normalize();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn read(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let source = fs::read_to_string(path).map_err(ManifestError::Read)?;
        Self::from_toml(&source)
    }

    pub fn to_toml(&self) -> Result<String, ManifestError> {
        if self.schema == LEGACY_APPLICATION_MANIFEST_SCHEMA {
            return Err(ManifestError::LegacyManifestReadOnly);
        }
        let mut manifest = self.clone();
        manifest.normalize();
        manifest.validate()?;
        let mut serialized = toml::to_string(&manifest).map_err(ManifestError::Serialize)?;
        if !serialized.ends_with('\n') {
            serialized.push('\n');
        }
        Ok(serialized)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        validate_application_name(&self.application)?;
        validate_framework_contract(&self.framework)?;
        match self.schema {
            LEGACY_APPLICATION_MANIFEST_SCHEMA => {
                if self.composition.is_some() {
                    return Err(ManifestError::IncompatibleSelection(
                        "legacy manifests cannot contain composition state".to_owned(),
                    ));
                }
                validate_legacy_selection(&self.selection)
            }
            APPLICATION_MANIFEST_SCHEMA => {
                if !self.selection.components.is_empty() {
                    return Err(ManifestError::IncompatibleSelection(
                        "current manifests record components only in composition.components"
                            .to_owned(),
                    ));
                }
                validate_current_selection(&self.selection)?;
                validate_composition(
                    self.composition
                        .as_ref()
                        .ok_or(ManifestError::MissingComposition)?,
                    &self.framework,
                )
            }
            schema => Err(ManifestError::UnsupportedSchema(schema)),
        }
    }

    pub fn installed_component_ids(&self) -> BTreeSet<String> {
        match &self.composition {
            Some(composition) => composition
                .components
                .iter()
                .map(|component| component.id.clone())
                .collect(),
            None => self.selection.components.clone(),
        }
    }

    pub fn validate_rendered_components<'a>(
        &self,
        rendered: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), ManifestError> {
        let rendered = rendered
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let installed = self.installed_component_ids();
        if rendered != installed {
            return Err(ManifestError::IncompatibleSelection(format!(
                "manifest components {:?} do not match rendered components {:?}",
                installed, rendered
            )));
        }
        Ok(())
    }

    fn normalize(&mut self) {
        if let Some(composition) = &mut self.composition {
            composition
                .components
                .sort_by(|left, right| left.id.cmp(&right.id));
            composition
                .modules
                .sort_by(|left, right| left.id.cmp(&right.id));
        }
    }

    pub fn mutation_compatibility(
        &self,
        policy: &MutationCompatibilityPolicy,
    ) -> Result<MutationCompatibility, ManifestError> {
        if self.schema != policy.schema {
            return Ok(MutationCompatibility::Unsupported(
                MutationCompatibilityIssue {
                    field: MutationManifestField::Schema,
                    actual: self.schema.to_string(),
                    expected: policy.schema.to_string(),
                },
            ));
        }

        validate_application_name(&self.application)?;
        validate_framework_contract(&self.framework)?;
        if !self.selection.components.is_empty() {
            return Err(ManifestError::IncompatibleSelection(
                "current manifests record components only in composition.components".to_owned(),
            ));
        }
        validate_composition(
            self.composition
                .as_ref()
                .ok_or(ManifestError::MissingComposition)?,
            &self.framework,
        )?;

        if self.framework.repository != policy.framework.repository {
            return Ok(MutationCompatibility::Incompatible(
                MutationCompatibilityIssue {
                    field: MutationManifestField::FrameworkRepository,
                    actual: self.framework.repository.clone(),
                    expected: policy.framework.repository.clone(),
                },
            ));
        }
        if self.framework.version != policy.framework.version {
            return Ok(MutationCompatibility::Unsupported(
                MutationCompatibilityIssue {
                    field: MutationManifestField::FrameworkVersion,
                    actual: self.framework.version.clone(),
                    expected: policy.framework.version.clone(),
                },
            ));
        }

        let composition = self
            .composition
            .as_ref()
            .ok_or(ManifestError::MissingComposition)?;
        if composition.package != policy.package {
            return Ok(MutationCompatibility::Unsupported(
                MutationCompatibilityIssue {
                    field: MutationManifestField::CompositionPackage,
                    actual: format!("{}@{}", composition.package.id, composition.package.version),
                    expected: format!("{}@{}", policy.package.id, policy.package.version),
                },
            ));
        }

        for component in &composition.components {
            if !policy.components.contains(&component.id) {
                return Ok(MutationCompatibility::Unsupported(
                    MutationCompatibilityIssue {
                        field: MutationManifestField::CompositionComponents,
                        actual: component.id.clone(),
                        expected: format_string_set(&policy.components),
                    },
                ));
            }
        }
        for module in &composition.modules {
            if !policy.modules.contains(&module.id) {
                return Ok(MutationCompatibility::Unsupported(
                    MutationCompatibilityIssue {
                        field: MutationManifestField::CompositionModules,
                        actual: module.id.clone(),
                        expected: format_string_set(&policy.modules),
                    },
                ));
            }
        }
        if let Some(capability) = composition
            .capabilities
            .iter()
            .find(|capability| !policy.capabilities.contains(capability))
        {
            return Ok(MutationCompatibility::Unsupported(
                MutationCompatibilityIssue {
                    field: MutationManifestField::CompositionCapabilities,
                    actual: capability_name(*capability).to_owned(),
                    expected: format_capability_set(&policy.capabilities),
                },
            ));
        }

        if self.selection.databases.len() != 1 {
            return Ok(incompatible_selection(
                MutationManifestField::SelectionDatabases,
                format_database_set(&self.selection.databases),
                format!(
                    "exactly one adapter from {}",
                    format_database_set(&policy.databases)
                ),
            ));
        }
        if let Some(database) = self
            .selection
            .databases
            .iter()
            .find(|database| !policy.databases.contains(database))
        {
            return Ok(MutationCompatibility::Unsupported(
                MutationCompatibilityIssue {
                    field: MutationManifestField::SelectionDatabases,
                    actual: database_name(*database).to_owned(),
                    expected: format_database_set(&policy.databases),
                },
            ));
        }

        if self.selection.clients.len() != 1 {
            return Ok(incompatible_selection(
                MutationManifestField::SelectionClients,
                format_client_set(&self.selection.clients),
                format!(
                    "exactly one adapter from {}",
                    format_client_set(&policy.clients)
                ),
            ));
        }
        if let Some(client) = self
            .selection
            .clients
            .iter()
            .find(|client| !policy.clients.contains(client))
        {
            return Ok(MutationCompatibility::Unsupported(
                MutationCompatibilityIssue {
                    field: MutationManifestField::SelectionClients,
                    actual: client_name(*client).to_owned(),
                    expected: format_client_set(&policy.clients),
                },
            ));
        }

        Ok(MutationCompatibility::Compatible)
    }
}

fn incompatible_selection(
    field: MutationManifestField,
    actual: String,
    expected: String,
) -> MutationCompatibility {
    MutationCompatibility::Incompatible(MutationCompatibilityIssue {
        field,
        actual,
        expected,
    })
}

fn format_string_set(values: &BTreeSet<String>) -> String {
    format!(
        "[{}]",
        values.iter().cloned().collect::<Vec<_>>().join(", ")
    )
}

fn format_database_set(values: &BTreeSet<DatabaseAdapter>) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|adapter| database_name(*adapter))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_client_set(values: &BTreeSet<ClientAdapter>) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|adapter| client_name(*adapter))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_capability_set(values: &BTreeSet<ApplicationCapability>) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|capability| capability_name(*capability))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn database_name(adapter: DatabaseAdapter) -> &'static str {
    match adapter {
        DatabaseAdapter::Postgres => "postgres",
        DatabaseAdapter::Sqlite => "sqlite",
    }
}

fn client_name(adapter: ClientAdapter) -> &'static str {
    match adapter {
        ClientAdapter::Leptos => "leptos",
    }
}

fn capability_name(capability: ApplicationCapability) -> &'static str {
    match capability {
        ApplicationCapability::Authentication => "authentication",
        ApplicationCapability::Authorization => "authorization",
    }
}

pub fn validate_application_name(name: &str) -> Result<(), ManifestError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.bytes().enumerate().all(|(index, character)| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || (character == b'-'
                    && index > 0
                    && index + 1 < name.len()
                    && name.as_bytes()[index - 1] != b'-')
        })
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase);
    if valid && !reserved_project_name(name) {
        Ok(())
    } else {
        Err(ManifestError::InvalidApplicationName(name.to_owned()))
    }
}

/// Names that collide with Rust keywords, Cargo output, or portable device names.
pub fn reserved_project_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "gen"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "union"
            | "target"
            | "con"
            | "prn"
            | "aux"
            | "nul"
    ) || ((name.starts_with("com") || name.starts_with("lpt"))
        && name.len() == 4
        && matches!(name.as_bytes()[3], b'1'..=b'9'))
}

fn validate_framework_contract(contract: &FrameworkContract) -> Result<(), ManifestError> {
    let repository = Url::parse(&contract.repository)
        .map_err(|_| ManifestError::InvalidFrameworkRepository(contract.repository.clone()))?;
    let host_is_local = match repository.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => true,
    };
    if repository.scheme() != "https"
        || host_is_local
        || !repository.username().is_empty()
        || repository.password().is_some()
        || repository.query().is_some()
        || repository.fragment().is_some()
        || !repository.path().ends_with(".git")
    {
        return Err(ManifestError::InvalidFrameworkRepository(
            contract.repository.clone(),
        ));
    }

    validate_release_version(&contract.version)
        .map_err(|_| ManifestError::InvalidFrameworkVersion(contract.version.clone()))
}

fn validate_release_version(version: &str) -> Result<(), ()> {
    let version = version.strip_prefix('v').ok_or(())?;
    let version = Version::parse(version).map_err(|_| ())?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(());
    }
    Ok(())
}

fn validate_legacy_selection(selection: &ApplicationSelection) -> Result<(), ManifestError> {
    if selection.databases.is_empty() {
        return Err(ManifestError::EmptyDatabaseSelection);
    }
    if selection.clients.is_empty() {
        return Err(ManifestError::EmptyClientSelection);
    }
    for component in &selection.components {
        validate_component_identifier(component)?;
        if !SUPPORTED_COMPONENTS.contains(&component.as_str()) {
            return Err(ManifestError::UnsupportedComponent(component.clone()));
        }
    }
    if !selection.components.contains(LAYERED_BASE_COMPONENT) {
        return Err(ManifestError::IncompatibleSelection(format!(
            "every layered application requires {LAYERED_BASE_COMPONENT}"
        )));
    }
    if selection.clients.contains(&ClientAdapter::Leptos)
        && !selection
            .components
            .contains(LAYERED_LEPTOS_IDENTITY_COMPONENT)
    {
        return Err(ManifestError::IncompatibleSelection(format!(
            "the Leptos client requires {LAYERED_LEPTOS_IDENTITY_COMPONENT}"
        )));
    }
    Ok(())
}

fn validate_current_selection(selection: &ApplicationSelection) -> Result<(), ManifestError> {
    if selection.databases.len() != 1 {
        return Err(ManifestError::IncompatibleSelection(
            "current manifests must select exactly one database adapter".to_owned(),
        ));
    }
    if selection.clients.len() != 1 {
        return Err(ManifestError::IncompatibleSelection(
            "current manifests must select exactly one client adapter".to_owned(),
        ));
    }
    Ok(())
}

fn validate_composition(
    composition: &ApplicationComposition,
    framework: &FrameworkContract,
) -> Result<(), ManifestError> {
    validate_component_identifier(&composition.package.id)
        .map_err(|_| ManifestError::InvalidPackage(composition.package.id.clone()))?;
    if composition.package.id != HEGIRA_COMPONENT_PACKAGE {
        return Err(ManifestError::InvalidPackage(
            composition.package.id.clone(),
        ));
    }
    validate_release_version(&composition.package.version)
        .map_err(|_| ManifestError::InvalidPackage(composition.package.version.clone()))?;
    if composition.package.version != framework.version {
        return Err(ManifestError::IncompatibleSelection(format!(
            "component package version {} does not match framework version {}",
            composition.package.version, framework.version
        )));
    }

    let mut component_ids = BTreeSet::new();
    for component in &composition.components {
        validate_component_identifier(&component.id)?;
        if !component_ids.insert(component.id.clone()) {
            return Err(ManifestError::DuplicateIdentity {
                kind: "component",
                identity: component.id.clone(),
            });
        }
        validate_release_version(&component.version).map_err(|_| {
            ManifestError::IncompatibleSelection(format!(
                "component {} has invalid version {}",
                component.id, component.version
            ))
        })?;
        if component.version != composition.package.version {
            return Err(ManifestError::IncompatibleSelection(format!(
                "component {} version {} does not match package version {}",
                component.id, component.version, composition.package.version
            )));
        }
    }
    if !component_ids.contains(LAYERED_BASE_COMPONENT) {
        return Err(ManifestError::IncompatibleSelection(format!(
            "every layered application requires {LAYERED_BASE_COMPONENT}"
        )));
    }

    let mut module_ids = BTreeSet::new();
    for module in &composition.modules {
        validate_module_identifier(&module.id)?;
        if !module_ids.insert(module.id.clone()) {
            return Err(ManifestError::DuplicateIdentity {
                kind: "module",
                identity: module.id.clone(),
            });
        }
        validate_release_version(&module.version).map_err(|_| {
            ManifestError::IncompatibleSelection(format!(
                "module {} has invalid version {}",
                module.id, module.version
            ))
        })?;
        if module.version != composition.package.version {
            return Err(ManifestError::IncompatibleSelection(format!(
                "module {} version {} does not match package version {}",
                module.id, module.version, composition.package.version
            )));
        }
    }

    let identity_component = component_ids.contains(LAYERED_LEPTOS_IDENTITY_COMPONENT)
        || component_ids.contains(IDENTITY_COMPONENT);
    let identity_module = module_ids.contains(IDENTITY_MODULE);
    if identity_component != identity_module {
        return Err(ManifestError::IncompatibleSelection(
            "the Identity component and module must be installed together".to_owned(),
        ));
    }
    let identity_capabilities = [
        ApplicationCapability::Authentication,
        ApplicationCapability::Authorization,
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let expected_capabilities = if identity_module {
        identity_capabilities
    } else {
        BTreeSet::new()
    };
    if composition.capabilities != expected_capabilities {
        return Err(ManifestError::IncompatibleSelection(format!(
            "composition capabilities {} do not match installed modules {}",
            format_capability_set(&composition.capabilities),
            format_string_set(&module_ids)
        )));
    }

    Ok(())
}

fn validate_module_identifier(module: &str) -> Result<(), ManifestError> {
    validate_component_identifier(module)
        .map_err(|_| ManifestError::InvalidModule(module.to_owned()))
}

fn validate_component_identifier(component: &str) -> Result<(), ManifestError> {
    let valid = !component.is_empty()
        && component.bytes().enumerate().all(|(index, character)| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || (character == b'-'
                    && index > 0
                    && index + 1 < component.len()
                    && component.as_bytes()[index - 1] != b'-')
        })
        && component
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_lowercase);
    if valid {
        Ok(())
    } else {
        Err(ManifestError::InvalidComponent(component.to_owned()))
    }
}

impl Display for ManifestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(error) => write!(formatter, "failed to read application manifest: {error}"),
            Self::Parse(error) => write!(formatter, "invalid application manifest: {error}"),
            Self::Serialize(error) => {
                write!(
                    formatter,
                    "failed to serialize application manifest: {error}"
                )
            }
            Self::UnsupportedSchema(schema) => write!(
                formatter,
                "unsupported application manifest schema {schema}; expected {APPLICATION_MANIFEST_SCHEMA}"
            ),
            Self::InvalidApplicationName(name) => {
                write!(formatter, "invalid application manifest name: {name}")
            }
            Self::InvalidFrameworkRepository(repository) => write!(
                formatter,
                "invalid framework repository in application manifest: {repository}"
            ),
            Self::InvalidFrameworkVersion(version) => write!(
                formatter,
                "invalid framework version in application manifest: {version}"
            ),
            Self::InvalidComponent(component) => {
                write!(formatter, "invalid component identifier: {component}")
            }
            Self::InvalidModule(module) => {
                write!(formatter, "invalid module identifier: {module}")
            }
            Self::InvalidPackage(package) => {
                write!(formatter, "invalid component package identity: {package}")
            }
            Self::UnsupportedComponent(component) => {
                write!(formatter, "unsupported application component: {component}")
            }
            Self::MissingComposition => {
                formatter.write_str("current application manifest has no composition state")
            }
            Self::LegacyManifestReadOnly => formatter.write_str(
                "legacy application manifests are readable but cannot be serialized for mutation",
            ),
            Self::DuplicateIdentity { kind, identity } => {
                write!(formatter, "duplicate {kind} identity: {identity}")
            }
            Self::EmptyDatabaseSelection => {
                formatter.write_str("application manifest selects no database adapter")
            }
            Self::EmptyClientSelection => {
                formatter.write_str("application manifest selects no client adapter")
            }
            Self::IncompatibleSelection(reason) => {
                write!(formatter, "incompatible application selection: {reason}")
            }
        }
    }
}

impl std::error::Error for ManifestError {}

#[cfg(test)]
mod tests {
    use super::*;

    const CANONICAL: &str = r#"schema = 2
application = "application"

[framework]
repository = "https://github.com/furkancemalcaliskan/hegira.git"
version = "v0.3.0"

[selection]
databases = ["sqlite"]
clients = ["leptos"]

[composition]
capabilities = ["authentication", "authorization"]

[composition.package]
id = "hegira-canonical"
version = "v0.3.0"

[[composition.components]]
id = "layered-leptos-identity"
version = "v0.3.0"

[[composition.components]]
id = "layered-base"
version = "v0.3.0"

[[composition.modules]]
id = "identity"
version = "v0.3.0"
"#;

    const LEGACY_V1: &str = r#"schema = 1
application = "application"

[framework]
repository = "https://github.com/furkancemalcaliskan/hegira.git"
version = "v0.5.0"

[selection]
components = ["layered-base", "layered-leptos-identity"]
databases = ["sqlite"]
clients = ["leptos"]
"#;

    fn mutable_manifest(version: &str) -> String {
        CANONICAL.replace("v0.3.0", version)
    }

    #[test]
    fn canonical_manifest_round_trips_deterministically() {
        let parsed = ApplicationManifest::from_toml(CANONICAL).expect("manifest should parse");
        let first = parsed.to_toml().expect("manifest should serialize");
        let reparsed = ApplicationManifest::from_toml(&first).expect("serialized manifest parses");
        let second = reparsed.to_toml().expect("manifest should serialize again");

        assert_eq!(parsed, reparsed);
        assert_eq!(first, second);
        assert!(
            first.find("layered-base").unwrap() < first.find("layered-leptos-identity").unwrap()
        );
    }

    #[test]
    fn rejects_unknown_fields_and_adapter_values() {
        let runtime_configuration = CANONICAL.replace(
            "version = \"v0.3.0\"",
            "version = \"v0.3.0\"\nruntime_profile = \"production\"",
        );
        assert!(matches!(
            ApplicationManifest::from_toml(&runtime_configuration),
            Err(ManifestError::Parse(_))
        ));

        let unknown = CANONICAL.replace("clients = [\"leptos\"]", "clients = [\"unknown\"]");
        assert!(matches!(
            ApplicationManifest::from_toml(&unknown),
            Err(ManifestError::Parse(_))
        ));
    }

    #[test]
    fn rejects_invalid_identity_framework_and_schema_values() {
        for invalid in [
            CANONICAL.replace("schema = 2", "schema = 3"),
            CANONICAL.replace("application = \"application\"", "application = \"../app\""),
            CANONICAL.replace(
                "https://github.com/furkancemalcaliskan/hegira.git",
                "file:///tmp/hegira",
            ),
            CANONICAL.replace("version = \"v0.3.0\"", "version = \"main\""),
        ] {
            assert!(ApplicationManifest::from_toml(&invalid).is_err());
        }
    }

    #[test]
    fn rejects_inconsistent_component_module_and_capability_state() {
        let incompatible = CANONICAL.replace(
            "[[composition.modules]]\nid = \"identity\"\nversion = \"v0.3.0\"\n",
            "",
        );
        assert!(matches!(
            ApplicationManifest::from_toml(&incompatible),
            Err(ManifestError::IncompatibleSelection(_))
        ));

        let missing_capability = CANONICAL.replace(
            "capabilities = [\"authentication\", \"authorization\"]",
            "capabilities = [\"authentication\"]",
        );
        assert!(matches!(
            ApplicationManifest::from_toml(&missing_capability),
            Err(ManifestError::IncompatibleSelection(_))
        ));
    }

    #[test]
    fn rejects_duplicate_component_and_module_identities() {
        let duplicate_component = CANONICAL.replace(
            "[[composition.modules]]",
            "[[composition.components]]\nid = \"layered-base\"\nversion = \"v0.3.0\"\n\n[[composition.modules]]",
        );
        assert!(matches!(
            ApplicationManifest::from_toml(&duplicate_component),
            Err(ManifestError::DuplicateIdentity {
                kind: "component",
                ..
            })
        ));

        let duplicate_module = format!(
            "{CANONICAL}\n[[composition.modules]]\nid = \"identity\"\nversion = \"v0.3.0\"\n"
        );
        assert!(matches!(
            ApplicationManifest::from_toml(&duplicate_module),
            Err(ManifestError::DuplicateIdentity { kind: "module", .. })
        ));
    }

    #[test]
    fn rejects_empty_provider_and_client_selections() {
        let no_database = CANONICAL.replace("databases = [\"sqlite\"]", "databases = []");
        assert!(matches!(
            ApplicationManifest::from_toml(&no_database),
            Err(ManifestError::IncompatibleSelection(_))
        ));

        let no_client = CANONICAL.replace("clients = [\"leptos\"]", "clients = []");
        assert!(matches!(
            ApplicationManifest::from_toml(&no_client),
            Err(ManifestError::IncompatibleSelection(_))
        ));
    }

    #[test]
    fn compatible_manifest_matches_the_explicit_mutation_policy() {
        let policy = MutationCompatibilityPolicy::for_framework_version("v0.6.0").unwrap();
        let source = mutable_manifest("v0.6.0");

        assert_eq!(
            assess_mutation_compatibility(&source, &policy).unwrap(),
            MutationCompatibility::Compatible
        );
        assert_eq!(policy.framework_version(), "v0.6.0");
    }

    #[test]
    fn current_release_policy_uses_the_package_release_without_panicking() {
        let policy = MutationCompatibilityPolicy::for_current_release().unwrap();

        assert_eq!(
            policy.framework_version(),
            format!("v{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn unknown_schema_is_unsupported_without_relaxing_normal_parsing() {
        let policy = MutationCompatibilityPolicy::for_framework_version("v0.6.0").unwrap();
        let source = mutable_manifest("v0.6.0").replace("schema = 2", "schema = 3");

        assert_eq!(
            assess_mutation_compatibility(&source, &policy).unwrap(),
            MutationCompatibility::Unsupported(MutationCompatibilityIssue {
                field: MutationManifestField::Schema,
                actual: "3".to_owned(),
                expected: "2".to_owned(),
            })
        );
        assert!(matches!(
            ApplicationManifest::from_toml(&source),
            Err(ManifestError::UnsupportedSchema(3))
        ));
    }

    #[test]
    fn legacy_v1_manifest_is_readable_but_never_writable() {
        let policy = MutationCompatibilityPolicy::for_framework_version("v0.6.0").unwrap();
        let manifest = ApplicationManifest::from_toml(LEGACY_V1)
            .expect("legacy v0.5 manifest should remain readable");

        assert_eq!(
            assess_mutation_compatibility(LEGACY_V1, &policy).unwrap(),
            MutationCompatibility::Unsupported(MutationCompatibilityIssue {
                field: MutationManifestField::Schema,
                actual: "1".to_owned(),
                expected: "2".to_owned(),
            })
        );
        assert!(matches!(
            manifest.to_toml(),
            Err(ManifestError::LegacyManifestReadOnly)
        ));
    }

    #[test]
    fn different_framework_identity_is_incompatible() {
        let policy = MutationCompatibilityPolicy::for_framework_version("v0.6.0").unwrap();
        let source = mutable_manifest("v0.6.0").replace(
            HEGIRA_FRAMEWORK_REPOSITORY,
            "https://github.com/example/hegira.git",
        );

        assert_eq!(
            assess_mutation_compatibility(&source, &policy).unwrap(),
            MutationCompatibility::Incompatible(MutationCompatibilityIssue {
                field: MutationManifestField::FrameworkRepository,
                actual: "https://github.com/example/hegira.git".to_owned(),
                expected: HEGIRA_FRAMEWORK_REPOSITORY.to_owned(),
            })
        );
    }

    #[test]
    fn unsupported_components_name_the_conflicting_field() {
        let policy = MutationCompatibilityPolicy::for_framework_version("v0.6.0").unwrap();
        let source = mutable_manifest("v0.6.0").replace(
            "[[composition.modules]]",
            "[[composition.components]]\nid = \"layered-future-client\"\nversion = \"v0.6.0\"\n\n[[composition.modules]]",
        );

        let compatibility = assess_mutation_compatibility(&source, &policy).unwrap();
        let MutationCompatibility::Unsupported(issue) = compatibility else {
            panic!("unknown components must be unsupported");
        };
        assert_eq!(issue.field, MutationManifestField::CompositionComponents);
        assert_eq!(issue.actual, "layered-future-client");
    }

    #[test]
    fn noncanonical_adapter_cardinality_names_each_selection_field() {
        let policy = MutationCompatibilityPolicy::for_framework_version("v0.6.0").unwrap();
        let multiple_databases = mutable_manifest("v0.6.0").replace(
            "databases = [\"sqlite\"]",
            "databases = [\"postgres\", \"sqlite\"]",
        );
        let no_clients =
            mutable_manifest("v0.6.0").replace("clients = [\"leptos\"]", "clients = []");

        let MutationCompatibility::Incompatible(database_issue) =
            assess_mutation_compatibility(&multiple_databases, &policy).unwrap()
        else {
            panic!("multiple databases must be incompatible");
        };
        assert_eq!(
            database_issue.field,
            MutationManifestField::SelectionDatabases
        );

        let MutationCompatibility::Incompatible(client_issue) =
            assess_mutation_compatibility(&no_clients, &policy).unwrap()
        else {
            panic!("an empty client selection must be incompatible");
        };
        assert_eq!(client_issue.field, MutationManifestField::SelectionClients);
    }

    #[test]
    fn malformed_current_schema_manifest_remains_invalid() {
        let policy = MutationCompatibilityPolicy::for_framework_version("v0.6.0").unwrap();
        let invalid =
            mutable_manifest("v0.6.0").replace(HEGIRA_FRAMEWORK_REPOSITORY, "file:///tmp/hegira");

        assert!(matches!(
            assess_mutation_compatibility(&invalid, &policy),
            Err(ManifestError::InvalidFrameworkRepository(_))
        ));
    }
}
