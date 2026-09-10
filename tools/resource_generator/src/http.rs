use std::fmt::{Display, Formatter, Write as _};

use application_mutator::{
    ChangePlan, FileCreation, PlannedFileChange, StructuredEditError, StructuredEditOutcome,
    StructuredFileEdit, plan_rust_managed_entry, plan_rust_module,
};

use crate::{LayerOwner, ResourceField, ResourceSpecification, ScalarType};

const PRESENTATION_ROOT: &str = "crates/presentation/src/lib.rs";
const INFRASTRUCTURE_SERVICES: &str = "crates/infrastructure/src/identity/services.rs";
const SERVER_SOURCE: &str = "apps/server/src/server.rs";

#[derive(Debug, Clone, Copy)]
pub struct HttpLayerSources<'a> {
    pub presentation_root: &'a [u8],
    pub infrastructure_resource: &'a [u8],
    pub infrastructure_services: &'a [u8],
    pub server_source: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedResourceHttp {
    plan: ChangePlan,
}

impl PlannedResourceHttp {
    pub fn plan(&self) -> &ChangePlan {
        &self.plan
    }

    pub fn into_plan(self) -> ChangePlan {
        self.plan
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpLayerErrorKind {
    StructuredEdit,
    ExistingRegistration,
    Planning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpLayerError {
    kind: HttpLayerErrorKind,
    message: String,
}

impl HttpLayerError {
    fn new(kind: HttpLayerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> HttpLayerErrorKind {
        self.kind
    }
}

impl Display for HttpLayerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for HttpLayerError {}

pub fn plan_resource_http(
    specification: &ResourceSpecification,
    sources: HttpLayerSources<'_>,
) -> Result<PlannedResourceHttp, HttpLayerError> {
    let module = specification.names().rust_module();
    let registration = plan_rust_module(PRESENTATION_ROOT, sources.presentation_root, module)
        .map_err(structured_edit_error)?;
    let StructuredEditOutcome::Planned {
        edit: presentation_root,
        ..
    } = registration
    else {
        return Err(existing(module, PRESENTATION_ROOT));
    };

    let infrastructure_resource_path = format!("crates/infrastructure/src/{module}.rs");
    let infrastructure_resource = managed_file_edit(
        &infrastructure_resource_path,
        sources.infrastructure_resource,
        [(
            "resource-http-authorization",
            authorization_source(specification),
        )],
        module,
    )?;
    let infrastructure_services = managed_file_edit(
        INFRASTRUCTURE_SERVICES,
        sources.infrastructure_services,
        [
            ("service-imports", service_import(specification)),
            ("service-type-aliases", service_alias(specification)),
            ("service-fields", service_field(specification)),
            ("service-init", service_initialization(specification)),
            ("service-factories", service_factory(specification)),
        ],
        module,
    )?;
    let server_source = managed_file_edit(
        SERVER_SOURCE,
        sources.server_source,
        [
            ("resource-bearer-routes", route_registration(specification)),
            (
                "resource-openapi-documents",
                openapi_registration(specification),
            ),
        ],
        module,
    )?;

    let presentation_path = specification
        .names()
        .artifacts()
        .iter()
        .find(|artifact| artifact.owner() == LayerOwner::Presentation)
        .expect("the validated resource has one Presentation artifact")
        .path();
    let presentation = FileCreation::new(
        presentation_path.as_str(),
        presentation_source(specification).into_bytes(),
    )
    .map(PlannedFileChange::from)
    .map_err(|error| HttpLayerError::new(HttpLayerErrorKind::Planning, error.to_string()))?;

    let plan = ChangePlan::new([
        PlannedFileChange::from(server_source),
        PlannedFileChange::from(infrastructure_resource),
        PlannedFileChange::from(infrastructure_services),
        presentation,
        PlannedFileChange::from(presentation_root),
    ])
    .map_err(|error| HttpLayerError::new(HttpLayerErrorKind::Planning, error.to_string()))?;
    Ok(PlannedResourceHttp { plan })
}

fn managed_file_edit<const N: usize>(
    path: &str,
    observed: &[u8],
    entries: [(&str, String); N],
    key: &str,
) -> Result<StructuredFileEdit, HttpLayerError> {
    let mut resulting = observed.to_vec();
    for (block, entry) in entries {
        match plan_rust_managed_entry(path, &resulting, block, key, &entry)
            .map_err(structured_edit_error)?
        {
            StructuredEditOutcome::Planned { edit, .. } => {
                resulting = edit.resulting_content().to_vec();
            }
            StructuredEditOutcome::AlreadyPresent { .. } => return Err(existing(key, path)),
        }
    }
    StructuredFileEdit::new(path, observed, resulting)
        .map_err(|error| HttpLayerError::new(HttpLayerErrorKind::Planning, error.to_string()))
}

fn existing(module: &str, path: &str) -> HttpLayerError {
    HttpLayerError::new(
        HttpLayerErrorKind::ExistingRegistration,
        format!("resource module `{module}` is already registered in `{path}`"),
    )
}

fn structured_edit_error(error: StructuredEditError) -> HttpLayerError {
    HttpLayerError::new(HttpLayerErrorKind::StructuredEdit, error.to_string())
}

fn authorization_source(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let entity = names.singular_type();
    let mut source = String::new();
    writeln!(source, "use crate::config::AppConfig;").unwrap();
    writeln!(
        source,
        "use crate::identity::{{IdentityRepositoryAdapter, authorization::RepositoryAuthorization, sessions::SessionRepositoryAdapter}};"
    )
    .unwrap();
    writeln!(
        source,
        "use crate::security::token_service::JwtTokenService;"
    )
    .unwrap();
    writeln!(source, "use identity_application::identity::authorization::{{AuthorizationService, CurrentUserProvider, TokenCurrentUserProvider}};").unwrap();
    writeln!(
        source,
        "use identity_application::shared::errors::ApplicationErrorKind;"
    )
    .unwrap();
    writeln!(
        source,
        "use identity_application_contracts::permissions::PermissionName;\n"
    )
    .unwrap();
    writeln!(source, "#[derive(Clone)]").unwrap();
    writeln!(source, "pub struct {entity}AuthorizationAdapter {{").unwrap();
    writeln!(source, "    current_users: TokenCurrentUserProvider<SessionRepositoryAdapter, IdentityRepositoryAdapter, JwtTokenService>,").unwrap();
    writeln!(
        source,
        "    authorization: RepositoryAuthorization<IdentityRepositoryAdapter>,"
    )
    .unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "impl {entity}AuthorizationAdapter {{").unwrap();
    writeln!(
        source,
        "    pub fn new(pool: DatabasePool, config: &AppConfig) -> Self {{"
    )
    .unwrap();
    writeln!(
        source,
        "        let repository = IdentityRepositoryAdapter::new(pool.clone());"
    )
    .unwrap();
    writeln!(source, "        let sessions = SessionRepositoryAdapter::from_database(config, pool).expect(\"failed to initialize resource authorization session store\");").unwrap();
    writeln!(source, "        let lifetime = chrono::Duration::seconds(config.sessions.max_lifetime_seconds as i64);").unwrap();
    writeln!(source, "        Self {{").unwrap();
    writeln!(source, "            current_users: TokenCurrentUserProvider::new(sessions, repository.clone(), JwtTokenService::new_with_lifetime(config.security.jwt_secret.clone(), lifetime)),").unwrap();
    writeln!(
        source,
        "            authorization: RepositoryAuthorization::new(repository),"
    )
    .unwrap();
    writeln!(source, "        }}").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(
        source,
        "impl {entity}Authorization for {entity}AuthorizationAdapter {{"
    )
    .unwrap();
    writeln!(source, "    async fn require(&self, actor: &str, permission: &'static str) -> Result<(), {entity}ServiceError> {{").unwrap();
    writeln!(source, "        let current_user = self.current_users.current_user(actor).await.map_err(resource_authorization_error)?;").unwrap();
    writeln!(source, "        self.authorization.require(&current_user, PermissionName(permission)).await.map_err(resource_authorization_error)").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "fn resource_authorization_error(error: identity_application::shared::errors::ApplicationError) -> {entity}ServiceError {{").unwrap();
    writeln!(source, "    match error.kind() {{").unwrap();
    writeln!(
        source,
        "        ApplicationErrorKind::Unauthorized => {entity}ServiceError::Unauthorized,"
    )
    .unwrap();
    writeln!(
        source,
        "        ApplicationErrorKind::Forbidden => {entity}ServiceError::Forbidden,"
    )
    .unwrap();
    writeln!(
        source,
        "        ApplicationErrorKind::Validation => {entity}ServiceError::InvalidInput,"
    )
    .unwrap();
    writeln!(
        source,
        "        ApplicationErrorKind::Conflict => {entity}ServiceError::Conflict,"
    )
    .unwrap();
    writeln!(
        source,
        "        ApplicationErrorKind::NotFound => {entity}ServiceError::NotFound,"
    )
    .unwrap();
    writeln!(source, "        ApplicationErrorKind::Infrastructure | ApplicationErrorKind::Unexpected => {entity}ServiceError::Persistence,").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}").unwrap();
    source
}

fn service_import(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    format!(
        "use crate::{}::{{Composed{}Service, {}AuthorizationAdapter, compose_{}_service}};",
        names.rust_module(),
        names.singular_type(),
        names.singular_type(),
        names.rust_module()
    )
}

fn service_alias(specification: &ResourceSpecification) -> String {
    let entity = specification.names().singular_type();
    format!(
        "pub type Application{entity}Service = Composed{entity}Service<{entity}AuthorizationAdapter>;"
    )
}

fn service_field(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    format!(
        "pub {}: Application{}Service,",
        names.rust_module(),
        names.singular_type()
    )
}

fn service_initialization(specification: &ResourceSpecification) -> String {
    let module = specification.names().rust_module();
    format!("{module}: {module}_service(pool.clone(), config),")
}

fn service_factory(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    format!(
        "pub fn {module}_service(pool: DatabasePool, config: &AppConfig) -> Application{entity}Service {{\n    let authorization = {entity}AuthorizationAdapter::new(pool.clone(), config);\n    compose_{module}_service(&pool, authorization).expect(\"selected resource database must match the application database\")\n}}"
    )
}

fn route_registration(specification: &ResourceSpecification) -> String {
    let module = specification.names().rust_module();
    format!(
        ".merge(app_presentation::{module}::bearer_api_routes(app_state.services.{module}.clone()))"
    )
}

fn openapi_registration(specification: &ResourceSpecification) -> String {
    let module = specification.names().rust_module();
    format!("let document = document.merge(app_presentation::{module}::openapi_document());")
}

fn presentation_source(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let route = names.route_path();
    let tag = split_type_words(entity).join(" ");
    let mut source =
        String::from("//! Generated Axum transport for an application-owned resource.\n\n");
    writeln!(source, "use app_application_contracts::{module}::{{").unwrap();
    writeln!(
        source,
        "    Create{entity}Input, Delete{entity}Input, Get{entity}Query, List{plural}Query,"
    )
    .unwrap();
    writeln!(source, "    {entity}Dto, {entity}Service, {entity}ServiceError, {plural}Response, Update{entity}Input,").unwrap();
    writeln!(source, "}};").unwrap();
    writeln!(source, "use axum::{{").unwrap();
    writeln!(source, "    Extension, Json, Router,").unwrap();
    writeln!(source, "    extract::{{Path, Query}},").unwrap();
    writeln!(source, "    http::StatusCode,").unwrap();
    writeln!(source, "    response::{{IntoResponse, Response}},").unwrap();
    writeln!(source, "    routing::get,").unwrap();
    writeln!(source, "}};").unwrap();
    writeln!(source, "use http_support::bearer::BearerToken;").unwrap();
    writeln!(source, "use serde::{{Deserialize, Serialize}};").unwrap();
    writeln!(source, "use uuid::Uuid;").unwrap();
    if uses_scalar(specification.fields(), ScalarType::DateTime) {
        writeln!(source, "use chrono::{{DateTime, Utc}};").unwrap();
    }
    source.push('\n');
    writeln!(source, "#[derive(Clone)]").unwrap();
    writeln!(
        source,
        "struct {entity}HttpState<Service> {{ service: Service }}\n"
    )
    .unwrap();
    writeln!(source, "#[derive(Debug, Serialize)]").unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(
        source,
        "struct ResourceErrorBody {{ code: &'static str, message: &'static str }}\n"
    )
    .unwrap();
    writeln!(source, "#[derive(Debug)]").unwrap();
    writeln!(source, "struct ResourceApiError({entity}ServiceError);\n").unwrap();
    write_error_response(&mut source, entity);
    write_update_request(&mut source, entity, specification.fields());
    writeln!(
        source,
        "pub fn bearer_api_routes<Service, S>(service: Service) -> Router<S>"
    )
    .unwrap();
    writeln!(source, "where").unwrap();
    writeln!(source, "    Service: {entity}Service + Clone + 'static,").unwrap();
    writeln!(source, "    S: Clone + Send + Sync + 'static,").unwrap();
    writeln!(source, "{{").unwrap();
    writeln!(source, "    Router::<S>::new()").unwrap();
    writeln!(source, "        .route(\"{route}\", get(list_{module}::<Service>).post(create_{module}::<Service>))").unwrap();
    writeln!(source, "        .route(\"{route}/{{id}}\", get(get_{module}::<Service>).put(update_{module}::<Service>).delete(delete_{module}::<Service>))").unwrap();
    writeln!(
        source,
        "        .layer(Extension({entity}HttpState {{ service }}))"
    )
    .unwrap();
    writeln!(source, "}}\n").unwrap();
    write_handlers(&mut source, specification, &tag);
    write_openapi(&mut source, specification, &tag);
    source
}

fn write_error_response(source: &mut String, entity: &str) {
    writeln!(source, "impl IntoResponse for ResourceApiError {{").unwrap();
    writeln!(source, "    fn into_response(self) -> Response {{").unwrap();
    writeln!(
        source,
        "        let (status, code, message) = match self.0 {{"
    )
    .unwrap();
    for (variant, status, code, message) in [
        (
            "Unauthorized",
            "UNAUTHORIZED",
            "auth:unauthorized",
            "unauthorized",
        ),
        ("Forbidden", "FORBIDDEN", "auth:forbidden", "forbidden"),
        (
            "NotFound",
            "NOT_FOUND",
            "resource:not_found",
            "resource not found",
        ),
        (
            "Conflict",
            "CONFLICT",
            "resource:conflict",
            "resource conflict",
        ),
        (
            "InvalidInput",
            "BAD_REQUEST",
            "resource:invalid_input",
            "invalid resource input",
        ),
        (
            "Persistence",
            "INTERNAL_SERVER_ERROR",
            "system:persistence_error",
            "internal server error",
        ),
    ] {
        writeln!(source, "            {entity}ServiceError::{variant} => (StatusCode::{status}, \"{code}\", \"{message}\"),").unwrap();
    }
    writeln!(source, "        }};").unwrap();
    writeln!(
        source,
        "        (status, Json(ResourceErrorBody {{ code, message }})).into_response()"
    )
    .unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();
}

fn write_update_request(source: &mut String, entity: &str, fields: &[ResourceField]) {
    writeln!(source, "#[derive(Debug, Deserialize)]").unwrap();
    writeln!(
        source,
        "#[cfg_attr(feature = \"openapi\", derive(utoipa::ToSchema))]"
    )
    .unwrap();
    writeln!(source, "struct Update{entity}Request {{").unwrap();
    write_fields(source, fields, 4);
    writeln!(source, "}}\n").unwrap();
}

fn write_handlers(source: &mut String, specification: &ResourceSpecification, tag: &str) {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let route = names.route_path();

    write_path_attribute(
        source,
        "get",
        &route,
        &format!("{plural}Response"),
        tag,
        None,
        None,
    );
    writeln!(source, "async fn list_{module}<Service>(Extension(state): Extension<{entity}HttpState<Service>>, BearerToken(actor): BearerToken, Query(query): Query<List{plural}Query>) -> Result<Json<{plural}Response>, ResourceApiError>").unwrap();
    writeln!(source, "where Service: {entity}Service + Clone + 'static,").unwrap();
    writeln!(
        source,
        "{{ state.service.list(&actor, query).await.map(Json).map_err(ResourceApiError) }}\n"
    )
    .unwrap();

    write_path_attribute(
        source,
        "get",
        &format!("{route}/{{id}}"),
        &format!("{entity}Dto"),
        tag,
        Some("id"),
        None,
    );
    writeln!(source, "async fn get_{module}<Service>(Extension(state): Extension<{entity}HttpState<Service>>, BearerToken(actor): BearerToken, Path(id): Path<Uuid>) -> Result<Json<{entity}Dto>, ResourceApiError>").unwrap();
    writeln!(source, "where Service: {entity}Service + Clone + 'static,").unwrap();
    writeln!(source, "{{ state.service.get(&actor, Get{entity}Query {{ id }}).await.map(Json).map_err(ResourceApiError) }}\n").unwrap();

    write_path_attribute(
        source,
        "post",
        &route,
        &format!("{entity}Dto"),
        tag,
        None,
        Some(&format!("Create{entity}Input")),
    );
    writeln!(source, "async fn create_{module}<Service>(Extension(state): Extension<{entity}HttpState<Service>>, BearerToken(actor): BearerToken, Json(input): Json<Create{entity}Input>) -> Result<(StatusCode, Json<{entity}Dto>), ResourceApiError>").unwrap();
    writeln!(source, "where Service: {entity}Service + Clone + 'static,").unwrap();
    writeln!(source, "{{ state.service.create(&actor, input).await.map(|value| (StatusCode::CREATED, Json(value))).map_err(ResourceApiError) }}\n").unwrap();

    write_path_attribute(
        source,
        "put",
        &format!("{route}/{{id}}"),
        &format!("{entity}Dto"),
        tag,
        Some("id"),
        Some(&format!("Update{entity}Request")),
    );
    writeln!(source, "async fn update_{module}<Service>(Extension(state): Extension<{entity}HttpState<Service>>, BearerToken(actor): BearerToken, Path(id): Path<Uuid>, Json(input): Json<Update{entity}Request>) -> Result<Json<{entity}Dto>, ResourceApiError>").unwrap();
    writeln!(source, "where Service: {entity}Service + Clone + 'static,").unwrap();
    writeln!(source, "{{").unwrap();
    writeln!(
        source,
        "    state.service.update(&actor, Update{entity}Input {{ id,"
    )
    .unwrap();
    for field in specification.fields() {
        writeln!(source, "        {0}: input.{0},", field.name()).unwrap();
    }
    writeln!(source, "    }}).await.map(Json).map_err(ResourceApiError)").unwrap();
    writeln!(source, "}}\n").unwrap();

    write_path_attribute(
        source,
        "delete",
        &format!("{route}/{{id}}"),
        "()",
        tag,
        Some("id"),
        None,
    );
    writeln!(source, "async fn delete_{module}<Service>(Extension(state): Extension<{entity}HttpState<Service>>, BearerToken(actor): BearerToken, Path(id): Path<Uuid>) -> Result<StatusCode, ResourceApiError>").unwrap();
    writeln!(source, "where Service: {entity}Service + Clone + 'static,").unwrap();
    writeln!(source, "{{ state.service.delete(&actor, Delete{entity}Input {{ id }}).await.map(|()| StatusCode::NO_CONTENT).map_err(ResourceApiError) }}\n").unwrap();
}

fn write_path_attribute(
    source: &mut String,
    method: &str,
    path: &str,
    response: &str,
    tag: &str,
    path_parameter: Option<&str>,
    request_body: Option<&str>,
) {
    writeln!(source, "#[cfg_attr(feature = \"openapi\", utoipa::path(").unwrap();
    writeln!(source, "    {method}, path = \"{path}\",").unwrap();
    if let Some(parameter) = path_parameter {
        writeln!(source, "    params((\"{parameter}\" = Uuid, Path)),").unwrap();
    }
    if let Some(request_body) = request_body {
        writeln!(source, "    request_body = {request_body},").unwrap();
    }
    let success = match method {
        "post" => "201",
        "delete" => "204",
        _ => "200",
    };
    if method == "delete" {
        writeln!(source, "    responses((status = {success}), (status = 400, body = ResourceErrorBody), (status = 401, body = ResourceErrorBody), (status = 403, body = ResourceErrorBody), (status = 404, body = ResourceErrorBody), (status = 409, body = ResourceErrorBody)),").unwrap();
    } else {
        writeln!(source, "    responses((status = {success}, body = {response}), (status = 400, body = ResourceErrorBody), (status = 401, body = ResourceErrorBody), (status = 403, body = ResourceErrorBody), (status = 404, body = ResourceErrorBody), (status = 409, body = ResourceErrorBody)),").unwrap();
    }
    writeln!(
        source,
        "    security((\"bearer_auth\" = [])), tag = \"{tag}\""
    )
    .unwrap();
    writeln!(source, "))]").unwrap();
}

fn write_openapi(source: &mut String, specification: &ResourceSpecification, tag: &str) {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    let plural = names.plural_type();
    writeln!(source, "#[cfg(feature = \"openapi\")]").unwrap();
    writeln!(source, "#[derive(utoipa::OpenApi)]").unwrap();
    writeln!(source, "#[openapi(").unwrap();
    writeln!(
        source,
        "    paths(list_{module}, get_{module}, create_{module}, update_{module}, delete_{module}),"
    )
    .unwrap();
    writeln!(source, "    components(schemas({entity}Dto, Create{entity}Input, Update{entity}Request, {plural}Response, ResourceErrorBody)),").unwrap();
    writeln!(
        source,
        "    tags((name = \"{tag}\", description = \"Generated {tag} resource operations\"))"
    )
    .unwrap();
    writeln!(source, ")]").unwrap();
    writeln!(source, "struct {entity}ApiDoc;\n").unwrap();
    writeln!(source, "#[cfg(feature = \"openapi\")]").unwrap();
    writeln!(
        source,
        "pub fn openapi_document() -> utoipa::openapi::OpenApi {{"
    )
    .unwrap();
    writeln!(source, "    use utoipa::OpenApi as _;").unwrap();
    writeln!(source, "    {entity}ApiDoc::openapi()").unwrap();
    writeln!(source, "}}").unwrap();
}

fn write_fields(source: &mut String, fields: &[ResourceField], indent: usize) {
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

fn split_type_words(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut starts = vec![0];
    for index in 1..bytes.len() {
        let previous = bytes[index - 1];
        let current = bytes[index];
        let next = bytes.get(index + 1).copied();
        if current.is_ascii_uppercase()
            && (previous.is_ascii_lowercase()
                || previous.is_ascii_digit()
                || next.is_some_and(|next| next.is_ascii_lowercase()))
        {
            starts.push(index);
        }
    }
    starts.push(bytes.len());
    starts
        .windows(2)
        .map(|range| value[range[0]..range[1]].to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use application_manifest::ApplicationManifest;
    use application_mutator::ChangeOperation;

    use super::*;
    use crate::{
        ArtifactNamespace, LayeredNamingInput, ResourceFieldInput, ResourceSpecificationInput,
    };

    const PRESENTATION: &[u8] =
        b"pub mod http;\n// hegira:generated-modules:start\n// hegira:generated-modules:end\n";
    const INFRASTRUCTURE_RESOURCE: &[u8] = b"use persistence::DatabasePool;\nuse app_application::order_item::OrderItemAuthorization;\nuse app_application_contracts::order_item::OrderItemServiceError;\n// hegira:resource-http-authorization\n// hegira:resource-http-authorization:end\n";
    const SERVICES: &[u8] = b"use persistence::DatabasePool;\nuse cache::CacheAdapter;\nuse crate::config::AppConfig;\n// hegira:service-imports\n// hegira:service-imports:end\n// hegira:service-type-aliases\n// hegira:service-type-aliases:end\nstruct AppServices {\n    // hegira:service-fields\n    // hegira:service-fields:end\n}\nfn services() {\n    let _ = AppServices {\n        // hegira:service-init\n        // hegira:service-init:end\n    };\n}\n// hegira:service-factories\n// hegira:service-factories:end\n";
    const SERVER: &[u8] = b"fn routes() {\n    let bearer_api_routes = bearer_api_routes\n        // hegira:resource-bearer-routes\n        // hegira:resource-bearer-routes:end\n        ;\n    let document = document();\n    // hegira:resource-openapi-documents\n    // hegira:resource-openapi-documents:end\n}\n";

    fn specification() -> ResourceSpecification {
        let manifest = ApplicationManifest::from_toml(
            r#"
schema = 1
application = "sample"
[framework]
repository = "https://example.invalid/hegira.git"
version = "v0.4.0"
[selection]
components = ["layered-base", "layered-leptos-identity"]
databases = ["sqlite"]
clients = ["leptos"]
"#,
        )
        .unwrap();
        ResourceSpecification::resolve(
            ResourceSpecificationInput::new(
                LayeredNamingInput::new("OrderItem"),
                [
                    ResourceFieldInput::new("active", "bool", false),
                    ResourceFieldInput::new("name", "string", false),
                    ResourceFieldInput::new("published_at", "datetime", true),
                ],
            ),
            &ArtifactNamespace::new("sample", ["identity"], std::iter::empty::<&str>()).unwrap(),
            &manifest,
        )
        .unwrap()
    }

    fn sources() -> HttpLayerSources<'static> {
        HttpLayerSources {
            presentation_root: PRESENTATION,
            infrastructure_resource: INFRASTRUCTURE_RESOURCE,
            infrastructure_services: SERVICES,
            server_source: SERVER,
        }
    }

    fn content<'a>(plan: &'a ChangePlan, path: &str) -> &'a str {
        let change = plan
            .changes()
            .iter()
            .find(|change| change.path().as_str() == path)
            .unwrap();
        std::str::from_utf8(change.resulting_content()).unwrap()
    }

    #[test]
    fn http_plan_is_deterministic_and_explicitly_composed() {
        let first = plan_resource_http(&specification(), sources()).unwrap();
        let second = plan_resource_http(&specification(), sources()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.plan().changes().len(), 5);

        let presentation = content(first.plan(), "crates/presentation/src/order_item.rs");
        syn::parse_file(presentation).unwrap_or_else(|error| panic!("{error}\n{presentation}"));
        assert!(presentation.contains("BearerToken(actor)"));
        assert!(presentation.contains("state.service.create(&actor, input)"));
        assert!(presentation.contains("security((\"bearer_auth\" = []))"));
        for forbidden in ["sqlx", "DatabasePool", "repository."] {
            assert!(!presentation.contains(forbidden));
        }

        let services = content(
            first.plan(),
            "crates/infrastructure/src/identity/services.rs",
        );
        assert!(services.contains("pub order_item: ApplicationOrderItemService"));
        assert!(services.contains("order_item_service(pool.clone(), config)"));
        syn::parse_file(services).unwrap();

        let infrastructure = content(first.plan(), "crates/infrastructure/src/order_item.rs");
        assert!(infrastructure.contains("RepositoryAuthorization"));
        assert!(!infrastructure.contains("CachedAuthorization"));
        assert!(infrastructure.contains("TokenCurrentUserProvider"));
        assert!(infrastructure.contains("self.authorization.require"));
        syn::parse_file(infrastructure).unwrap();

        let server = content(first.plan(), SERVER_SOURCE);
        assert!(server.contains("app_presentation::order_item::bearer_api_routes"));
        assert!(server.contains(
            "let document = document.merge(app_presentation::order_item::openapi_document())"
        ));
        syn::parse_file(server).unwrap();
    }

    #[test]
    fn generated_handlers_only_map_transport_and_delegate() {
        let planned = plan_resource_http(&specification(), sources()).unwrap();
        let source = content(planned.plan(), "crates/presentation/src/order_item.rs");
        for operation in ["list", "get", "create", "update", "delete"] {
            assert!(source.contains(&format!("state.service.{operation}(&actor")));
        }
        assert!(source.contains("UpdateOrderItemInput { id,"));
        assert!(source.contains("DeleteOrderItemInput { id }"));
        assert!(source.contains("StatusCode::NO_CONTENT"));
    }

    #[test]
    fn existing_or_malformed_registrations_fail_before_a_plan_exists() {
        let existing = HttpLayerSources {
            presentation_root: b"// hegira:generated-modules:start\npub mod order_item;\n// hegira:generated-modules:end\n",
            ..sources()
        };
        assert_eq!(
            plan_resource_http(&specification(), existing)
                .unwrap_err()
                .kind(),
            HttpLayerErrorKind::ExistingRegistration
        );

        let malformed = HttpLayerSources {
            server_source: b"fn routes() {}\n",
            ..sources()
        };
        assert_eq!(
            plan_resource_http(&specification(), malformed)
                .unwrap_err()
                .kind(),
            HttpLayerErrorKind::StructuredEdit
        );
    }

    #[test]
    fn plan_uses_absent_and_digest_preconditions() {
        let planned = plan_resource_http(&specification(), sources()).unwrap();
        assert!(planned.plan().changes().iter().any(|change| {
            change.path().as_str() == "crates/presentation/src/order_item.rs"
                && change.operation() == ChangeOperation::Create
        }));
        assert!(
            planned
                .plan()
                .changes()
                .iter()
                .filter(|change| { change.operation() == ChangeOperation::Edit })
                .count()
                == 4
        );
    }
}
