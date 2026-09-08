use std::{io::Write, path::Path};

use application_mutator::{
    ChangeOperation, ChangePlan, ChangePlanError, ChangePlanErrorKind, ChangePlanSummary,
    MutationError, MutationErrorKind, publish_change_plan,
};
use clap::Args;
use serde::Serialize;

use crate::{CliDiagnostic, CliExit, output_diagnostic, write_diagnostic};

pub const MUTATION_OUTPUT_SCHEMA: u32 = 1;

/// Common output and execution options for commands that change an application.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Args)]
pub struct MutationOptions {
    /// Print the validated change plan without writing application files.
    #[arg(long)]
    dry_run: bool,

    /// Emit the versioned machine-readable change-plan result.
    #[arg(long)]
    json: bool,
}

impl MutationOptions {
    pub const fn new(dry_run: bool, json: bool) -> Self {
        Self { dry_run, json }
    }

    pub const fn dry_run(self) -> bool {
        self.dry_run
    }

    pub const fn json(self) -> bool {
        self.json
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MutationMode {
    DryRun,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum MutationOutcome {
    Planned,
    Applied,
    NoOp,
}

#[derive(Serialize)]
struct MutationOutput<'a> {
    output_schema: u32,
    mode: MutationMode,
    outcome: MutationOutcome,
    changed_files: usize,
    plan: &'a ChangePlanSummary,
}

/// Preview or publish one already validated mutation plan.
///
/// Mutation commands should build their plan once and pass that same value to
/// this function. Dry-run mode never opens or writes the application root.
pub fn execute_mutation_plan(
    application_root: &Path,
    plan: &ChangePlan,
    options: MutationOptions,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let summary = plan.summary();
    let mode = if options.dry_run {
        MutationMode::DryRun
    } else {
        MutationMode::Apply
    };
    let changed_files = summary.changes.len();
    let outcome = if plan.is_empty() {
        MutationOutcome::NoOp
    } else if options.dry_run {
        MutationOutcome::Planned
    } else {
        match publish_change_plan(application_root, plan) {
            Ok(receipt) if receipt.changed_files() == changed_files => MutationOutcome::Applied,
            Ok(_) => {
                return write_diagnostic(
                    CliDiagnostic::internal(
                        "mutation publisher returned a receipt inconsistent with the validated plan",
                    ),
                    diagnostics,
                );
            }
            Err(error) => return write_diagnostic(mutation_diagnostic(error), diagnostics),
        }
    };

    let rendered = if options.json {
        serde_json::to_string_pretty(&MutationOutput {
            output_schema: MUTATION_OUTPUT_SCHEMA,
            mode,
            outcome,
            changed_files,
            plan: &summary,
        })
        .map_err(|error| {
            CliDiagnostic::internal(format!("cannot serialize mutation result: {error}"))
        })
    } else {
        Ok(render_human_mutation(mode, outcome, &summary))
    };
    let rendered = match rendered {
        Ok(rendered) => rendered,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    if let Err(error) = writeln!(output, "{rendered}") {
        return write_diagnostic(output_diagnostic(error), diagnostics);
    }
    CliExit::Success
}

fn render_human_mutation(
    mode: MutationMode,
    outcome: MutationOutcome,
    summary: &ChangePlanSummary,
) -> String {
    if outcome == MutationOutcome::NoOp {
        return match mode {
            MutationMode::DryRun => "No application changes planned (dry run).".to_owned(),
            MutationMode::Apply => "No application changes were required.".to_owned(),
        };
    }

    let mut output = match mode {
        MutationMode::DryRun => "Planned application changes (dry run):\n".to_owned(),
        MutationMode::Apply => "Applied application changes:\n".to_owned(),
    };
    for change in &summary.changes {
        let operation = match change.operation {
            ChangeOperation::Create => "create",
            ChangeOperation::Edit => "edit",
        };
        output.push_str("  ");
        output.push_str(operation);
        output.push_str("  ");
        output.push_str(&change.path);
        output.push('\n');
    }
    output.push_str(match mode {
        MutationMode::DryRun => "No application files were changed.",
        MutationMode::Apply => "Application changes published successfully.",
    });
    output
}

/// Convert plan-construction failures into stable CLI outcomes.
pub fn change_plan_diagnostic(error: ChangePlanError) -> CliDiagnostic {
    match error.kind() {
        ChangePlanErrorKind::InvalidPath => CliDiagnostic::validation(error.to_string()),
        ChangePlanErrorKind::DuplicatePath
        | ChangePlanErrorKind::ConflictingOperations
        | ChangePlanErrorKind::UnchangedEdit => CliDiagnostic::conflict(error.to_string()),
        ChangePlanErrorKind::NonDeterministicOrder | ChangePlanErrorKind::InvalidDigest => {
            CliDiagnostic::internal(format!("invalid generated change plan: {error}"))
        }
    }
}

fn mutation_diagnostic(error: MutationError) -> CliDiagnostic {
    match error.kind() {
        MutationErrorKind::InvalidPlan
        | MutationErrorKind::UnsupportedPlatform
        | MutationErrorKind::UnsafeRoot => CliDiagnostic::validation(error.to_string()),
        MutationErrorKind::RecoveryRequired | MutationErrorKind::PreconditionFailed => {
            CliDiagnostic::conflict(error.to_string())
        }
        MutationErrorKind::PublicationFailed | MutationErrorKind::RollbackIncomplete => {
            CliDiagnostic::internal(format!("application mutation failed: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use application_mutator::{FileCreation, PlannedFileChange, StructuredFileEdit};
    use clap::Parser;

    use super::*;

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "hegira-cli-mutation-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn plan() -> ChangePlan {
        ChangePlan::new([
            PlannedFileChange::from(
                FileCreation::new("crates/domain/src/order.rs", b"secret result".to_vec()).unwrap(),
            ),
            PlannedFileChange::from(
                StructuredFileEdit::new(
                    "crates/domain/src/lib.rs",
                    b"secret original",
                    b"secret edited".to_vec(),
                )
                .unwrap(),
            ),
        ])
        .unwrap()
    }

    #[derive(Debug, Parser)]
    struct MutationCommandHarness {
        #[command(flatten)]
        options: MutationOptions,
    }

    #[test]
    fn common_mutation_options_flatten_into_commands() {
        let command =
            MutationCommandHarness::try_parse_from(["mutation", "--dry-run", "--json"]).unwrap();

        assert!(command.options.dry_run());
        assert!(command.options.json());
    }

    #[test]
    fn dry_run_reports_every_operation_without_reading_or_writing_the_root() {
        let missing_root = std::env::temp_dir().join("hegira-cli-deliberately-missing-root");
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();

        let exit = execute_mutation_plan(
            &missing_root,
            &plan(),
            MutationOptions::new(true, false),
            &mut output,
            &mut diagnostics,
        );

        assert_eq!(exit, CliExit::Success);
        assert!(diagnostics.is_empty());
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("create  crates/domain/src/order.rs"));
        assert!(output.contains("edit  crates/domain/src/lib.rs"));
        assert!(output.contains("No application files were changed."));
        assert!(!output.contains("secret"));
        assert!(!missing_root.exists());
    }

    #[test]
    fn json_output_is_versioned_deterministic_and_content_redacted() {
        let mut first = Vec::new();
        let mut second = Vec::new();
        for output in [&mut first, &mut second] {
            assert_eq!(
                execute_mutation_plan(
                    Path::new("unused"),
                    &plan(),
                    MutationOptions::new(true, true),
                    output,
                    &mut Vec::new(),
                ),
                CliExit::Success
            );
        }

        assert_eq!(first, second);
        let text = String::from_utf8(first).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["output_schema"], MUTATION_OUTPUT_SCHEMA);
        assert_eq!(value["mode"], "dry-run");
        assert_eq!(value["outcome"], "planned");
        assert_eq!(value["changed_files"], 2);
        assert_eq!(value["plan"]["schema"], 1);
        assert!(!text.contains("secret"));
    }

    #[test]
    fn empty_plan_has_a_stable_no_op_outcome() {
        let plan = ChangePlan::new([]).unwrap();
        let mut output = Vec::new();

        assert_eq!(
            execute_mutation_plan(
                Path::new("unused"),
                &plan,
                MutationOptions::new(false, true),
                &mut output,
                &mut Vec::new(),
            ),
            CliExit::Success
        );

        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["mode"], "apply");
        assert_eq!(value["outcome"], "no-op");
        assert_eq!(value["changed_files"], 0);
        assert_eq!(value["plan"]["changes"], serde_json::json!([]));
    }

    #[test]
    fn planning_conflicts_map_to_the_conflict_exit_contract() {
        let error = ChangePlan::new([
            PlannedFileChange::from(FileCreation::new("same.rs", b"one".to_vec()).unwrap()),
            PlannedFileChange::from(FileCreation::new("same.rs", b"two".to_vec()).unwrap()),
        ])
        .unwrap_err();
        let mut diagnostics = Vec::new();

        let exit = write_diagnostic(change_plan_diagnostic(error), &mut diagnostics);

        assert_eq!(exit, CliExit::Conflict);
        assert!(
            String::from_utf8(diagnostics)
                .unwrap()
                .contains("duplicate operations")
        );
    }

    #[test]
    fn invalid_plan_input_maps_to_the_validation_exit_contract() {
        let error = FileCreation::new("../outside.rs", Vec::new()).unwrap_err();
        let mut diagnostics = Vec::new();

        let exit = write_diagnostic(change_plan_diagnostic(error), &mut diagnostics);

        assert_eq!(exit, CliExit::Validation);
        assert!(
            String::from_utf8(diagnostics)
                .unwrap()
                .contains("change paths may not contain")
        );
    }

    #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
    #[test]
    fn dry_run_and_apply_report_the_identical_plan() {
        let root = TestDirectory::new("apply");
        fs::create_dir_all(root.0.join("crates/domain/src")).unwrap();
        fs::write(root.0.join("crates/domain/src/lib.rs"), b"secret original").unwrap();
        let plan = plan();
        let mut dry_run = Vec::new();
        let mut applied = Vec::new();

        assert_eq!(
            execute_mutation_plan(
                &root.0,
                &plan,
                MutationOptions::new(true, true),
                &mut dry_run,
                &mut Vec::new(),
            ),
            CliExit::Success
        );
        assert_eq!(
            execute_mutation_plan(
                &root.0,
                &plan,
                MutationOptions::new(false, true),
                &mut applied,
                &mut Vec::new(),
            ),
            CliExit::Success
        );

        let dry_run: serde_json::Value = serde_json::from_slice(&dry_run).unwrap();
        let applied: serde_json::Value = serde_json::from_slice(&applied).unwrap();
        assert_eq!(dry_run["plan"], applied["plan"]);
        assert_eq!(applied["outcome"], "applied");
        assert_eq!(
            fs::read(root.0.join("crates/domain/src/order.rs")).unwrap(),
            b"secret result"
        );
        assert_eq!(
            fs::read(root.0.join("crates/domain/src/lib.rs")).unwrap(),
            b"secret edited"
        );
    }
}
