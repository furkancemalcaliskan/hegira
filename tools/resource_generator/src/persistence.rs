use std::{
    fmt::{Display, Formatter, Write as _},
    path::Path,
};

use application_mutator::{
    ChangePlan, FileCreation, PlannedFileChange, StructuredEditError, StructuredEditOutcome,
    plan_rust_module,
};

use crate::{
    LayerOwner, MigrationError, MigrationIdentity, ResourceField, ResourceSpecification,
    ScalarType, SelectedDatabase, migration::plan_application_migration_with_source,
};

const INFRASTRUCTURE_ROOT: &str = "crates/infrastructure/src/lib.rs";

#[derive(Debug, Clone, Copy)]
pub struct PersistenceLayerSources<'a> {
    pub infrastructure_root: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedResourcePersistence {
    migration_version: i64,
    migration_path: String,
    plan: ChangePlan,
}

impl PlannedResourcePersistence {
    pub const fn migration_version(&self) -> i64 {
        self.migration_version
    }

    pub fn migration_path(&self) -> &str {
        &self.migration_path
    }

    pub fn plan(&self) -> &ChangePlan {
        &self.plan
    }

    pub fn into_plan(self) -> ChangePlan {
        self.plan
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceLayerErrorKind {
    Migration,
    StructuredEdit,
    ExistingRegistration,
    Planning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceLayerError {
    kind: PersistenceLayerErrorKind,
    message: String,
}

impl PersistenceLayerError {
    fn new(kind: PersistenceLayerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> PersistenceLayerErrorKind {
        self.kind
    }
}

impl Display for PersistenceLayerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PersistenceLayerError {}

pub fn plan_resource_persistence(
    application_root: &Path,
    specification: &ResourceSpecification,
    sources: PersistenceLayerSources<'_>,
) -> Result<PlannedResourcePersistence, PersistenceLayerError> {
    let module = specification.names().rust_module();
    let registration = plan_rust_module(INFRASTRUCTURE_ROOT, sources.infrastructure_root, module)
        .map_err(structured_edit_error)?;
    let StructuredEditOutcome::Planned { edit, .. } = registration else {
        return Err(PersistenceLayerError::new(
            PersistenceLayerErrorKind::ExistingRegistration,
            format!("resource module `{module}` is already registered in `{INFRASTRUCTURE_ROOT}`"),
        ));
    };

    let database = specification.selection().database();
    let identity = MigrationIdentity::new(module.to_owned()).map_err(migration_error)?;
    let migration = plan_application_migration_with_source(
        application_root,
        database,
        identity,
        schema_migration(specification).into_bytes(),
    )
    .map_err(migration_error)?;

    let mut changes = migration.plan().changes().to_vec();
    changes.push(create_infrastructure(specification)?);
    changes.push(PlannedFileChange::from(edit));
    let plan = ChangePlan::new(changes).map_err(|error| {
        PersistenceLayerError::new(PersistenceLayerErrorKind::Planning, error.to_string())
    })?;

    Ok(PlannedResourcePersistence {
        migration_version: migration.version(),
        migration_path: migration.path().to_owned(),
        plan,
    })
}

fn migration_error(error: MigrationError) -> PersistenceLayerError {
    PersistenceLayerError::new(PersistenceLayerErrorKind::Migration, error.to_string())
}

fn structured_edit_error(error: StructuredEditError) -> PersistenceLayerError {
    PersistenceLayerError::new(PersistenceLayerErrorKind::StructuredEdit, error.to_string())
}

fn create_infrastructure(
    specification: &ResourceSpecification,
) -> Result<PlannedFileChange, PersistenceLayerError> {
    let path = specification
        .names()
        .artifacts()
        .iter()
        .find(|artifact| artifact.owner() == LayerOwner::Infrastructure)
        .expect("the validated resource has one Infrastructure artifact")
        .path();
    FileCreation::new(
        path.as_str(),
        infrastructure_source(specification).into_bytes(),
    )
    .map(PlannedFileChange::from)
    .map_err(|error| {
        PersistenceLayerError::new(PersistenceLayerErrorKind::Planning, error.to_string())
    })
}

fn schema_migration(specification: &ResourceSpecification) -> String {
    let database = specification.selection().database();
    let table = specification.names().database_table();
    let mut source = format!(
        "CREATE TABLE {table} (\n    id {} PRIMARY KEY",
        sql_type(ScalarType::Uuid, database)
    );
    for field in specification.fields() {
        write!(
            source,
            ",\n    {} {}",
            field.name(),
            sql_type(field.scalar(), database)
        )
        .unwrap();
        if !field.nullable() {
            source.push_str(" NOT NULL");
        }
        if database == SelectedDatabase::Sqlite && field.scalar() == ScalarType::Bool {
            if field.nullable() {
                write!(
                    source,
                    " CHECK ({} IS NULL OR {} IN (0, 1))",
                    field.name(),
                    field.name()
                )
                .unwrap();
            } else {
                write!(source, " CHECK ({} IN (0, 1))", field.name()).unwrap();
            }
        }
    }
    source.push_str("\n);\n");
    source
}

fn sql_type(scalar: ScalarType, database: SelectedDatabase) -> &'static str {
    match (database, scalar) {
        (SelectedDatabase::Postgres, ScalarType::String) => "TEXT",
        (SelectedDatabase::Postgres, ScalarType::Bool) => "BOOLEAN",
        (SelectedDatabase::Postgres, ScalarType::I64) => "BIGINT",
        (SelectedDatabase::Postgres, ScalarType::Uuid) => "UUID",
        (SelectedDatabase::Postgres, ScalarType::DateTime) => "TIMESTAMPTZ",
        (SelectedDatabase::Sqlite, ScalarType::String) => "TEXT",
        (SelectedDatabase::Sqlite, ScalarType::Bool | ScalarType::I64) => "INTEGER",
        (SelectedDatabase::Sqlite, ScalarType::Uuid) => "BLOB",
        (SelectedDatabase::Sqlite, ScalarType::DateTime) => "TEXT",
    }
}

fn infrastructure_source(specification: &ResourceSpecification) -> String {
    let names = specification.names();
    let entity = names.singular_type();
    let plural = names.plural_type();
    let module = names.rust_module();
    let table = names.database_table();
    let database = specification.selection().database();
    let feature = match database {
        SelectedDatabase::Postgres => "db-postgres",
        SelectedDatabase::Sqlite => "db-sqlite",
    };
    let pool = match database {
        SelectedDatabase::Postgres => "PgPool",
        SelectedDatabase::Sqlite => "SqlitePool",
    };
    let variant = match database {
        SelectedDatabase::Postgres => "Postgres",
        SelectedDatabase::Sqlite => "Sqlite",
    };
    let fields = specification.fields();
    let columns = std::iter::once("id")
        .chain(fields.iter().map(ResourceField::name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut source = format!(
        "//! Application-owned SQLx persistence and explicit service composition.\n\n#![cfg(feature = \"{feature}\")]\n\n"
    );
    writeln!(source, "use app_application::{module}::{{").unwrap();
    writeln!(
        source,
        "    {entity}AppService, {entity}Authorization, {entity}IdGenerator, {entity}Repository,"
    )
    .unwrap();
    writeln!(source, "}};").unwrap();
    writeln!(
        source,
        "use app_application_contracts::{module}::{{List{plural}Query, {entity}ServiceError}};"
    )
    .unwrap();
    writeln!(
        source,
        "use app_domain::{module}::{{{entity}, {entity}Id}};"
    )
    .unwrap();
    writeln!(source, "use persistence::DatabasePool;").unwrap();
    writeln!(source, "use sqlx::{{FromRow, {pool}}};").unwrap();
    writeln!(source, "use uuid::Uuid;").unwrap();
    if fields
        .iter()
        .any(|field| field.scalar() == ScalarType::DateTime)
    {
        writeln!(source, "use chrono::{{DateTime, Utc}};").unwrap();
    }
    source.push('\n');
    writeln!(
        source,
        "#[derive(Debug, Clone)]\npub struct Sqlx{entity}Repository {{\n    pool: {pool},\n}}\n"
    )
    .unwrap();
    writeln!(source, "impl Sqlx{entity}Repository {{").unwrap();
    writeln!(
        source,
        "    pub fn from_database(pool: &DatabasePool) -> Result<Self, {entity}ServiceError> {{"
    )
    .unwrap();
    writeln!(source, "        match pool {{").unwrap();
    writeln!(
        source,
        "            DatabasePool::{variant}(pool) => Ok(Self {{ pool: pool.clone() }}),"
    )
    .unwrap();
    writeln!(source, "            #[allow(unreachable_patterns)]").unwrap();
    writeln!(
        source,
        "            _ => Err({entity}ServiceError::Persistence),"
    )
    .unwrap();
    writeln!(source, "        }}\n    }}\n}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, FromRow)]\nstruct {entity}Row {{\n    id: Uuid,"
    )
    .unwrap();
    write_fields(&mut source, fields, 4);
    writeln!(source, "}}\n").unwrap();
    writeln!(source, "impl From<{entity}Row> for {entity} {{\n    fn from(row: {entity}Row) -> Self {{\n        Self {{\n            id: {entity}Id::new(row.id),").unwrap();
    for field in fields {
        writeln!(source, "            {0}: row.{0},", field.name()).unwrap();
    }
    writeln!(source, "        }}\n    }}\n}}\n").unwrap();
    writeln!(
        source,
        "impl {entity}Repository for Sqlx{entity}Repository {{"
    )
    .unwrap();
    writeln!(source, "    async fn list(&self, query: &List{plural}Query) -> Result<(Vec<{entity}>, u64), {entity}ServiceError> {{").unwrap();
    writeln!(source, "        let offset = i64::try_from(query.offset).map_err(|_| {entity}ServiceError::InvalidInput)?;").unwrap();
    writeln!(
        source,
        "        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;"
    )
    .unwrap();
    writeln!(source, "        let total: i64 = sqlx::query_scalar(\"SELECT COUNT(*) FROM {table}\")\n            .fetch_one(&mut *transaction).await.map_err(persistence_error)?;").unwrap();
    writeln!(source, "        let rows = sqlx::query_as::<_, {entity}Row>(\"SELECT {columns} FROM {table} ORDER BY id LIMIT {} OFFSET {}\")", placeholder(database, 1), placeholder(database, 2)).unwrap();
    writeln!(source, "            .bind(i64::from(query.limit)).bind(offset)\n            .fetch_all(&mut *transaction).await.map_err(persistence_error)?;").unwrap();
    writeln!(
        source,
        "        transaction.commit().await.map_err(persistence_error)?;"
    )
    .unwrap();
    writeln!(
        source,
        "        let total = u64::try_from(total).map_err(|_| {entity}ServiceError::Persistence)?;"
    )
    .unwrap();
    writeln!(
        source,
        "        Ok((rows.into_iter().map(Into::into).collect(), total))\n    }}\n"
    )
    .unwrap();
    writeln!(source, "    async fn find(&self, id: {entity}Id) -> Result<Option<{entity}>, {entity}ServiceError> {{").unwrap();
    writeln!(
        source,
        "        sqlx::query_as::<_, {entity}Row>(\"SELECT {columns} FROM {table} WHERE id = {}\")",
        placeholder(database, 1)
    )
    .unwrap();
    writeln!(source, "            .bind(id.into_inner()).fetch_optional(&self.pool).await\n            .map(|row| row.map(Into::into)).map_err(persistence_error)\n    }}\n").unwrap();
    writeln!(
        source,
        "    async fn insert(&self, entity: {entity}) -> Result<{entity}, {entity}ServiceError> {{"
    )
    .unwrap();
    let insert_values = std::iter::once(placeholder(database, 1))
        .chain((2..=fields.len() + 1).map(|i| placeholder(database, i)))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(source, "        let query = sqlx::query_as::<_, {entity}Row>(\"INSERT INTO {table} ({columns}) VALUES ({insert_values}) RETURNING {columns}\")\n            .bind(entity.id.into_inner());").unwrap();
    write_binds(&mut source, fields, "entity", 8);
    writeln!(
        source,
        "        query.fetch_one(&self.pool).await.map(Into::into).map_err(write_error)\n    }}\n"
    )
    .unwrap();
    writeln!(source, "    async fn update(&self, entity: {entity}) -> Result<Option<{entity}>, {entity}ServiceError> {{").unwrap();
    let assignments = fields
        .iter()
        .enumerate()
        .map(|(index, field)| format!("{} = {}", field.name(), placeholder(database, index + 1)))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(source, "        let query = sqlx::query_as::<_, {entity}Row>(\"UPDATE {table} SET {assignments} WHERE id = {} RETURNING {columns}\");", placeholder(database, fields.len()+1)).unwrap();
    write_binds(&mut source, fields, "entity", 8);
    writeln!(source, "        query.bind(entity.id.into_inner()).fetch_optional(&self.pool).await\n            .map(|row| row.map(Into::into)).map_err(write_error)\n    }}\n").unwrap();
    writeln!(
        source,
        "    async fn delete(&self, id: {entity}Id) -> Result<bool, {entity}ServiceError> {{"
    )
    .unwrap();
    writeln!(
        source,
        "        sqlx::query(\"DELETE FROM {table} WHERE id = {}\").bind(id.into_inner())",
        placeholder(database, 1)
    )
    .unwrap();
    writeln!(source, "            .execute(&self.pool).await.map(|result| result.rows_affected() == 1).map_err(persistence_error)\n    }}\n}}\n").unwrap();
    writeln!(
        source,
        "#[derive(Debug, Clone, Copy, Default)]\npub struct Uuid{entity}IdGenerator;\n"
    )
    .unwrap();
    writeln!(source, "impl {entity}IdGenerator for Uuid{entity}IdGenerator {{\n    fn next_id(&self) -> {entity}Id {{ {entity}Id::new(Uuid::new_v4()) }}\n}}\n").unwrap();
    writeln!(source, "pub type Composed{entity}Service<Authorization> = {entity}AppService<Sqlx{entity}Repository, Authorization, Uuid{entity}IdGenerator>;\n").unwrap();
    writeln!(source, "pub fn compose_{module}_service<Authorization>(pool: &DatabasePool, authorization: Authorization) -> Result<Composed{entity}Service<Authorization>, {entity}ServiceError>\nwhere\n    Authorization: {entity}Authorization,\n{{").unwrap();
    writeln!(source, "    Ok({entity}AppService::new(Sqlx{entity}Repository::from_database(pool)?, authorization, Uuid{entity}IdGenerator))\n}}\n").unwrap();
    writeln!(source, "fn persistence_error(_: sqlx::Error) -> {entity}ServiceError {{ {entity}ServiceError::Persistence }}").unwrap();
    writeln!(source, "fn write_error(error: sqlx::Error) -> {entity}ServiceError {{\n    if error.as_database_error().is_some_and(|database| database.is_unique_violation()) {{\n        {entity}ServiceError::Conflict\n    }} else {{\n        {entity}ServiceError::Persistence\n    }}\n}}").unwrap();
    source
}

fn placeholder(database: SelectedDatabase, index: usize) -> String {
    match database {
        SelectedDatabase::Postgres => format!("${index}"),
        SelectedDatabase::Sqlite => format!("?{index}"),
    }
}

fn write_binds(source: &mut String, fields: &[ResourceField], owner: &str, indent: usize) {
    let padding = " ".repeat(indent);
    for field in fields {
        writeln!(
            source,
            "{padding}let query = query.bind(&{owner}.{});",
            field.name()
        )
        .unwrap();
    }
}

fn write_fields(source: &mut String, fields: &[ResourceField], indent: usize) {
    let padding = " ".repeat(indent);
    for field in fields {
        let base = match field.scalar() {
            ScalarType::String => "String",
            ScalarType::Bool => "bool",
            ScalarType::I64 => "i64",
            ScalarType::Uuid => "Uuid",
            ScalarType::DateTime => "DateTime<Utc>",
        };
        let kind = if field.nullable() {
            format!("Option<{base}>")
        } else {
            base.to_owned()
        };
        writeln!(source, "{padding}{}: {kind},", field.name()).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArtifactNamespace, LayeredNamingInput, ResourceFieldInput, ResourceSpecificationInput,
    };
    use application_manifest::ApplicationManifest;
    use application_mutator::ChangeOperation;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    const ROOT: &[u8] = b"// hegira:generated-modules:start\n// hegira:generated-modules:end\n";

    struct TestApplication(PathBuf);
    impl TestApplication {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "hegira-persistence-planner-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("crates/infrastructure/migrations/sqlite")).unwrap();
            fs::create_dir_all(root.join("crates/infrastructure/migrations/postgres")).unwrap();
            Self(fs::canonicalize(root).unwrap())
        }
    }
    impl Drop for TestApplication {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn specification(database: &str) -> ResourceSpecification {
        let manifest = ApplicationManifest::from_toml(&format!(
            r#"
schema = 1
application = "sample"
[framework]
repository = "https://example.invalid/hegira.git"
version = "v0.4.0"
[selection]
components = ["layered-base", "layered-leptos-identity"]
databases = ["{database}"]
clients = ["leptos"]
"#
        ))
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

    fn content<'a>(plan: &'a ChangePlan, path_suffix: &str) -> &'a str {
        let change = plan
            .changes()
            .iter()
            .find(|change| change.path().as_str().ends_with(path_suffix))
            .unwrap();
        std::str::from_utf8(change.resulting_content()).unwrap()
    }

    #[test]
    fn sqlite_plan_is_provider_specific_bound_and_explicitly_composed() {
        let application = TestApplication::new();
        let planned = plan_resource_persistence(
            &application.0,
            &specification("sqlite"),
            PersistenceLayerSources {
                infrastructure_root: ROOT,
            },
        )
        .unwrap();
        assert_eq!(planned.migration_version(), 1);
        assert_eq!(
            planned.migration_path(),
            "crates/infrastructure/migrations/sqlite/001_order_item.sql"
        );
        assert_eq!(planned.plan().changes().len(), 4);
        let migration = content(planned.plan(), "001_order_item.sql");
        assert!(migration.contains("id BLOB PRIMARY KEY"));
        assert!(migration.contains("active INTEGER NOT NULL CHECK (active IN (0, 1))"));
        let source = content(planned.plan(), "src/order_item.rs");
        assert!(source.contains("#![cfg(feature = \"db-sqlite\")]"));
        assert!(source.contains("WHERE id = ?1"));
        assert!(source.contains(".bind(id.into_inner())"));
        assert!(source.contains("compose_order_item_service"));
        syn::parse_file(source).unwrap();
        assert!(
            planned
                .plan()
                .changes()
                .iter()
                .any(|change| change.operation() == ChangeOperation::Edit
                    && change.path().as_str() == INFRASTRUCTURE_ROOT)
        );
    }

    #[test]
    fn postgres_plan_uses_postgres_types_and_placeholders_only() {
        let application = TestApplication::new();
        let planned = plan_resource_persistence(
            &application.0,
            &specification("postgres"),
            PersistenceLayerSources {
                infrastructure_root: ROOT,
            },
        )
        .unwrap();
        let migration = content(planned.plan(), "001_order_item.sql");
        assert!(migration.contains("id UUID PRIMARY KEY"));
        assert!(migration.contains("published_at TIMESTAMPTZ"));
        let source = content(planned.plan(), "src/order_item.rs");
        assert!(source.contains("#![cfg(feature = \"db-postgres\")]"));
        assert!(source.contains("WHERE id = $1"));
        assert!(!source.contains("WHERE id = ?1"));
        syn::parse_file(source).unwrap();
    }

    #[test]
    fn an_existing_infrastructure_registration_fails_before_migration_planning() {
        let application = TestApplication::new();
        let error = plan_resource_persistence(&application.0, &specification("sqlite"), PersistenceLayerSources { infrastructure_root: b"// hegira:generated-modules:start\npub mod order_item;\n// hegira:generated-modules:end\n" }).unwrap_err();
        assert_eq!(
            error.kind(),
            PersistenceLayerErrorKind::ExistingRegistration
        );
        assert!(
            application
                .0
                .join("crates/infrastructure/migrations/sqlite")
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn planned_history_is_append_only_and_does_not_modify_existing_sql() {
        let application = TestApplication::new();
        fs::write(
            application
                .0
                .join("crates/infrastructure/migrations/sqlite/004_existing.sql"),
            "-- keep\n",
        )
        .unwrap();
        let planned = plan_resource_persistence(
            &application.0,
            &specification("sqlite"),
            PersistenceLayerSources {
                infrastructure_root: ROOT,
            },
        )
        .unwrap();
        assert_eq!(planned.migration_version(), 5);
        assert!(planned.plan().changes().iter().all(|change| {
            change.operation() != ChangeOperation::Edit || !change.path().as_str().ends_with(".sql")
        }));
    }
}
