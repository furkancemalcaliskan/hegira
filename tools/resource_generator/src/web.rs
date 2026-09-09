use std::fmt::{Display, Formatter, Write as _};

use application_mutator::{
    ChangePlan, FileCreation, PlannedFileChange, StructuredEditError, StructuredEditOutcome,
    StructuredFileEdit, plan_rust_managed_entry, plan_rust_module,
};

use crate::{LayerOwner, ResourceField, ResourceSpecification, ScalarType, SelectedClient};

const WEB_ROOT: &str = "apps/web/src/lib.rs";
const WEB_ROUTES: &str = "apps/web/src/routes.rs";
const WEB_NAVIGATION: &str = "apps/web/src/app/navigation.rs";
const WEB_I18N: &str = "apps/web/src/shared/i18n/mod.rs";
const WEB_SIDEBAR: &str = "apps/web/src/app/sidebar.rs";
const SERVER_SOURCE: &str = "apps/server/src/server.rs";

#[derive(Debug, Clone, Copy)]
pub struct WebLayerSources<'a> {
    pub web_root: &'a [u8],
    pub routes: &'a [u8],
    pub navigation: &'a [u8],
    pub i18n: &'a [u8],
    pub sidebar: &'a [u8],
    pub server_source: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedResourceWeb {
    plan: ChangePlan,
}

impl PlannedResourceWeb {
    pub fn plan(&self) -> &ChangePlan {
        &self.plan
    }

    pub fn into_plan(self) -> ChangePlan {
        self.plan
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebLayerErrorKind {
    UnsupportedClient,
    StructuredEdit,
    ExistingRegistration,
    RouteConflict,
    LocalizationConflict,
    Planning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebLayerError {
    kind: WebLayerErrorKind,
    message: String,
}

impl WebLayerError {
    fn new(kind: WebLayerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> WebLayerErrorKind {
        self.kind
    }
}

impl Display for WebLayerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for WebLayerError {}

pub fn plan_resource_web(
    specification: &ResourceSpecification,
    sources: WebLayerSources<'_>,
) -> Result<PlannedResourceWeb, WebLayerError> {
    if specification.selection().client() != SelectedClient::Leptos {
        return Err(WebLayerError::new(
            WebLayerErrorKind::UnsupportedClient,
            "the selected client does not support Leptos resource generation",
        ));
    }

    reject_route_conflicts(specification, sources.routes, sources.navigation)?;
    reject_localization_conflicts(specification, sources.i18n)?;

    let module = specification.names().rust_module();
    let web_root = match plan_rust_module(WEB_ROOT, sources.web_root, module)
        .map_err(structured_edit_error)?
    {
        StructuredEditOutcome::Planned { edit, .. } => edit,
        StructuredEditOutcome::AlreadyPresent { .. } => return Err(existing(module, WEB_ROOT)),
    };

    let routes = managed_file_edit(
        WEB_ROUTES,
        sources.routes,
        [
            (
                "resource-routes-native",
                native_route_registration(specification),
            ),
            (
                "resource-routes-split",
                split_route_registration(specification),
            ),
        ],
        module,
    )?;
    let navigation = managed_file_edit(
        WEB_NAVIGATION,
        sources.navigation,
        [
            ("nav-icons", navigation_icon(specification)),
            ("nav-items", navigation_item(specification)),
            ("nav-titles", navigation_title(specification)),
        ],
        module,
    )?;
    let i18n = managed_file_edit(
        WEB_I18N,
        sources.i18n,
        [
            ("resource-i18n-keys", localization_keys(specification)),
            ("resource-i18n-en", english_localization(specification)),
            ("resource-i18n-tr", turkish_localization(specification)),
        ],
        module,
    )?;
    let sidebar = managed_file_edit(
        WEB_SIDEBAR,
        sources.sidebar,
        [("nav-icon-views", navigation_icon_view(specification))],
        module,
    )?;
    let server_source = managed_file_edit(
        SERVER_SOURCE,
        sources.server_source,
        [("resource-leptos-contexts", server_context(specification))],
        module,
    )?;

    let web_path = specification
        .names()
        .artifacts()
        .iter()
        .find(|artifact| artifact.owner() == LayerOwner::Web)
        .expect("the validated resource has one Web artifact")
        .path();
    let web_source =
        FileCreation::new(web_path.as_str(), leptos_source(specification).into_bytes())
            .map(PlannedFileChange::from)
            .map_err(|error| WebLayerError::new(WebLayerErrorKind::Planning, error.to_string()))?;

    let plan = ChangePlan::new([
        PlannedFileChange::from(server_source),
        PlannedFileChange::from(sidebar),
        PlannedFileChange::from(navigation),
        PlannedFileChange::from(routes),
        PlannedFileChange::from(i18n),
        web_source,
        PlannedFileChange::from(web_root),
    ])
    .map_err(|error| WebLayerError::new(WebLayerErrorKind::Planning, error.to_string()))?;
    Ok(PlannedResourceWeb { plan })
}

fn managed_file_edit<const N: usize>(
    path: &str,
    observed: &[u8],
    entries: [(&str, String); N],
    key: &str,
) -> Result<StructuredFileEdit, WebLayerError> {
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
        .map_err(|error| WebLayerError::new(WebLayerErrorKind::Planning, error.to_string()))
}

fn existing(module: &str, path: &str) -> WebLayerError {
    WebLayerError::new(
        WebLayerErrorKind::ExistingRegistration,
        format!("resource module `{module}` is already registered in `{path}`"),
    )
}

fn structured_edit_error(error: StructuredEditError) -> WebLayerError {
    WebLayerError::new(WebLayerErrorKind::StructuredEdit, error.to_string())
}

fn reject_route_conflicts(
    specification: &ResourceSpecification,
    routes: &[u8],
    navigation: &[u8],
) -> Result<(), WebLayerError> {
    let route = specification.names().route_segment();
    let route_source = String::from_utf8_lossy(routes);
    let navigation_source = String::from_utf8_lossy(navigation);
    if route_source.contains(&format!("StaticSegment(\"{route}\")"))
        || navigation_source.contains(&format!("href: \"/{route}\""))
        || navigation_source.contains(&format!("key: \"{route}\""))
    {
        return Err(WebLayerError::new(
            WebLayerErrorKind::RouteConflict,
            format!("web route `/{route}` is already occupied"),
        ));
    }
    Ok(())
}

fn reject_localization_conflicts(
    specification: &ResourceSpecification,
    i18n: &[u8],
) -> Result<(), WebLayerError> {
    let source = String::from_utf8_lossy(i18n);
    for key in localization_key_names(specification) {
        if source.lines().any(|line| line.trim() == format!("{key},")) {
            return Err(WebLayerError::new(
                WebLayerErrorKind::LocalizationConflict,
                format!("application localization key `{key}` is already occupied"),
            ));
        }
    }
    Ok(())
}

fn native_route_registration(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    format!(
        "<Route path=StaticSegment(\"{}\") view=crate::{}::{}Route/>",
        names.route_segment(),
        names.rust_module(),
        names.plural_type()
    )
}

fn split_route_registration(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    format!(
        "<Route path=StaticSegment(\"{}\") view={{Lazy::<crate::{}::{}LazyRoute>::new()}}/>",
        names.route_segment(),
        names.rust_module(),
        names.plural_type()
    )
}

fn navigation_icon(specification: &ResourceSpecification) -> String {
    format!("{},", specification.names().singular_type())
}

fn navigation_item(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    format!(
        "NavItem {{\n    key: \"{}\",\n    label: T::{},\n    href: \"/{}\",\n    icon: NavIcon::{},\n    permission: Some(permissions::PermissionName(app_application_contracts::{}::LIST_PERMISSION)),\n}},",
        names.route_segment(),
        names.plural_type(),
        names.route_segment(),
        names.singular_type(),
        names.rust_module(),
    )
}

fn navigation_title(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    format!(
        "\"/{}\" => T::{},",
        names.route_segment(),
        names.plural_type()
    )
}

fn navigation_icon_view(specification: &ResourceSpecification) -> String {
    format!(
        "NavIcon::{} => view! {{ <span class=\"inline-grid size-4 place-items-center text-xs\" aria-hidden=\"true\">\"◆\"</span> }}.into_any(),",
        specification.names().singular_type()
    )
}

fn server_context(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    format!(
        "provide_context(app_web::{}::{}LeptosServices::new(services.{}.clone()));",
        names.rust_module(),
        names.singular_type(),
        names.rust_module()
    )
}

fn localization_key_names(specification: &ResourceSpecification) -> Vec<String> {
    let names = specification.names();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let mut keys = vec![
        entity.to_owned(),
        plural.to_owned(),
        format!("New{entity}"),
        format!("Edit{entity}"),
        format!("Save{entity}"),
        format!("Delete{entity}"),
        format!("Empty{plural}"),
        format!("Loading{plural}"),
        format!("{entity}Saved"),
        format!("{entity}Deleted"),
        format!("{entity}SaveFailed"),
        format!("{entity}DeleteFailed"),
        format!("{plural}Actions"),
    ];
    keys.extend(
        specification
            .fields()
            .iter()
            .map(|field| format!("{entity}{}Field", upper_camel_field(field.name()))),
    );
    keys
}

fn localization_keys(specification: &ResourceSpecification) -> String {
    localization_key_names(specification)
        .into_iter()
        .map(|key| format!("{key},"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn english_localization(specification: &ResourceSpecification) -> String {
    localization(specification, false)
}

fn turkish_localization(specification: &ResourceSpecification) -> String {
    localization(specification, true)
}

fn localization(specification: &ResourceSpecification, turkish: bool) -> String {
    let names = specification.names();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let entity_label = humanize_type(entity);
    let plural_label = humanize_type(plural);
    let locale = if turkish { "Tr" } else { "En" };
    let values = if turkish {
        vec![
            (entity.to_owned(), entity_label.clone()),
            (plural.to_owned(), plural_label.clone()),
            (format!("New{entity}"), format!("Yeni {entity_label}")),
            (format!("Edit{entity}"), format!("{entity_label} düzenle")),
            (format!("Save{entity}"), "Kaydet".to_owned()),
            (format!("Delete{entity}"), "Sil".to_owned()),
            (
                format!("Empty{plural}"),
                format!("Henüz {plural_label} yok."),
            ),
            (format!("Loading{plural}"), "Yükleniyor...".to_owned()),
            (
                format!("{entity}Saved"),
                format!("{entity_label} kaydedildi"),
            ),
            (
                format!("{entity}Deleted"),
                format!("{entity_label} silindi"),
            ),
            (format!("{entity}SaveFailed"), "Kayıt başarısız".to_owned()),
            (
                format!("{entity}DeleteFailed"),
                "Silme başarısız".to_owned(),
            ),
            (format!("{plural}Actions"), "İşlemler".to_owned()),
        ]
    } else {
        vec![
            (entity.to_owned(), entity_label.clone()),
            (plural.to_owned(), plural_label.clone()),
            (format!("New{entity}"), format!("New {entity_label}")),
            (format!("Edit{entity}"), format!("Edit {entity_label}")),
            (format!("Save{entity}"), "Save".to_owned()),
            (format!("Delete{entity}"), "Delete".to_owned()),
            (format!("Empty{plural}"), format!("No {plural_label} yet.")),
            (format!("Loading{plural}"), "Loading...".to_owned()),
            (format!("{entity}Saved"), format!("{entity_label} saved")),
            (
                format!("{entity}Deleted"),
                format!("{entity_label} deleted"),
            ),
            (format!("{entity}SaveFailed"), "Save failed".to_owned()),
            (format!("{entity}DeleteFailed"), "Delete failed".to_owned()),
            (format!("{plural}Actions"), "Actions".to_owned()),
        ]
    };
    let mut lines = values
        .into_iter()
        .map(|(key, value)| format!("(Locale::{locale}, T::{key}) => \"{value}\","))
        .collect::<Vec<_>>();
    lines.extend(specification.fields().iter().map(|field| {
        let key = format!("{entity}{}Field", upper_camel_field(field.name()));
        let value = humanize_field(field.name());
        format!("(Locale::{locale}, T::{key}) => \"{value}\",")
    }));
    lines.join("\n")
}

fn leptos_source(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let mut source = String::from("//! Generated Leptos UI for an application-owned resource.\n\n");
    writeln!(source, "use app_application_contracts::{module}::{{").unwrap();
    writeln!(
        source,
        "    CREATE_PERMISSION, Create{entity}Input, DELETE_PERMISSION, Delete{entity}Input,"
    )
    .unwrap();
    writeln!(
        source,
        "    Get{entity}Query, LIST_PERMISSION, List{plural}Query, {entity}Dto,"
    )
    .unwrap();
    writeln!(
        source,
        "    {plural}Response, UPDATE_PERMISSION, Update{entity}Input,"
    )
    .unwrap();
    writeln!(source, "}};").unwrap();
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(
        source,
        "use app_application_contracts::{module}::{{{entity}Service, {entity}ServiceError}};"
    )
    .unwrap();
    writeln!(source, "use identity_leptos::identity_application_contracts::identity::permissions::PermissionName;").unwrap();
    writeln!(source, "use leptos::prelude::*;").unwrap();
    writeln!(source, "use leptos::task::spawn_local;").unwrap();
    writeln!(source, "#[cfg(feature = \"wasm-split\")]").unwrap();
    writeln!(source, "use leptos_router::{{LazyRoute, lazy_route}};").unwrap();
    writeln!(source, "use uuid::Uuid;").unwrap();
    if uses_scalar(specification.fields(), ScalarType::DateTime) {
        writeln!(source, "use chrono::{{DateTime, Utc}};").unwrap();
    }
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(
        source,
        "use std::{{future::Future, pin::Pin, sync::Arc}};\n"
    )
    .unwrap();
    writeln!(source, "use crate::{{").unwrap();
    writeln!(source, "    app::{{layout::WorkspaceRouteLayout, page::{{PageHeaderKey, PageSection}}, protected::RequirePermission}},").unwrap();
    writeln!(
        source,
        "    shared::{{feedback::toast::use_toast, i18n::{{T, use_i18n}}}},"
    )
    .unwrap();
    writeln!(source, "}};").unwrap();
    writeln!(
        source,
        "use identity_leptos::shared::authorization::PermissionGate;"
    )
    .unwrap();
    writeln!(source, "use leptos_support::mutation::MutationStatus;\n").unwrap();

    write_server_adapter(&mut source, specification);
    write_server_functions(&mut source, specification);
    write_form_parsers(&mut source, specification);
    write_form_values(&mut source, specification);
    write_resource_page(&mut source, specification);
    source
}

fn write_form_values(source: &mut String, specification: &ResourceSpecification) {
    let entity = specification.names().singular_type();
    writeln!(source, "struct {entity}FormValues {{").unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "    {}: {},",
            field.name(),
            rust_type(field.scalar(), field.nullable())
        )
        .unwrap();
    }
    writeln!(source, "}}\n").unwrap();
}

fn write_form_parsers(source: &mut String, specification: &ResourceSpecification) {
    let fields = specification.fields();
    if fields
        .iter()
        .any(|field| field.scalar() == ScalarType::String && field.nullable())
    {
        writeln!(
            source,
            "fn optional_string(value: String) -> Option<String> {{"
        )
        .unwrap();
        writeln!(source, "    (!value.is_empty()).then_some(value)").unwrap();
        writeln!(source, "}}\n").unwrap();
    }
    if fields.iter().any(|field| {
        !field.nullable()
            && field.scalar() != ScalarType::String
            && field.scalar() != ScalarType::Bool
    }) {
        writeln!(source, "fn parse_required<Value>(value: String, field: &str, kind: &str) -> Result<Value, String>").unwrap();
        writeln!(source, "where Value: std::str::FromStr,").unwrap();
        writeln!(source, "{{").unwrap();
        writeln!(
            source,
            "    value.parse().map_err(|_| format!(\"{{field}} must be a valid {{kind}} value\"))"
        )
        .unwrap();
        writeln!(source, "}}\n").unwrap();
    }
    if fields
        .iter()
        .any(|field| field.scalar() != ScalarType::String && field.nullable())
    {
        writeln!(source, "fn parse_optional<Value>(value: String, field: &str, kind: &str) -> Result<Option<Value>, String>").unwrap();
        writeln!(source, "where Value: std::str::FromStr,").unwrap();
        writeln!(source, "{{").unwrap();
        writeln!(source, "    if value.is_empty() {{ Ok(None) }} else {{ value.parse().map(Some).map_err(|_| format!(\"{{field}} must be a valid {{kind}} value\")) }}").unwrap();
        writeln!(source, "}}\n").unwrap();
    }
}

fn write_server_adapter(source: &mut String, specification: &ResourceSpecification) {
    let names = specification.names();
    let entity = names.singular_type();
    let plural = names.plural_type();
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(source, "#[doc(hidden)]").unwrap();
    writeln!(source, "pub trait {entity}ServerService: Send + Sync {{").unwrap();
    for (method, input, output) in [
        (
            "list",
            format!("List{plural}Query"),
            format!("{plural}Response"),
        ),
        ("get", format!("Get{entity}Query"), format!("{entity}Dto")),
        (
            "create",
            format!("Create{entity}Input"),
            format!("{entity}Dto"),
        ),
        (
            "update",
            format!("Update{entity}Input"),
            format!("{entity}Dto"),
        ),
        ("delete", format!("Delete{entity}Input"), "()".to_owned()),
    ] {
        writeln!(source, "    fn {method}<'a>(&'a self, actor: &'a str, input: {input}) -> Pin<Box<dyn Future<Output = Result<{output}, {entity}ServiceError>> + Send + 'a>>;").unwrap();
    }
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(source, "impl<Service> {entity}ServerService for Service").unwrap();
    writeln!(source, "where Service: {entity}Service + Send + Sync,").unwrap();
    writeln!(source, "{{").unwrap();
    for (method, input, output) in [
        (
            "list",
            format!("List{plural}Query"),
            format!("{plural}Response"),
        ),
        ("get", format!("Get{entity}Query"), format!("{entity}Dto")),
        (
            "create",
            format!("Create{entity}Input"),
            format!("{entity}Dto"),
        ),
        (
            "update",
            format!("Update{entity}Input"),
            format!("{entity}Dto"),
        ),
        ("delete", format!("Delete{entity}Input"), "()".to_owned()),
    ] {
        writeln!(source, "    fn {method}<'a>(&'a self, actor: &'a str, input: {input}) -> Pin<Box<dyn Future<Output = Result<{output}, {entity}ServiceError>> + Send + 'a>> {{").unwrap();
        writeln!(
            source,
            "        Box::pin({entity}Service::{method}(self, actor, input))"
        )
        .unwrap();
        writeln!(source, "    }}").unwrap();
    }
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(source, "#[derive(Clone)]").unwrap();
    writeln!(
        source,
        "pub struct {entity}LeptosServices {{ service: Arc<dyn {entity}ServerService> }}"
    )
    .unwrap();
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(source, "impl {entity}LeptosServices {{").unwrap();
    writeln!(source, "    pub fn new<Service>(service: Service) -> Self where Service: {entity}Service + Send + Sync + 'static {{").unwrap();
    writeln!(source, "        Self {{ service: Arc::new(service) }}").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(
        source,
        "fn resource_service() -> Arc<dyn {entity}ServerService> {{"
    )
    .unwrap();
    writeln!(
        source,
        "    leptos_support::server::context::<{entity}LeptosServices>().service"
    )
    .unwrap();
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "#[cfg(feature = \"ssr\")]").unwrap();
    writeln!(
        source,
        "fn server_error(error: {entity}ServiceError) -> ServerFnError {{"
    )
    .unwrap();
    writeln!(source, "    match error {{").unwrap();
    writeln!(source, "        {entity}ServiceError::Persistence => leptos_support::server::internal_error(\"resource persistence failure\"),").unwrap();
    writeln!(source, "        {entity}ServiceError::Unauthorized => leptos_support::server::public_error(\"unauthorized\"),").unwrap();
    writeln!(source, "        {entity}ServiceError::Forbidden => leptos_support::server::public_error(\"forbidden\"),").unwrap();
    writeln!(source, "        {entity}ServiceError::NotFound => leptos_support::server::public_error(\"resource not found\"),").unwrap();
    writeln!(source, "        {entity}ServiceError::Conflict => leptos_support::server::public_error(\"resource conflict\"),").unwrap();
    writeln!(source, "        {entity}ServiceError::InvalidInput => leptos_support::server::public_error(\"invalid resource input\"),").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();
}

fn write_server_functions(source: &mut String, specification: &ResourceSpecification) {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    let plural = names.plural_type();
    for (method, function, input, output) in [
        (
            "list",
            format!("list_{module}_resource"),
            format!("List{plural}Query"),
            format!("{plural}Response"),
        ),
        (
            "get",
            format!("get_{module}_resource"),
            format!("Get{entity}Query"),
            format!("{entity}Dto"),
        ),
        (
            "create",
            format!("create_{module}_resource"),
            format!("Create{entity}Input"),
            format!("{entity}Dto"),
        ),
        (
            "update",
            format!("update_{module}_resource"),
            format!("Update{entity}Input"),
            format!("{entity}Dto"),
        ),
        (
            "delete",
            format!("delete_{module}_resource"),
            format!("Delete{entity}Input"),
            "()".to_owned(),
        ),
    ] {
        writeln!(source, "#[server]").unwrap();
        writeln!(
            source,
            "pub async fn {function}(input: {input}) -> Result<{output}, ServerFnError> {{"
        )
        .unwrap();
        writeln!(
            source,
            "    let token = identity_leptos::identity::server::session::require_token().await?;"
        )
        .unwrap();
        writeln!(
            source,
            "    resource_service().{method}(&token, input).await.map_err(server_error)"
        )
        .unwrap();
        writeln!(source, "}}\n").unwrap();
    }
}

fn write_resource_page(source: &mut String, specification: &ResourceSpecification) {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    let plural = names.plural_type();

    writeln!(source, "#[component]").unwrap();
    writeln!(source, "pub fn {plural}Route() -> impl IntoView {{").unwrap();
    writeln!(source, "    view! {{").unwrap();
    writeln!(
        source,
        "        <RequirePermission permission=PermissionName(LIST_PERMISSION)>"
    )
    .unwrap();
    writeln!(source, "            <{plural}Page/>").unwrap();
    writeln!(source, "        </RequirePermission>").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();

    writeln!(source, "#[derive(Debug)]").unwrap();
    writeln!(source, "#[cfg(feature = \"wasm-split\")]").unwrap();
    writeln!(source, "pub struct {plural}LazyRoute;").unwrap();
    writeln!(source, "#[lazy_route]").unwrap();
    writeln!(source, "#[cfg(feature = \"wasm-split\")]").unwrap();
    writeln!(source, "impl LazyRoute for {plural}LazyRoute {{").unwrap();
    writeln!(source, "    fn data() -> Self {{ Self }}").unwrap();
    writeln!(source, "    fn view(_this: Self) -> AnyView {{").unwrap();
    writeln!(source, "        view! {{ <{plural}Route/> }}.into_any()").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}\n").unwrap();

    writeln!(source, "#[component]").unwrap();
    writeln!(source, "fn {plural}Page() -> impl IntoView {{").unwrap();
    writeln!(source, "    let i18n = use_i18n();").unwrap();
    writeln!(source, "    let toast = use_toast();").unwrap();
    writeln!(
        source,
        "    let items = RwSignal::new(Vec::<{entity}Dto>::new());"
    )
    .unwrap();
    writeln!(source, "    let loading = RwSignal::new(false);").unwrap();
    writeln!(source, "    let error = RwSignal::new(None::<String>);").unwrap();
    writeln!(
        source,
        "    let mutation = RwSignal::new(MutationStatus::Idle);"
    )
    .unwrap();
    writeln!(source, "    let form_open = RwSignal::new(false);").unwrap();
    writeln!(source, "    let editing = RwSignal::new(None::<Uuid>);").unwrap();
    writeln!(
        source,
        "    let delete_candidate = RwSignal::new(None::<Uuid>);"
    )
    .unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "    let {} = RwSignal::new({});",
            field.name(),
            signal_default(field)
        )
        .unwrap();
    }
    source.push('\n');

    writeln!(source, "    let load = Callback::new(move |()| {{").unwrap();
    writeln!(source, "        loading.set(true);").unwrap();
    writeln!(source, "        error.set(None);").unwrap();
    writeln!(source, "        spawn_local(async move {{").unwrap();
    writeln!(source, "            match list_{module}_resource(List{plural}Query {{ offset: 0, limit: 50 }}).await {{").unwrap();
    writeln!(
        source,
        "                Ok(result) => items.set(result.items),"
    )
    .unwrap();
    writeln!(
        source,
        "                Err(failure) => error.set(Some(failure.to_string())),"
    )
    .unwrap();
    writeln!(source, "            }}").unwrap();
    writeln!(source, "            loading.set(false);").unwrap();
    writeln!(source, "        }});").unwrap();
    writeln!(source, "    }});").unwrap();
    writeln!(source, "    Effect::new(move |_| load.run(()));\n").unwrap();

    writeln!(source, "    let begin_create = Callback::new(move |()| {{").unwrap();
    writeln!(source, "        editing.set(None);").unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "        {}.set({});",
            field.name(),
            signal_default(field)
        )
        .unwrap();
    }
    writeln!(source, "        error.set(None);").unwrap();
    writeln!(source, "        form_open.set(true);").unwrap();
    writeln!(source, "    }});\n").unwrap();

    writeln!(
        source,
        "    let begin_edit = Callback::new(move |item: {entity}Dto| {{"
    )
    .unwrap();
    writeln!(source, "        editing.set(Some(item.id));").unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "        {}.set({});",
            field.name(),
            dto_to_signal(field)
        )
        .unwrap();
    }
    writeln!(source, "        error.set(None);").unwrap();
    writeln!(source, "        form_open.set(true);").unwrap();
    writeln!(source, "    }});\n").unwrap();

    writeln!(source, "    let cancel_form = Callback::new(move |()| {{").unwrap();
    writeln!(source, "        form_open.set(false);").unwrap();
    writeln!(source, "        editing.set(None);").unwrap();
    writeln!(source, "        error.set(None);").unwrap();
    writeln!(source, "    }});\n").unwrap();

    write_submit_callback(source, specification);
    write_delete_callback(source, specification);

    writeln!(source, "    view! {{").unwrap();
    writeln!(source, "        <WorkspaceRouteLayout title=T::{plural}>").unwrap();
    writeln!(source, "            <div class=\"page-stack\">").unwrap();
    writeln!(source, "                <PageHeaderKey title=T::{plural}/>").unwrap();
    writeln!(
        source,
        "                <PermissionGate permission=PermissionName(CREATE_PERMISSION)>"
    )
    .unwrap();
    writeln!(source, "                    <button type=\"button\" class=\"w-fit rounded-md bg-primary px-4 py-2 text-sm text-primary-foreground\" on:click=move |_| begin_create.run(())>").unwrap();
    writeln!(
        source,
        "                        {{move || i18n.t(T::New{entity})}}"
    )
    .unwrap();
    writeln!(source, "                    </button>").unwrap();
    writeln!(source, "                </PermissionGate>").unwrap();
    write_form_view(source, specification);
    write_list_view(source, specification);
    writeln!(source, "            </div>").unwrap();
    writeln!(source, "        </WorkspaceRouteLayout>").unwrap();
    writeln!(source, "    }}").unwrap();
    writeln!(source, "}}").unwrap();
}

fn write_form_view(source: &mut String, specification: &ResourceSpecification) {
    let names = specification.names();
    let entity = names.singular_type();
    writeln!(
        source,
        "                <Show when=move || form_open.get()>"
    )
    .unwrap();
    writeln!(
        source,
        "                    <PageSection class=\"grid gap-4\">"
    )
    .unwrap();
    writeln!(source, "                        <h2 class=\"text-lg font-semibold\">{{move || if editing.get().is_some() {{ i18n.t(T::Edit{entity}) }} else {{ i18n.t(T::New{entity}) }} }}</h2>").unwrap();
    writeln!(source, "                        <form class=\"grid gap-4\" on:submit=move |event| {{ event.prevent_default(); submit.run(()); }}>").unwrap();
    for field in specification.fields() {
        let key = format!("{entity}{}Field", upper_camel_field(field.name()));
        writeln!(
            source,
            "                            <label class=\"grid gap-1.5 text-sm\">"
        )
        .unwrap();
        writeln!(source, "                                <span class=\"font-medium\">{{move || i18n.t(T::{key})}}</span>").unwrap();
        if field.scalar() == ScalarType::Bool && !field.nullable() {
            writeln!(source, "                                <input type=\"checkbox\" prop:checked=move || {}.get() on:change=move |event| {}.set(event_target_checked(&event))/>", field.name(), field.name()).unwrap();
        } else {
            let input_type = match field.scalar() {
                ScalarType::I64 => "number",
                _ => "text",
            };
            writeln!(source, "                                <input type=\"{input_type}\" class=\"rounded-md border bg-background px-3 py-2\" prop:value=move || {}.get() on:input=move |event| {}.set(event_target_value(&event))/>", field.name(), field.name()).unwrap();
        }
        writeln!(source, "                            </label>").unwrap();
    }
    writeln!(
        source,
        "                            <Show when=move || error.get().is_some()>"
    )
    .unwrap();
    writeln!(source, "                                <p class=\"text-sm text-destructive\">{{move || error.get().unwrap_or_default()}}</p>").unwrap();
    writeln!(source, "                            </Show>").unwrap();
    writeln!(
        source,
        "                            <div class=\"flex gap-2\">"
    )
    .unwrap();
    writeln!(source, "                                <Show when=move || editing.get().is_some() fallback=move || view! {{").unwrap();
    writeln!(source, "                                    <PermissionGate permission=PermissionName(CREATE_PERMISSION)>").unwrap();
    writeln!(source, "                                        <button type=\"submit\" class=\"rounded-md bg-primary px-4 py-2 text-sm text-primary-foreground\" disabled=move || mutation.get().is_pending()>{{move || i18n.t(T::Save{entity})}}</button>").unwrap();
    writeln!(
        source,
        "                                    </PermissionGate>"
    )
    .unwrap();
    writeln!(source, "                                }}>").unwrap();
    writeln!(source, "                                    <PermissionGate permission=PermissionName(UPDATE_PERMISSION)>").unwrap();
    writeln!(source, "                                        <button type=\"submit\" class=\"rounded-md bg-primary px-4 py-2 text-sm text-primary-foreground\" disabled=move || mutation.get().is_pending()>{{move || i18n.t(T::Save{entity})}}</button>").unwrap();
    writeln!(
        source,
        "                                    </PermissionGate>"
    )
    .unwrap();
    writeln!(source, "                                </Show>").unwrap();
    writeln!(source, "                                <button type=\"button\" class=\"rounded-md border px-4 py-2 text-sm\" on:click=move |_| cancel_form.run(())>{{move || i18n.t(T::Cancel)}}</button>").unwrap();
    writeln!(source, "                            </div>").unwrap();
    writeln!(source, "                        </form>").unwrap();
    writeln!(source, "                    </PageSection>").unwrap();
    writeln!(source, "                </Show>").unwrap();
    writeln!(
        source,
        "                <Show when=move || delete_candidate.get().is_some()>"
    )
    .unwrap();
    writeln!(
        source,
        "                    <PageSection class=\"flex items-center justify-between gap-4\">"
    )
    .unwrap();
    writeln!(
        source,
        "                        <p class=\"text-sm\">{{move || i18n.t(T::Delete{entity})}}</p>"
    )
    .unwrap();
    writeln!(source, "                        <div class=\"flex gap-2\">").unwrap();
    writeln!(source, "                            <button type=\"button\" class=\"rounded-md border px-3 py-2 text-sm\" on:click=move |_| delete_candidate.set(None)>{{move || i18n.t(T::Cancel)}}</button>").unwrap();
    writeln!(
        source,
        "                            <PermissionGate permission=PermissionName(DELETE_PERMISSION)>"
    )
    .unwrap();
    writeln!(source, "                                <button type=\"button\" class=\"rounded-md bg-destructive px-3 py-2 text-sm text-destructive-foreground\" disabled=move || mutation.get().is_pending() on:click=move |_| {{ if let Some(id) = delete_candidate.get_untracked() {{ delete_candidate.set(None); delete_item.run(id); }} }}>{{move || i18n.t(T::Delete{entity})}}</button>").unwrap();
    writeln!(source, "                            </PermissionGate>").unwrap();
    writeln!(source, "                        </div>").unwrap();
    writeln!(source, "                    </PageSection>").unwrap();
    writeln!(source, "                </Show>").unwrap();
}

fn write_list_view(source: &mut String, specification: &ResourceSpecification) {
    let names = specification.names();
    let entity = names.singular_type();
    let plural = names.plural_type();
    writeln!(
        source,
        "                <PageSection class=\"overflow-x-auto\">"
    )
    .unwrap();
    writeln!(
        source,
        "                    <Show when=move || loading.get()>"
    )
    .unwrap();
    writeln!(source, "                        <p class=\"text-sm text-muted-foreground\">{{move || i18n.t(T::Loading{plural})}}</p>").unwrap();
    writeln!(source, "                    </Show>").unwrap();
    writeln!(
        source,
        "                    <Show when=move || !loading.get() && error.get().is_some()>"
    )
    .unwrap();
    writeln!(source, "                        <p class=\"text-sm text-destructive\">{{move || error.get().unwrap_or_default()}}</p>").unwrap();
    writeln!(source, "                    </Show>").unwrap();
    writeln!(source, "                    <Show when=move || !loading.get() && error.get().is_none() && items.get().is_empty()>").unwrap();
    writeln!(source, "                        <p class=\"text-sm text-muted-foreground\">{{move || i18n.t(T::Empty{plural})}}</p>").unwrap();
    writeln!(source, "                    </Show>").unwrap();
    writeln!(
        source,
        "                    <Show when=move || !items.get().is_empty()>"
    )
    .unwrap();
    writeln!(
        source,
        "                        <table class=\"w-full text-left text-sm\">"
    )
    .unwrap();
    writeln!(
        source,
        "                            <thead><tr class=\"border-b\"><th class=\"p-3\">\"ID\"</th>"
    )
    .unwrap();
    for field in specification.fields() {
        let key = format!("{entity}{}Field", upper_camel_field(field.name()));
        writeln!(
            source,
            "                                <th class=\"p-3\">{{move || i18n.t(T::{key})}}</th>"
        )
        .unwrap();
    }
    writeln!(source, "                                <th class=\"p-3\">{{move || i18n.t(T::{plural}Actions)}}</th>").unwrap();
    writeln!(source, "                            </tr></thead>").unwrap();
    writeln!(source, "                            <tbody>").unwrap();
    writeln!(
        source,
        "                                {{move || items.get().into_iter().map(|item| {{"
    )
    .unwrap();
    writeln!(
        source,
        "                                    let edit_value = StoredValue::new(item.clone());"
    )
    .unwrap();
    writeln!(
        source,
        "                                    let id = item.id;"
    )
    .unwrap();
    writeln!(
        source,
        "                                    view! {{ <tr class=\"border-b\">"
    )
    .unwrap();
    writeln!(source, "                                        <td class=\"p-3 font-mono text-xs\">{{item.id.to_string()}}</td>").unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "                                        <td class=\"p-3\">{{{}}}</td>",
            dto_display(field)
        )
        .unwrap();
    }
    writeln!(
        source,
        "                                        <td class=\"p-3\"><div class=\"flex gap-2\">"
    )
    .unwrap();
    writeln!(source, "                                            <PermissionGate permission=PermissionName(UPDATE_PERMISSION)>").unwrap();
    writeln!(source, "                                                <button type=\"button\" class=\"rounded-md border px-2 py-1\" on:click=move |_| begin_edit.run(edit_value.get_value())>{{move || i18n.t(T::Edit{entity})}}</button>").unwrap();
    writeln!(
        source,
        "                                            </PermissionGate>"
    )
    .unwrap();
    writeln!(source, "                                            <PermissionGate permission=PermissionName(DELETE_PERMISSION)>").unwrap();
    writeln!(source, "                                                <button type=\"button\" class=\"rounded-md border px-2 py-1 text-destructive\" on:click=move |_| delete_candidate.set(Some(id))>{{move || i18n.t(T::Delete{entity})}}</button>").unwrap();
    writeln!(
        source,
        "                                            </PermissionGate>"
    )
    .unwrap();
    writeln!(
        source,
        "                                        </div></td>"
    )
    .unwrap();
    writeln!(source, "                                    </tr> }}").unwrap();
    writeln!(
        source,
        "                                }}).collect_view()}}"
    )
    .unwrap();
    writeln!(source, "                            </tbody>").unwrap();
    writeln!(source, "                        </table>").unwrap();
    writeln!(source, "                    </Show>").unwrap();
    writeln!(source, "                </PageSection>").unwrap();
}

fn signal_default(field: &ResourceField) -> &'static str {
    if field.scalar() == ScalarType::Bool && !field.nullable() {
        "false"
    } else {
        "String::new()"
    }
}

fn dto_to_signal(field: &ResourceField) -> String {
    if field.scalar() == ScalarType::Bool && !field.nullable() {
        format!("item.{}", field.name())
    } else if field.nullable() {
        match field.scalar() {
            ScalarType::String => format!("item.{}.unwrap_or_default()", field.name()),
            ScalarType::DateTime => format!(
                "item.{}.map(|value| value.to_rfc3339()).unwrap_or_default()",
                field.name()
            ),
            _ => format!(
                "item.{}.map(|value| value.to_string()).unwrap_or_default()",
                field.name()
            ),
        }
    } else if field.scalar() == ScalarType::String {
        format!("item.{}", field.name())
    } else if field.scalar() == ScalarType::DateTime {
        format!("item.{}.to_rfc3339()", field.name())
    } else {
        format!("item.{}.to_string()", field.name())
    }
}

fn signal_to_value(field: &ResourceField) -> String {
    let name = field.name();
    if field.scalar() == ScalarType::Bool && !field.nullable() {
        return format!("{name}.get_untracked()");
    }
    if field.scalar() == ScalarType::String {
        return if field.nullable() {
            format!("optional_string({name}.get_untracked())")
        } else {
            format!("{name}.get_untracked()")
        };
    }
    let kind = field.scalar().identifier();
    if field.nullable() {
        format!("parse_optional({name}.get_untracked(), \"{name}\", \"{kind}\")?")
    } else {
        format!("parse_required({name}.get_untracked(), \"{name}\", \"{kind}\")?")
    }
}

fn dto_display(field: &ResourceField) -> String {
    let name = field.name();
    if field.nullable() {
        match field.scalar() {
            ScalarType::String => format!("item.{name}.unwrap_or_else(|| \"—\".to_owned())"),
            ScalarType::DateTime => format!(
                "item.{name}.map(|value| value.to_rfc3339()).unwrap_or_else(|| \"—\".to_owned())"
            ),
            _ => format!(
                "item.{name}.map(|value| value.to_string()).unwrap_or_else(|| \"—\".to_owned())"
            ),
        }
    } else if field.scalar() == ScalarType::String {
        format!("item.{name}")
    } else if field.scalar() == ScalarType::DateTime {
        format!("item.{name}.to_rfc3339()")
    } else {
        format!("item.{name}.to_string()")
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

fn upper_camel_field(value: &str) -> String {
    value
        .split('_')
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + characters.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn humanize_field(value: &str) -> String {
    let mut value = value.replace('_', " ");
    if let Some(first) = value.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    value
}

fn humanize_type(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            output.push(' ');
        }
        output.push(if index == 0 {
            character
        } else {
            character.to_ascii_lowercase()
        });
    }
    output
}

fn write_submit_callback(source: &mut String, specification: &ResourceSpecification) {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    writeln!(source, "    let submit = Callback::new({{").unwrap();
    writeln!(source, "        let toast = toast.clone();").unwrap();
    writeln!(source, "        move |()| {{").unwrap();
    writeln!(
        source,
        "            if mutation.get_untracked().is_pending() {{ return; }}"
    )
    .unwrap();
    writeln!(
        source,
        "            let values = (|| -> Result<{entity}FormValues, String> {{"
    )
    .unwrap();
    writeln!(source, "                Ok({entity}FormValues {{").unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "                    {}: {},",
            field.name(),
            signal_to_value(field)
        )
        .unwrap();
    }
    writeln!(source, "                }})").unwrap();
    writeln!(source, "            }})();").unwrap();
    writeln!(source, "            let values = match values {{").unwrap();
    writeln!(source, "                Ok(values) => values,").unwrap();
    writeln!(
        source,
        "                Err(message) => {{ error.set(Some(message)); return; }}"
    )
    .unwrap();
    writeln!(source, "            }};").unwrap();
    writeln!(source, "            mutation.set(MutationStatus::Pending);").unwrap();
    writeln!(source, "            error.set(None);").unwrap();
    writeln!(
        source,
        "            let editing_id = editing.get_untracked();"
    )
    .unwrap();
    writeln!(source, "            let toast = toast.clone();").unwrap();
    writeln!(source, "            spawn_local(async move {{").unwrap();
    writeln!(
        source,
        "                let result = if let Some(id) = editing_id {{"
    )
    .unwrap();
    writeln!(
        source,
        "                    update_{module}_resource(Update{entity}Input {{ id,"
    )
    .unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "                        {}: values.{},",
            field.name(),
            field.name()
        )
        .unwrap();
    }
    writeln!(source, "                    }}).await").unwrap();
    writeln!(source, "                }} else {{").unwrap();
    writeln!(
        source,
        "                    create_{module}_resource(Create{entity}Input {{"
    )
    .unwrap();
    for field in specification.fields() {
        writeln!(
            source,
            "                        {}: values.{},",
            field.name(),
            field.name()
        )
        .unwrap();
    }
    writeln!(source, "                    }}).await").unwrap();
    writeln!(source, "                }};").unwrap();
    writeln!(source, "                match result {{").unwrap();
    writeln!(source, "                    Ok(_) => {{").unwrap();
    writeln!(
        source,
        "                        mutation.set(MutationStatus::Success);"
    )
    .unwrap();
    writeln!(source, "                        form_open.set(false);").unwrap();
    writeln!(source, "                        editing.set(None);").unwrap();
    writeln!(source, "                        toast.success(i18n.t_untracked(T::{entity}Saved), i18n.t_untracked(T::{entity}));").unwrap();
    writeln!(source, "                        load.run(());").unwrap();
    writeln!(source, "                    }}").unwrap();
    writeln!(source, "                    Err(failure) => {{").unwrap();
    writeln!(
        source,
        "                        let message = failure.to_string();"
    )
    .unwrap();
    writeln!(
        source,
        "                        mutation.set(MutationStatus::Failed(message.clone()));"
    )
    .unwrap();
    writeln!(
        source,
        "                        error.set(Some(message.clone()));"
    )
    .unwrap();
    writeln!(
        source,
        "                        toast.error(i18n.t_untracked(T::{entity}SaveFailed), message);"
    )
    .unwrap();
    writeln!(source, "                    }}").unwrap();
    writeln!(source, "                }}").unwrap();
    writeln!(source, "            }});").unwrap();
    writeln!(source, "        }}").unwrap();
    writeln!(source, "    }});\n").unwrap();
}
fn write_delete_callback(source: &mut String, specification: &ResourceSpecification) {
    let names = specification.names();
    let module = names.rust_module();
    let entity = names.singular_type();
    writeln!(source, "    let delete_item = Callback::new({{").unwrap();
    writeln!(source, "        let toast = toast.clone();").unwrap();
    writeln!(source, "        move |id: Uuid| {{").unwrap();
    writeln!(
        source,
        "            if mutation.get_untracked().is_pending() {{ return; }}"
    )
    .unwrap();
    writeln!(source, "            mutation.set(MutationStatus::Pending);").unwrap();
    writeln!(source, "            let toast = toast.clone();").unwrap();
    writeln!(source, "            spawn_local(async move {{").unwrap();
    writeln!(
        source,
        "                match delete_{module}_resource(Delete{entity}Input {{ id }}).await {{"
    )
    .unwrap();
    writeln!(source, "                    Ok(()) => {{").unwrap();
    writeln!(
        source,
        "                        mutation.set(MutationStatus::Success);"
    )
    .unwrap();
    writeln!(source, "                        toast.success(i18n.t_untracked(T::{entity}Deleted), i18n.t_untracked(T::{entity}));").unwrap();
    writeln!(source, "                        load.run(());").unwrap();
    writeln!(source, "                    }}").unwrap();
    writeln!(source, "                    Err(failure) => {{").unwrap();
    writeln!(
        source,
        "                        let message = failure.to_string();"
    )
    .unwrap();
    writeln!(
        source,
        "                        mutation.set(MutationStatus::Failed(message.clone()));"
    )
    .unwrap();
    writeln!(
        source,
        "                        error.set(Some(message.clone()));"
    )
    .unwrap();
    writeln!(
        source,
        "                        toast.error(i18n.t_untracked(T::{entity}DeleteFailed), message);"
    )
    .unwrap();
    writeln!(source, "                    }}").unwrap();
    writeln!(source, "                }}").unwrap();
    writeln!(source, "            }});").unwrap();
    writeln!(source, "        }}").unwrap();
    writeln!(source, "    }});\n").unwrap();
}

#[cfg(test)]
mod tests {
    use application_manifest::ApplicationManifest;
    use application_mutator::ChangeOperation;

    use super::*;
    use crate::{
        ArtifactNamespace, LayeredNamingInput, ResourceFieldInput, ResourceSpecificationInput,
    };

    const ROOT: &[u8] = b"// hegira:generated-modules:start\n// hegira:generated-modules:end\n";
    const ROUTES: &[u8] = b"fn routes() { view! {\n// hegira:resource-routes-native\n// hegira:resource-routes-native:end\n// hegira:resource-routes-split\n// hegira:resource-routes-split:end\n} }\n";
    const NAVIGATION: &[u8] = b"enum NavIcon { Home,\n// hegira:nav-icons\n// hegira:nav-icons:end\n}\nstruct NavItem;\nconst ITEMS: &[NavItem] = &[\n// hegira:nav-items\n// hegira:nav-items:end\n];\nfn title(path: &str) { match path {\n// hegira:nav-titles\n// hegira:nav-titles:end\n_ => (), } }\n";
    const I18N: &[u8] = b"enum T { Home,\n// hegira:resource-i18n-keys\n// hegira:resource-i18n-keys:end\n}\nfn translate(locale: Locale, key: T) { match (locale, key) {\n// hegira:resource-i18n-en\n// hegira:resource-i18n-en:end\n// hegira:resource-i18n-tr\n// hegira:resource-i18n-tr:end\n_ => (), } }\n";
    const SIDEBAR: &[u8] = b"fn icon(icon: NavIcon) { match icon {\n// hegira:nav-icon-views\n// hegira:nav-icon-views:end\n} }\n";
    const SERVER: &[u8] = b"fn context() {\n// hegira:resource-leptos-contexts\n// hegira:resource-leptos-contexts:end\n}\n";

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

    fn sources() -> WebLayerSources<'static> {
        WebLayerSources {
            web_root: ROOT,
            routes: ROUTES,
            navigation: NAVIGATION,
            i18n: I18N,
            sidebar: SIDEBAR,
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
    fn web_plan_is_deterministic_and_explicitly_composed() {
        let first = plan_resource_web(&specification(), sources()).unwrap();
        let second = plan_resource_web(&specification(), sources()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.plan().changes().len(), 7);

        let web = content(first.plan(), "apps/web/src/order_item.rs");
        syn::parse_file(web).unwrap_or_else(|error| panic!("{error}\n{web}"));
        assert!(web.contains("<RequirePermission permission=PermissionName(LIST_PERMISSION)>"));
        assert!(web.contains("<PermissionGate permission=PermissionName(CREATE_PERMISSION)>"));
        assert!(web.contains("identity_leptos::identity::server::session::require_token()"));
        assert!(web.contains("OrderItemService::create(self, actor, input)"));
        assert!(web.contains("Option<DateTime<Utc>>"));
        for forbidden in ["sqlx", "app_infrastructure", "axum"] {
            assert!(!web.contains(forbidden));
        }

        let routes = content(first.plan(), WEB_ROUTES);
        assert_eq!(routes.matches("StaticSegment(\"order-items\")").count(), 2);
        assert!(routes.contains("Lazy::<crate::order_item::OrderItemsLazyRoute>::new()"));
        let navigation = content(first.plan(), WEB_NAVIGATION);
        assert!(navigation.contains("href: \"/order-items\""));
        assert!(navigation.contains("app_application_contracts::order_item::LIST_PERMISSION"));
        let i18n = content(first.plan(), WEB_I18N);
        assert!(i18n.contains("OrderItemPublishedAtField,"));
        assert!(i18n.contains("(Locale::Tr, T::OrderItemSaved)"));
        let server = content(first.plan(), SERVER_SOURCE);
        assert!(server.contains("OrderItemLeptosServices::new(services.order_item.clone())"));
    }

    #[test]
    fn route_and_localization_conflicts_fail_before_a_plan_exists() {
        let route_conflict = WebLayerSources {
            routes: b"StaticSegment(\"order-items\")\n",
            ..sources()
        };
        assert_eq!(
            plan_resource_web(&specification(), route_conflict)
                .unwrap_err()
                .kind(),
            WebLayerErrorKind::RouteConflict
        );

        let localization_conflict = WebLayerSources {
            i18n: b"OrderItem,\n",
            ..sources()
        };
        assert_eq!(
            plan_resource_web(&specification(), localization_conflict)
                .unwrap_err()
                .kind(),
            WebLayerErrorKind::LocalizationConflict
        );
    }

    #[test]
    fn malformed_or_existing_registrations_fail_closed() {
        let malformed = WebLayerSources {
            sidebar: b"fn icon() {}\n",
            ..sources()
        };
        assert_eq!(
            plan_resource_web(&specification(), malformed)
                .unwrap_err()
                .kind(),
            WebLayerErrorKind::StructuredEdit
        );

        let existing = WebLayerSources {
            web_root: b"// hegira:generated-modules:start\npub mod order_item;\n// hegira:generated-modules:end\n",
            ..sources()
        };
        assert_eq!(
            plan_resource_web(&specification(), existing)
                .unwrap_err()
                .kind(),
            WebLayerErrorKind::ExistingRegistration
        );
    }

    #[test]
    fn web_plan_uses_one_absent_file_and_digest_guarded_edits() {
        let planned = plan_resource_web(&specification(), sources()).unwrap();
        assert_eq!(
            planned
                .plan()
                .changes()
                .iter()
                .filter(|change| change.operation() == ChangeOperation::Create)
                .count(),
            1
        );
        assert_eq!(
            planned
                .plan()
                .changes()
                .iter()
                .filter(|change| change.operation() == ChangeOperation::Edit)
                .count(),
            6
        );
    }
}
