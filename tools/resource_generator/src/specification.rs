use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    str::FromStr,
};

use application_manifest::{ApplicationManifest, ClientAdapter, DatabaseAdapter, ManifestError};
use serde::Serialize;

use crate::{ArtifactNamespace, LayeredArtifactNames, LayeredNamingInput, NamingError};

pub const RESOURCE_SPECIFICATION_SCHEMA: u32 = 1;
const MAX_FIELDS: usize = 64;

const RUST_FIELD_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "try", "type",
    "unsafe", "use", "where", "while",
];

const SQL_FIELD_KEYWORDS: &[&str] = &[
    "all",
    "and",
    "as",
    "asc",
    "between",
    "by",
    "case",
    "check",
    "column",
    "constraint",
    "create",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "exists",
    "false",
    "foreign",
    "from",
    "group",
    "having",
    "in",
    "index",
    "insert",
    "into",
    "is",
    "join",
    "key",
    "limit",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "primary",
    "references",
    "select",
    "set",
    "table",
    "then",
    "true",
    "union",
    "unique",
    "update",
    "user",
    "values",
    "when",
    "where",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum ScalarType {
    #[serde(rename = "string")]
    String,
    #[serde(rename = "bool")]
    Bool,
    #[serde(rename = "i64")]
    I64,
    #[serde(rename = "uuid")]
    Uuid,
    #[serde(rename = "datetime")]
    DateTime,
}

impl ScalarType {
    pub const ALL: [Self; 5] = [
        Self::String,
        Self::Bool,
        Self::I64,
        Self::Uuid,
        Self::DateTime,
    ];

    pub const fn identifier(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Bool => "bool",
            Self::I64 => "i64",
            Self::Uuid => "uuid",
            Self::DateTime => "datetime",
        }
    }
}

impl FromStr for ScalarType {
    type Err = SpecificationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "string" => Ok(Self::String),
            "bool" => Ok(Self::Bool),
            "i64" => Ok(Self::I64),
            "uuid" => Ok(Self::Uuid),
            "datetime" => Ok(Self::DateTime),
            _ => Err(SpecificationError::new(
                SpecificationErrorKind::UnsupportedScalar,
                "unsupported scalar type; expected one of string, bool, i64, uuid, or datetime",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceFieldInput {
    name: String,
    scalar: String,
    nullable: bool,
}

impl ResourceFieldInput {
    pub fn new(name: impl Into<String>, scalar: impl Into<String>, nullable: bool) -> Self {
        Self {
            name: name.into(),
            scalar: scalar.into(),
            nullable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpecificationInput {
    naming: LayeredNamingInput,
    fields: Vec<ResourceFieldInput>,
}

impl ResourceSpecificationInput {
    pub fn new(
        naming: LayeredNamingInput,
        fields: impl IntoIterator<Item = ResourceFieldInput>,
    ) -> Self {
        Self {
            naming,
            fields: fields.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectedDatabase {
    Postgres,
    Sqlite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectedClient {
    Leptos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResourceSelection {
    database: SelectedDatabase,
    client: SelectedClient,
}

impl ResourceSelection {
    pub const fn database(self) -> SelectedDatabase {
        self.database
    }

    pub const fn client(self) -> SelectedClient {
        self.client
    }

    fn from_manifest(manifest: &ApplicationManifest) -> Result<Self, SpecificationError> {
        manifest
            .validate()
            .map_err(SpecificationError::invalid_manifest)?;
        let database = exactly_one(&manifest.selection.databases, "database")?;
        let client = exactly_one(&manifest.selection.clients, "client")?;
        Ok(Self {
            database: match database {
                DatabaseAdapter::Postgres => SelectedDatabase::Postgres,
                DatabaseAdapter::Sqlite => SelectedDatabase::Sqlite,
            },
            client: match client {
                ClientAdapter::Leptos => SelectedClient::Leptos,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceIdentifier {
    name: &'static str,
    scalar: ScalarType,
    nullable: bool,
}

impl ResourceIdentifier {
    const fn canonical() -> Self {
        Self {
            name: "id",
            scalar: ScalarType::Uuid,
            nullable: false,
        }
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn scalar(&self) -> ScalarType {
        self.scalar
    }

    pub const fn nullable(&self) -> bool {
        self.nullable
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceField {
    name: String,
    scalar: ScalarType,
    nullable: bool,
}

impl ResourceField {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn scalar(&self) -> ScalarType {
        self.scalar
    }

    pub const fn nullable(&self) -> bool {
        self.nullable
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpecification {
    names: LayeredArtifactNames,
    selection: ResourceSelection,
    identifier: ResourceIdentifier,
    fields: Vec<ResourceField>,
}

impl ResourceSpecification {
    pub fn resolve(
        input: ResourceSpecificationInput,
        namespace: &ArtifactNamespace,
        manifest: &ApplicationManifest,
    ) -> Result<Self, SpecificationError> {
        let selection = ResourceSelection::from_manifest(manifest)?;
        let names = LayeredArtifactNames::resolve(input.naming, namespace)
            .map_err(SpecificationError::naming)?;
        if input.fields.is_empty() {
            return Err(SpecificationError::new(
                SpecificationErrorKind::InvalidCombination,
                "a resource requires at least one non-identifier field",
            ));
        }
        if input.fields.len() > MAX_FIELDS {
            return Err(SpecificationError::new(
                SpecificationErrorKind::InvalidCombination,
                format!("a resource may define at most {MAX_FIELDS} non-identifier fields"),
            ));
        }

        let mut field_inputs = BTreeMap::new();
        for input in input.fields {
            validate_field_name(&input.name)?;
            if input.name == "id" {
                return Err(SpecificationError::new(
                    SpecificationErrorKind::ReservedField,
                    "field `id` is reserved for the required non-null UUID identifier",
                ));
            }
            let name = input.name.clone();
            if field_inputs.insert(name.clone(), input).is_some() {
                return Err(SpecificationError::new(
                    SpecificationErrorKind::DuplicateField,
                    format!("resource field `{name}` is declared more than once"),
                ));
            }
        }
        let fields = field_inputs
            .into_values()
            .map(|input| {
                Ok(ResourceField {
                    name: input.name,
                    scalar: ScalarType::from_str(&input.scalar)?,
                    nullable: input.nullable,
                })
            })
            .collect::<Result<Vec<_>, SpecificationError>>()?;

        Ok(Self {
            names,
            selection,
            identifier: ResourceIdentifier::canonical(),
            fields,
        })
    }

    pub fn names(&self) -> &LayeredArtifactNames {
        &self.names
    }

    pub const fn selection(&self) -> ResourceSelection {
        self.selection
    }

    pub fn identifier(&self) -> &ResourceIdentifier {
        &self.identifier
    }

    pub fn fields(&self) -> &[ResourceField] {
        &self.fields
    }

    pub fn summary(&self) -> ResourceSpecificationSummary<'_> {
        ResourceSpecificationSummary {
            schema: RESOURCE_SPECIFICATION_SCHEMA,
            singular_type: self.names.singular_type(),
            plural_type: self.names.plural_type(),
            rust_module: self.names.rust_module(),
            route_segment: self.names.route_segment(),
            database_table: self.names.database_table(),
            permission_prefix: self.names.permission_prefix(),
            selection: self.selection,
            identifier: &self.identifier,
            fields: &self.fields,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceSpecificationSummary<'a> {
    pub schema: u32,
    pub singular_type: &'a str,
    pub plural_type: &'a str,
    pub rust_module: &'a str,
    pub route_segment: &'a str,
    pub database_table: &'a str,
    pub permission_prefix: &'a str,
    pub selection: ResourceSelection,
    pub identifier: &'a ResourceIdentifier,
    pub fields: &'a [ResourceField],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecificationErrorKind {
    Naming,
    InvalidManifest,
    UnsupportedSelection,
    InvalidField,
    ReservedField,
    DuplicateField,
    UnsupportedScalar,
    InvalidCombination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecificationError {
    kind: SpecificationErrorKind,
    message: String,
}

impl SpecificationError {
    fn new(kind: SpecificationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn naming(error: NamingError) -> Self {
        Self::new(SpecificationErrorKind::Naming, error.to_string())
    }

    fn invalid_manifest(error: ManifestError) -> Self {
        Self::new(
            SpecificationErrorKind::InvalidManifest,
            format!("cannot resolve resource generation context: {error}"),
        )
    }

    pub const fn kind(&self) -> SpecificationErrorKind {
        self.kind
    }
}

impl Display for SpecificationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SpecificationError {}

fn exactly_one<T: Copy + Ord>(
    selection: &BTreeSet<T>,
    label: &str,
) -> Result<T, SpecificationError> {
    if selection.len() != 1 {
        return Err(SpecificationError::new(
            SpecificationErrorKind::UnsupportedSelection,
            format!("resource generation requires exactly one selected {label} adapter"),
        ));
    }
    Ok(*selection
        .first()
        .expect("a one-element selection must have a first value"))
}

fn validate_field_name(name: &str) -> Result<(), SpecificationError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !name.as_bytes().windows(2).any(|pair| pair == b"__");
    if !valid {
        return Err(SpecificationError::new(
            SpecificationErrorKind::InvalidField,
            "field names must use 1–64 bytes of lowercase ASCII snake_case",
        ));
    }
    if RUST_FIELD_KEYWORDS.contains(&name) || SQL_FIELD_KEYWORDS.contains(&name) {
        return Err(SpecificationError::new(
            SpecificationErrorKind::ReservedField,
            format!("field name `{name}` is reserved by Rust or SQL"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use application_manifest::{
        APPLICATION_MANIFEST_SCHEMA, ApplicationSelection, FrameworkContract,
        HEGIRA_FRAMEWORK_REPOSITORY, LAYERED_BASE_COMPONENT, LAYERED_LEPTOS_IDENTITY_COMPONENT,
    };

    use super::*;

    fn manifest(database: DatabaseAdapter) -> ApplicationManifest {
        ApplicationManifest {
            schema: APPLICATION_MANIFEST_SCHEMA,
            application: "my-application".to_owned(),
            framework: FrameworkContract {
                repository: HEGIRA_FRAMEWORK_REPOSITORY.to_owned(),
                version: "v0.4.0".to_owned(),
            },
            selection: ApplicationSelection {
                components: [
                    LAYERED_BASE_COMPONENT.to_owned(),
                    LAYERED_LEPTOS_IDENTITY_COMPONENT.to_owned(),
                ]
                .into_iter()
                .collect(),
                databases: [database].into_iter().collect(),
                clients: [ClientAdapter::Leptos].into_iter().collect(),
            },
        }
    }

    fn namespace() -> ArtifactNamespace {
        ArtifactNamespace::new("my-application", ["identity"], ["dashboard"]).unwrap()
    }

    fn input(fields: impl IntoIterator<Item = ResourceFieldInput>) -> ResourceSpecificationInput {
        ResourceSpecificationInput::new(LayeredNamingInput::new("OrderItem"), fields)
    }

    #[test]
    fn inputs_resolve_into_one_immutable_typed_specification() {
        let specification = ResourceSpecification::resolve(
            input([
                ResourceFieldInput::new("quantity", "i64", false),
                ResourceFieldInput::new("description", "string", true),
                ResourceFieldInput::new("active", "bool", false),
                ResourceFieldInput::new("external_id", "uuid", true),
                ResourceFieldInput::new("available_at", "datetime", true),
            ]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap();

        assert_eq!(specification.names().rust_module(), "order_item");
        assert_eq!(
            specification.selection(),
            ResourceSelection {
                database: SelectedDatabase::Sqlite,
                client: SelectedClient::Leptos,
            }
        );
        assert_eq!(specification.identifier().name(), "id");
        assert_eq!(specification.identifier().scalar(), ScalarType::Uuid);
        assert!(!specification.identifier().nullable());
        assert_eq!(
            specification
                .fields()
                .iter()
                .map(ResourceField::name)
                .collect::<Vec<_>>(),
            [
                "active",
                "available_at",
                "description",
                "external_id",
                "quantity"
            ]
        );
        assert!(specification.fields()[2].nullable());
    }

    #[test]
    fn provider_and_client_are_resolved_only_from_the_manifest() {
        let sqlite = ResourceSpecification::resolve(
            input([ResourceFieldInput::new("name", "string", false)]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap();
        let postgres = ResourceSpecification::resolve(
            input([ResourceFieldInput::new("name", "string", false)]),
            &namespace(),
            &manifest(DatabaseAdapter::Postgres),
        )
        .unwrap();

        assert_eq!(sqlite.selection().database(), SelectedDatabase::Sqlite);
        assert_eq!(postgres.selection().database(), SelectedDatabase::Postgres);
        assert_eq!(sqlite.selection().client(), SelectedClient::Leptos);
    }

    #[test]
    fn specification_summary_is_versioned_and_deterministic() {
        let first = ResourceSpecification::resolve(
            input([
                ResourceFieldInput::new("quantity", "i64", false),
                ResourceFieldInput::new("name", "string", false),
            ]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap();
        let second = ResourceSpecification::resolve(
            input([
                ResourceFieldInput::new("name", "string", false),
                ResourceFieldInput::new("quantity", "i64", false),
            ]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap();

        let first = serde_json::to_string_pretty(&first.summary()).unwrap();
        let second = serde_json::to_string_pretty(&second.summary()).unwrap();
        assert_eq!(first, second);
        let value: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(value["schema"], RESOURCE_SPECIFICATION_SCHEMA);
        assert_eq!(value["identifier"]["name"], "id");
        assert_eq!(value["identifier"]["scalar"], "uuid");
        assert_eq!(value["selection"]["database"], "sqlite");
        assert_eq!(value["fields"][0]["name"], "name");
    }

    #[test]
    fn unsupported_scalars_and_injection_shaped_inputs_are_rejected() {
        let cases = [
            ResourceFieldInput::new("display_name", "Vec<String>", false),
            ResourceFieldInput::new("display-name", "string", false),
            ResourceFieldInput::new("name;drop_table", "string", false),
            ResourceFieldInput::new("../name", "string", false),
            ResourceFieldInput::new("name\nvalue", "string", false),
        ];

        for field in cases {
            let error = ResourceSpecification::resolve(
                input([field]),
                &namespace(),
                &manifest(DatabaseAdapter::Sqlite),
            )
            .unwrap_err();
            assert!(matches!(
                error.kind(),
                SpecificationErrorKind::UnsupportedScalar | SpecificationErrorKind::InvalidField
            ));
        }
    }

    #[test]
    fn duplicate_reserved_and_empty_fields_fail_before_planning() {
        let duplicate = ResourceSpecification::resolve(
            input([
                ResourceFieldInput::new("name", "string", false),
                ResourceFieldInput::new("name", "string", true),
            ]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap_err();
        let identifier = ResourceSpecification::resolve(
            input([ResourceFieldInput::new("id", "uuid", false)]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap_err();
        let sql = ResourceSpecification::resolve(
            input([ResourceFieldInput::new("select", "string", false)]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap_err();
        let empty = ResourceSpecification::resolve(
            input([]),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap_err();

        assert_eq!(duplicate.kind(), SpecificationErrorKind::DuplicateField);
        assert_eq!(identifier.kind(), SpecificationErrorKind::ReservedField);
        assert_eq!(sql.kind(), SpecificationErrorKind::ReservedField);
        assert_eq!(empty.kind(), SpecificationErrorKind::InvalidCombination);
    }

    #[test]
    fn invalid_or_ambiguous_manifest_selection_is_rejected() {
        let mut ambiguous = manifest(DatabaseAdapter::Sqlite);
        ambiguous
            .selection
            .databases
            .insert(DatabaseAdapter::Postgres);
        let ambiguous = ResourceSpecification::resolve(
            input([ResourceFieldInput::new("name", "string", false)]),
            &namespace(),
            &ambiguous,
        )
        .unwrap_err();

        let mut invalid = manifest(DatabaseAdapter::Sqlite);
        invalid.selection.clients.clear();
        let invalid = ResourceSpecification::resolve(
            input([ResourceFieldInput::new("name", "string", false)]),
            &namespace(),
            &invalid,
        )
        .unwrap_err();

        assert_eq!(
            ambiguous.kind(),
            SpecificationErrorKind::UnsupportedSelection
        );
        assert_eq!(invalid.kind(), SpecificationErrorKind::InvalidManifest);
    }

    #[test]
    fn resource_naming_failures_are_preserved_before_field_validation() {
        let error = ResourceSpecification::resolve(
            ResourceSpecificationInput::new(
                LayeredNamingInput::new("Identity"),
                [ResourceFieldInput::new("name", "string", false)],
            ),
            &namespace(),
            &manifest(DatabaseAdapter::Sqlite),
        )
        .unwrap_err();

        assert_eq!(error.kind(), SpecificationErrorKind::Naming);
        assert!(error.to_string().contains("reserved"));
    }

    #[test]
    fn every_supported_scalar_has_one_stable_input_and_serialized_identity() {
        for scalar in ScalarType::ALL {
            assert_eq!(ScalarType::from_str(scalar.identifier()).unwrap(), scalar);
            assert_eq!(
                serde_json::to_string(&scalar).unwrap(),
                format!("\"{}\"", scalar.identifier())
            );
        }
    }
}
