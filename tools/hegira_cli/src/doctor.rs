use std::{
    env,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

use application_manifest::{DatabaseAdapter, MutationCompatibility, MutationCompatibilityPolicy};
use application_mutator::MUTATION_MARKER;
use clap::Args;
use serde::Serialize;

use crate::{
    ApplicationContext, ApplicationContextErrorKind, ApplicationContextRequest, CliDiagnostic,
    CliExit, CompositionInspectionStatus, inspect_composition, resolve_application_context,
    write_diagnostic,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum CheckStatus {
    Pass,
    Warning,
    Failure,
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    code: &'static str,
    status: CheckStatus,
    message: &'static str,
    action: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    output_schema: u32,
    status: CheckStatus,
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn new() -> Self {
        Self {
            output_schema: DOCTOR_OUTPUT_SCHEMA,
            status: CheckStatus::Pass,
            checks: Vec::new(),
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
    check_prerequisites(&mut report);

    let rendered = if command.json {
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
        render_human(&report)
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
    let file_flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
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
