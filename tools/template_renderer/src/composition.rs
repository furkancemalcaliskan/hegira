use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
};

use application_manifest::{
    ApplicationCapability, FrameworkContract, InstalledComponent, InstalledModule, PackageIdentity,
};
use serde::Serialize;

use crate::{ComponentManifest, ComponentPackageManifest};

pub const COMPOSITION_GRAPH_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionRequest {
    pub framework: FrameworkContract,
    pub package: PackageIdentity,
    pub components: Vec<String>,
    pub recorded_modules: Option<Vec<InstalledModule>>,
    pub recorded_capabilities: Option<BTreeSet<ApplicationCapability>>,
}

impl CompositionRequest {
    pub fn new(
        framework: FrameworkContract,
        package: PackageIdentity,
        components: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            framework,
            package,
            components: components.into_iter().collect(),
            recorded_modules: None,
            recorded_capabilities: None,
        }
    }

    pub fn with_recorded_state(
        mut self,
        modules: impl IntoIterator<Item = InstalledModule>,
        capabilities: impl IntoIterator<Item = ApplicationCapability>,
    ) -> Self {
        self.recorded_modules = Some(modules.into_iter().collect());
        self.recorded_capabilities = Some(capabilities.into_iter().collect());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedComposition {
    pub schema: u32,
    pub framework: FrameworkContract,
    pub package: PackageIdentity,
    pub components: Vec<ResolvedComponent>,
    pub modules: Vec<InstalledModule>,
    pub capabilities: BTreeSet<ApplicationCapability>,
}

impl ResolvedComposition {
    pub fn to_toml(&self) -> Result<String, CompositionError> {
        let mut output = toml::to_string(self).map_err(|error| {
            CompositionError::one(CompositionDiagnostic::new(
                CompositionDiagnosticKind::Serialization,
                "composition",
                None,
                None,
                Some(error.to_string()),
            ))
        })?;
        if !output.ends_with('\n') {
            output.push('\n');
        }
        Ok(output)
    }

    pub fn installed_components(&self) -> impl ExactSizeIterator<Item = InstalledComponent> + '_ {
        self.components.iter().map(|component| InstalledComponent {
            id: component.id.clone(),
            version: component.version.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedComponent {
    pub id: String,
    pub version: String,
    pub required_dependencies: Vec<String>,
    pub optional_dependencies: Vec<String>,
    pub modules: Vec<String>,
    pub provides_capabilities: Vec<ApplicationCapability>,
    pub requires_capabilities: Vec<ApplicationCapability>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionDiagnosticKind {
    PackageUnavailable,
    InvalidComponent,
    DuplicateComponent,
    MissingComponent,
    MissingOptionalDependency,
    DependencyCycle,
    ComponentConflict,
    FrameworkRepositoryMismatch,
    FrameworkVersionMismatch,
    PackageIdentityMismatch,
    PackageVersionMismatch,
    ComponentVersionMismatch,
    UndeclaredModule,
    DuplicateModuleOwner,
    DuplicateRecordedModule,
    MissingRecordedModule,
    UnexpectedRecordedModule,
    ModuleVersionMismatch,
    MissingCapability,
    RecordedCapabilityMismatch,
    Serialization,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDiagnostic {
    pub kind: CompositionDiagnosticKind,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

impl CompositionDiagnostic {
    fn new(
        kind: CompositionDiagnosticKind,
        subject: impl Into<String>,
        related: Option<String>,
        expected: Option<String>,
        actual: Option<String>,
    ) -> Self {
        Self {
            kind,
            subject: subject.into(),
            related,
            expected,
            actual,
        }
    }
}

impl Display for CompositionDiagnostic {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {}",
            diagnostic_kind_name(self.kind),
            self.subject
        )?;
        if let Some(related) = &self.related {
            write!(formatter, " ({related})")?;
        }
        if let Some(expected) = &self.expected {
            write!(formatter, "; expected {expected}")?;
        }
        if let Some(actual) = &self.actual {
            write!(formatter, "; found {actual}")?;
        }
        Ok(())
    }
}

fn diagnostic_kind_name(kind: CompositionDiagnosticKind) -> &'static str {
    match kind {
        CompositionDiagnosticKind::PackageUnavailable => "package-unavailable",
        CompositionDiagnosticKind::InvalidComponent => "invalid-component",
        CompositionDiagnosticKind::DuplicateComponent => "duplicate-component",
        CompositionDiagnosticKind::MissingComponent => "missing-component",
        CompositionDiagnosticKind::MissingOptionalDependency => "missing-optional-dependency",
        CompositionDiagnosticKind::DependencyCycle => "dependency-cycle",
        CompositionDiagnosticKind::ComponentConflict => "component-conflict",
        CompositionDiagnosticKind::FrameworkRepositoryMismatch => "framework-repository-mismatch",
        CompositionDiagnosticKind::FrameworkVersionMismatch => "framework-version-mismatch",
        CompositionDiagnosticKind::PackageIdentityMismatch => "package-identity-mismatch",
        CompositionDiagnosticKind::PackageVersionMismatch => "package-version-mismatch",
        CompositionDiagnosticKind::ComponentVersionMismatch => "component-version-mismatch",
        CompositionDiagnosticKind::UndeclaredModule => "undeclared-module",
        CompositionDiagnosticKind::DuplicateModuleOwner => "duplicate-module-owner",
        CompositionDiagnosticKind::DuplicateRecordedModule => "duplicate-recorded-module",
        CompositionDiagnosticKind::MissingRecordedModule => "missing-recorded-module",
        CompositionDiagnosticKind::UnexpectedRecordedModule => "unexpected-recorded-module",
        CompositionDiagnosticKind::ModuleVersionMismatch => "module-version-mismatch",
        CompositionDiagnosticKind::MissingCapability => "missing-capability",
        CompositionDiagnosticKind::RecordedCapabilityMismatch => "recorded-capability-mismatch",
        CompositionDiagnosticKind::Serialization => "serialization",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionError {
    diagnostics: Vec<CompositionDiagnostic>,
}

impl CompositionError {
    fn one(diagnostic: CompositionDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }

    fn from_diagnostics(mut diagnostics: Vec<CompositionDiagnostic>) -> Self {
        diagnostics.sort();
        diagnostics.dedup();
        Self { diagnostics }
    }

    pub fn diagnostics(&self) -> &[CompositionDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn missing_package() -> Self {
        Self::one(CompositionDiagnostic::new(
            CompositionDiagnosticKind::PackageUnavailable,
            "component package",
            None,
            None,
            None,
        ))
    }
}

impl Display for CompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        for (index, diagnostic) in self.diagnostics.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            Display::fmt(diagnostic, formatter)?;
        }
        Ok(())
    }
}

impl std::error::Error for CompositionError {}

pub(crate) fn resolve(
    package: &ComponentPackageManifest,
    components: &BTreeMap<String, ComponentManifest>,
    request: &CompositionRequest,
) -> Result<ResolvedComposition, CompositionError> {
    let mut diagnostics = compatibility_diagnostics(package, request);
    let mut roots = BTreeSet::new();
    for component in &request.components {
        if !valid_identifier(component) {
            diagnostics.push(CompositionDiagnostic::new(
                CompositionDiagnosticKind::InvalidComponent,
                component,
                None,
                None,
                None,
            ));
        } else if !roots.insert(component.clone()) {
            diagnostics.push(CompositionDiagnostic::new(
                CompositionDiagnosticKind::DuplicateComponent,
                component,
                None,
                None,
                None,
            ));
        }
    }

    let mut state = BTreeMap::new();
    let mut stack = Vec::new();
    let mut ordered = Vec::new();
    for root in roots {
        visit(
            &root,
            None,
            components,
            &mut state,
            &mut stack,
            &mut ordered,
            &mut diagnostics,
        );
    }

    let selected = ordered.iter().cloned().collect::<BTreeSet<_>>();
    let mut provided_capabilities = BTreeSet::new();
    let mut module_owners = BTreeMap::<String, Vec<String>>::new();
    let packaged_modules = package.modules.iter().cloned().collect::<BTreeSet<_>>();

    for id in &ordered {
        let component = &components[id];
        let version = component.version.as_deref().unwrap_or_default();
        if version != package.version {
            diagnostics.push(CompositionDiagnostic::new(
                CompositionDiagnosticKind::ComponentVersionMismatch,
                id,
                None,
                Some(package.version.clone()),
                Some(version.to_owned()),
            ));
        }
        for optional in &component.optional_dependencies {
            if !components.contains_key(optional) {
                diagnostics.push(CompositionDiagnostic::new(
                    CompositionDiagnosticKind::MissingOptionalDependency,
                    optional,
                    Some(id.clone()),
                    None,
                    None,
                ));
            }
        }
        for conflict in &component.conflicts {
            if selected.contains(conflict) {
                let (subject, related) = if id < conflict {
                    (id.clone(), conflict.clone())
                } else {
                    (conflict.clone(), id.clone())
                };
                diagnostics.push(CompositionDiagnostic::new(
                    CompositionDiagnosticKind::ComponentConflict,
                    subject,
                    Some(related),
                    None,
                    None,
                ));
            }
        }
        provided_capabilities.extend(component.provides_capabilities.iter().copied());
        for module in &component.modules {
            if !packaged_modules.contains(module) {
                diagnostics.push(CompositionDiagnostic::new(
                    CompositionDiagnosticKind::UndeclaredModule,
                    module,
                    Some(id.clone()),
                    None,
                    None,
                ));
            }
            module_owners
                .entry(module.clone())
                .or_default()
                .push(id.clone());
        }
    }

    for id in &ordered {
        let component = &components[id];
        for capability in &component.requires_capabilities {
            if !provided_capabilities.contains(capability) {
                diagnostics.push(CompositionDiagnostic::new(
                    CompositionDiagnosticKind::MissingCapability,
                    capability_name(*capability),
                    Some(id.clone()),
                    None,
                    None,
                ));
            }
        }
    }

    for (module, owners) in &module_owners {
        if owners.len() > 1 {
            diagnostics.push(CompositionDiagnostic::new(
                CompositionDiagnosticKind::DuplicateModuleOwner,
                module,
                Some(owners.join(",")),
                None,
                None,
            ));
        }
    }

    let modules = module_owners
        .keys()
        .map(|id| InstalledModule {
            id: id.clone(),
            version: package.version.clone(),
        })
        .collect::<Vec<_>>();
    validate_recorded_state(&modules, &provided_capabilities, request, &mut diagnostics);

    if !diagnostics.is_empty() {
        return Err(CompositionError::from_diagnostics(diagnostics));
    }

    let resolved_components = ordered
        .into_iter()
        .map(|id| {
            let component = &components[&id];
            ResolvedComponent {
                id,
                version: component
                    .version
                    .clone()
                    .expect("packaged components require a validated version"),
                required_dependencies: component.requires.clone(),
                optional_dependencies: component.optional_dependencies.clone(),
                modules: component.modules.clone(),
                provides_capabilities: component.provides_capabilities.clone(),
                requires_capabilities: component.requires_capabilities.clone(),
            }
        })
        .collect();

    Ok(ResolvedComposition {
        schema: COMPOSITION_GRAPH_SCHEMA,
        framework: package.framework.clone(),
        package: PackageIdentity {
            id: package.id.clone(),
            version: package.version.clone(),
        },
        components: resolved_components,
        modules,
        capabilities: provided_capabilities,
    })
}

fn compatibility_diagnostics(
    package: &ComponentPackageManifest,
    request: &CompositionRequest,
) -> Vec<CompositionDiagnostic> {
    let mut diagnostics = Vec::new();
    for (kind, subject, expected, actual) in [
        (
            CompositionDiagnosticKind::FrameworkRepositoryMismatch,
            "framework.repository",
            package.framework.repository.as_str(),
            request.framework.repository.as_str(),
        ),
        (
            CompositionDiagnosticKind::FrameworkVersionMismatch,
            "framework.version",
            package.framework.version.as_str(),
            request.framework.version.as_str(),
        ),
        (
            CompositionDiagnosticKind::PackageIdentityMismatch,
            "package.id",
            package.id.as_str(),
            request.package.id.as_str(),
        ),
        (
            CompositionDiagnosticKind::PackageVersionMismatch,
            "package.version",
            package.version.as_str(),
            request.package.version.as_str(),
        ),
    ] {
        if expected != actual {
            diagnostics.push(CompositionDiagnostic::new(
                kind,
                subject,
                None,
                Some(expected.to_owned()),
                Some(actual.to_owned()),
            ));
        }
    }
    diagnostics
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Resolved,
}

#[allow(clippy::too_many_arguments)]
fn visit(
    id: &str,
    required_by: Option<&str>,
    components: &BTreeMap<String, ComponentManifest>,
    state: &mut BTreeMap<String, VisitState>,
    stack: &mut Vec<String>,
    ordered: &mut Vec<String>,
    diagnostics: &mut Vec<CompositionDiagnostic>,
) {
    match state.get(id) {
        Some(VisitState::Resolved) => return,
        Some(VisitState::Visiting) => {
            let start = stack
                .iter()
                .position(|component| component == id)
                .unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(id.to_owned());
            diagnostics.push(CompositionDiagnostic::new(
                CompositionDiagnosticKind::DependencyCycle,
                id,
                Some(cycle.join(" -> ")),
                None,
                None,
            ));
            return;
        }
        None => {}
    }

    let Some(component) = components.get(id) else {
        diagnostics.push(CompositionDiagnostic::new(
            CompositionDiagnosticKind::MissingComponent,
            id,
            required_by.map(str::to_owned),
            None,
            None,
        ));
        return;
    };
    state.insert(id.to_owned(), VisitState::Visiting);
    stack.push(id.to_owned());
    for dependency in &component.requires {
        visit(
            dependency,
            Some(id),
            components,
            state,
            stack,
            ordered,
            diagnostics,
        );
    }
    stack.pop();
    state.insert(id.to_owned(), VisitState::Resolved);
    if !ordered.iter().any(|component| component == id) {
        ordered.push(id.to_owned());
    }
}

fn validate_recorded_state(
    modules: &[InstalledModule],
    capabilities: &BTreeSet<ApplicationCapability>,
    request: &CompositionRequest,
    diagnostics: &mut Vec<CompositionDiagnostic>,
) {
    if let Some(recorded) = &request.recorded_modules {
        let expected = modules
            .iter()
            .map(|module| (module.id.as_str(), module.version.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut actual = BTreeMap::new();
        for module in recorded {
            if actual
                .insert(module.id.as_str(), module.version.as_str())
                .is_some()
            {
                diagnostics.push(CompositionDiagnostic::new(
                    CompositionDiagnosticKind::DuplicateRecordedModule,
                    &module.id,
                    None,
                    None,
                    None,
                ));
            }
        }
        for (id, version) in &expected {
            match actual.get(id) {
                None => diagnostics.push(CompositionDiagnostic::new(
                    CompositionDiagnosticKind::MissingRecordedModule,
                    *id,
                    None,
                    Some((*version).to_owned()),
                    None,
                )),
                Some(actual_version) if actual_version != version => {
                    diagnostics.push(CompositionDiagnostic::new(
                        CompositionDiagnosticKind::ModuleVersionMismatch,
                        *id,
                        None,
                        Some((*version).to_owned()),
                        Some((*actual_version).to_owned()),
                    ));
                }
                Some(_) => {}
            }
        }
        for (id, version) in actual {
            if !expected.contains_key(id) {
                diagnostics.push(CompositionDiagnostic::new(
                    CompositionDiagnosticKind::UnexpectedRecordedModule,
                    id,
                    None,
                    None,
                    Some(version.to_owned()),
                ));
            }
        }
    }

    if let Some(recorded) = &request.recorded_capabilities
        && recorded != capabilities
    {
        diagnostics.push(CompositionDiagnostic::new(
            CompositionDiagnosticKind::RecordedCapabilityMismatch,
            "capabilities",
            None,
            Some(format_capabilities(capabilities)),
            Some(format_capabilities(recorded)),
        ));
    }
}

fn format_capabilities(capabilities: &BTreeSet<ApplicationCapability>) -> String {
    capabilities
        .iter()
        .map(|capability| capability_name(*capability))
        .collect::<Vec<_>>()
        .join(",")
}

fn capability_name(capability: ApplicationCapability) -> &'static str {
    match capability {
        ApplicationCapability::Authentication => "authentication",
        ApplicationCapability::Authorization => "authorization",
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().enumerate().all(|(index, character)| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || (index > 0 && matches!(character, b'-' | b'_'))
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::FrameworkDependency;

    fn framework() -> FrameworkContract {
        FrameworkContract {
            repository: "https://github.com/furkancemalcaliskan/hegira.git".to_owned(),
            version: "v0.5.0".to_owned(),
        }
    }

    fn package() -> ComponentPackageManifest {
        ComponentPackageManifest {
            schema: 2,
            id: "hegira-canonical".to_owned(),
            version: "v0.5.0".to_owned(),
            framework: framework(),
            templates: vec!["layered".to_owned()],
            components: vec!["base".to_owned(), "identity".to_owned()],
            modules: vec!["identity".to_owned()],
            content_digest: format!("sha256:{}", "0".repeat(64)),
        }
    }

    fn component(id: &str) -> ComponentManifest {
        ComponentManifest {
            schema: 2,
            id: id.to_owned(),
            version: Some("v0.5.0".to_owned()),
            source: PathBuf::from("applications/layered"),
            include: vec![PathBuf::from(format!("{id}.txt"))],
            requires: Vec::new(),
            conflicts: Vec::new(),
            optional_dependencies: Vec::new(),
            modules: Vec::new(),
            provides_capabilities: Vec::new(),
            requires_capabilities: Vec::new(),
            framework_dependencies: Vec::<FrameworkDependency>::new(),
            manifest_path: PathBuf::new(),
        }
    }

    fn components() -> BTreeMap<String, ComponentManifest> {
        let mut base = component("base");
        base.optional_dependencies = vec!["identity".to_owned()];
        let mut identity = component("identity");
        identity.requires = vec!["base".to_owned()];
        identity.modules = vec!["identity".to_owned()];
        identity.provides_capabilities = vec![
            ApplicationCapability::Authentication,
            ApplicationCapability::Authorization,
        ];
        [(base.id.clone(), base), (identity.id.clone(), identity)]
            .into_iter()
            .collect()
    }

    fn request(roots: impl IntoIterator<Item = &'static str>) -> CompositionRequest {
        CompositionRequest::new(
            framework(),
            PackageIdentity {
                id: "hegira-canonical".to_owned(),
                version: "v0.5.0".to_owned(),
            },
            roots.into_iter().map(str::to_owned),
        )
    }

    fn diagnostic_kinds(error: &CompositionError) -> BTreeSet<CompositionDiagnosticKind> {
        error
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.kind)
            .collect()
    }

    #[test]
    fn equivalent_root_order_produces_byte_equivalent_canonical_graphs() {
        let package = package();
        let components = components();
        let first = resolve(&package, &components, &request(["identity", "base"]))
            .expect("canonical graph should resolve");
        let second = resolve(&package, &components, &request(["base", "identity"]))
            .expect("reordered graph should resolve");

        assert_eq!(first, second);
        assert_eq!(first.to_toml().unwrap(), second.to_toml().unwrap());
        assert_eq!(
            first
                .components
                .iter()
                .map(|component| component.id.as_str())
                .collect::<Vec<_>>(),
            ["base", "identity"]
        );
        assert_eq!(first.modules[0].id, "identity");
        assert_eq!(first.capabilities.len(), 2);
    }

    #[test]
    fn canonical_graph_matches_the_stable_machine_snapshot() {
        let graph = resolve(&package(), &components(), &request(["identity", "base"]))
            .expect("canonical graph should resolve");

        assert_eq!(
            graph.to_toml().expect("graph should serialize"),
            include_str!("../tests/snapshots/composition-canonical.toml")
        );
    }

    #[test]
    fn required_and_optional_dependencies_have_distinct_resolution_behavior() {
        let package = package();
        let components = components();
        let base = resolve(&package, &components, &request(["base"]))
            .expect("an unselected optional dependency should not be installed");
        let identity = resolve(&package, &components, &request(["identity"]))
            .expect("required dependencies should be installed");

        assert_eq!(base.components.len(), 1);
        assert!(base.modules.is_empty());
        assert!(base.capabilities.is_empty());
        assert_eq!(identity.components.len(), 2);
    }

    #[test]
    fn invalid_graph_classes_return_typed_sorted_diagnostics() {
        let package = package();
        let mut components = components();
        components.get_mut("base").unwrap().requires = vec!["identity".to_owned()];
        components.get_mut("base").unwrap().conflicts = vec!["identity".to_owned()];
        components.get_mut("base").unwrap().requires_capabilities =
            vec![ApplicationCapability::Authorization];
        components
            .get_mut("identity")
            .unwrap()
            .provides_capabilities
            .clear();

        let error = resolve(&package, &components, &request(["identity"]))
            .expect_err("cycles, conflicts, and missing capabilities must fail");
        let kinds = diagnostic_kinds(&error);

        assert!(kinds.contains(&CompositionDiagnosticKind::DependencyCycle));
        assert!(kinds.contains(&CompositionDiagnosticKind::ComponentConflict));
        assert!(kinds.contains(&CompositionDiagnosticKind::MissingCapability));
        assert!(
            error
                .diagnostics()
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );
    }

    #[test]
    fn missing_metadata_and_incompatible_recorded_state_fail_closed() {
        let package = package();
        let mut components = components();
        components.get_mut("base").unwrap().optional_dependencies = vec!["missing".to_owned()];
        components.get_mut("identity").unwrap().requires =
            vec!["base".to_owned(), "missing-required".to_owned()];
        let mut incompatible = request(["identity"]).with_recorded_state(
            [InstalledModule {
                id: "identity".to_owned(),
                version: "v0.4.0".to_owned(),
            }],
            [ApplicationCapability::Authentication],
        );
        incompatible.framework.version = "v0.4.0".to_owned();

        let error = resolve(&package, &components, &incompatible)
            .expect_err("untrusted incompatible state must fail closed");
        let kinds = diagnostic_kinds(&error);

        for expected in [
            CompositionDiagnosticKind::FrameworkVersionMismatch,
            CompositionDiagnosticKind::MissingComponent,
            CompositionDiagnosticKind::MissingOptionalDependency,
            CompositionDiagnosticKind::ModuleVersionMismatch,
            CompositionDiagnosticKind::RecordedCapabilityMismatch,
        ] {
            assert!(kinds.contains(&expected), "missing diagnostic {expected:?}");
        }
    }

    #[test]
    fn adversarial_metadata_returns_every_typed_rejection_without_sensitive_content() {
        const SOURCE_BODY: &str = "source-body-must-not-appear";
        const ENVIRONMENT_VALUE: &str = "environment-value-must-not-appear";

        let package = package();
        let mut components = components();
        let base = components.get_mut("base").unwrap();
        base.version = Some("v0.4.0".to_owned());
        base.source = PathBuf::from(SOURCE_BODY);
        base.include = vec![PathBuf::from(ENVIRONMENT_VALUE)];
        base.requires = vec!["identity".to_owned(), "missing-required".to_owned()];
        base.optional_dependencies = vec!["missing-optional".to_owned()];
        base.conflicts = vec!["identity".to_owned()];
        base.modules = vec!["identity".to_owned(), "undeclared".to_owned()];
        base.requires_capabilities = vec![ApplicationCapability::Authorization];
        components
            .get_mut("identity")
            .unwrap()
            .provides_capabilities
            .clear();

        let mut adversarial = request(["identity", "identity", "Invalid", "missing-root"])
            .with_recorded_state(
                [
                    InstalledModule {
                        id: "identity".to_owned(),
                        version: "v0.4.0".to_owned(),
                    },
                    InstalledModule {
                        id: "identity".to_owned(),
                        version: "v0.4.0".to_owned(),
                    },
                    InstalledModule {
                        id: "unexpected".to_owned(),
                        version: "v0.5.0".to_owned(),
                    },
                ],
                [ApplicationCapability::Authentication],
            );
        adversarial.framework.repository = "https://example.com/framework.git".to_owned();
        adversarial.framework.version = "v0.4.0".to_owned();
        adversarial.package.id = "other-package".to_owned();
        adversarial.package.version = "v0.4.0".to_owned();

        let error = resolve(&package, &components, &adversarial)
            .expect_err("adversarial composition must fail closed");
        let kinds = diagnostic_kinds(&error);

        for expected in [
            CompositionDiagnosticKind::InvalidComponent,
            CompositionDiagnosticKind::DuplicateComponent,
            CompositionDiagnosticKind::MissingComponent,
            CompositionDiagnosticKind::MissingOptionalDependency,
            CompositionDiagnosticKind::DependencyCycle,
            CompositionDiagnosticKind::ComponentConflict,
            CompositionDiagnosticKind::FrameworkRepositoryMismatch,
            CompositionDiagnosticKind::FrameworkVersionMismatch,
            CompositionDiagnosticKind::PackageIdentityMismatch,
            CompositionDiagnosticKind::PackageVersionMismatch,
            CompositionDiagnosticKind::ComponentVersionMismatch,
            CompositionDiagnosticKind::UndeclaredModule,
            CompositionDiagnosticKind::DuplicateModuleOwner,
            CompositionDiagnosticKind::DuplicateRecordedModule,
            CompositionDiagnosticKind::MissingRecordedModule,
            CompositionDiagnosticKind::UnexpectedRecordedModule,
            CompositionDiagnosticKind::ModuleVersionMismatch,
            CompositionDiagnosticKind::MissingCapability,
            CompositionDiagnosticKind::RecordedCapabilityMismatch,
        ] {
            assert!(kinds.contains(&expected), "missing diagnostic {expected:?}");
        }
        assert_eq!(
            error.to_string(),
            include_str!("../tests/snapshots/composition-invalid.txt").trim_end()
        );
        assert!(
            error
                .diagnostics()
                .windows(2)
                .all(|pair| pair[0] <= pair[1])
        );
        assert!(!error.to_string().contains(SOURCE_BODY));
        assert!(!error.to_string().contains(ENVIRONMENT_VALUE));
    }

    #[test]
    fn resolution_does_not_read_component_source_or_runtime_environment() {
        const UNAVAILABLE_SOURCE: &str = "/unavailable/private/component-source";
        const UNAVAILABLE_INCLUDE: &str = "runtime-environment-value";

        let package = package();
        let mut components = components();
        for component in components.values_mut() {
            component.source = PathBuf::from(UNAVAILABLE_SOURCE);
            component.include = vec![PathBuf::from(UNAVAILABLE_INCLUDE)];
        }

        let graph = resolve(&package, &components, &request(["identity"]))
            .expect("composition must use only loaded metadata");
        let output = graph.to_toml().expect("graph should serialize");

        assert!(!output.contains(UNAVAILABLE_SOURCE));
        assert!(!output.contains(UNAVAILABLE_INCLUDE));
    }
}
