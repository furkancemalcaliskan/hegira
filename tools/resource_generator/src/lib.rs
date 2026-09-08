//! Typed names and ownership for generated layered application resources.
//!
//! This crate accepts identities, not source fragments or paths. Emitters use
//! the resulting canonical names and application-relative paths to construct a
//! complete [`application_mutator::ChangePlan`].

use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
};

use application_mutator::ChangePath;

const MAX_IDENTITY_BYTES: usize = 64;

const RESERVED_IDENTITIES: &[&str] = &[
    "app",
    "application",
    "applicationcontracts",
    "audit",
    "config",
    "crate",
    "dashboard",
    "database",
    "domain",
    "domainshared",
    "hegira",
    "http",
    "identity",
    "infrastructure",
    "lib",
    "main",
    "operations",
    "presentation",
    "root",
    "routes",
    "security",
    "self",
    "server",
    "settings",
    "shared",
    "super",
    "web",
    "workeroperations",
];

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayerOwner {
    Domain,
    ApplicationContracts,
    Application,
    Infrastructure,
    Presentation,
    Web,
}

impl LayerOwner {
    pub const ALL: [Self; 6] = [
        Self::Domain,
        Self::ApplicationContracts,
        Self::Application,
        Self::Infrastructure,
        Self::Presentation,
        Self::Web,
    ];

    pub const fn package(self) -> &'static str {
        match self {
            Self::Domain => "app_domain",
            Self::ApplicationContracts => "app_application_contracts",
            Self::Application => "app_application",
            Self::Infrastructure => "app_infrastructure",
            Self::Presentation => "app_presentation",
            Self::Web => "app_web",
        }
    }

    pub const fn source_root(self) -> &'static str {
        match self {
            Self::Domain => "crates/domain/src",
            Self::ApplicationContracts => "crates/application_contracts/src",
            Self::Application => "crates/application/src",
            Self::Infrastructure => "crates/infrastructure/src",
            Self::Presentation => "crates/presentation/src",
            Self::Web => "apps/web/src",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayeredArtifactKind {
    Domain,
    ApplicationContracts,
    Application,
    Infrastructure,
    Presentation,
    Web,
}

impl LayeredArtifactKind {
    pub const ALL: [Self; 6] = [
        Self::Domain,
        Self::ApplicationContracts,
        Self::Application,
        Self::Infrastructure,
        Self::Presentation,
        Self::Web,
    ];

    pub const fn owner(self) -> LayerOwner {
        match self {
            Self::Domain => LayerOwner::Domain,
            Self::ApplicationContracts => LayerOwner::ApplicationContracts,
            Self::Application => LayerOwner::Application,
            Self::Infrastructure => LayerOwner::Infrastructure,
            Self::Presentation => LayerOwner::Presentation,
            Self::Web => LayerOwner::Web,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PermissionAction {
    List,
    Read,
    Create,
    Update,
    Delete,
}

impl PermissionAction {
    pub const ALL: [Self; 5] = [
        Self::List,
        Self::Read,
        Self::Create,
        Self::Update,
        Self::Delete,
    ];

    const fn identifier(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Read => "read",
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccupiedNameOwner {
    Application,
    OfficialModule,
    ApplicationSource,
}

impl Display for OccupiedNameOwner {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Application => "the application identity",
            Self::OfficialModule => "an official module",
            Self::ApplicationSource => "existing application source",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactNamespace {
    occupied: BTreeMap<String, OccupiedNameOwner>,
}

impl ArtifactNamespace {
    pub fn new<A, M, S>(
        application: A,
        official_modules: impl IntoIterator<Item = M>,
        application_source_names: impl IntoIterator<Item = S>,
    ) -> Result<Self, NamingError>
    where
        A: AsRef<str>,
        M: AsRef<str>,
        S: AsRef<str>,
    {
        let mut namespace = Self {
            occupied: BTreeMap::new(),
        };
        namespace.insert(application.as_ref(), OccupiedNameOwner::Application)?;

        let mut official_modules = official_modules
            .into_iter()
            .map(|name| canonical_occupied_name(name.as_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        official_modules.sort();
        official_modules.dedup();
        for name in official_modules {
            namespace
                .occupied
                .entry(name)
                .or_insert(OccupiedNameOwner::OfficialModule);
        }

        let mut application_source_names = application_source_names
            .into_iter()
            .map(|name| canonical_occupied_name(name.as_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        application_source_names.sort();
        application_source_names.dedup();
        for name in application_source_names {
            namespace
                .occupied
                .entry(name)
                .or_insert(OccupiedNameOwner::ApplicationSource);
        }
        Ok(namespace)
    }

    fn insert(&mut self, name: &str, owner: OccupiedNameOwner) -> Result<(), NamingError> {
        self.occupied
            .entry(canonical_occupied_name(name)?)
            .or_insert(owner);
        Ok(())
    }

    fn owner(&self, canonical_name: &str) -> Option<OccupiedNameOwner> {
        self.occupied.get(canonical_name).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayeredNamingInput {
    singular_type: String,
    plural_type: Option<String>,
}

impl LayeredNamingInput {
    pub fn new(singular_type: impl Into<String>) -> Self {
        Self {
            singular_type: singular_type.into(),
            plural_type: None,
        }
    }

    pub fn with_plural_type(mut self, plural_type: impl Into<String>) -> Self {
        self.plural_type = Some(plural_type.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayeredArtifact {
    kind: LayeredArtifactKind,
    owner: LayerOwner,
    path: ChangePath,
}

impl LayeredArtifact {
    pub const fn kind(&self) -> LayeredArtifactKind {
        self.kind
    }

    pub const fn owner(&self) -> LayerOwner {
        self.owner
    }

    pub fn path(&self) -> &ChangePath {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayeredArtifactNames {
    singular_type: String,
    plural_type: String,
    rust_module: String,
    route_segment: String,
    database_table: String,
    permission_prefix: String,
    artifacts: Vec<LayeredArtifact>,
}

impl LayeredArtifactNames {
    pub fn resolve(
        input: LayeredNamingInput,
        namespace: &ArtifactNamespace,
    ) -> Result<Self, NamingError> {
        let singular_words = validate_rust_type("resource", &input.singular_type)?;
        let plural_type = input
            .plural_type
            .unwrap_or_else(|| format!("{}s", input.singular_type));
        let plural_words = validate_rust_type("plural resource", &plural_type)?;
        let singular_identity = singular_words.join("");
        let plural_identity = plural_words.join("");

        if singular_identity == plural_identity {
            return Err(NamingError::new(
                NamingErrorKind::InvalidIdentity,
                "singular and plural resource identities must be distinct",
            ));
        }
        reject_reserved_or_occupied(&singular_identity, namespace)?;
        reject_reserved_or_occupied(&plural_identity, namespace)?;

        let rust_module = singular_words.join("_");
        let route_segment = plural_words.join("-");
        let database_table = plural_words.join("_");
        let permission_prefix = route_segment.clone();
        validate_derived_identity(&rust_module, '_', "Rust module")?;
        validate_derived_identity(&route_segment, '-', "route")?;
        validate_derived_identity(&database_table, '_', "database")?;
        validate_derived_identity(&permission_prefix, '-', "permission")?;

        let artifacts = LayeredArtifactKind::ALL
            .into_iter()
            .map(|kind| {
                let owner = kind.owner();
                let path = ChangePath::new(format!("{}/{rust_module}.rs", owner.source_root()))
                    .map_err(|error| {
                        NamingError::new(
                            NamingErrorKind::InvalidPath,
                            format!("generated artifact path is invalid: {error}"),
                        )
                    })?;
                Ok(LayeredArtifact { kind, owner, path })
            })
            .collect::<Result<Vec<_>, NamingError>>()?;

        Ok(Self {
            singular_type: input.singular_type,
            plural_type,
            rust_module,
            route_segment,
            database_table,
            permission_prefix,
            artifacts,
        })
    }

    pub fn singular_type(&self) -> &str {
        &self.singular_type
    }

    pub fn plural_type(&self) -> &str {
        &self.plural_type
    }

    pub fn rust_module(&self) -> &str {
        &self.rust_module
    }

    pub fn route_segment(&self) -> &str {
        &self.route_segment
    }

    pub fn route_path(&self) -> String {
        format!("/api/{}", self.route_segment)
    }

    pub fn database_table(&self) -> &str {
        &self.database_table
    }

    pub fn permission(&self, action: PermissionAction) -> String {
        format!("{}.{}", self.permission_prefix, action.identifier())
    }

    pub fn permission_prefix(&self) -> &str {
        &self.permission_prefix
    }

    pub fn artifacts(&self) -> &[LayeredArtifact] {
        &self.artifacts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingErrorKind {
    InvalidIdentity,
    ReservedIdentity,
    Collision,
    InvalidPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamingError {
    kind: NamingErrorKind,
    message: String,
}

impl NamingError {
    fn new(kind: NamingErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> NamingErrorKind {
        self.kind
    }
}

impl Display for NamingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NamingError {}

fn validate_rust_type(label: &str, value: &str) -> Result<Vec<String>, NamingError> {
    if value.is_empty() || value.len() > MAX_IDENTITY_BYTES {
        return Err(NamingError::new(
            NamingErrorKind::InvalidIdentity,
            format!("{label} type must contain 1–{MAX_IDENTITY_BYTES} ASCII bytes"),
        ));
    }
    if !value.is_ascii() || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(NamingError::new(
            NamingErrorKind::InvalidIdentity,
            format!("{label} type must contain only ASCII letters and digits"),
        ));
    }
    if !value.as_bytes()[0].is_ascii_uppercase() {
        return Err(NamingError::new(
            NamingErrorKind::InvalidIdentity,
            format!("{label} type must use UpperCamelCase and start with an ASCII letter"),
        ));
    }

    let characters = value.chars().collect::<Vec<_>>();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, character) in characters.iter().copied().enumerate() {
        let uppercase = character.is_ascii_uppercase();
        let starts_word = uppercase
            && index != 0
            && (characters[index - 1].is_ascii_lowercase()
                || characters[index - 1].is_ascii_digit()
                || characters
                    .get(index + 1)
                    .is_some_and(char::is_ascii_lowercase));
        if starts_word {
            words.push(current);
            current = String::new();
        }
        current.push(character.to_ascii_lowercase());
    }
    words.push(current);

    let module = words.join("_");
    if RUST_KEYWORDS.contains(&module.as_str()) {
        return Err(NamingError::new(
            NamingErrorKind::ReservedIdentity,
            format!("{label} type resolves to reserved Rust identifier `{module}`"),
        ));
    }
    Ok(words)
}

fn canonical_occupied_name(value: &str) -> Result<String, NamingError> {
    if value.is_empty() || value.len() > MAX_IDENTITY_BYTES || !value.is_ascii() {
        return Err(NamingError::new(
            NamingErrorKind::InvalidIdentity,
            "occupied names must contain 1–64 ASCII bytes",
        ));
    }
    let valid_bytes = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    let starts_and_ends_with_identity = value
        .as_bytes()
        .first()
        .zip(value.as_bytes().last())
        .is_some_and(|(first, last)| first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric());
    let has_adjacent_separators = value
        .as_bytes()
        .windows(2)
        .any(|pair| matches!(pair, [b'-' | b'_', b'-' | b'_']));
    if !valid_bytes || !starts_and_ends_with_identity || has_adjacent_separators {
        return Err(NamingError::new(
            NamingErrorKind::InvalidIdentity,
            "occupied names must use ASCII letters and digits separated by single hyphens or underscores",
        ));
    }
    let canonical = value
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect::<String>();
    if canonical.is_empty() {
        return Err(NamingError::new(
            NamingErrorKind::InvalidIdentity,
            "occupied names must contain at least one ASCII letter or digit",
        ));
    }
    Ok(canonical)
}

fn reject_reserved_or_occupied(
    identity: &str,
    namespace: &ArtifactNamespace,
) -> Result<(), NamingError> {
    if RESERVED_IDENTITIES.contains(&identity) {
        return Err(NamingError::new(
            NamingErrorKind::ReservedIdentity,
            format!("resource identity `{identity}` is reserved by the canonical application"),
        ));
    }
    if let Some(owner) = namespace.owner(identity) {
        return Err(NamingError::new(
            NamingErrorKind::Collision,
            format!("resource identity `{identity}` collides with {owner}"),
        ));
    }
    Ok(())
}

fn validate_derived_identity(value: &str, separator: char, label: &str) -> Result<(), NamingError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_IDENTITY_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == u8::try_from(separator).expect("separator should be ASCII")
        })
        && !value.starts_with(separator)
        && !value.ends_with(separator)
        && !value.contains(&format!("{separator}{separator}"));
    if !valid {
        return Err(NamingError::new(
            NamingErrorKind::InvalidIdentity,
            format!("generated {label} identifier is invalid"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn namespace() -> ArtifactNamespace {
        ArtifactNamespace::new("my-application", ["identity"], ["existing_resource"]).unwrap()
    }

    #[test]
    fn one_input_resolves_every_layered_name_deterministically() {
        let first =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("OrderItem"), &namespace())
                .unwrap();
        let second =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("OrderItem"), &namespace())
                .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.singular_type(), "OrderItem");
        assert_eq!(first.plural_type(), "OrderItems");
        assert_eq!(first.rust_module(), "order_item");
        assert_eq!(first.route_segment(), "order-items");
        assert_eq!(first.route_path(), "/api/order-items");
        assert_eq!(first.database_table(), "order_items");
        assert_eq!(
            first.permission(PermissionAction::Create),
            "order-items.create"
        );
    }

    #[test]
    fn explicit_plural_avoids_hidden_natural_language_inference() {
        let names = LayeredArtifactNames::resolve(
            LayeredNamingInput::new("Person").with_plural_type("People"),
            &namespace(),
        )
        .unwrap();

        assert_eq!(names.plural_type(), "People");
        assert_eq!(names.route_segment(), "people");
        assert_eq!(names.database_table(), "people");
        assert_eq!(names.permission(PermissionAction::List), "people.list");
    }

    #[test]
    fn acronym_boundaries_have_one_deterministic_rust_and_route_spelling() {
        let names = LayeredArtifactNames::resolve(
            LayeredNamingInput::new("URLValue").with_plural_type("URLValues"),
            &namespace(),
        )
        .unwrap();

        assert_eq!(names.rust_module(), "url_value");
        assert_eq!(names.route_segment(), "url-values");
        assert_eq!(names.database_table(), "url_values");
    }

    #[test]
    fn every_artifact_has_exactly_one_canonical_owner_and_safe_path() {
        let names =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("OrderItem"), &namespace())
                .unwrap();
        let owners = names
            .artifacts()
            .iter()
            .map(LayeredArtifact::owner)
            .collect::<BTreeSet<_>>();

        assert_eq!(names.artifacts().len(), LayeredArtifactKind::ALL.len());
        assert_eq!(owners, LayerOwner::ALL.into_iter().collect());
        for artifact in names.artifacts() {
            assert_eq!(artifact.owner(), artifact.kind().owner());
            assert!(artifact.path().as_str().ends_with("/order_item.rs"));
            assert!(!artifact.owner().package().contains("hegira"));
        }
    }

    #[test]
    fn canonical_owner_packages_and_paths_match_the_layered_application() {
        let expected = [
            (LayerOwner::Domain, "app_domain", "crates/domain/src"),
            (
                LayerOwner::ApplicationContracts,
                "app_application_contracts",
                "crates/application_contracts/src",
            ),
            (
                LayerOwner::Application,
                "app_application",
                "crates/application/src",
            ),
            (
                LayerOwner::Infrastructure,
                "app_infrastructure",
                "crates/infrastructure/src",
            ),
            (
                LayerOwner::Presentation,
                "app_presentation",
                "crates/presentation/src",
            ),
            (LayerOwner::Web, "app_web", "apps/web/src"),
        ];

        for (owner, package, source_root) in expected {
            assert_eq!(owner.package(), package);
            assert_eq!(owner.source_root(), source_root);

            let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(std::path::Path::parent)
                .unwrap();
            let manifest = repository
                .join("templates/applications/layered")
                .join(source_root)
                .join("../Cargo.toml");
            let source = std::fs::read_to_string(std::fs::canonicalize(manifest).unwrap()).unwrap();
            assert!(source.contains(&format!("name = \"{package}\"")));
        }
    }

    #[test]
    fn invalid_rust_route_sql_and_path_shaped_inputs_fail_before_planning() {
        for value in [
            "",
            "product",
            "123Product",
            "Order_Item",
            "Order-Item",
            "Order/Item",
            "Order.Item",
            "Order Item",
            "Order\nItem",
            "Ürün",
        ] {
            let error = LayeredArtifactNames::resolve(LayeredNamingInput::new(value), &namespace())
                .unwrap_err();
            assert_eq!(error.kind(), NamingErrorKind::InvalidIdentity, "{value:?}");
        }
    }

    #[test]
    fn rust_and_application_reserved_names_are_rejected() {
        let rust = LayeredArtifactNames::resolve(LayeredNamingInput::new("Type"), &namespace())
            .unwrap_err();
        let application =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("Dashboard"), &namespace())
                .unwrap_err();
        let integration_root =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("Lib"), &namespace())
                .unwrap_err();

        assert_eq!(rust.kind(), NamingErrorKind::ReservedIdentity);
        assert_eq!(application.kind(), NamingErrorKind::ReservedIdentity);
        assert_eq!(integration_root.kind(), NamingErrorKind::ReservedIdentity);
    }

    #[test]
    fn application_module_and_source_collisions_are_typed() {
        let application =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("MyApplication"), &namespace())
                .unwrap_err();
        let source = LayeredArtifactNames::resolve(
            LayeredNamingInput::new("ExistingResource"),
            &namespace(),
        )
        .unwrap_err();

        assert_eq!(application.kind(), NamingErrorKind::Collision);
        assert!(application.to_string().contains("application identity"));
        assert_eq!(source.kind(), NamingErrorKind::Collision);
        assert!(source.to_string().contains("existing application source"));
    }

    #[test]
    fn namespace_diagnostics_do_not_depend_on_input_order() {
        let first =
            ArtifactNamespace::new("shop", ["Accounts", "Identity"], ["Order", "User"]).unwrap();
        let second =
            ArtifactNamespace::new("shop", ["Identity", "Accounts"], ["User", "Order"]).unwrap();

        assert_eq!(first, second);
        let first_error =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("Accounts"), &first).unwrap_err();
        let second_error =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("Accounts"), &second)
                .unwrap_err();
        assert_eq!(first_error, second_error);
        assert!(first_error.to_string().contains("official module"));
    }

    #[test]
    fn permission_identifiers_are_valid_for_every_supported_action() {
        let names =
            LayeredArtifactNames::resolve(LayeredNamingInput::new("OrderItem"), &namespace())
                .unwrap();

        let permissions = PermissionAction::ALL
            .map(|action| names.permission(action))
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(permissions.len(), PermissionAction::ALL.len());
        assert!(permissions.iter().all(|permission| {
            permission
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || matches!(byte, b'-' | b'.'))
        }));
    }
}
