//! Public application operations; previews never acquire execution authority.

use std::{io::Write, path::PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;

use super::{
    OperationError, OperationErrorKind, OperationIntent, OperationPlan, OperationPlanSummary,
    OperationRequest, RuntimeProfile,
    execution::{
        ChildOutput, DatabaseExecutionApproval, ExecutionConsent, ExecutionControl,
        ExecutionReport, TrustedToolchain, execute_application_operation,
        execute_database_operation,
    },
    plan_application_operation,
};
use crate::{ApplicationContextRequest, CliDiagnostic, CliExit, write_diagnostic};

#[derive(Debug, Args)]
pub(crate) struct ValidationCommand {
    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,

    /// Review the typed plan without probing tools, compiling, testing, or starting a server.
    #[arg(long, required_unless_present = "execute", conflicts_with = "execute")]
    dry_run: bool,

    /// Execute the reviewed operation; requires explicit trust and tool selection.
    #[arg(long, requires_all = ["trust_application", "cargo", "tool_directory"])]
    execute: bool,

    /// Trust application code, build scripts, tests, Cargo configuration, tools and environment.
    #[arg(long, requires = "execute")]
    trust_application: bool,

    /// Absolute trusted Cargo executable, outside the application (Linux only).
    #[arg(long, value_name = "PATH", requires = "execute")]
    cargo: Option<PathBuf>,

    /// Absolute trusted auxiliary PATH directory, outside the application; repeat as needed.
    #[arg(long, value_name = "PATH", requires = "execute")]
    tool_directory: Vec<PathBuf>,

    /// Emit a versioned redacted report; discard arbitrary child output during execution.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct DevelopmentCommand {
    #[command(flatten)]
    options: ValidationCommand,

    /// Absolute, explicitly trusted wasm-bindgen CLI matching the application's Cargo.lock.
    #[arg(
        long,
        value_name = "PATH",
        required_if_eq("execute", "true"),
        requires = "execute"
    )]
    wasm_bindgen: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct BuildCommand {
    /// Only production-bundle compilation is supported; this does not run or deploy it.
    #[arg(long, required = true)]
    release: bool,

    #[command(flatten)]
    frontend: DevelopmentCommand,

    /// Absolute trusted Binaryen wasm-opt 123 executable; never downloaded automatically.
    #[arg(
        long,
        value_name = "PATH",
        required_if_eq("execute", "true"),
        requires = "execute"
    )]
    wasm_opt: Option<PathBuf>,
}

#[derive(Default)]
struct ExecutionOptions {
    wasm_bindgen: Option<PathBuf>,
    wasm_opt: Option<PathBuf>,
    database_approval: Option<DatabaseExecutionApproval>,
}

#[derive(Debug, Args)]
pub(crate) struct DatabaseCommand {
    #[command(subcommand)]
    command: DatabaseAction,
}

#[derive(Debug, Subcommand)]
enum DatabaseAction {
    /// Inspect composed migration history; never provision, seed, or migrate.
    Status(DatabaseOptions),
    /// Apply only forward migrations to an existing, explicitly selected database.
    Migrate(DatabaseMigrateOptions),
}

#[derive(Debug, Args)]
struct DatabaseOptions {
    /// Explicit baseline profile matching the application's recorded provider.
    #[arg(long, value_enum)]
    profile: RuntimeProfile,
    #[command(flatten)]
    options: ValidationCommand,
}

#[derive(Debug, Args)]
struct DatabaseMigrateOptions {
    #[command(flatten)]
    database: DatabaseOptions,
    /// Separately approve production migration after reviewing the target and backups.
    #[arg(long, requires = "execute")]
    approve_production_migration: bool,
}

pub(crate) fn run_database(
    command: DatabaseCommand,
    repository: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let (options, intent, approval) = match command.command {
        DatabaseAction::Status(command) => (
            command.options,
            OperationIntent::DatabaseStatus {
                profile: command.profile,
            },
            DatabaseExecutionApproval::Ordinary,
        ),
        DatabaseAction::Migrate(command) => (
            command.database.options,
            OperationIntent::DatabaseMigrate {
                profile: command.database.profile,
            },
            if command.approve_production_migration {
                DatabaseExecutionApproval::ProductionMigration
            } else {
                DatabaseExecutionApproval::Ordinary
            },
        ),
    };
    run_operation(
        options,
        intent,
        ExecutionOptions {
            database_approval: Some(approval),
            ..Default::default()
        },
        repository,
        working_directory,
        output,
        diagnostics,
    )
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Preview,
    Execute,
}

/// One envelope for previews, successful execution, and typed failures.
/// Parser/usage errors retain the CLI's stderr contract.
#[derive(Serialize)]
struct Report<'a> {
    output_schema: u32,
    mode: Mode,
    plan: Option<&'a OperationPlanSummary>,
    execution: Option<&'a ExecutionReport>,
    diagnostics: Vec<&'a OperationError>,
}

pub(crate) fn run(
    command: ValidationCommand,
    intent: OperationIntent,
    repository: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    run_operation(
        command,
        intent,
        ExecutionOptions::default(),
        repository,
        working_directory,
        output,
        diagnostics,
    )
}

pub(crate) fn run_development(
    command: DevelopmentCommand,
    repository: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    run_operation(
        command.options,
        OperationIntent::Develop,
        ExecutionOptions {
            wasm_bindgen: command.wasm_bindgen,
            wasm_opt: None,
            database_approval: None,
        },
        repository,
        working_directory,
        output,
        diagnostics,
    )
}

pub(crate) fn run_build(
    command: BuildCommand,
    repository: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    run_operation(
        command.frontend.options,
        OperationIntent::ReleaseBuild,
        ExecutionOptions {
            wasm_bindgen: command.frontend.wasm_bindgen,
            wasm_opt: command.wasm_opt,
            database_approval: None,
        },
        repository,
        working_directory,
        output,
        diagnostics,
    )
}

fn run_operation(
    command: ValidationCommand,
    intent: OperationIntent,
    frontend: ExecutionOptions,
    repository: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let request = OperationRequest {
        application: ApplicationContextRequest {
            working_directory,
            application_root: command.application_root.clone(),
        },
        intent,
    };
    let plan = match plan_application_operation(&repository, &request) {
        Ok(plan) => plan,
        Err(error) => return emit(&command, None, Err(error), output, diagnostics),
    };
    if command.dry_run {
        return emit(&command, Some(&plan), Ok(None), output, diagnostics);
    }
    let result = execute(&command, &repository, &plan, &frontend).map(Some);
    emit(&command, Some(&plan), result, output, diagnostics)
}

fn execute(
    command: &ValidationCommand,
    repository: &std::path::Path,
    plan: &OperationPlan,
    frontend: &ExecutionOptions,
) -> Result<ExecutionReport, OperationError> {
    // Defense in depth for callers other than Clap. No default or preview grants consent.
    if !command.execute || !command.trust_application || command.dry_run {
        return Err(OperationError::new(
            OperationErrorKind::Validation,
            "execution-consent",
            "Execution requires --execute and --trust-application; a preview grants no authority.",
        ));
    }
    if let Some(approval) = frontend.database_approval {
        approval.validate(plan)?;
    }
    let cargo = command.cargo.as_deref().ok_or_else(|| {
        OperationError::new(
            OperationErrorKind::Validation,
            "execution-tool-selection",
            "Select an absolute trusted --cargo executable and --tool-directory entries.",
        )
    })?;
    let mut toolchain = TrustedToolchain::resolve(cargo, &command.tool_directory)?;
    if matches!(
        plan.summary().intent,
        OperationIntent::Develop | OperationIntent::ReleaseBuild
    ) {
        let wasm_bindgen = frontend.wasm_bindgen.as_deref().ok_or_else(|| {
            OperationError::new(
                OperationErrorKind::Validation,
                "development-tool-selection",
                "Leptos execution requires an absolute trusted --wasm-bindgen matching Cargo.lock.",
            )
        })?;
        let proxy = std::env::current_exe().map_err(|_| {
            OperationError::new(
                OperationErrorKind::Internal,
                "development-cargo-proxy",
                "Cannot resolve the source-built Hegira Cargo proxy.",
            )
        })?;
        toolchain = toolchain.with_development_tools(wasm_bindgen, &proxy)?;
        if plan.summary().intent == OperationIntent::ReleaseBuild {
            let wasm_opt = frontend.wasm_opt.as_deref().ok_or_else(|| OperationError::new(
                OperationErrorKind::Validation,
                "release-tool-selection",
                "Release execution requires an absolute trusted --wasm-opt version 123; install it explicitly.",
            ))?;
            toolchain = toolchain.with_release_optimizer(wasm_opt)?;
        }
    }
    let control = ExecutionControl::default();
    let _signals = CommandSignals::register(&control)?;
    if let Some(approval) = frontend.database_approval {
        return execute_database_operation(
            repository,
            plan,
            ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
            approval,
            &toolchain,
            &control,
        );
    }
    execute_application_operation(
        repository,
        plan,
        ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
        &toolchain,
        &control,
        if command.json {
            ChildOutput::Discard
        } else {
            ChildOutput::Inherit
        },
    )
}

fn emit(
    command: &ValidationCommand,
    plan: Option<&OperationPlan>,
    result: Result<Option<ExecutionReport>, OperationError>,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let exit = match &result {
        Ok(Some(report)) => report.exit(),
        Ok(None) => CliExit::Success,
        Err(error) => error.exit(),
    };
    let report = Report {
        output_schema: 1,
        mode: if command.dry_run {
            Mode::Preview
        } else {
            Mode::Execute
        },
        plan: plan.map(OperationPlan::summary),
        execution: result.as_ref().ok().and_then(Option::as_ref),
        diagnostics: result.as_ref().err().into_iter().collect(),
    };
    let written = if command.json {
        serde_json::to_string_pretty(&report)
            .ok()
            .is_some_and(|json| writeln!(output, "{json}").is_ok())
    } else {
        match result {
            Ok(None) => plan.is_some_and(|plan| write!(output, "{}", plan.render_human()).is_ok()),
            Ok(Some(execution)) => {
                let written = writeln!(
                    output,
                    "Application operation {:?}: {:?}\nCompleted steps: {}/{}",
                    execution.intent,
                    execution.outcome,
                    execution.completed_steps,
                    plan.map_or(0, |plan| plan.summary().steps.len()),
                )
                .is_ok();
                let written = written
                    && execution.database.as_ref().is_none_or(|database| {
                        writeln!(output, "{}", database.render_human()).is_ok()
                    });
                written && execution.artifacts.as_ref().is_none_or(|artifacts| {
                    writeln!(output, "Verified artifacts (application-relative; owner: {}):\nServer: {}\nSite: {}\nBrowser WASM: {}",
                        artifacts.owner, artifacts.server, artifacts.site, artifacts.browser_wasm).is_ok()
                })
            }
            Err(error) => writeln!(diagnostics, "{error}").is_ok(),
        }
    };
    if written {
        exit
    } else {
        write_diagnostic(
            CliDiagnostic::internal("cannot write the application operation result"),
            diagnostics,
        )
    }
}

// Safe atomic flag registration avoids custom async-signal-unsafe handlers,
// polling threads, or an async runtime. Registration occurs only after consent
// and trusted tool resolution. Both SIGINT and SIGTERM use the executor's
// existing group cleanup. Dropping unregisters our actions on every return path.
#[cfg(target_os = "linux")]
pub(crate) struct CommandSignals(Vec<signal_hook::SigId>);

#[cfg(target_os = "linux")]
impl CommandSignals {
    pub(crate) fn register(control: &ExecutionControl) -> Result<Self, OperationError> {
        let mut guard = Self(Vec::new());
        for (signal, state) in [
            (signal_hook::consts::SIGINT, 1),
            (signal_hook::consts::SIGTERM, 2),
        ] {
            let id = signal_hook::flag::register_usize(signal, control.signal_state(), state)
                .map_err(|_| {
                    OperationError::new(
                        OperationErrorKind::Internal,
                        "execution-signals",
                        "Cannot establish operation signal handling; no child was started.",
                    )
                })?;
            guard.0.push(id);
        }
        Ok(guard)
    }
}

#[cfg(target_os = "linux")]
impl Drop for CommandSignals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

#[cfg(not(target_os = "linux"))]
struct CommandSignals;

#[cfg(not(target_os = "linux"))]
impl CommandSignals {
    fn register(_: &ExecutionControl) -> Result<Self, OperationError> {
        Err(OperationError::new(
            OperationErrorKind::Validation,
            "execution-platform",
            "Anchored process execution requires Linux and an accessible /proc/self/fd.",
        ))
    }
}
