use super::*;
use application_manifest::SourceOwnershipClass;
use template_renderer::{UpgradePlan, UpgradePlanSummary, UpgradePlanningErrorKind};

const UPGRADE_PREVIEW_OUTPUT_SCHEMA: u32 = 1;

#[derive(Serialize)]
struct Preview {
    output_schema: u32,
    mode: &'static str,
    outcome: &'static str,
    assessment: Report,
    plan: Option<UpgradePlanSummary>,
    preserved_boundaries: Vec<SourceOwnershipClass>,
}

pub(super) fn run(
    repository: &Path,
    working_directory: PathBuf,
    options: StatusCommand,
    target: Option<String>,
    output: &mut impl Write,
) -> CliExit {
    let request = ApplicationContextRequest {
        working_directory: working_directory.clone(),
        application_root: options.application_root.clone(),
    };
    let mut report = assess(repository, working_directory, options.application_root);
    let mut plan = None;
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
    if matches!(report.status, Status::Ready) {
        match prepare(repository, &request, &mut report) {
            Some(prepared) => plan = Some(prepared.summary()),
            None => debug_assert!(!matches!(report.status, Status::Ready)),
        }
    }
    let exit = report.status.exit();
    let preview = Preview {
        output_schema: UPGRADE_PREVIEW_OUTPUT_SCHEMA,
        mode: "dry-run",
        outcome: match report.status {
            Status::Ready => "planned",
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
/// Future apply must pass this same in-memory plan to the mutation boundary.
pub(super) fn prepare(
    repository: &Path,
    request: &ApplicationContextRequest,
    report: &mut Report,
) -> Option<UpgradePlan> {
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
    check_recovery(&context.root, report).then_some(plan)
}

fn render_human_preview(preview: &Preview) -> String {
    let mut lines = vec![
        render_human(&preview.assessment),
        format!("Upgrade preview: {} (dry-run)", preview.outcome),
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
    lines.push("No application files were changed. No cached plan was written.".to_owned());
    lines.join("\n")
}
