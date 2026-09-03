use std::{ffi::OsString, io::Write};

use clap::{Args, Parser, Subcommand, error::ErrorKind};

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
struct NewCommand {}

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

    match cli.command {
        CliCommand::New(_) => write_diagnostic(
            CliDiagnostic::usage(
                "application generation arguments are not available in this CLI foundation",
                "run `hegira new --help` to inspect the command contract",
            ),
            diagnostics,
        ),
    }
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
