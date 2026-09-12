use std::{
    collections::BTreeMap,
    ffi::OsString,
    fmt::Write as _,
    fs,
    io::{BufRead, ErrorKind as IoErrorKind, Write},
    path::{Path, PathBuf},
    str::FromStr,
};

use application_manifest::{
    ClientAdapter, DatabaseAdapter, MutationCompatibility, MutationCompatibilityPolicy,
};
use application_mutator::{ChangePlan, PlannedFileChange};
use clap::{Args, Parser, Subcommand, ValueEnum, error::ErrorKind};
use resource_generator::{
    ArtifactNamespace, HttpLayerError, HttpLayerErrorKind, HttpLayerSources, InwardLayerError,
    InwardLayerErrorKind, InwardLayerSources, LayeredArtifactNames, LayeredNamingInput,
    MigrationError, MigrationErrorKind, MigrationIdentity, NamingErrorKind, PersistenceLayerError,
    PersistenceLayerErrorKind, PersistenceLayerSources, ResourceFieldInput, ResourceSelection,
    ResourceSpecification, ResourceSpecificationInput, WebLayerError, WebLayerErrorKind,
    WebLayerSources, plan_application_migration, plan_inward_resource_layers, plan_resource_http,
    plan_resource_persistence, plan_resource_web,
};
use serde::Serialize;
use template_renderer::{RenderRequest, RendererError, RendererErrorKind, render};

mod application_context;
mod mutation;

pub use application_context::{
    ApplicationContext, ApplicationContextError, ApplicationContextErrorKind,
    ApplicationContextRequest, ApplicationPaths, resolve_application_context,
};
pub use mutation::{
    MUTATION_OUTPUT_SCHEMA, MutationOptions, change_plan_diagnostic, execute_mutation_plan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CliExit {
    Success = 0,
    Internal = 1,
    Usage = 2,
    Validation = 3,
    Conflict = 4,
}

impl CliExit {
    pub const fn code(self) -> u8 {
        self as u8
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "hegira",
    version,
    about = "Create and maintain Hegira applications",
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Create a new Hegira application.
    New(NewCommand),
    /// Inspect an existing Hegira application without modifying it.
    Inspect(InspectCommand),
    /// Generate application-owned source through validated change plans.
    Generate(GenerateCommand),
}

#[derive(Debug, Args)]
struct GenerateCommand {
    #[command(subcommand)]
    command: GeneratorCommand,
}

#[derive(Debug, Subcommand)]
enum GeneratorCommand {
    /// Generate a complete layered resource for the selected application adapters.
    Resource(ResourceCommand),
    /// Create an append-only migration for the selected database adapter.
    Migration(MigrationCommand),
}

#[derive(Debug, Args)]
struct ResourceCommand {
    /// Singular UpperCamelCase resource type, for example `OrderItem`.
    #[arg(value_name = "NAME")]
    name: String,

    /// Irregular plural UpperCamelCase resource type.
    #[arg(long, value_name = "NAME")]
    plural: Option<String>,

    /// Resource field as `name:type` or nullable `name:type?`; repeat for each field.
    #[arg(long, value_name = "NAME:TYPE", required = true)]
    field: Vec<ResourceFieldArgument>,

    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,

    #[command(flatten)]
    mutation: MutationOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResourceFieldArgument {
    name: String,
    scalar: String,
    nullable: bool,
}

impl FromStr for ResourceFieldArgument {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, scalar) = value.split_once(':').ok_or_else(|| {
            "field must use `name:type` or nullable `name:type?` syntax".to_owned()
        })?;
        let (scalar, nullable) = match scalar.strip_suffix('?') {
            Some(scalar) => (scalar, true),
            None => (scalar, false),
        };
        if name.is_empty() || scalar.is_empty() || scalar.contains(':') || scalar.contains('?') {
            return Err("field must use `name:type` or nullable `name:type?` syntax".to_owned());
        }
        Ok(Self {
            name: name.to_owned(),
            scalar: scalar.to_owned(),
            nullable,
        })
    }
}

#[derive(Debug, Args)]
struct MigrationCommand {
    /// Stable lowercase snake_case migration identity.
    #[arg(value_name = "IDENTITY")]
    identity: String,

    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,

    #[command(flatten)]
    mutation: MutationOptions,
}

#[derive(Debug, Args)]
struct NewCommand {
    /// Application identity recorded in hegira.toml.
    #[arg(value_name = "NAME")]
    name: Option<String>,

    /// Directory that will own the generated application.
    #[arg(long, value_name = "PATH")]
    destination: Option<PathBuf>,

    /// Default database adapter.
    #[arg(long, value_enum)]
    database: Option<DatabaseChoice>,

    /// Browser client adapter.
    #[arg(long, value_enum)]
    client: Option<ClientChoice>,

    /// Official application component.
    #[arg(long, value_enum)]
    component: Option<ComponentChoice>,
}

#[derive(Debug, Args)]
struct InspectCommand {
    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,

    /// Emit the versioned machine-readable representation.
    #[arg(long)]
    json: bool,
}

#[derive(Debug)]
struct ResolvedNewCommand {
    name: String,
    destination: PathBuf,
    database: DatabaseChoice,
    client: ClientChoice,
    component: ComponentChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DatabaseChoice {
    Sqlite,
    Postgres,
}

impl DatabaseChoice {
    const fn adapter(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }

    const fn feature(self) -> &'static str {
        match self {
            Self::Sqlite => "db-sqlite",
            Self::Postgres => "db-postgres",
        }
    }

    const fn environment(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "development",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ClientChoice {
    Leptos,
}

impl ClientChoice {
    const fn adapter(self) -> &'static str {
        match self {
            Self::Leptos => "leptos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ComponentChoice {
    Identity,
}

impl ComponentChoice {
    const fn id(self) -> &'static str {
        match self {
            Self::Identity => "layered-leptos-identity",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Identity => "identity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliDiagnosticKind {
    Usage,
    Validation,
    Conflict,
    Internal,
}

impl CliDiagnosticKind {
    const fn exit(self) -> CliExit {
        match self {
            Self::Usage => CliExit::Usage,
            Self::Validation => CliExit::Validation,
            Self::Conflict => CliExit::Conflict,
            Self::Internal => CliExit::Internal,
        }
    }
}

#[derive(Debug)]
pub struct CliDiagnostic {
    kind: CliDiagnosticKind,
    message: String,
    hint: Option<String>,
}

impl CliDiagnostic {
    pub fn usage(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            kind: CliDiagnosticKind::Usage,
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: CliDiagnosticKind::Validation,
            message: message.into(),
            hint: None,
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: CliDiagnosticKind::Conflict,
            message: message.into(),
            hint: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: CliDiagnosticKind::Internal,
            message: message.into(),
            hint: None,
        }
    }
}

pub fn run_from<I, T>(
    arguments: I,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_from_mode(arguments, None, output, diagnostics)
}

pub fn run_interactive_from<I, T>(
    arguments: I,
    input: &mut impl BufRead,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_from_mode(arguments, Some(input), output, diagnostics)
}

fn run_from_mode<I, T>(
    arguments: I,
    input: Option<&mut dyn BufRead>,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) => return write_parser_result(error, output, diagnostics),
    };
    let working_directory = match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => {
            return write_diagnostic(
                CliDiagnostic::validation("cannot resolve the current working directory"),
                diagnostics,
            );
        }
    };

    run_command(
        cli.command,
        source_repository_root(),
        working_directory,
        input,
        output,
        diagnostics,
    )
}

fn run_command(
    command: CliCommand,
    repository_root: PathBuf,
    working_directory: PathBuf,
    input: Option<&mut dyn BufRead>,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    match command {
        CliCommand::New(command) => match resolve_new_command(command, input, output) {
            Ok(Some(command)) => create_application(command, repository_root, output, diagnostics),
            Ok(None) => CliExit::Success,
            Err(diagnostic) => write_diagnostic(diagnostic, diagnostics),
        },
        CliCommand::Inspect(command) => {
            inspect_application(command, working_directory, output, diagnostics)
        }
        CliCommand::Generate(command) => match command.command {
            GeneratorCommand::Resource(command) => {
                generate_resource(command, working_directory, output, diagnostics)
            }
            GeneratorCommand::Migration(command) => {
                generate_migration(command, working_directory, output, diagnostics)
            }
        },
    }
}

const RESOURCE_SOURCE_PATHS: [&str; 11] = [
    "crates/domain/src/lib.rs",
    "crates/application_contracts/src/lib.rs",
    "crates/application/src/lib.rs",
    "crates/infrastructure/src/lib.rs",
    "crates/infrastructure/src/identity/services.rs",
    "crates/presentation/src/lib.rs",
    "apps/server/src/server.rs",
    "apps/web/src/lib.rs",
    "apps/web/src/routes.rs",
    "apps/web/src/app/navigation.rs",
    "apps/web/src/shared/i18n/mod.rs",
];
const WEB_SIDEBAR_PATH: &str = "apps/web/src/app/sidebar.rs";
const MAX_GENERATION_SOURCE_BYTES: u64 = 4 * 1024 * 1024;

fn generate_resource(
    command: ResourceCommand,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let context = match resolve_mutation_context(command.application_root, working_directory) {
        Ok(context) => context,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    let manifest = context
        .manifest
        .as_ref()
        .expect("a compatible mutation context must contain a typed manifest");
    let sources = match read_resource_sources(&context.root) {
        Ok(sources) => sources,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    let source_names = match application_source_names(&context.root) {
        Ok(names) => names,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    let namespace = match ArtifactNamespace::new(
        &manifest.application,
        manifest.selection.components.iter(),
        source_names.iter(),
    ) {
        Ok(namespace) => namespace,
        Err(error) => {
            return write_diagnostic(CliDiagnostic::validation(error.to_string()), diagnostics);
        }
    };
    let mut naming = LayeredNamingInput::new(command.name);
    if let Some(plural) = command.plural {
        naming = naming.with_plural_type(plural);
    }
    if let Err(error) = LayeredArtifactNames::resolve(naming.clone(), &namespace) {
        let diagnostic = match error.kind() {
            NamingErrorKind::Collision => CliDiagnostic::conflict(error.to_string()),
            NamingErrorKind::InvalidIdentity
            | NamingErrorKind::ReservedIdentity
            | NamingErrorKind::InvalidPath => CliDiagnostic::validation(error.to_string()),
        };
        return write_diagnostic(diagnostic, diagnostics);
    }
    let input = ResourceSpecificationInput::new(
        naming,
        command
            .field
            .into_iter()
            .map(|field| ResourceFieldInput::new(field.name, field.scalar, field.nullable)),
    );
    let specification = match ResourceSpecification::resolve(input, &namespace, manifest) {
        Ok(specification) => specification,
        Err(error) => {
            return write_diagnostic(CliDiagnostic::validation(error.to_string()), diagnostics);
        }
    };
    let plan = match plan_complete_resource(&context.root, &specification, &sources) {
        Ok(plan) => plan,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    let exit = execute_mutation_plan(&context.root, &plan, command.mutation, output, diagnostics);
    if exit == CliExit::Success && !command.mutation.json() {
        let prefix = if command.mutation.dry_run() {
            "After applying the plan"
        } else {
            "Next"
        };
        if let Err(error) = writeln!(
            output,
            "{prefix}: review generated authorization and validation rules, apply the new migration, then run `cargo fmt --all` and the application checks."
        ) {
            return write_diagnostic(output_diagnostic(error), diagnostics);
        }
    }
    exit
}

struct ResourceSources {
    files: BTreeMap<&'static str, Vec<u8>>,
}

impl ResourceSources {
    fn get(&self, path: &'static str) -> &[u8] {
        self.files
            .get(path)
            .expect("all resource composition sources are loaded")
    }
}

fn read_resource_sources(root: &Path) -> Result<ResourceSources, CliDiagnostic> {
    let mut files = BTreeMap::new();
    for path in RESOURCE_SOURCE_PATHS.into_iter().chain([WEB_SIDEBAR_PATH]) {
        files.insert(path, read_application_source(root, path)?);
    }
    Ok(ResourceSources { files })
}

fn read_application_source(root: &Path, relative: &str) -> Result<Vec<u8>, CliDiagnostic> {
    let path = validate_application_path(root, relative, false)?;
    let metadata = fs::symlink_metadata(&path).expect("validated application source must exist");
    if metadata.len() > MAX_GENERATION_SOURCE_BYTES {
        return Err(CliDiagnostic::conflict(format!(
            "required application source `{relative}` exceeds the supported size limit"
        )));
    }
    fs::read(path).map_err(|_| {
        CliDiagnostic::conflict(format!(
            "required application source `{relative}` changed or became unreadable"
        ))
    })
}

fn validate_application_path(
    root: &Path,
    relative: &str,
    directory: bool,
) -> Result<PathBuf, CliDiagnostic> {
    let relative_path = Path::new(relative);
    let components = relative_path.components().collect::<Vec<_>>();
    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(component) = component else {
            return Err(CliDiagnostic::validation(format!(
                "application source path `{relative}` is not canonical"
            )));
        };
        path.push(component);
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            CliDiagnostic::conflict(format!(
                "required application source `{relative}` is missing or unreadable"
            ))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(CliDiagnostic::conflict(format!(
                "application source path `{relative}` contains a symlink"
            )));
        }
        let last = index + 1 == components.len();
        if (!last || directory) && !metadata.is_dir() {
            return Err(CliDiagnostic::conflict(format!(
                "application source path `{relative}` must be a directory"
            )));
        }
        if last && !directory && !metadata.is_file() {
            return Err(CliDiagnostic::conflict(format!(
                "required application source `{relative}` must be a regular file"
            )));
        }
    }
    Ok(path)
}

fn application_source_names(root: &Path) -> Result<Vec<String>, CliDiagnostic> {
    let mut names = Vec::new();
    for source_root in [
        "crates/domain/src",
        "crates/application_contracts/src",
        "crates/application/src",
        "crates/infrastructure/src",
        "crates/presentation/src",
        "apps/web/src",
    ] {
        let directory = validate_application_path(root, source_root, true)?;
        for entry in fs::read_dir(&directory).map_err(|_| {
            CliDiagnostic::conflict(format!(
                "application source directory `{source_root}` is unreadable"
            ))
        })? {
            let entry = entry.map_err(|_| {
                CliDiagnostic::conflict(format!(
                    "application source directory `{source_root}` changed while being inspected"
                ))
            })?;
            let file_type = entry.file_type().map_err(|_| {
                CliDiagnostic::conflict(format!(
                    "application source entry in `{source_root}` cannot be inspected"
                ))
            })?;
            if file_type.is_symlink() {
                return Err(CliDiagnostic::conflict(format!(
                    "application source directory `{source_root}` contains a symlink"
                )));
            }
            if !file_type.is_file()
                || entry.path().extension().and_then(|value| value.to_str()) != Some("rs")
            {
                continue;
            }
            let entry_path = entry.path();
            let Some(stem) = entry_path.file_stem().and_then(|value| value.to_str()) else {
                return Err(CliDiagnostic::validation(format!(
                    "application source directory `{source_root}` contains a non-UTF-8 Rust filename"
                )));
            };
            if stem != "lib" && stem != "main" {
                names.push(stem.to_owned());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn plan_complete_resource(
    root: &Path,
    specification: &ResourceSpecification,
    sources: &ResourceSources,
) -> Result<ChangePlan, CliDiagnostic> {
    let inward = plan_inward_resource_layers(
        specification,
        InwardLayerSources {
            domain_root: sources.get("crates/domain/src/lib.rs"),
            application_contracts_root: sources.get("crates/application_contracts/src/lib.rs"),
            application_root: sources.get("crates/application/src/lib.rs"),
        },
    )
    .map_err(inward_layer_diagnostic)?;
    let persistence = plan_resource_persistence(
        root,
        specification,
        PersistenceLayerSources {
            infrastructure_root: sources.get("crates/infrastructure/src/lib.rs"),
        },
    )
    .map_err(persistence_layer_diagnostic)?;
    let infrastructure_path = format!(
        "crates/infrastructure/src/{}.rs",
        specification.names().rust_module()
    );
    let infrastructure_resource = planned_content(persistence.plan(), &infrastructure_path)?;
    let http = plan_resource_http(
        specification,
        HttpLayerSources {
            presentation_root: sources.get("crates/presentation/src/lib.rs"),
            infrastructure_resource,
            infrastructure_services: sources.get("crates/infrastructure/src/identity/services.rs"),
            server_source: sources.get("apps/server/src/server.rs"),
        },
    )
    .map_err(http_layer_diagnostic)?;
    let server_source = planned_content(http.plan(), "apps/server/src/server.rs")?;
    let web = plan_resource_web(
        specification,
        WebLayerSources {
            web_root: sources.get("apps/web/src/lib.rs"),
            routes: sources.get("apps/web/src/routes.rs"),
            navigation: sources.get("apps/web/src/app/navigation.rs"),
            i18n: sources.get("apps/web/src/shared/i18n/mod.rs"),
            sidebar: sources.get(WEB_SIDEBAR_PATH),
            server_source,
        },
    )
    .map_err(web_layer_diagnostic)?;
    ChangePlan::compose([
        inward.into_plan(),
        persistence.into_plan(),
        http.into_plan(),
        web.into_plan(),
    ])
    .map_err(change_plan_diagnostic)
}

fn planned_content<'a>(plan: &'a ChangePlan, path: &str) -> Result<&'a [u8], CliDiagnostic> {
    plan.changes()
        .iter()
        .find(|change| change.path().as_str() == path)
        .map(PlannedFileChange::resulting_content)
        .ok_or_else(|| {
            CliDiagnostic::internal(format!(
                "resource emitter omitted required intermediate source `{path}`"
            ))
        })
}

fn inward_layer_diagnostic(error: InwardLayerError) -> CliDiagnostic {
    match error.kind() {
        InwardLayerErrorKind::Planning => {
            CliDiagnostic::internal(format!("cannot construct inward resource layers: {error}"))
        }
        InwardLayerErrorKind::StructuredEdit | InwardLayerErrorKind::ExistingRegistration => {
            CliDiagnostic::conflict(error.to_string())
        }
    }
}

fn persistence_layer_diagnostic(error: PersistenceLayerError) -> CliDiagnostic {
    match error.kind() {
        PersistenceLayerErrorKind::Planning => {
            CliDiagnostic::internal(format!("cannot construct resource persistence: {error}"))
        }
        PersistenceLayerErrorKind::Migration
        | PersistenceLayerErrorKind::StructuredEdit
        | PersistenceLayerErrorKind::ExistingRegistration => {
            CliDiagnostic::conflict(error.to_string())
        }
    }
}

fn http_layer_diagnostic(error: HttpLayerError) -> CliDiagnostic {
    match error.kind() {
        HttpLayerErrorKind::Planning => {
            CliDiagnostic::internal(format!("cannot construct resource HTTP adapter: {error}"))
        }
        HttpLayerErrorKind::StructuredEdit | HttpLayerErrorKind::ExistingRegistration => {
            CliDiagnostic::conflict(error.to_string())
        }
    }
}

fn web_layer_diagnostic(error: WebLayerError) -> CliDiagnostic {
    match error.kind() {
        WebLayerErrorKind::UnsupportedClient => CliDiagnostic::validation(error.to_string()),
        WebLayerErrorKind::Planning => {
            CliDiagnostic::internal(format!("cannot construct resource web adapter: {error}"))
        }
        WebLayerErrorKind::StructuredEdit
        | WebLayerErrorKind::ExistingRegistration
        | WebLayerErrorKind::RouteConflict
        | WebLayerErrorKind::LocalizationConflict => CliDiagnostic::conflict(error.to_string()),
    }
}

fn generate_migration(
    command: MigrationCommand,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let context = match resolve_mutation_context(command.application_root, working_directory) {
        Ok(context) => context,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    let manifest = context
        .manifest
        .as_ref()
        .expect("a compatible mutation context must contain a typed manifest");
    let selection = match ResourceSelection::resolve(manifest) {
        Ok(selection) => selection,
        Err(error) => {
            return write_diagnostic(CliDiagnostic::validation(error.to_string()), diagnostics);
        }
    };
    let identity = match MigrationIdentity::new(command.identity) {
        Ok(identity) => identity,
        Err(error) => return write_diagnostic(migration_diagnostic(error), diagnostics),
    };
    let migration = match plan_application_migration(&context.root, selection.database(), identity)
    {
        Ok(migration) => migration,
        Err(error) => return write_diagnostic(migration_diagnostic(error), diagnostics),
    };

    execute_mutation_plan(
        &context.root,
        migration.plan(),
        command.mutation,
        output,
        diagnostics,
    )
}

fn resolve_mutation_context(
    application_root: Option<PathBuf>,
    working_directory: PathBuf,
) -> Result<ApplicationContext, CliDiagnostic> {
    let policy = MutationCompatibilityPolicy::for_current_release().map_err(|error| {
        CliDiagnostic::internal(format!(
            "cannot establish the CLI compatibility policy: {error}"
        ))
    })?;
    let request = match application_root {
        Some(root) => ApplicationContextRequest::explicit(working_directory, root),
        None => ApplicationContextRequest::discover_from(working_directory),
    };
    let context =
        resolve_application_context(&request, &policy).map_err(|error| match error.kind() {
            ApplicationContextErrorKind::Validation => CliDiagnostic::validation(error.to_string()),
            ApplicationContextErrorKind::Conflict => CliDiagnostic::conflict(error.to_string()),
        })?;
    match &context.compatibility {
        MutationCompatibility::Compatible if context.manifest.is_some() => Ok(context),
        MutationCompatibility::Compatible => Err(CliDiagnostic::internal(
            "compatible application context has no typed manifest",
        )),
        MutationCompatibility::Incompatible(issue) | MutationCompatibility::Unsupported(issue) => {
            Err(CliDiagnostic::conflict(format!(
                "application cannot be mutated: {issue}"
            )))
        }
    }
}

fn migration_diagnostic(error: MigrationError) -> CliDiagnostic {
    match error.kind() {
        MigrationErrorKind::InvalidIdentity | MigrationErrorKind::UnsafeApplication => {
            CliDiagnostic::validation(error.to_string())
        }
        MigrationErrorKind::InvalidHistory
        | MigrationErrorKind::IdentityCollision
        | MigrationErrorKind::VersionExhausted
        | MigrationErrorKind::InvalidState => CliDiagnostic::conflict(error.to_string()),
        MigrationErrorKind::Planning => {
            CliDiagnostic::internal(format!("cannot construct migration change plan: {error}"))
        }
    }
}

#[derive(Serialize)]
struct InspectionOutput<'a> {
    output_schema: u32,
    application_root: &'a str,
    manifest: Option<&'a application_manifest::ApplicationManifest>,
    mutation_compatibility: &'a MutationCompatibility,
}

fn inspect_application(
    command: InspectCommand,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let policy = match MutationCompatibilityPolicy::for_current_release() {
        Ok(policy) => policy,
        Err(error) => {
            return write_diagnostic(
                CliDiagnostic::internal(format!(
                    "cannot establish the CLI compatibility policy: {error}"
                )),
                diagnostics,
            );
        }
    };
    let request = match command.application_root {
        Some(root) => ApplicationContextRequest::explicit(working_directory, root),
        None => ApplicationContextRequest::discover_from(working_directory),
    };
    let context = match resolve_application_context(&request, &policy) {
        Ok(context) => context,
        Err(error) => {
            let diagnostic = match error.kind() {
                ApplicationContextErrorKind::Validation => {
                    CliDiagnostic::validation(error.to_string())
                }
                ApplicationContextErrorKind::Conflict => CliDiagnostic::conflict(error.to_string()),
            };
            return write_diagnostic(diagnostic, diagnostics);
        }
    };

    let rendered = if command.json {
        let root = context
            .root
            .to_str()
            .expect("application context paths are validated as UTF-8");
        serde_json::to_string_pretty(&InspectionOutput {
            output_schema: 1,
            application_root: root,
            manifest: context.manifest.as_ref(),
            mutation_compatibility: &context.compatibility,
        })
        .map_err(|error| {
            CliDiagnostic::internal(format!("cannot serialize application inspection: {error}"))
        })
    } else {
        Ok(render_human_inspection(&context))
    };
    let rendered = match rendered {
        Ok(rendered) => rendered,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    if writeln!(output, "{rendered}").is_err() {
        return CliExit::Internal;
    }
    CliExit::Success
}

fn render_human_inspection(context: &ApplicationContext) -> String {
    let mut output = String::new();
    if let Some(manifest) = &context.manifest {
        let components = manifest
            .selection
            .components
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let databases = manifest
            .selection
            .databases
            .iter()
            .map(|database| match database {
                DatabaseAdapter::Postgres => "postgres",
                DatabaseAdapter::Sqlite => "sqlite",
            })
            .collect::<Vec<_>>()
            .join(", ");
        let clients = manifest
            .selection
            .clients
            .iter()
            .map(|client| match client {
                ClientAdapter::Leptos => "leptos",
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(output, "Application: {}", manifest.application).unwrap();
        writeln!(output, "Root: {}", context.root.display()).unwrap();
        writeln!(output, "Manifest schema: {}", manifest.schema).unwrap();
        writeln!(
            output,
            "Framework: {} @ {}",
            manifest.framework.repository, manifest.framework.version
        )
        .unwrap();
        writeln!(output, "Components: {components}").unwrap();
        writeln!(output, "Databases: {databases}").unwrap();
        writeln!(output, "Clients: {clients}").unwrap();
    } else {
        writeln!(output, "Root: {}", context.root.display()).unwrap();
        writeln!(output, "Manifest: unsupported by the current parser").unwrap();
    }
    match &context.compatibility {
        MutationCompatibility::Compatible => {
            write!(output, "Mutation compatibility: compatible").unwrap();
        }
        MutationCompatibility::Incompatible(issue) => {
            write!(output, "Mutation compatibility: incompatible ({issue})").unwrap();
        }
        MutationCompatibility::Unsupported(issue) => {
            write!(output, "Mutation compatibility: unsupported ({issue})").unwrap();
        }
    }
    output
}

fn resolve_new_command(
    command: NewCommand,
    input: Option<&mut dyn BufRead>,
    output: &mut impl Write,
) -> Result<Option<ResolvedNewCommand>, CliDiagnostic> {
    let guided = command.name.is_none() || command.destination.is_none();
    if !guided {
        return Ok(Some(ResolvedNewCommand {
            name: command.name.expect("complete command should have a name"),
            destination: command
                .destination
                .expect("complete command should have a destination"),
            database: command.database.unwrap_or(DatabaseChoice::Sqlite),
            client: command.client.unwrap_or(ClientChoice::Leptos),
            component: command.component.unwrap_or(ComponentChoice::Identity),
        }));
    }

    let Some(input) = input else {
        return Err(CliDiagnostic::usage(
            "non-interactive application creation requires a name and destination",
            "provide `hegira new <NAME> --destination <PATH>` or run the command in a terminal",
        ));
    };

    let Some(name) = resolve_name(command.name, input, output)? else {
        return cancel(output);
    };
    template_renderer::validate_project_identity(&name).map_err(renderer_diagnostic)?;
    let Some(destination) = resolve_destination(command.destination, &name, input, output)? else {
        return cancel(output);
    };
    template_renderer::validate_destination(&destination).map_err(renderer_diagnostic)?;
    let Some(database) = resolve_database(command.database, input, output)? else {
        return cancel(output);
    };
    let Some(client) = resolve_client(command.client, input, output)? else {
        return cancel(output);
    };
    let Some(component) = resolve_component(command.component, input, output)? else {
        return cancel(output);
    };

    writeln!(output, "\nApplication summary:")
        .and_then(|()| writeln!(output, "  Name: {name}"))
        .and_then(|()| writeln!(output, "  Destination: {}", destination.display()))
        .and_then(|()| writeln!(output, "  Database: {}", database.adapter()))
        .and_then(|()| writeln!(output, "  Client: {}", client.adapter()))
        .and_then(|()| writeln!(output, "  Component: {}", component.name()))
        .map_err(output_diagnostic)?;

    match confirm(input, output)? {
        Some(true) => Ok(Some(ResolvedNewCommand {
            name,
            destination,
            database,
            client,
            component,
        })),
        Some(false) | None => cancel(output),
    }
}

fn resolve_name(
    value: Option<String>,
    input: &mut dyn BufRead,
    output: &mut impl Write,
) -> Result<Option<String>, CliDiagnostic> {
    if value.is_some() {
        return Ok(value);
    }
    loop {
        let Some(value) = prompt(input, output, "Application name: ")? else {
            return Ok(None);
        };
        if !value.is_empty() {
            return Ok(Some(value));
        }
        writeln!(output, "Please enter an application name.").map_err(output_diagnostic)?;
    }
}

fn resolve_destination(
    value: Option<PathBuf>,
    name: &str,
    input: &mut dyn BufRead,
    output: &mut impl Write,
) -> Result<Option<PathBuf>, CliDiagnostic> {
    if value.is_some() {
        return Ok(value);
    }
    let prompt_text = format!("Destination [{name}]: ");
    Ok(prompt(input, output, &prompt_text)?.map(|value| {
        if value.is_empty() {
            PathBuf::from(name)
        } else {
            PathBuf::from(value)
        }
    }))
}

fn resolve_database(
    value: Option<DatabaseChoice>,
    input: &mut dyn BufRead,
    output: &mut impl Write,
) -> Result<Option<DatabaseChoice>, CliDiagnostic> {
    if value.is_some() {
        return Ok(value);
    }
    loop {
        let Some(value) = prompt(input, output, "Database [sqlite] (sqlite/postgres): ")? else {
            return Ok(None);
        };
        match value.to_ascii_lowercase().as_str() {
            "" | "sqlite" => return Ok(Some(DatabaseChoice::Sqlite)),
            "postgres" => return Ok(Some(DatabaseChoice::Postgres)),
            _ => writeln!(output, "Please choose `sqlite` or `postgres`.")
                .map_err(output_diagnostic)?,
        }
    }
}

fn resolve_client(
    value: Option<ClientChoice>,
    input: &mut dyn BufRead,
    output: &mut impl Write,
) -> Result<Option<ClientChoice>, CliDiagnostic> {
    if value.is_some() {
        return Ok(value);
    }
    loop {
        let Some(value) = prompt(input, output, "Client [leptos]: ")? else {
            return Ok(None);
        };
        match value.to_ascii_lowercase().as_str() {
            "" | "leptos" => return Ok(Some(ClientChoice::Leptos)),
            _ => writeln!(output, "The currently supported client is `leptos`.")
                .map_err(output_diagnostic)?,
        }
    }
}

fn resolve_component(
    value: Option<ComponentChoice>,
    input: &mut dyn BufRead,
    output: &mut impl Write,
) -> Result<Option<ComponentChoice>, CliDiagnostic> {
    if value.is_some() {
        return Ok(value);
    }
    loop {
        let Some(value) = prompt(input, output, "Component [identity]: ")? else {
            return Ok(None);
        };
        match value.to_ascii_lowercase().as_str() {
            "" | "identity" => return Ok(Some(ComponentChoice::Identity)),
            _ => writeln!(output, "The currently supported component is `identity`.")
                .map_err(output_diagnostic)?,
        }
    }
}

fn confirm(
    input: &mut dyn BufRead,
    output: &mut impl Write,
) -> Result<Option<bool>, CliDiagnostic> {
    loop {
        let Some(value) = prompt(input, output, "Create application? [Y/n]: ")? else {
            return Ok(None);
        };
        match value.to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(Some(true)),
            "n" | "no" => return Ok(Some(false)),
            _ => writeln!(output, "Please answer `yes` or `no`.").map_err(output_diagnostic)?,
        }
    }
}

fn prompt(
    input: &mut dyn BufRead,
    output: &mut impl Write,
    message: &str,
) -> Result<Option<String>, CliDiagnostic> {
    write!(output, "{message}")
        .and_then(|()| output.flush())
        .map_err(output_diagnostic)?;
    let mut value = String::new();
    match input.read_line(&mut value) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(value.trim_end_matches(['\r', '\n']).to_string())),
        Err(error) if error.kind() == IoErrorKind::Interrupted => Ok(None),
        Err(error) => Err(CliDiagnostic::internal(format!(
            "failed to read interactive input: {error}"
        ))),
    }
}

fn cancel(output: &mut impl Write) -> Result<Option<ResolvedNewCommand>, CliDiagnostic> {
    writeln!(output, "\nCancelled; no files were written.").map_err(output_diagnostic)?;
    Ok(None)
}

fn output_diagnostic(error: std::io::Error) -> CliDiagnostic {
    CliDiagnostic::internal(format!("failed to write command output: {error}"))
}

fn create_application(
    command: ResolvedNewCommand,
    repository_root: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    if let Err(error) = template_renderer::validate_project_identity(&command.name)
        .and_then(|()| template_renderer::validate_destination(&command.destination))
    {
        return write_diagnostic(renderer_diagnostic(error), diagnostics);
    }
    let mut variables = BTreeMap::new();
    variables.insert("application_name".to_string(), command.name.clone());
    variables.insert(
        "client_adapter".to_string(),
        command.client.adapter().to_string(),
    );
    variables.insert(
        "component_id".to_string(),
        command.component.id().to_string(),
    );
    variables.insert(
        "database_adapter".to_string(),
        command.database.adapter().to_string(),
    );
    variables.insert(
        "database_feature".to_string(),
        command.database.feature().to_string(),
    );

    let request = RenderRequest {
        repository_root,
        template: "layered".to_string(),
        output: command.destination.clone(),
        variables,
    };
    if let Err(error) = render(&request) {
        return write_diagnostic(renderer_diagnostic(error), diagnostics);
    }

    let destination = command.destination.display();
    let database = command.database.environment();
    if writeln!(output, "Created {} at {destination}", command.name).is_err()
        || writeln!(output).is_err()
        || writeln!(output, "Next steps:").is_err()
        || writeln!(output, "  cd -- '{}'", command.destination.to_string_lossy().replace('\'', "'\\''")).is_err()
        || writeln!(output, "  rustup target add wasm32-unknown-unknown").is_err()
        || writeln!(output, "  cargo install cargo-leptos").is_err()
        || writeln!(output, "  npm ci --prefix apps/web/src").is_err()
        || writeln!(
            output,
            "  APP_ENV={database} cargo leptos watch -p app_server --bin-features ssr,{} --lib-features hydrate --bin-cargo-args=--locked --lib-cargo-args=--locked",
            command.database.feature()
        )
        .is_err()
    {
        return CliExit::Internal;
    }

    CliExit::Success
}

fn renderer_diagnostic(error: RendererError) -> CliDiagnostic {
    match error.kind() {
        RendererErrorKind::ApplicationManifest
        | RendererErrorKind::Variables
        | RendererErrorKind::Safety => CliDiagnostic::validation(error.to_string()),
        RendererErrorKind::Conflict => CliDiagnostic::conflict(error.to_string()),
        _ => CliDiagnostic::internal(format!("application generation failed: {error}")),
    }
}

fn source_repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("hegira_cli should live under the repository tools directory")
        .to_path_buf()
}

fn write_parser_result(
    error: clap::Error,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let informational = matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    );
    let target: &mut dyn Write = if informational { output } else { diagnostics };
    if write!(target, "{error}").is_err() {
        return CliExit::Internal;
    }
    if informational {
        CliExit::Success
    } else {
        CliExit::Usage
    }
}

pub fn write_diagnostic(diagnostic: CliDiagnostic, target: &mut impl Write) -> CliExit {
    if writeln!(target, "error: {}", diagnostic.message).is_err() {
        return CliExit::Internal;
    }
    if let Some(hint) = diagnostic.hint
        && writeln!(target, "hint: {hint}").is_err()
    {
        return CliExit::Internal;
    }
    diagnostic.kind.exit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_catalog_failure_is_internal_and_leaves_destination_untouched() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("hegira-cli-catalog-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let destination = root.join("application");
        let cli = Cli::try_parse_from([
            "hegira",
            "new",
            "safe-app",
            "--destination",
            destination.to_str().unwrap(),
        ])
        .unwrap();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit = run_command(
            cli.command,
            root.join("missing-source"),
            root.clone(),
            None,
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(exit, CliExit::Internal);
        assert!(stdout.is_empty());
        let diagnostic = String::from_utf8(stderr).unwrap();
        assert!(diagnostic.starts_with("error: application generation failed:"));
        assert!(!diagnostic.contains("stack backtrace"));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(CliExit::Success.code(), 0);
        assert_eq!(CliExit::Internal.code(), 1);
        assert_eq!(CliExit::Usage.code(), 2);
        assert_eq!(CliExit::Validation.code(), 3);
        assert_eq!(CliExit::Conflict.code(), 4);
    }

    #[test]
    fn validation_diagnostics_are_concise_and_do_not_render_backtraces() {
        let mut diagnostics = Vec::new();

        let exit = write_diagnostic(
            CliDiagnostic::validation("application name is invalid"),
            &mut diagnostics,
        );

        assert_eq!(exit, CliExit::Validation);
        assert_eq!(
            String::from_utf8(diagnostics).expect("diagnostic should be UTF-8"),
            "error: application name is invalid\n"
        );
    }

    #[test]
    fn conflict_and_internal_diagnostics_have_distinct_outcomes() {
        let mut conflict = Vec::new();
        let mut internal = Vec::new();

        assert_eq!(
            write_diagnostic(CliDiagnostic::conflict("destination exists"), &mut conflict),
            CliExit::Conflict
        );
        assert_eq!(
            write_diagnostic(CliDiagnostic::internal("generation failed"), &mut internal),
            CliExit::Internal
        );
    }
}
