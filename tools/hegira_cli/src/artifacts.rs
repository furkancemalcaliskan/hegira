//! Read-only artifact diagnostics; ownership is never cleanup authority.

use std::{io::Write, path::PathBuf};

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::{
    ApplicationContextRequest, CliDiagnostic, CliExit,
    operations::{
        OperationError, OperationIntent, OperationRequest, ValidationCache,
        execution::inspect_artifact_usage, execution::maintain_validation_cache,
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
    /// Inspect claimed validation caches and their budget without writes or tools.
    CacheStatus(CacheOptions),
    /// Preview or explicitly reclaim inactive owned validation caches to meet a budget.
    Prune(PruneOptions),
}

#[derive(Debug, Args)]
struct CacheOptions {
    #[command(flatten)]
    options: ArtifactOptions,
    /// Validation storage budget in MiB; excludes ordinary developer/release output.
    #[arg(long, default_value_t = ValidationCache::DEFAULT_BUDGET_MIB)]
    cache_budget_mib: u64,
}

#[derive(Debug, Args)]
struct PruneOptions {
    #[command(flatten)]
    cache: CacheOptions,
    /// Read-only LRU preview; never creates a claim or removes files.
    #[arg(long, required_unless_present = "execute", conflicts_with = "execute")]
    dry_run: bool,
    /// Reauthenticate and remove only eligible owned inactive validation caches.
    #[arg(long)]
    execute: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum CacheAction {
    Status,
    Preview,
    Prune,
}

impl CacheAction {
    #[cfg(target_os = "linux")]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Preview => "preview",
            Self::Prune => "prune",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct CacheReport {
    pub output_schema: u32,
    pub mode: &'static str,
    pub application: String,
    pub path: &'static str,
    pub operation_activity: &'static str,
    pub budget_bytes: u64,
    pub allocated_bytes: u64,
    pub logical_bytes: u64,
    pub remaining_bytes: u64,
    pub budget_satisfied: bool,
    pub caches: Vec<CacheEntry>,
    pub selected: Vec<&'static str>,
    pub guidance: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct CacheEntry {
    pub cache: &'static str,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub unique_files: u64,
    pub last_used: u64,
    pub active: bool,
    pub cleanup_eligible: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation_cache: Option<CacheReport>,
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
    let (options, cache) = match command.command {
        ArtifactAction::Status(options) => (options, None),
        ArtifactAction::CacheStatus(cache) => (
            cache.options,
            Some((cache.cache_budget_mib, CacheAction::Status)),
        ),
        ArtifactAction::Prune(prune) => (
            prune.cache.options,
            Some((
                prune.cache.cache_budget_mib,
                if prune.execute {
                    CacheAction::Prune
                } else {
                    CacheAction::Preview
                },
            )),
        ),
    };
    if let Some((mib, action)) = cache {
        return run_cache(
            options,
            mib,
            action,
            repository_root,
            working_directory,
            output,
            diagnostics,
        );
    }
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

fn run_cache(
    options: ArtifactOptions,
    mib: u64,
    action: CacheAction,
    repository: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let application = match options.application_root {
        Some(root) => ApplicationContextRequest::explicit(working_directory, root),
        None => ApplicationContextRequest::discover_from(working_directory),
    };
    let result = ValidationCache::from_mib(mib).and_then(|budget| {
        let plan = plan_application_operation(
            &repository,
            &OperationRequest {
                application,
                intent: OperationIntent::Check,
            },
        )?;
        maintain_validation_cache(&plan, budget, action)
    });
    let (text, exit) = match result {
        Ok(report) => {
            let exit = if matches!(action, CacheAction::Status) || report.budget_satisfied {
                CliExit::Success
            } else {
                CliExit::Validation
            };
            let text = if options.json {
                serde_json::to_string_pretty(&report).unwrap()
            } else {
                let mut text = format!(
                    "Application validation cache (schema 1; {})\nApplication: {}\nPath: {}\nOperation: {}\nBudget: {} bytes; allocated: {} bytes; remaining/projected: {} bytes; budget satisfied: {}\n",
                    report.mode,
                    report.application,
                    report.path,
                    report.operation_activity,
                    report.budget_bytes,
                    report.allocated_bytes,
                    report.remaining_bytes,
                    report.budget_satisfied
                );
                for cache in &report.caches {
                    text.push_str(&format!("{}: {} allocated bytes, {} logical bytes; last used: {}; active: {}; cleanup eligible: {}\n", cache.cache, cache.allocated_bytes, cache.logical_bytes, cache.last_used, cache.active, cache.cleanup_eligible));
                }
                text.push_str(&format!(
                    "Selected in LRU order: {:?}\n{}",
                    report.selected, report.guidance
                ));
                if !report.budget_satisfied {
                    text.push_str("\nBudget cannot be met with eligible inactive caches; finish active work or raise --cache-budget-mib explicitly.");
                }
                text
            };
            (text, exit)
        }
        Err(error) => {
            if !options.json {
                return if writeln!(diagnostics, "{error}").is_ok() {
                    error.exit()
                } else {
                    CliExit::Internal
                };
            }
            (
                serde_json::to_string_pretty(
                    &serde_json::json!({"output_schema": 1, "diagnostics": [error]}),
                )
                .unwrap(),
                error.exit(),
            )
        }
    };
    if writeln!(output, "{text}").is_ok() {
        exit
    } else {
        CliExit::Internal
    }
}
