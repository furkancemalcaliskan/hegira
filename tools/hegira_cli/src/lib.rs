use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::Write,
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
    name: String,

    /// Directory that will own the generated application.
    #[arg(long, value_name = "PATH")]
    destination: PathBuf,

    /// Default database adapter.
    #[arg(long, value_enum, default_value_t = DatabaseChoice::Sqlite)]
    database: DatabaseChoice,

    /// Browser client adapter.
    #[arg(long, value_enum, default_value_t = ClientChoice::Leptos)]
    client: ClientChoice,

    /// Official application component.
    #[arg(long, value_enum, default_value_t = ComponentChoice::Identity)]
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
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error) => return write_parser_result(error, output, diagnostics),
    };

    run_command(cli.command, source_repository_root(), output, diagnostics)
}

fn run_command(
    command: CliCommand,
    repository_root: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    match command {
        CliCommand::New(command) => {
            create_application(command, repository_root, output, diagnostics)
        }
    }
}

fn create_application(
    command: NewCommand,
    repository_root: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
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
        return write_diagnostic(
            renderer_diagnostic(error, &command.destination),
            diagnostics,
        );
    }

    let destination = command.destination.display();
    let database = command.database.environment();
    if writeln!(output, "Created {} at {destination}", command.name).is_err()
        || writeln!(output).is_err()
        || writeln!(output, "Next steps:").is_err()
        || writeln!(output, "  cd {destination}").is_err()
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

fn renderer_diagnostic(error: RendererError, destination: &Path) -> CliDiagnostic {
    match error.kind() {
        RendererErrorKind::ApplicationManifest | RendererErrorKind::Variables => {
            CliDiagnostic::validation(error.to_string())
        }
        RendererErrorKind::Output if destination.exists() => CliDiagnostic::conflict(format!(
            "destination already exists: {}",
            destination.display()
        )),
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
