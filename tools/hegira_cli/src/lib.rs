use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{BufRead, ErrorKind as IoErrorKind, Write},
    path::{Path, PathBuf},
};

use clap::{Args, Parser, Subcommand, ValueEnum, error::ErrorKind};
use template_renderer::{RenderRequest, RendererError, RendererErrorKind, render};

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

    run_command(
        cli.command,
        source_repository_root(),
        input,
        output,
        diagnostics,
    )
}

fn run_command(
    command: CliCommand,
    repository_root: PathBuf,
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
    }
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
            "  APP_ENV={database} cargo leptos watch -p app_server --bin-features ssr,{} --lib-features hydrate",
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
