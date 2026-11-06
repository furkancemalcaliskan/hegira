use std::{
    env,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

use application_manifest::{DatabaseAdapter, MutationCompatibility, MutationCompatibilityPolicy};
use application_mutator::MUTATION_MARKER;
use clap::{Args, ValueEnum};
use serde::Serialize;

use crate::{
    ApplicationContext, ApplicationContextErrorKind, ApplicationContextRequest, CliDiagnostic,
    CliExit, CompositionInspectionStatus, inspect_composition,
    operations::{
        OperationIntent, OperationRequest, RuntimeProfile, plan_application_operation,
        readiness::{
            ReadinessCheck as DoctorCheck, ReadinessStatus as CheckStatus, ReadinessTools,
        },
    },
    resolve_application_context, write_diagnostic,
};

pub const DOCTOR_OUTPUT_SCHEMA: u32 = 1;
const MAX_SOURCE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Args)]
pub(crate) struct DoctorCommand {
    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,

    /// Emit a versioned, machine-readable diagnostic report.
    #[arg(long)]
    json: bool,

    /// Diagnose one closed application operation without executing it.
    #[arg(long, value_enum)]
    operation: Option<DoctorOperation>,

    /// Required only for database operation diagnostics; never loads the profile.
    #[arg(long, value_enum, requires = "operation",
        required_if_eq_any([("operation", "database-status"), ("operation", "database-migrate")]))]
    profile: Option<DoctorProfile>,

    /// Consent to bounded, sanitized native tool probes; never application hooks.
    #[arg(long, requires_all = ["operation", "cargo", "tool_directory"])]
    probe_tools: bool,

    /// Absolute trusted Cargo executable (Linux native probes only).
    #[arg(long, value_name = "PATH", requires = "probe_tools")]
    cargo: Option<PathBuf>,

    /// Absolute trusted external tool directory; repeat as needed.
    #[arg(long, value_name = "PATH", requires = "probe_tools")]
    tool_directory: Vec<PathBuf>,

    /// Explicit trusted wasm-bindgen CLI for development/release diagnostics.
    #[arg(long, value_name = "PATH", requires = "probe_tools")]
    wasm_bindgen: Option<PathBuf>,

    /// Explicit trusted Binaryen optimizer for release diagnostics.
    #[arg(long, value_name = "PATH", requires = "probe_tools")]
    wasm_opt: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DoctorOperation {
    Dev,
    Check,
    Test,
    Build,
    DatabaseStatus,
    DatabaseMigrate,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DoctorProfile {
    Sqlite,
    Development,
    Test,
    Production,
}

impl From<DoctorProfile> for RuntimeProfile {
    fn from(profile: DoctorProfile) -> Self {
        match profile {
            DoctorProfile::Sqlite => Self::Sqlite,
            DoctorProfile::Development => Self::Development,
            DoctorProfile::Test => Self::Test,
            DoctorProfile::Production => Self::Production,
        }
    }
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    output_schema: u32,
    status: CheckStatus,
    checks: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<OperationIntent>,
}

impl DoctorReport {
    fn new() -> Self {
        Self {
            output_schema: DOCTOR_OUTPUT_SCHEMA,
            status: CheckStatus::Pass,
            checks: Vec::new(),
            operation: None,
        }
    }

    fn push(
        &mut self,
        code: &'static str,
        status: CheckStatus,
        message: &'static str,
        action: Option<&'static str>,
    ) {
        if status == CheckStatus::Failure
            || (status == CheckStatus::Warning && self.status == CheckStatus::Pass)
        {
            self.status = status;
        }
        self.checks.push(DoctorCheck {
            code,
            status,
            message,
            action,
        });
    }
}

pub(crate) fn run(
    command: DoctorCommand,
    repository_root: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let intent = match (command.operation, command.profile) {
        (None, None) => None,
        (Some(DoctorOperation::Dev), None) => Some(OperationIntent::Develop),
        (Some(DoctorOperation::Check), None) => Some(OperationIntent::Check),
        (Some(DoctorOperation::Test), None) => Some(OperationIntent::Test),
        (Some(DoctorOperation::Build), None) => Some(OperationIntent::ReleaseBuild),
        (Some(DoctorOperation::DatabaseStatus), Some(profile)) => {
            Some(OperationIntent::DatabaseStatus {
                profile: profile.into(),
            })
        }
        (Some(DoctorOperation::DatabaseMigrate), Some(profile)) => {
            Some(OperationIntent::DatabaseMigrate {
                profile: profile.into(),
            })
        }
        _ => {
            return write_diagnostic(
                CliDiagnostic::usage(
                    "doctor profile is valid only for database operation diagnostics",
                    "Select --operation database-status or database-migrate with --profile.",
                ),
                diagnostics,
            );
        }
    };
    let policy = match MutationCompatibilityPolicy::for_current_release() {
        Ok(policy) => policy,
        Err(_) => {
            return write_diagnostic(
                CliDiagnostic::internal("cannot establish the CLI compatibility policy"),
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
            let message = match error.kind() {
                ApplicationContextErrorKind::Validation => {
                    "cannot safely locate or read an application manifest"
                }
                ApplicationContextErrorKind::Conflict => {
                    "application root is ambiguous or changed during inspection"
                }
            };
            if let Some(intent) = intent {
                let mut report = DoctorReport::new();
                report.operation = Some(intent);
                report.push("application-context", CheckStatus::Failure, message,
                    Some("Restore a valid, unambiguous real application root and manifest; no tool was probed."));
                return emit_report(&report, command.json, output, diagnostics);
            }
            return write_diagnostic(CliDiagnostic::validation(message), diagnostics);
        }
    };

    let mut report = DoctorReport::new();
    check_manifest(&context, &mut report);
    match inspect_composition(&repository_root, &context) {
        Ok(composition) => match composition.status {
            CompositionInspectionStatus::Compatible => report.push(
                "composition",
                CheckStatus::Pass,
                "Recorded component composition is compatible with the bundled package.",
                None,
            ),
            CompositionInspectionStatus::Unresolved => report.push(
                "composition",
                CheckStatus::Failure,
                "Recorded component composition cannot be resolved.",
                Some("Run `hegira inspect` for component graph diagnostics."),
            ),
            CompositionInspectionStatus::Unavailable => report.push(
                "composition",
                CheckStatus::Failure,
                "Current component composition is unavailable.",
                Some("Use a current supported application manifest."),
            ),
        },
        Err(_) => report.push(
            "composition",
            CheckStatus::Failure,
            "Bundled component composition could not be inspected.",
            Some("Check that the CLI source checkout and bundled package are intact."),
        ),
    }
    check_recovery_marker(&context.root, &mut report);
    check_integrations(&context, &mut report);
    check_provider(&context, &mut report);
    if let Some(intent) = intent {
        report.operation = Some(intent);
        // Invalid composition/recovery/integrations never authorize a tool probe.
        if report.status != CheckStatus::Failure {
            match plan_application_operation(&repository_root, &OperationRequest { application: request, intent }) {
                Ok(plan) => {
                    let tools = command.cargo.filter(|_| command.probe_tools).map(|cargo| ReadinessTools {
                        cargo, directories: command.tool_directory,
                        wasm_bindgen: command.wasm_bindgen, wasm_opt: command.wasm_opt,
                    });
                    for check in crate::operations::execution::diagnose_operation(&plan, tools.as_ref()) {
                        report.push(check.code, check.status, check.message, check.action);
                    }
                }
                Err(_) => report.push("operation-plan", CheckStatus::Failure,
                    "The selected operation is not supported by the recorded composition or provider/profile.",
                    Some("Review `hegira inspect` and select a supported operation and provider-compatible profile.")),
            }
        } else {
            report.push("operation-plan", CheckStatus::Failure,
                "Operation prerequisites were not probed because application state is blocked.",
                Some("Resolve the reported manifest, composition, integration or recovery blocker first; doctor repairs nothing."));
        }
    } else {
        check_prerequisites(&mut report);
    }
    emit_report(&report, command.json, output, diagnostics)
}

fn emit_report(
    report: &DoctorReport,
    json: bool,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let rendered = if json {
        match serde_json::to_string_pretty(&report) {
            Ok(rendered) => rendered,
            Err(_) => {
                return write_diagnostic(
                    CliDiagnostic::internal("cannot serialize application doctor report"),
                    diagnostics,
                );
            }
        }
    } else {
        render_human(report)
    };
    if writeln!(output, "{rendered}").is_err() {
        return CliExit::Internal;
    }
    if report.status == CheckStatus::Failure {
        CliExit::Validation
    } else {
        CliExit::Success
    }
}

fn check_manifest(context: &ApplicationContext, report: &mut DoctorReport) {
    match &context.compatibility {
        MutationCompatibility::Compatible if context.manifest.is_some() => report.push(
            "manifest",
            CheckStatus::Pass,
            "Application manifest is valid for this CLI release.",
            None,
        ),
        _ => report.push(
            "manifest",
            CheckStatus::Failure,
            "Application manifest is not compatible with this CLI release.",
            Some("Run `hegira inspect` and review the framework and package versions."),
        ),
    }
}

fn check_recovery_marker(root: &Path, report: &mut DoctorReport) {
    match std::fs::symlink_metadata(root.join(MUTATION_MARKER)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => report.push(
            "recovery-marker",
            CheckStatus::Pass,
            "No pending application mutation recovery marker was found.",
            None,
        ),
        Ok(_) => report.push(
            "recovery-marker",
            CheckStatus::Failure,
            "An application mutation recovery marker is present.",
            Some("Inspect the marker and transaction files against version control before any mutation; do not delete it blindly."),
        ),
        Err(_) => report.push(
            "recovery-marker",
            CheckStatus::Failure,
            "Application mutation recovery state could not be inspected.",
            Some("Check application-root access before attempting a mutation."),
        ),
    }
}

fn check_integrations(context: &ApplicationContext, report: &mut DoctorReport) {
    let Some(manifest) = &context.manifest else {
        report.push(
            "managed-integrations",
            CheckStatus::Failure,
            "Managed integration state cannot be checked without a current manifest.",
            Some("Restore a valid application manifest and rerun doctor."),
        );
        return;
    };
    let (Ok(server), Ok(routes), Ok(operations)) = (
        read_managed_source(&context.root, "apps/server/src/server.rs"),
        read_managed_source(&context.root, "apps/web/src/routes.rs"),
        read_managed_source(&context.root, "crates/infrastructure/src/operations.rs"),
    ) else {
        report.push(
            "managed-integrations",
            CheckStatus::Failure,
            "A required managed integration source is missing, unsafe, or unreadable.",
            Some("Review the server, web routes, and infrastructure migration source against application-owned changes."),
        );
        return;
    };
    let identity = manifest.composition.as_ref().is_some_and(|composition| {
        composition
            .modules
            .iter()
            .any(|module| module.id == "identity")
    });
    let provider = manifest.selection.databases.iter().next().copied();
    let migration = match provider {
        Some(DatabaseAdapter::Sqlite) => {
            "identity_sqlx::identity::migrations::sqlite_migration_source()"
        }
        Some(DatabaseAdapter::Postgres) => {
            "identity_sqlx::identity::migrations::postgres_migration_source()"
        }
        None => "",
    };
    let has_identity_transport = server.contains("identity_http::bearer_api_routes")
        || server.contains("identity_runtime.bearer_routes()");
    let has_cookie_policy = server.contains("http_support::csrf::validate");
    let valid = if identity {
        routes.contains("IdentityRoutes")
            && operations.contains(migration)
            && has_identity_transport
            && has_cookie_policy
    } else {
        !routes.contains("IdentityRoutes")
            && !operations.contains("identity_sqlx::identity::migrations")
            && !has_identity_transport
    };
    if valid {
        report.push(
            "managed-integrations",
            CheckStatus::Pass,
            "Expected Identity route, transport, and migration references are present for the recorded composition.",
            None,
        );
    } else {
        report.push(
            "managed-integrations",
            CheckStatus::Failure,
            "Expected Identity route, transport, or migration references are missing or unexpected.",
            Some("Review the application-owned integration points; doctor does not repair them."),
        );
    }
}

fn check_provider(context: &ApplicationContext, report: &mut DoctorReport) {
    let provider = context
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.selection.databases.iter().next());
    match provider {
        Some(DatabaseAdapter::Sqlite) => report.push(
            "database-provider",
            CheckStatus::Pass,
            "SQLite is selected; no external database service is required by this selection.",
            None,
        ),
        Some(DatabaseAdapter::Postgres) => report.push(
            "database-provider",
            CheckStatus::Warning,
            "PostgreSQL is selected; service reachability and credentials were not probed.",
            Some("Provide a PostgreSQL service and runtime database configuration before starting the application."),
        ),
        None => report.push(
            "database-provider",
            CheckStatus::Failure,
            "No supported database provider could be determined.",
            Some("Review the database selection in hegira.toml."),
        ),
    }
}

fn check_prerequisites(report: &mut DoctorReport) {
    for (binary, code, description, action) in [
        (
            "rustc",
            "rust-toolchain",
            "Rust compiler is available.",
            "Install the repository-pinned Rust toolchain.",
        ),
        (
            "cargo",
            "cargo",
            "Cargo is available.",
            "Install Cargo with the Rust toolchain.",
        ),
        (
            "cargo-leptos",
            "cargo-leptos",
            "cargo-leptos is available.",
            "Install cargo-leptos to build the Leptos client.",
        ),
        (
            "node",
            "node",
            "Node.js is available.",
            "Install Node.js for the Leptos asset build.",
        ),
        (
            "npm",
            "npm",
            "npm is available.",
            "Install npm and run `npm ci --prefix apps/web/src`.",
        ),
    ] {
        if executable_on_path(binary) {
            report.push(code, CheckStatus::Pass, description, None);
        } else {
            report.push(
                code,
                CheckStatus::Warning,
                "A local development tool is unavailable on PATH.",
                Some(action),
            );
        }
    }
    if !executable_on_path("rustup") {
        report.push(
            "wasm-target",
            CheckStatus::Warning,
            "The wasm32 target could not be checked because rustup is unavailable.",
            Some("Install rustup and add the wasm32-unknown-unknown target."),
        );
        return;
    }
    let mut probe = Command::new("rustup");
    probe.env_clear().args(["target", "list", "--installed"]);
    for variable in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
    ] {
        if let Some(value) = env::var_os(variable) {
            probe.env(variable, value);
        }
    }
    let target_installed = probe
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line == "wasm32-unknown-unknown")
        });
    if target_installed {
        report.push(
            "wasm-target",
            CheckStatus::Pass,
            "The wasm32-unknown-unknown Rust target is installed.",
            None,
        );
    } else {
        report.push(
            "wasm-target",
            CheckStatus::Warning,
            "The wasm32-unknown-unknown Rust target is unavailable or could not be verified.",
            Some(
                "Run `rustup target add wasm32-unknown-unknown` before building the Leptos client.",
            ),
        );
    }
}

fn executable_on_path(binary: &str) -> bool {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .any(|directory| directory.join(binary).is_file())
}

fn render_human(report: &DoctorReport) -> String {
    let mut output = String::new();
    for check in &report.checks {
        let status = match check.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warning => "WARN",
            CheckStatus::Failure => "FAIL",
        };
        output.push_str(&format!("[{status}] {}: {}\n", check.code, check.message));
        if let Some(action) = check.action {
            output.push_str(&format!("       {action}\n"));
        }
    }
    let summary = match report.status {
        CheckStatus::Pass => "pass",
        CheckStatus::Warning => "warning",
        CheckStatus::Failure => "failure",
    };
    output.push_str(&format!("Doctor status: {summary}"));
    output
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn read_managed_source(root: &Path, relative: &str) -> Result<String, ()> {
    use rustix::fs::{self, FileType, Mode, OFlags};
    let directory_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let file_flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let mut directory = fs::open(root, directory_flags, Mode::empty()).map_err(|_| ())?;
    let path = Path::new(relative);
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        let Component::Normal(name) = part else {
            return Err(());
        };
        if parts.peek().is_some() {
            directory =
                fs::openat(&directory, name, directory_flags, Mode::empty()).map_err(|_| ())?;
        } else {
            let file = fs::openat(&directory, name, file_flags, Mode::empty()).map_err(|_| ())?;
            let metadata = fs::fstat(&file).map_err(|_| ())?;
            if !FileType::from_raw_mode(metadata.st_mode).is_file()
                || metadata.st_size < 0
                || metadata.st_size as u64 > MAX_SOURCE_BYTES
            {
                return Err(());
            }
            let mut source = String::new();
            File::from(file)
                .take(MAX_SOURCE_BYTES + 1)
                .read_to_string(&mut source)
                .map_err(|_| ())?;
            if source.len() as u64 > MAX_SOURCE_BYTES {
                return Err(());
            }
            return Ok(source);
        }
    }
    Err(())
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn read_managed_source(_root: &Path, _relative: &str) -> Result<String, ()> {
    Err(())
}
