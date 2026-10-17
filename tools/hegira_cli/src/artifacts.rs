//! Read-only artifact diagnostics; ownership is never cleanup authority.

use std::{io::Write, path::PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::{
    ApplicationContextRequest, CliDiagnostic, CliExit,
    operations::{
        OperationError, OperationIntent, OperationRequest, execution::inspect_artifact_usage,
        plan_application_operation,
    },
    write_diagnostic,
};

#[derive(Debug, Args)]
pub(crate) struct ArtifactsCommand {
    #[command(subcommand)]
    command: ArtifactAction,
}

#[derive(Debug, Subcommand)]
enum ArtifactAction {
    /// Measure local target output; never build, claim, prune, or clean it.
    Status(ArtifactOptions),
}

#[derive(Debug, Args)]
struct ArtifactOptions {
    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,
    /// Emit a versioned report containing no file contents or machine-local paths.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArtifactReport {
    pub output_schema: u32,
    pub application: String,
    pub operation_activity: &'static str,
    pub budget_bytes: Option<u64>,
    pub budget_status: &'static str,
    pub regions: Vec<ArtifactRegion>,
    pub exclusions: Vec<&'static str>,
    pub guidance: Vec<&'static str>,
    pub diagnostics: Vec<OperationError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArtifactRegion {
    pub path: &'static str,
    pub excluded_paths: Vec<&'static str>,
    pub owner: &'static str,
    pub status: &'static str,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub unique_files: u64,
    pub skipped_entries: u64,
    pub cleanup_eligible: bool,
    pub cleanup_reason: &'static str,
}

#[cfg(target_os = "linux")]
impl ArtifactRegion {
    pub(crate) fn new(path: &'static str, owner: &'static str) -> Self {
        Self {
            path,
            excluded_paths: Vec::new(),
            owner,
            status: "missing",
            logical_bytes: 0,
            allocated_bytes: 0,
            unique_files: 0,
            skipped_entries: 0,
            cleanup_eligible: false,
            cleanup_reason: "No cleanup contract exists; ownership reporting grants no deletion authority.",
        }
    }
}

pub(crate) fn run(
    command: ArtifactsCommand,
    repository_root: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let ArtifactAction::Status(options) = command.command;
    let application = match options.application_root {
        Some(root) => ApplicationContextRequest::explicit(working_directory, root),
        None => ApplicationContextRequest::discover_from(working_directory),
    };
    let result = plan_application_operation(
        &repository_root,
        &OperationRequest {
            application,
            intent: OperationIntent::Check,
        },
    )
    .and_then(|plan| inspect_artifact_usage(&plan));
    let (report, exit) = match result {
        Ok(report) => {
            let exit = report
                .diagnostics
                .first()
                .map_or(CliExit::Success, OperationError::exit);
            (report, exit)
        }
        Err(error) => {
            let exit = error.exit();
            if !options.json {
                let diagnostic = match exit {
                    CliExit::Conflict => CliDiagnostic::conflict(error.to_string()),
                    CliExit::Internal => CliDiagnostic::internal(error.to_string()),
                    _ => CliDiagnostic::validation(error.to_string()),
                };
                return write_diagnostic(diagnostic, diagnostics);
            }
            let value = serde_json::json!({"output_schema": 1, "diagnostics": [error]});
            return if writeln!(output, "{}", serde_json::to_string_pretty(&value).unwrap()).is_ok()
            {
                exit
            } else {
                CliExit::Internal
            };
        }
    };
    let text = if options.json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => text,
            Err(_) => return CliExit::Internal,
        }
    } else {
        let mut lines = vec![
            "Application artifacts (schema 1; read-only)".to_owned(),
            format!("Application: {}", report.application),
            format!(
                "Application operation: {} (observation only)",
                report.operation_activity
            ),
            "Budget: not enforced; no application output is currently size-bounded.".to_owned(),
        ];
        for region in &report.regions {
            lines.push(format!("{}: {}; {}; {} logical bytes, {} allocated bytes, {} unique files, {} skipped entries; cleanup eligible: {}",
                region.path, region.owner, region.status, region.logical_bytes, region.allocated_bytes,
                region.unique_files, region.skipped_entries, region.cleanup_eligible));
            for excluded in &region.excluded_paths {
                lines.push(format!("  Counted separately: {excluded}"));
            }
            lines.push(format!("  {}", region.cleanup_reason));
        }
        lines.extend(
            report
                .exclusions
                .iter()
                .map(|value| format!("Excluded: {value}")),
        );
        lines.extend(report.guidance.iter().map(|value| (*value).to_owned()));
        lines.extend(report.diagnostics.iter().map(ToString::to_string));
        lines.join("\n")
    };
    if writeln!(output, "{text}").is_err() {
        CliExit::Internal
    } else {
        exit
    }
}
