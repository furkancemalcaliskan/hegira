use std::{io::Write, path::PathBuf};

use application_mutator::ComponentInstallationPlan;
use clap::{Args, Subcommand};
use template_renderer::{
    CompositionDiagnosticKind, CompositionError, CompositionRequest, ManifestCatalog,
    ResolvedComposition,
};

use crate::{
    ApplicationContext, CliDiagnostic, CliExit, MutationOptions, execute_mutation_plan,
    renderer_diagnostic, resolve_mutation_context, write_diagnostic,
};

#[derive(Debug, Args)]
pub(crate) struct ComponentCommand {
    #[command(subcommand)]
    command: ComponentSubcommand,
}

#[derive(Debug, Subcommand)]
enum ComponentSubcommand {
    /// Add one trusted component from the bundled release package.
    Add(ComponentAddCommand),
}

#[derive(Debug, Args)]
struct ComponentAddCommand {
    /// Bundled component identifier.
    #[arg(value_name = "COMPONENT")]
    component: String,

    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,

    #[command(flatten)]
    mutation: MutationOptions,
}

pub(crate) fn run(
    command: ComponentCommand,
    repository_root: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    match command.command {
        ComponentSubcommand::Add(command) => add_component_with(
            command,
            repository_root,
            working_directory,
            unavailable_installation_plan,
            output,
            diagnostics,
        ),
    }
}

fn add_component_with(
    command: ComponentAddCommand,
    repository_root: PathBuf,
    working_directory: PathBuf,
    planner: impl FnOnce(
        &ApplicationContext,
        &ResolvedComposition,
        &str,
    ) -> Result<ComponentInstallationPlan, CliDiagnostic>,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
) -> CliExit {
    let context = match resolve_mutation_context(command.application_root, working_directory) {
        Ok(context) => context,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    let manifest = context
        .manifest
        .as_ref()
        .expect("a compatible mutation context must contain a typed manifest");
    let installed = manifest.installed_component_ids();
    if installed.contains(&command.component) {
        return write_diagnostic(
            CliDiagnostic::conflict(format!(
                "component `{}` is already installed",
                command.component
            )),
            diagnostics,
        );
    }
    let composition = manifest
        .composition
        .as_ref()
        .expect("a compatible current manifest must contain composition state");
    let mut requested = installed.into_iter().collect::<Vec<_>>();
    requested.push(command.component.clone());

    let catalog = match ManifestCatalog::load(&repository_root, "layered") {
        Ok(catalog) => catalog,
        Err(error) => return write_diagnostic(renderer_diagnostic(error), diagnostics),
    };
    let request = CompositionRequest::new(
        manifest.framework.clone(),
        composition.package.clone(),
        requested,
    );
    let resolved = match catalog.resolve_composition(&request) {
        Ok(resolved) => resolved,
        Err(error) => {
            return write_diagnostic(composition_diagnostic(error), diagnostics);
        }
    };
    let plan = match planner(&context, &resolved, &command.component) {
        Ok(plan) => plan,
        Err(diagnostic) => return write_diagnostic(diagnostic, diagnostics),
    };
    if plan.component() != command.component {
        return write_diagnostic(
            CliDiagnostic::internal(
                "component planner returned a plan for a different component identity",
            ),
            diagnostics,
        );
    }

    execute_mutation_plan(
        &context.root,
        plan.changes(),
        command.mutation,
        output,
        diagnostics,
    )
}

fn unavailable_installation_plan(
    _: &ApplicationContext,
    _: &ResolvedComposition,
    component: &str,
) -> Result<ComponentInstallationPlan, CliDiagnostic> {
    Err(CliDiagnostic::validation(format!(
        "bundled component `{component}` does not declare additive installation contributions"
    )))
}

fn composition_diagnostic(error: CompositionError) -> CliDiagnostic {
    if error.diagnostics().iter().all(|diagnostic| {
        matches!(
            diagnostic.kind,
            CompositionDiagnosticKind::InvalidComponent
                | CompositionDiagnosticKind::MissingComponent
                | CompositionDiagnosticKind::MissingOptionalDependency
        )
    }) {
        return CliDiagnostic::validation(error.to_string());
    }
    if error.diagnostics().iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            CompositionDiagnosticKind::ComponentConflict
                | CompositionDiagnosticKind::DependencyCycle
                | CompositionDiagnosticKind::DuplicateComponent
                | CompositionDiagnosticKind::DuplicateModuleOwner
                | CompositionDiagnosticKind::MissingCapability
        )
    }) {
        return CliDiagnostic::conflict(error.to_string());
    }
    CliDiagnostic::internal(format!(
        "bundled component graph is incompatible with the application: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use application_mutator::{
        ApplicationFileOwner, ComponentArtifact, ComponentContribution, plan_component_installation,
    };

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str, version: &str) -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "hegira-component-command-{}-{sequence}-{name}",
                std::process::id()
            ));
            for directory in ["apps", "crates/domain/src", "config"] {
                fs::create_dir_all(root.join(directory)).unwrap();
            }
            fs::write(root.join("hegira.toml"), minimal_manifest(version)).unwrap();
            Self { root }
        }

        fn command(&self, dry_run: bool) -> ComponentAddCommand {
            ComponentAddCommand {
                component: "layered-leptos-identity".to_owned(),
                application_root: Some(self.root.clone()),
                mutation: MutationOptions::new(dry_run, true),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn minimal_manifest(version: &str) -> String {
        format!(
            r#"schema = 2
application = "component-command"

[framework]
repository = "https://github.com/furkancemalcaliskan/hegira.git"
version = "{version}"

[selection]
databases = ["sqlite"]
clients = ["leptos"]

[composition]
capabilities = []

[composition.package]
id = "hegira-canonical"
version = "{version}"

[[composition.components]]
id = "layered-base"
version = "{version}"
"#
        )
    }

    fn probe_plan(
        _: &ApplicationContext,
        _: &ResolvedComposition,
        component: &str,
    ) -> Result<ComponentInstallationPlan, CliDiagnostic> {
        let contribution: ComponentContribution = ComponentArtifact::new(
            ApplicationFileOwner::Domain,
            "crates/domain/src/component_probe.rs",
            b"private component source\n".to_vec(),
        )
        .unwrap()
        .into();
        plan_component_installation(component, ["layered-base"], [contribution])
            .map_err(|error| CliDiagnostic::internal(error.to_string()))
    }

    #[test]
    fn dry_run_and_apply_execute_the_same_resolved_component_plan() {
        let version = format!("v{}", env!("CARGO_PKG_VERSION"));
        let fixture = Fixture::new("dry-run-apply", &version);
        let target = fixture.root.join("crates/domain/src/component_probe.rs");
        let mut dry_run_output = Vec::new();
        let mut dry_run_diagnostics = Vec::new();

        let dry_run = add_component_with(
            fixture.command(true),
            crate::source_repository_root(),
            fixture.root.clone(),
            probe_plan,
            &mut dry_run_output,
            &mut dry_run_diagnostics,
        );

        assert_eq!(dry_run, CliExit::Success);
        assert!(dry_run_diagnostics.is_empty());
        assert!(!target.exists());

        let mut apply_output = Vec::new();
        let mut apply_diagnostics = Vec::new();
        let applied = add_component_with(
            fixture.command(false),
            crate::source_repository_root(),
            fixture.root.clone(),
            probe_plan,
            &mut apply_output,
            &mut apply_diagnostics,
        );

        assert_eq!(applied, CliExit::Success);
        assert!(apply_diagnostics.is_empty());
        assert_eq!(fs::read(&target).unwrap(), b"private component source\n");
        let dry_run: serde_json::Value = serde_json::from_slice(&dry_run_output).unwrap();
        let applied: serde_json::Value = serde_json::from_slice(&apply_output).unwrap();
        assert_eq!(dry_run["mode"], "dry-run");
        assert_eq!(dry_run["outcome"], "planned");
        assert_eq!(applied["mode"], "apply");
        assert_eq!(applied["outcome"], "applied");
        assert_eq!(dry_run["plan"], applied["plan"]);
        assert!(
            !String::from_utf8(dry_run_output)
                .unwrap()
                .contains("private component source")
        );
    }

    #[test]
    fn unsupported_application_fails_before_component_planning() {
        let fixture = Fixture::new("unsupported", "v0.1.0");
        let mut planner_called = false;
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();

        let exit = add_component_with(
            fixture.command(true),
            crate::source_repository_root(),
            fixture.root.clone(),
            |_, _, _| {
                planner_called = true;
                Err(CliDiagnostic::internal(
                    "unsupported application reached component planning",
                ))
            },
            &mut output,
            &mut diagnostics,
        );

        assert_eq!(exit, CliExit::Conflict);
        assert!(!planner_called);
        assert!(output.is_empty());
        assert!(
            String::from_utf8(diagnostics)
                .unwrap()
                .contains("application cannot be mutated")
        );
    }
}
