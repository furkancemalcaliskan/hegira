use std::fmt::{Display, Formatter, Write as _};

use application_mutator::{
    ChangePlan, FileCreation, PlannedFileChange, StructuredEditError, StructuredEditOutcome,
    plan_rust_module,
};

use crate::{LayerOwner, PermissionAction, ResourceField, ResourceSpecification, ScalarType};

const DOMAIN_ROOT: &str = "crates/domain/src/lib.rs";
const APPLICATION_CONTRACTS_ROOT: &str = "crates/application_contracts/src/lib.rs";
const APPLICATION_ROOT: &str = "crates/application/src/lib.rs";

#[derive(Debug, Clone, Copy)]
pub struct InwardLayerSources<'a> {
    pub domain_root: &'a [u8],
    pub application_contracts_root: &'a [u8],
    pub application_root: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedInwardLayers {
    plan: ChangePlan,
}

impl PlannedInwardLayers {
    pub fn plan(&self) -> &ChangePlan {
        &self.plan
    }

    pub fn into_plan(self) -> ChangePlan {
        self.plan
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InwardLayerErrorKind {
    StructuredEdit,
    ExistingRegistration,
    Planning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InwardLayerError {
    kind: InwardLayerErrorKind,
    message: String,
}

impl InwardLayerError {
    fn new(kind: InwardLayerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> InwardLayerErrorKind {
        self.kind
    }
}

impl Display for InwardLayerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InwardLayerError {}

pub fn plan_inward_resource_layers(
    specification: &ResourceSpecification,
    sources: InwardLayerSources<'_>,
) -> Result<PlannedInwardLayers, InwardLayerError> {
    let module = specification.names().rust_module();
    let mut changes = vec![
        create_layer(
            specification,
            LayerOwner::Domain,
            domain_source(specification),
        )?,
        create_layer(
            specification,
            LayerOwner::ApplicationContracts,
            contracts_source(specification),
        )?,
        create_layer(
            specification,
            LayerOwner::Application,
            application_source(specification),
        )?,
    ];
    for (path, observed) in [
        (DOMAIN_ROOT, sources.domain_root),
        (
            APPLICATION_CONTRACTS_ROOT,
            sources.application_contracts_root,
        ),
        (APPLICATION_ROOT, sources.application_root),
    ] {
        let outcome = plan_rust_module(path, observed, module).map_err(structured_edit_error)?;
        match outcome {
            StructuredEditOutcome::Planned { edit, .. } => {
                changes.push(PlannedFileChange::from(edit));
            }
            StructuredEditOutcome::AlreadyPresent { path, .. } => {
                return Err(InwardLayerError::new(
                    InwardLayerErrorKind::ExistingRegistration,
                    format!("resource module `{module}` is already registered in `{path}`"),
                ));
            }
        }
    }

    let plan = ChangePlan::new(changes).map_err(|error| {
        InwardLayerError::new(InwardLayerErrorKind::Planning, error.to_string())
    })?;
    Ok(PlannedInwardLayers { plan })
}

fn create_layer(
    specification: &ResourceSpecification,
    owner: LayerOwner,
    source: String,
) -> Result<PlannedFileChange, InwardLayerError> {
    let path = specification
        .names()
        .artifacts()
        .iter()
        .find(|artifact| artifact.owner() == owner)
        .expect("every inward layer has one validated artifact path")
        .path();
    FileCreation::new(path.as_str(), source.into_bytes())
        .map(PlannedFileChange::from)
        .map_err(|error| InwardLayerError::new(InwardLayerErrorKind::Planning, error.to_string()))
}

fn structured_edit_error(error: StructuredEditError) -> InwardLayerError {
    InwardLayerError::new(InwardLayerErrorKind::StructuredEdit, error.to_string())
}

fn domain_source(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let entity = names.singular_type();
    let id = format!("{entity}Id");
    let mut source = String::from(
        "//! Application-owned domain source. Add resource invariants and behavior here.\n\nuse uuid::Uuid;\n",
    );
    if uses_scalar(specification.fields(), ScalarType::DateTime) {
        source.push_str("use chrono::{DateTime, Utc};\n");
    }
    source.push('\n');
    writeln!(source, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]").unwrap();
    writeln!(source, "pub struct {id}(Uuid);\n").unwrap();
    writeln!(source, "impl {id} {{").unwrap();
    writeln!(source, "    pub const fn new(value: Uuid) -> Self {{").unwrap();
    writeln!(source, "        Self(value)").unwrap();
    writeln!(source, "    }}\n").unwrap();
    writeln!(source, "    pub const fn into_inner(self) -> Uuid {{").unwrap();
    writeln!(source, "        self.0").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "#[derive(Debug, Clone, PartialEq, Eq)]").unwrap();
    writeln!(source, "pub struct {entity} {{").unwrap();
    writeln!(source, "    pub id: {id},").unwrap();
    write_field_declarations(&mut source, specification.fields(), 4);
    writeln!(source, "}}\n").unwrap();
    source
}

fn contracts_source(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let fields = specification.fields();
    let mut source = String::from(
        "//! Application-owned, transport-neutral resource contracts.\n\nuse serde::{Deserialize, Serialize};\nuse uuid::Uuid;\n",
    );
    if uses_scalar(fields, ScalarType::DateTime) {
        source.push_str("use chrono::{DateTime, Utc};\n");
    }
    source.push('\n');
    for (constant, action) in [
        ("LIST", PermissionAction::List),
        ("READ", PermissionAction::Read),
        ("CREATE", PermissionAction::Create),
        ("UPDATE", PermissionAction::Update),
        ("DELETE", PermissionAction::Delete),
    ] {
        writeln!(
            source,
            "pub const {constant}_PERMISSION: &str = \"{}\";",
            names.permission(action)
        )
        .unwrap();
    }
    source.push('\n');
    writeln!(
        source,
        "#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "pub struct {entity}Dto {{").unwrap();
    writeln!(source, "    pub id: Uuid,").unwrap();
    write_field_declarations(&mut source, fields, 4);
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "pub struct Create{entity}Input {{").unwrap();
    write_field_declarations(&mut source, fields, 4);
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "pub struct Update{entity}Input {{").unwrap();
    writeln!(source, "    pub id: Uuid,").unwrap();
    write_field_declarations(&mut source, fields, 4);
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "pub struct Get{entity}Query {{").unwrap();
    writeln!(source, "    pub id: Uuid,").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "pub struct Delete{entity}Input {{").unwrap();
    writeln!(source, "    pub id: Uuid,").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "pub struct List{plural}Query {{").unwrap();
    writeln!(source, "    pub offset: u64,").unwrap();
    writeln!(source, "    pub limit: u32,").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]"
    )
    .unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "pub struct {plural}Response {{").unwrap();
    writeln!(source, "    pub items: Vec<{entity}Dto>,").unwrap();
    writeln!(source, "    pub total: u64,").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]").unwrap();
    writeln!(source, "pub enum {entity}ServiceError {{").unwrap();
    for variant in [
        "Unauthorized",
        "Forbidden",
        "NotFound",
        "Conflict",
        "InvalidInput",
        "Persistence",
    ] {
        writeln!(source, "    {variant},").unwrap();
    }
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "pub trait {entity}Service: Send + Sync {{").unwrap();
    writeln!(source, "    fn list(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        actor: &str,").unwrap();
    writeln!(source, "        query: List{plural}Query,").unwrap();
    writeln!(source, "    ) -> impl std::future::Future<Output = Result<{plural}Response, {entity}ServiceError>> + Send;").unwrap();
    writeln!(source, "    fn get(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        actor: &str,").unwrap();
    writeln!(source, "        query: Get{entity}Query,").unwrap();
    writeln!(source, "    ) -> impl std::future::Future<Output = Result<{entity}Dto, {entity}ServiceError>> + Send;").unwrap();
    writeln!(source, "    fn create(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        actor: &str,").unwrap();
    writeln!(source, "        input: Create{entity}Input,").unwrap();
    writeln!(source, "    ) -> impl std::future::Future<Output = Result<{entity}Dto, {entity}ServiceError>> + Send;").unwrap();
    writeln!(source, "    fn update(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        actor: &str,").unwrap();
    writeln!(source, "        input: Update{entity}Input,").unwrap();
    writeln!(source, "    ) -> impl std::future::Future<Output = Result<{entity}Dto, {entity}ServiceError>> + Send;").unwrap();
    writeln!(source, "    fn delete(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        actor: &str,").unwrap();
    writeln!(source, "        input: Delete{entity}Input,").unwrap();
    writeln!(
        source,
        "    ) -> impl std::future::Future<Output = Result<(), {entity}ServiceError>> + Send;"
    )
    .unwrap();
    writeln!(source, "}}").unwrap();
    source
}

fn application_source(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let module = names.rust_module();
    let fields = specification.fields();
    let mut source =
        String::from("//! Application-owned resource use cases and outward-facing ports.\n\n");
    writeln!(source, "use app_application_contracts::{module}::{{").unwrap();
    writeln!(
        source,
        "    CREATE_PERMISSION, Create{entity}Input, DELETE_PERMISSION, Delete{entity}Input,"
    )
    .unwrap();
    writeln!(
        source,
        "    Get{entity}Query, LIST_PERMISSION, List{plural}Query, READ_PERMISSION, {entity}Dto,"
    )
    .unwrap();
    writeln!(
        source,
        "    {entity}Service, {entity}ServiceError, {plural}Response, UPDATE_PERMISSION,"
    )
    .unwrap();
    writeln!(source, "    Update{entity}Input,").unwrap();
    writeln!(source, "}};").unwrap();
    writeln!(
        source,
        "use app_domain::{module}::{{{entity}, {entity}Id}};\n"
    )
    .unwrap();
    writeln!(source, "pub trait {entity}Repository: Send + Sync {{").unwrap();
    writeln!(source, "    fn list(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        query: &List{plural}Query,").unwrap();
    writeln!(source, "    ) -> impl std::future::Future<Output = Result<(Vec<{entity}>, u64), {entity}ServiceError>> + Send;").unwrap();
    writeln!(source, "    fn find(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        id: {entity}Id,").unwrap();
    writeln!(source, "    ) -> impl std::future::Future<Output = Result<Option<{entity}>, {entity}ServiceError>> + Send;").unwrap();
    for (method, output) in [
        ("insert", format!("Result<{entity}, {entity}ServiceError>")),
        (
            "update",
            format!("Result<Option<{entity}>, {entity}ServiceError>"),
        ),
    ] {
        writeln!(source, "    fn {method}(").unwrap();
        writeln!(source, "        &self,").unwrap();
        writeln!(source, "        entity: {entity},").unwrap();
        writeln!(
            source,
            "    ) -> impl std::future::Future<Output = {output}> + Send;"
        )
        .unwrap();
    }
    writeln!(source, "    fn delete(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        id: {entity}Id,").unwrap();
    writeln!(
        source,
        "    ) -> impl std::future::Future<Output = Result<bool, {entity}ServiceError>> + Send;"
    )
    .unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "pub trait {entity}Authorization: Send + Sync {{").unwrap();
    writeln!(source, "    fn require(").unwrap();
    writeln!(source, "        &self,").unwrap();
    writeln!(source, "        actor: &str,").unwrap();
    writeln!(source, "        permission: &'static str,").unwrap();
    writeln!(
        source,
        "    ) -> impl std::future::Future<Output = Result<(), {entity}ServiceError>> + Send;"
    )
    .unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "pub trait {entity}IdGenerator: Send + Sync {{").unwrap();
    writeln!(source, "    fn next_id(&self) -> {entity}Id;").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "#[derive(Debug, Clone)]").unwrap();
    writeln!(
        source,
        "pub struct {entity}AppService<Repository, Authorization, Ids> {{"
    )
    .unwrap();
    writeln!(source, "    repository: Repository,").unwrap();
    writeln!(source, "    authorization: Authorization,").unwrap();
    writeln!(source, "    ids: Ids,").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "impl<Repository, Authorization, Ids> {entity}AppService<Repository, Authorization, Ids> {{"
    )
    .unwrap();
    writeln!(
        source,
        "    pub fn new(repository: Repository, authorization: Authorization, ids: Ids) -> Self {{"
    )
    .unwrap();
    writeln!(source, "        Self {{ repository, authorization, ids }}").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "impl<Repository, Authorization, Ids> {entity}Service"
    )
    .unwrap();
    writeln!(
        source,
        "    for {entity}AppService<Repository, Authorization, Ids>"
    )
    .unwrap();
    writeln!(source, "where").unwrap();
    writeln!(source, "    Repository: {entity}Repository,").unwrap();
    writeln!(source, "    Authorization: {entity}Authorization,").unwrap();
    writeln!(source, "    Ids: {entity}IdGenerator,").unwrap();
    writeln!(source, "{{").unwrap();
    write_list_method(&mut source, entity, plural, module);
    write_get_method(&mut source, entity, module);
    write_create_method(&mut source, entity, module, fields);
    write_update_method(&mut source, entity, module, fields);
    write_delete_method(&mut source, entity);
    writeln!(source, "}}\n").unwrap();
    write_mapper(&mut source, entity, module, fields);
    source
}

fn write_list_method(source: &mut String, entity: &str, plural: &str, module: &str) {
    writeln!(source, "    async fn list(&self, actor: &str, query: List{plural}Query) -> Result<{plural}Response, {entity}ServiceError> {{").unwrap();
    writeln!(
        source,
        "        self.authorization.require(actor, LIST_PERMISSION).await?;"
    )
    .unwrap();
    writeln!(
        source,
        "        let (items, total) = self.repository.list(&query).await?;"
    )
    .unwrap();
    writeln!(source, "        Ok({plural}Response {{").unwrap();
    writeln!(
        source,
        "            items: items.into_iter().map({module}_dto).collect(),"
    )
    .unwrap();
    writeln!(source, "            total,").unwrap();
    writeln!(source, "        }})").unwrap();
    writeln!(source, "    }}\n").unwrap();
}

fn write_get_method(source: &mut String, entity: &str, module: &str) {
    writeln!(source, "    async fn get(&self, actor: &str, query: Get{entity}Query) -> Result<{entity}Dto, {entity}ServiceError> {{").unwrap();
    writeln!(
        source,
        "        self.authorization.require(actor, READ_PERMISSION).await?;"
    )
    .unwrap();
    writeln!(source, "        self.repository").unwrap();
    writeln!(source, "            .find({entity}Id::new(query.id))").unwrap();
    writeln!(source, "            .await?").unwrap();
    writeln!(source, "            .map({module}_dto)").unwrap();
    writeln!(source, "            .ok_or({entity}ServiceError::NotFound)").unwrap();
    writeln!(source, "    }}\n").unwrap();
}

fn write_create_method(source: &mut String, entity: &str, module: &str, fields: &[ResourceField]) {
    writeln!(source, "    async fn create(&self, actor: &str, input: Create{entity}Input) -> Result<{entity}Dto, {entity}ServiceError> {{").unwrap();
    writeln!(
        source,
        "        self.authorization.require(actor, CREATE_PERMISSION).await?;"
    )
    .unwrap();
    writeln!(source, "        let entity = {entity} {{").unwrap();
    writeln!(source, "            id: self.ids.next_id(),").unwrap();
    for field in fields {
        writeln!(source, "            {0}: input.{0},", field.name()).unwrap();
    }
    writeln!(source, "        }};").unwrap();
    writeln!(
        source,
        "        self.repository.insert(entity).await.map({module}_dto)"
    )
    .unwrap();
    writeln!(source, "    }}\n").unwrap();
}

fn write_update_method(source: &mut String, entity: &str, module: &str, fields: &[ResourceField]) {
    writeln!(source, "    async fn update(&self, actor: &str, input: Update{entity}Input) -> Result<{entity}Dto, {entity}ServiceError> {{").unwrap();
    writeln!(
        source,
        "        self.authorization.require(actor, UPDATE_PERMISSION).await?;"
    )
    .unwrap();
    writeln!(source, "        let entity = {entity} {{").unwrap();
    writeln!(source, "            id: {entity}Id::new(input.id),").unwrap();
    for field in fields {
        writeln!(source, "            {0}: input.{0},", field.name()).unwrap();
    }
    writeln!(source, "        }};").unwrap();
    writeln!(source, "        self.repository").unwrap();
    writeln!(source, "            .update(entity)").unwrap();
    writeln!(source, "            .await?").unwrap();
    writeln!(source, "            .map({module}_dto)").unwrap();
    writeln!(source, "            .ok_or({entity}ServiceError::NotFound)").unwrap();
    writeln!(source, "    }}\n").unwrap();
}

fn write_delete_method(source: &mut String, entity: &str) {
    writeln!(source, "    async fn delete(&self, actor: &str, input: Delete{entity}Input) -> Result<(), {entity}ServiceError> {{").unwrap();
    writeln!(
        source,
        "        self.authorization.require(actor, DELETE_PERMISSION).await?;"
    )
    .unwrap();
    writeln!(
        source,
        "        if self.repository.delete({entity}Id::new(input.id)).await? {{"
    )
    .unwrap();
    writeln!(source, "            Ok(())").unwrap();
    writeln!(source, "        }} else {{").unwrap();
    writeln!(source, "            Err({entity}ServiceError::NotFound)").unwrap();
    writeln!(source, "        }}").unwrap();
    writeln!(source, "    }}").unwrap();
}

fn write_mapper(source: &mut String, entity: &str, module: &str, fields: &[ResourceField]) {
    let mapper = format!("{module}_dto");
    writeln!(source, "fn {mapper}(entity: {entity}) -> {entity}Dto {{").unwrap();
    writeln!(source, "    {entity}Dto {{").unwrap();
    writeln!(source, "        id: entity.id.into_inner(),").unwrap();
    for field in fields {
        writeln!(source, "        {0}: entity.{0},", field.name()).unwrap();
    }
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}").unwrap();
}

fn write_field_declarations(source: &mut String, fields: &[ResourceField], indent: usize) {
    let padding = " ".repeat(indent);
    for field in fields {
        writeln!(
            source,
            "{padding}pub {}: {},",
            field.name(),
            rust_type(field.scalar(), field.nullable())
        )
        .unwrap();
    }
}

fn rust_type(scalar: ScalarType, nullable: bool) -> String {
    let base = match scalar {
        ScalarType::String => "String",
        ScalarType::Bool => "bool",
        ScalarType::I64 => "i64",
        ScalarType::Uuid => "Uuid",
        ScalarType::DateTime => "DateTime<Utc>",
    };
    if nullable {
        format!("Option<{base}>")
    } else {
        base.to_owned()
    }
}

fn uses_scalar(fields: &[ResourceField], scalar: ScalarType) -> bool {
    fields.iter().any(|field| field.scalar() == scalar)
}

#[cfg(test)]
mod tests {
    use application_manifest::ApplicationManifest;
    use application_mutator::{ChangeOperation, PlannedFileChange};

    use super::*;
    use crate::{
        ArtifactNamespace, LayeredNamingInput, ResourceFieldInput, ResourceSpecificationInput,
    };

    const ROOT: &[u8] = b"// hegira:generated-modules:start\n// hegira:generated-modules:end\n";

    fn specification() -> ResourceSpecification {
        specification_named("OrderItem")
    }

    fn specification_named(name: &str) -> ResourceSpecification {
        let manifest = ApplicationManifest::from_toml(
            r#"
schema = 1
application = "sample-app"

[framework]
repository = "https://example.invalid/hegira.git"
version = "v0.5.0"

[selection]
components = ["layered-base", "layered-leptos-identity"]
databases = ["sqlite"]
clients = ["leptos"]
"#,
        )
        .unwrap();
        ResourceSpecification::resolve(
            ResourceSpecificationInput::new(
                LayeredNamingInput::new(name),
                [
                    ResourceFieldInput::new("active", "bool", false),
                    ResourceFieldInput::new("description", "string", true),
                    ResourceFieldInput::new("published_at", "datetime", true),
                    ResourceFieldInput::new("quantity", "i64", false),
                    ResourceFieldInput::new("related_id", "uuid", false),
                ],
            ),
            &ArtifactNamespace::new("sample-app", ["identity"], Vec::<String>::new()).unwrap(),
            &manifest,
        )
        .unwrap()
    }

    fn source(change: &PlannedFileChange) -> &str {
        std::str::from_utf8(change.resulting_content()).unwrap()
    }

    #[test]
    fn inward_plan_is_deterministic_and_owns_only_three_layers() {
        let sources = InwardLayerSources {
            domain_root: ROOT,
            application_contracts_root: ROOT,
            application_root: ROOT,
        };
        let first = plan_inward_resource_layers(&specification(), sources).unwrap();
        let second = plan_inward_resource_layers(&specification(), sources).unwrap();
        assert_eq!(first, second);

        let summary = first.plan().summary();
        assert_eq!(summary.changes.len(), 6);
        assert_eq!(
            summary
                .changes
                .iter()
                .map(|change| (change.path.as_str(), change.operation))
                .collect::<Vec<_>>(),
            [
                ("crates/application/src/lib.rs", ChangeOperation::Edit),
                (
                    "crates/application/src/order_item.rs",
                    ChangeOperation::Create
                ),
                (
                    "crates/application_contracts/src/lib.rs",
                    ChangeOperation::Edit
                ),
                (
                    "crates/application_contracts/src/order_item.rs",
                    ChangeOperation::Create
                ),
                ("crates/domain/src/lib.rs", ChangeOperation::Edit),
                ("crates/domain/src/order_item.rs", ChangeOperation::Create),
            ]
        );
    }

    #[test]
    fn generated_source_is_transport_independent_and_syntactically_valid() {
        let planned = plan_inward_resource_layers(
            &specification(),
            InwardLayerSources {
                domain_root: ROOT,
                application_contracts_root: ROOT,
                application_root: ROOT,
            },
        )
        .unwrap();
        for change in planned.plan().changes() {
            let source = source(change);
            syn::parse_file(source).unwrap();
            for forbidden in ["axum", "leptos", "sqlx", "reqwest"] {
                assert!(
                    !source.contains(forbidden),
                    "{} contains {forbidden}",
                    change.path()
                );
            }
        }
    }

    #[test]
    fn generated_helpers_use_the_canonical_acronym_module_name() {
        let planned = plan_inward_resource_layers(
            &specification_named("URLValue"),
            InwardLayerSources {
                domain_root: ROOT,
                application_contracts_root: ROOT,
                application_root: ROOT,
            },
        )
        .unwrap();
        let application = planned
            .plan()
            .changes()
            .iter()
            .find(|change| change.path().as_str() == "crates/application/src/url_value.rs")
            .map(source)
            .unwrap();
        assert!(application.contains("fn url_value_dto"));
        assert!(!application.contains("u_r_l_value"));
    }

    #[test]
    fn contracts_and_application_require_explicit_authorization() {
        let planned = plan_inward_resource_layers(
            &specification(),
            InwardLayerSources {
                domain_root: ROOT,
                application_contracts_root: ROOT,
                application_root: ROOT,
            },
        )
        .unwrap();
        let contracts = planned
            .plan()
            .changes()
            .iter()
            .find(|change| {
                change
                    .path()
                    .as_str()
                    .ends_with("application_contracts/src/order_item.rs")
            })
            .map(source)
            .unwrap();
        assert!(contracts.contains("pub struct CreateOrderItemInput"));
        assert!(contracts.contains("pub struct ListOrderItemsQuery"));
        assert!(contracts.contains("pub trait OrderItemService"));
        assert!(contracts.contains("order-items.create"));
        assert!(contracts.contains("derive(utoipa::ToSchema)"));
        assert!(contracts.contains("Forbidden"));

        let application = planned
            .plan()
            .changes()
            .iter()
            .find(|change| {
                change
                    .path()
                    .as_str()
                    .ends_with("application/src/order_item.rs")
            })
            .map(source)
            .unwrap();
        assert!(application.contains("pub trait OrderItemRepository"));
        assert!(application.contains("pub trait OrderItemAuthorization"));
        assert!(application.contains("pub trait OrderItemIdGenerator"));
        for permission in [
            "LIST_PERMISSION",
            "READ_PERMISSION",
            "CREATE_PERMISSION",
            "UPDATE_PERMISSION",
            "DELETE_PERMISSION",
        ] {
            assert!(application.contains(&format!("require(actor, {permission}).await?")));
        }
        assert!(
            application
                .find("require(actor, CREATE_PERMISSION)")
                .unwrap()
                < application.find("repository.insert(entity)").unwrap()
        );
        assert!(
            application
                .find("require(actor, UPDATE_PERMISSION)")
                .unwrap()
                < application.find(".update(entity)").unwrap()
        );
        assert!(
            application
                .find("require(actor, DELETE_PERMISSION)")
                .unwrap()
                < application.find("repository.delete").unwrap()
        );
    }

    #[test]
    fn existing_or_conflicting_registrations_fail_without_an_overwrite_plan() {
        let existing = b"// hegira:generated-modules:start\npub mod order_item;\n// hegira:generated-modules:end\n";
        let error = plan_inward_resource_layers(
            &specification(),
            InwardLayerSources {
                domain_root: existing,
                application_contracts_root: ROOT,
                application_root: ROOT,
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), InwardLayerErrorKind::ExistingRegistration);

        let conflicting = b"pub mod order_item;\n// hegira:generated-modules:start\n// hegira:generated-modules:end\n";
        let error = plan_inward_resource_layers(
            &specification(),
            InwardLayerSources {
                domain_root: conflicting,
                application_contracts_root: ROOT,
                application_root: ROOT,
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), InwardLayerErrorKind::StructuredEdit);
    }
}
