use super::*;
use application_manifest::SourceOwnershipClass;
use application_mutator::{MutationErrorKind, publish_change_plan};
use template_renderer::{UpgradePlan, UpgradePlanSummary, UpgradePlanningErrorKind};

const UPGRADE_EXECUTION_OUTPUT_SCHEMA: u32 = 1;

#[derive(Serialize)]
struct UpgradeExecution {
    output_schema: u32,
    mode: &'static str,
    outcome: &'static str,
    assessment: Report,
    plan: Option<UpgradePlanSummary>,
    preserved_boundaries: Vec<SourceOwnershipClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Receipt>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    next_steps: Vec<&'static str>,
}

#[derive(Serialize)]
struct Receipt {
    changed_files: usize,
    edge: String,
    target: template_renderer::UpgradeReleaseSummary,
}

const NEXT_STEPS: [&str; 3] = [
    "Review the managed Cargo.toml, Cargo.lock, and hegira.toml changes; do not regenerate the lockfile blindly.",
    "Apply application-owned database migrations through the application's documented operation workflow, with backups and the correct environment.",
    "Run application tests, native/hydration checks, and deployment validation before starting or deploying services.",
];

pub(super) fn run(
    repository: &Path,
    working_directory: PathBuf,
    options: StatusCommand,
    target: Option<String>,
    dry_run: bool,
    output: &mut impl Write,
) -> CliExit {
    let request = ApplicationContextRequest {
        working_directory: working_directory.clone(),
        application_root: options.application_root.clone(),
    };
    let mut report = assess(repository, working_directory, options.application_root);
    let mut plan = None;
    let mut receipt = None;
    if matches!(report.status, Status::Ready | Status::NoUpgrade)
        && let Some(target) = &target
        && report
            .target
            .as_ref()
            .map(|release| &release.framework_version)
            != Some(target)
    {
        report.fail(
            Status::Unsupported,
            "target",
            "The requested target is not the authenticated direct upgrade target.",
        );
    }
    if !dry_run && matches!(report.status, Status::NoUpgrade) {
        report.fail(
            Status::Unsupported,
            "direct-edge",
            "No applicable direct upgrade remains; no application files were changed.",
        );
    }
    if matches!(report.status, Status::Ready) {
        match prepare(repository, &request, &mut report) {
            Some((root, prepared)) => {
                let summary = prepared.summary();
                if dry_run {
                    plan = Some(summary);
                } else {
                    match publish_change_plan(&root, prepared.change_plan()) {
                        Ok(published) if published.changed_files() == summary.changes.len() => {
                            receipt = Some(Receipt { changed_files: published.changed_files(), edge: summary.edge.clone(), target: summary.target.clone() });
                            plan = Some(summary);
                        }
                        Ok(_) => report.fail(Status::InternalError, "receipt-mismatch", "Publication returned an inconsistent receipt; inspect application state before retrying."),
                        Err(error) => publication_failure(error.kind(), &mut report),
                    }
                }
            }
            None => debug_assert!(!matches!(report.status, Status::Ready)),
        }
    }
    let exit = report.status.exit();
    let preview = UpgradeExecution {
        output_schema: UPGRADE_EXECUTION_OUTPUT_SCHEMA,
        mode: if dry_run { "dry-run" } else { "apply" },
        outcome: match report.status {
            Status::Ready if dry_run => "planned",
            Status::Ready => "applied",
            Status::NoUpgrade => "no-upgrade",
            _ => "unavailable",
        },
        assessment: report,
        plan,
        preserved_boundaries: vec![
            SourceOwnershipClass::ApplicationOwned,
            SourceOwnershipClass::GeneratedOnce,
            SourceOwnershipClass::ImmutableHistory,
        ],
        next_steps: if receipt.is_some() {
            NEXT_STEPS.to_vec()
        } else {
            Vec::new()
        },
        receipt,
    };
    let rendered = if options.json {
        serde_json::to_string_pretty(&preview).ok()
    } else {
        Some(render_human_preview(&preview))
    };
    match rendered {
        Some(rendered) if writeln!(output, "{rendered}").is_ok() => exit,
        _ => CliExit::Internal,
    }
}

/// Build exactly the renderer's typed plan; no publication state is created.
/// Both preview and apply consume this same in-memory plan contract.
pub(super) fn prepare(
    repository: &Path,
    request: &ApplicationContextRequest,
    report: &mut Report,
) -> Option<(PathBuf, UpgradePlan)> {
    let policy = match MutationCompatibilityPolicy::for_current_release() {
        Ok(policy) => policy,
        Err(_) => {
            report.fail(
                Status::InternalError,
                "compatibility-policy",
                "Cannot establish the CLI release contract.",
            );
            return None;
        }
    };
    let context = match resolve_application_context(request, &policy) {
        Ok(context) => context,
        Err(_) => {
            report.fail(
                Status::Conflict,
                "application-changed",
                "The application changed before planning.",
            );
            return None;
        }
    };
    let catalog = match ManifestCatalog::load(repository, "layered") {
        Ok(catalog) => catalog,
        Err(_) => {
            report.fail(
                Status::InternalError,
                "package-authentication",
                "The bundled package could not be authenticated.",
            );
            return None;
        }
    };
    if !check_recovery(&context.root, report) {
        return None;
    }
    let boundary = match catalog.authenticate_upgrade_source(&context.root) {
        Ok(boundary) if context.manifest.as_ref() == Some(boundary.application()) => boundary,
        Ok(_) => {
            report.fail(
                Status::Conflict,
                "manifest-changed",
                "The application manifest changed before planning.",
            );
            return None;
        }
        Err(error) => {
            authentication_failure(error.diagnostic().kind, report);
            return None;
        }
    };
    // Require the same observed readiness state, not a replacement composition.
    let manifest = boundary.application();
    let composition = manifest
        .composition
        .as_ref()
        .expect("authenticated composition");
    let same_source = report.source.as_ref().is_some_and(|source| {
        source.framework_version == manifest.framework.version
            && source.package == composition.package
    });
    let same_target = report.target.as_ref().is_some_and(|target| {
        target.framework_version == boundary.edge().target.framework.version
            && target.package == boundary.edge().target.package
    });
    let same_composition = report.composition.as_ref().is_some_and(|recorded| {
        let mut components = composition.components.clone();
        components.sort_by(|a, b| a.id.cmp(&b.id));
        let mut modules = composition.modules.clone();
        modules.sort_by(|a, b| a.id.cmp(&b.id));
        recorded.components == components
            && recorded.modules == modules
            && recorded.capabilities == composition.capabilities
            && recorded.databases == manifest.selection.databases
            && recorded.clients == manifest.selection.clients
    });
    if !same_source || !same_target || !same_composition {
        report.fail(
            Status::Conflict,
            "application-changed",
            "The recorded release or composition changed before planning.",
        );
        return None;
    }
    let plan = match UpgradePlan::from_authenticated(&catalog, boundary) {
        Ok(plan) => plan,
        Err(error) => {
            let status = match error.diagnostic().kind {
                UpgradePlanningErrorKind::Blocked | UpgradePlanningErrorKind::Conflict => {
                    Status::Conflict
                }
                UpgradePlanningErrorKind::Unsupported => Status::Unsupported,
                UpgradePlanningErrorKind::Incompatible => Status::Incompatible,
            };
            report.fail(
                status,
                "upgrade-plan",
                "The authenticated transition cannot produce an applicable upgrade plan.",
            );
            return None;
        }
    };
    match resolve_application_context(request, &policy) {
        Ok(final_context) if final_context == context => {}
        _ => {
            report.fail(
                Status::Conflict,
                "application-changed",
                "The application changed during planning.",
            );
            return None;
        }
    }
    check_recovery(&context.root, report).then_some((context.root, plan))
}

fn publication_failure(kind: MutationErrorKind, report: &mut Report) {
    let (status, code, message) = match kind {
        MutationErrorKind::RecoveryRequired => (
            Status::RecoveryBlocked,
            "recovery-pending",
            "A concurrent or interrupted mutation requires inspection. Do not delete its marker or staging files; verify the application and recover manually before retrying.",
        ),
        MutationErrorKind::PreconditionFailed => (
            Status::Conflict,
            "publication-precondition",
            "Application preconditions changed before publication; inspect changes and run a new dry-run.",
        ),
        MutationErrorKind::UnsupportedPlatform | MutationErrorKind::UnsafeRoot => (
            Status::InvalidInput,
            "publication-platform",
            "The application filesystem cannot satisfy safe publication requirements.",
        ),
        MutationErrorKind::InvalidPlan => (
            Status::InternalError,
            "publication-plan",
            "The authenticated plan failed publisher validation.",
        ),
        MutationErrorKind::PublicationFailed => (
            Status::InternalError,
            "publication-failed",
            "Publication failed without a success receipt. Inspect application state and recovery information before retrying.",
        ),
        MutationErrorKind::RollbackIncomplete => (
            Status::InternalError,
            "recovery-uncertain",
            "Rollback could not be completed. Preserve the recovery marker and staged files; inspect and recover manually before retrying.",
        ),
    };
    if matches!(
        kind,
        MutationErrorKind::RecoveryRequired | MutationErrorKind::RollbackIncomplete
    ) {
        report.recovery = Preflight::Blocked;
    }
    report.fail(status, code, message);
}

fn render_human_preview(preview: &UpgradeExecution) -> String {
    let mut lines = vec![
        render_human(&preview.assessment),
        format!("Upgrade result: {} ({})", preview.outcome, preview.mode),
    ];
    if let Some(plan) = &preview.plan {
        lines.push(format!("Edge: {}", plan.edge));
        lines.push(format!(
            "Source package digest: {}",
            plan.source_package_digest
        ));
        lines.push(format!(
            "Source baseline digest: {}",
            plan.source_baseline_digest
        ));
        lines.push(format!(
            "Manifest transitions: {}",
            serde_json::to_string(&plan.manifest_transitions).expect("typed transitions")
        ));
        lines.push(format!(
            "Target components: {}",
            plan.target_components.join(", ")
        ));
        lines.push(format!(
            "Target modules: {}",
            plan.target_modules.join(", ")
        ));
        lines.push(format!(
            "Framework dependencies: {}",
            plan.framework_dependencies.join(", ")
        ));
        for change in &plan.changes {
            let operation = match change.operation {
                application_mutator::ChangeOperation::Create => "create",
                application_mutator::ChangeOperation::Edit => "edit",
                application_mutator::ChangeOperation::Retire => "retire",
            };
            lines.push(format!(
                "  {operation} {} (owner={}, integration={}, ownership=managed-integration)",
                change.path, change.component, change.integration
            ));
            lines.push(format!(
                "    precondition: {}",
                serde_json::to_string(&change.precondition).expect("typed precondition")
            ));
            lines.push(format!(
                "    result: {}",
                serde_json::to_string(&change.result).expect("typed result")
            ));
        }
    }
    lines.push(
        "Preserved: application-owned, generated-once, immutable-history boundaries.".to_owned(),
    );
    if let Some(receipt) = &preview.receipt {
        lines.push(format!(
            "Applied receipt: {} managed files; edge {}; target {}.",
            receipt.changed_files, receipt.edge, receipt.target.framework_version
        ));
        lines.extend(
            preview
                .next_steps
                .iter()
                .map(|step| format!("Next: {step}")),
        );
    } else if preview.mode == "dry-run" {
        lines.push("No application files were changed. No cached plan was written.".to_owned());
    } else {
        lines.push("No successful upgrade receipt was issued. Inspect diagnostics and recovery state before retrying.".to_owned());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_outcomes_are_static_redacted_and_preserve_recovery_blocking() {
        for (kind, exit, code) in [
            (
                MutationErrorKind::RecoveryRequired,
                CliExit::Conflict,
                "recovery-pending",
            ),
            (
                MutationErrorKind::PreconditionFailed,
                CliExit::Conflict,
                "publication-precondition",
            ),
            (
                MutationErrorKind::UnsupportedPlatform,
                CliExit::Validation,
                "publication-platform",
            ),
            (
                MutationErrorKind::UnsafeRoot,
                CliExit::Validation,
                "publication-platform",
            ),
            (
                MutationErrorKind::InvalidPlan,
                CliExit::Internal,
                "publication-plan",
            ),
            (
                MutationErrorKind::PublicationFailed,
                CliExit::Internal,
                "publication-failed",
            ),
            (
                MutationErrorKind::RollbackIncomplete,
                CliExit::Internal,
                "recovery-uncertain",
            ),
        ] {
            let mut report = Report::new();
            publication_failure(kind, &mut report);
            assert_eq!(report.status.exit(), exit);
            assert_eq!(report.diagnostics[0].code, code);
            if matches!(
                kind,
                MutationErrorKind::RecoveryRequired | MutationErrorKind::RollbackIncomplete
            ) {
                assert_eq!(report.recovery.name(), "blocked");
                assert!(report.diagnostics[0].message.contains("marker"));
            }
        }
    }
}
