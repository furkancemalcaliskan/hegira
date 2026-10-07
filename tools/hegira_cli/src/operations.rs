//! Application operation planning and explicitly consented execution.
//! Plans are not execution authority.
//!
//! Planning reads only the application manifest and authenticated bundled
//! composition. It never probes tools, reads runtime configuration, executes a
//! process, resolves dependencies, connects to a database, or publishes source.
//! The separate executor requires explicit consent and trusted tool selection.

pub(crate) mod command;
pub mod execution;
pub(crate) mod readiness;

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

use application_manifest::{
    ApplicationCapability, ClientAdapter, DatabaseAdapter, InstalledComponent, InstalledModule,
    MutationCompatibility, MutationCompatibilityPolicy, PackageIdentity,
};
use serde::{Deserialize, Serialize};

use crate::{
    ApplicationContextErrorKind, ApplicationContextRequest, CliExit, CompositionInspectionStatus,
    inspect_composition, resolve_application_context,
};

pub const OPERATION_PLAN_SCHEMA: u32 = 1;
pub const OPERATION_DIAGNOSTIC_SCHEMA: u32 = 1;

/// Closed, application-relative output locations; not arbitrary destinations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseBuildArtifacts {
    pub owner: &'static str,
    pub root: &'static str,
    pub server: &'static str,
    pub site: &'static str,
    pub browser_wasm: &'static str,
}

impl Default for ReleaseBuildArtifacts {
    fn default() -> Self {
        Self {
            owner: "hegira-release-build",
            root: "target/hegira/release-build",
            server: "target/hegira/release-build/release/app_server",
            site: "target/hegira/release-build/site",
            browser_wasm: "target/hegira/release-build/site/pkg/app_bg.wasm",
        }
    }
}

/// Closed intents, not arbitrary executable names or user-supplied arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "kebab-case",
    from = "OperationIntentInput"
)]
pub enum OperationIntent {
    Develop,
    Check,
    Test,
    ReleaseBuild,
    DatabaseStatus { profile: RuntimeProfile },
    DatabaseMigrate { profile: RuntimeProfile },
}

// Internally tagged Serde unit variants ignore extra input fields even with
// deny_unknown_fields. Empty struct input variants keep the wire contract closed.
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum OperationIntentInput {
    Develop {},
    Check {},
    Test {},
    ReleaseBuild {},
    DatabaseStatus { profile: RuntimeProfile },
    DatabaseMigrate { profile: RuntimeProfile },
}

impl From<OperationIntentInput> for OperationIntent {
    fn from(input: OperationIntentInput) -> Self {
        match input {
            OperationIntentInput::Develop {} => Self::Develop,
            OperationIntentInput::Check {} => Self::Check,
            OperationIntentInput::Test {} => Self::Test,
            OperationIntentInput::ReleaseBuild {} => Self::ReleaseBuild,
            OperationIntentInput::DatabaseStatus { profile } => Self::DatabaseStatus { profile },
            OperationIntentInput::DatabaseMigrate { profile } => Self::DatabaseMigrate { profile },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeProfile {
    Sqlite,
    Development,
    Test,
    Production,
}

impl RuntimeProfile {
    fn name(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRequest {
    pub application: ApplicationContextRequest,
    pub intent: OperationIntent,
}

/// Private construction prevents deserialized summaries becoming trusted plans.
#[derive(Clone, PartialEq, Eq)]
pub struct OperationPlan {
    root: PathBuf,
    anchor: execution::RootAnchor,
    summary: OperationPlanSummary,
}

impl fmt::Debug for OperationPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("OperationPlan")
            .field(&self.summary)
            .finish()
    }
}

impl OperationPlan {
    /// The safely resolved root is operational data, excluded from summaries.
    pub fn application_root(&self) -> &Path {
        &self.root
    }

    pub fn summary(&self) -> &OperationPlanSummary {
        &self.summary
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.summary)
    }

    /// Review text, deliberately not a copy/paste shell script.
    pub fn render_human(&self) -> String {
        let mut lines = vec![
            format!("Application operation plan (schema {})", OPERATION_PLAN_SCHEMA),
            format!("Application: {}", self.summary.application),
            format!("Intent: {:?}", self.summary.intent),
            format!("Database: {:?}; client: {:?}", self.summary.database, self.summary.client),
            "Working directory: application-root (machine-local path omitted)".to_owned(),
            "Planning only: no command, tool probe, configuration load, or database operation was executed.".to_owned(),
            "Execution requires explicit intent and trusted application/toolchain source; this is not a sandbox.".to_owned(),
        ];
        for (index, step) in self.summary.steps.iter().enumerate() {
            // Debug renders separate, quoted arguments rather than a shell command.
            lines.push(format!("Step {}: {step:?}", index + 1));
        }
        for requirement in &self.summary.prerequisites {
            lines.push(format!("Required (not probed): {requirement:?}"));
        }
        if let Some(artifacts) = &self.summary.artifacts {
            lines.push(format!("Expected artifacts (not built): {artifacts:?}"));
        }
        if self.summary.policy.production_migration_approval_required {
            lines.push(
                "Production migration requires additional explicit approval before execution."
                    .to_owned(),
            );
        }
        if self
            .summary
            .steps
            .iter()
            .any(|step| matches!(step, OperationStep::ApplicationDatabase { .. }))
        {
            lines.push("Database entry point is a typed requirement, not an available executable in current templates.".to_owned());
        }
        lines.join("\n") + "\n"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationPlanSummary {
    pub output_schema: u32,
    pub application: String,
    pub framework_version: String,
    pub package: PackageIdentity,
    pub components: Vec<InstalledComponent>,
    pub modules: Vec<InstalledModule>,
    pub capabilities: Vec<ApplicationCapability>,
    pub database: DatabaseAdapter,
    pub client: ClientAdapter,
    pub intent: OperationIntent,
    pub effect: OperationEffect,
    pub working_directory: WorkingDirectory,
    pub policy: ExecutionRequirements,
    pub prerequisites: Vec<OperationPrerequisite>,
    pub steps: Vec<OperationStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<ReleaseBuildArtifacts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkingDirectory {
    ApplicationRoot,
}

/// Potential effects of later explicit execution, not effects of planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationEffect {
    DevelopmentRuntime,
    Compilation,
    ApplicationTests,
    ProductionBundle,
    DatabaseRead,
    DatabaseWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExecutionRequirements {
    pub explicit_intent_required: bool,
    pub trusted_source_required: bool,
    pub production_migration_approval_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum OperationPrerequisite {
    ApplicationRustToolchain {
        path: &'static str,
    },
    Cargo,
    CargoLockfile {
        path: &'static str,
    },
    WasmTarget {
        target: &'static str,
    },
    CargoLeptos {
        version: &'static str,
    },
    FrontendDependencies {
        lockfile: &'static str,
    },
    LockfileWasmBindgen {
        lockfile: &'static str,
    },
    LockfileTailwindCli {
        lockfile: &'static str,
    },
    WasmOpt {
        version: &'static str,
    },
    RuntimeConfiguration {
        profile: RuntimeProfile,
    },
    PostgresService,
    /// No executable protocol is invented before its implementation issue.
    ApplicationDatabaseEntryPoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum OperationStep {
    Tool {
        program: OperationProgram,
        arguments: Vec<String>,
        /// Closed values only; runtime credentials are not captured here.
        environment: BTreeMap<String, String>,
    },
    /// The application owns migration composition, not this tool.
    ApplicationDatabase {
        operation: DatabaseOperation,
        profile: RuntimeProfile,
        database: DatabaseAdapter,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationProgram {
    Cargo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DatabaseOperation {
    Status,
    Migrate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationErrorKind {
    Validation,
    Conflict,
    Internal,
}

/// Static, content-redacted diagnostics; parser errors may contain input bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationError {
    pub output_schema: u32,
    pub kind: OperationErrorKind,
    pub code: &'static str,
    pub message: &'static str,
}

impl OperationError {
    fn new(kind: OperationErrorKind, code: &'static str, message: &'static str) -> Self {
        Self {
            output_schema: OPERATION_DIAGNOSTIC_SCHEMA,
            kind,
            code,
            message,
        }
    }

    pub fn exit(&self) -> CliExit {
        match self.kind {
            OperationErrorKind::Validation => CliExit::Validation,
            OperationErrorKind::Conflict => CliExit::Conflict,
            OperationErrorKind::Internal => CliExit::Internal,
        }
    }
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for OperationError {}

/// Resolves the real application root and authenticated current composition.
/// Requirements are declarations, not readiness or executable-authority claims.
pub fn plan_application_operation(
    repository_root: &Path,
    request: &OperationRequest,
) -> Result<OperationPlan, OperationError> {
    let policy = MutationCompatibilityPolicy::for_current_release().map_err(|_| {
        OperationError::new(
            OperationErrorKind::Internal,
            "compatibility-policy",
            "Cannot establish the current application compatibility policy.",
        )
    })?;
    let context = resolve_application_context(&request.application, &policy).map_err(|error| {
        let kind = match error.kind() {
            ApplicationContextErrorKind::Validation => OperationErrorKind::Validation,
            ApplicationContextErrorKind::Conflict => OperationErrorKind::Conflict,
        };
        OperationError::new(
            kind,
            "application-context",
            "Cannot safely resolve a unique application root and valid manifest.",
        )
    })?;
    if context.compatibility != MutationCompatibility::Compatible {
        return Err(OperationError::new(
            OperationErrorKind::Conflict,
            "application-compatibility",
            "Application state is unsupported by the current operation contract.",
        ));
    }
    let manifest = context.manifest.as_ref().ok_or_else(|| {
        OperationError::new(
            OperationErrorKind::Validation,
            "application-manifest",
            "A validated current application manifest is required.",
        )
    })?;
    let inspected = inspect_composition(repository_root, &context).map_err(|_| {
        OperationError::new(
            OperationErrorKind::Internal,
            "component-package",
            "Cannot authenticate the bundled component package.",
        )
    })?;
    if inspected.status != CompositionInspectionStatus::Compatible {
        return Err(OperationError::new(
            OperationErrorKind::Validation,
            "application-composition",
            "Recorded components, modules, versions, or capabilities do not match the supported composition.",
        ));
    }
    let database = *manifest.selection.databases.iter().next().ok_or_else(|| {
        OperationError::new(
            OperationErrorKind::Validation,
            "database-selection",
            "A single supported database provider is required.",
        )
    })?;
    let client = *manifest.selection.clients.iter().next().ok_or_else(|| {
        OperationError::new(
            OperationErrorKind::Validation,
            "client-selection",
            "A single supported client is required.",
        )
    })?;
    let composition = manifest.composition.as_ref().ok_or_else(|| {
        OperationError::new(
            OperationErrorKind::Validation,
            "application-composition",
            "Current component composition is required.",
        )
    })?;
    let default_identity = manifest
        .installed_component_ids()
        .contains(application_manifest::LAYERED_LEPTOS_IDENTITY_COMPONENT);
    let (effect, prerequisites, steps) =
        operation_steps(request.intent, database, default_identity)?;
    let anchor = execution::RootAnchor::capture(&context.root)?;
    anchor.matches_manifest(manifest)?;
    Ok(OperationPlan {
        root: context.root,
        anchor,
        summary: OperationPlanSummary {
            output_schema: OPERATION_PLAN_SCHEMA,
            application: manifest.application.clone(),
            framework_version: manifest.framework.version.clone(),
            package: composition.package.clone(),
            components: composition.components.clone(),
            modules: composition.modules.clone(),
            capabilities: composition.capabilities.iter().copied().collect(),
            database,
            client,
            intent: request.intent,
            effect,
            working_directory: WorkingDirectory::ApplicationRoot,
            policy: ExecutionRequirements {
                explicit_intent_required: true,
                trusted_source_required: true,
                production_migration_approval_required: matches!(
                    request.intent,
                    OperationIntent::DatabaseMigrate {
                        profile: RuntimeProfile::Production
                    }
                ),
            },
            prerequisites,
            steps,
            artifacts: (request.intent == OperationIntent::ReleaseBuild)
                .then(ReleaseBuildArtifacts::default),
        },
    })
}

type PlannedSteps = (
    OperationEffect,
    Vec<OperationPrerequisite>,
    Vec<OperationStep>,
);

fn operation_steps(
    intent: OperationIntent,
    database: DatabaseAdapter,
    default_identity: bool,
) -> Result<PlannedSteps, OperationError> {
    let provider = match database {
        DatabaseAdapter::Sqlite => "db-sqlite",
        DatabaseAdapter::Postgres => "db-postgres",
    };
    let mut requirements = vec![
        OperationPrerequisite::ApplicationRustToolchain {
            path: "rust-toolchain.toml",
        },
        OperationPrerequisite::Cargo,
        OperationPrerequisite::CargoLockfile { path: "Cargo.lock" },
    ];
    match intent {
        OperationIntent::Check | OperationIntent::Test => {
            requirements.push(OperationPrerequisite::WasmTarget {
                target: "wasm32-unknown-unknown",
            });
            let verb = if intent == OperationIntent::Check {
                "check"
            } else {
                "test"
            };
            let mut native = vec![
                verb.to_owned(),
                "--locked".to_owned(),
                "--workspace".to_owned(),
            ];
            if intent == OperationIntent::Check {
                native.push("--all-targets".to_owned());
            }
            native.extend([
                "--no-default-features".to_owned(),
                "--features".to_owned(),
                format!("app_server/ssr,app_server/{provider}"),
            ]);
            let hydration = [
                "check",
                "--locked",
                "-p",
                "app_server",
                "--no-default-features",
                "--features",
                "hydrate",
                "--target",
                "wasm32-unknown-unknown",
            ];
            Ok((
                if intent == OperationIntent::Check {
                    OperationEffect::Compilation
                } else {
                    OperationEffect::ApplicationTests
                },
                requirements,
                vec![
                    tool(native, None),
                    tool(hydration.map(str::to_owned).to_vec(), None),
                ],
            ))
        }
        OperationIntent::Develop | OperationIntent::ReleaseBuild => {
            requirements.extend([
                OperationPrerequisite::WasmTarget {
                    target: "wasm32-unknown-unknown",
                },
                OperationPrerequisite::CargoLeptos { version: "0.3.7" },
                OperationPrerequisite::FrontendDependencies {
                    lockfile: "apps/web/src/package-lock.json",
                },
                OperationPrerequisite::LockfileWasmBindgen {
                    lockfile: "Cargo.lock",
                },
                OperationPrerequisite::LockfileTailwindCli {
                    lockfile: "apps/web/src/package-lock.json",
                },
            ]);
            let development = intent == OperationIntent::Develop;
            if !development {
                requirements.push(OperationPrerequisite::WasmOpt { version: "123" });
            }
            let mut arguments = vec![
                "leptos".to_owned(),
                if development { "watch" } else { "build" }.to_owned(),
                "-p".to_owned(),
                "app_server".to_owned(),
            ];
            if !development {
                arguments.push("--release".to_owned());
            }
            arguments.extend([
                "--bin-features".to_owned(),
                format!("ssr,{provider}"),
                "--lib-features".to_owned(),
                "hydrate".to_owned(),
                "--bin-cargo-args=--locked".to_owned(),
                "--lib-cargo-args=--locked".to_owned(),
            ]);
            let profile = development.then_some(match database {
                DatabaseAdapter::Sqlite => RuntimeProfile::Sqlite,
                DatabaseAdapter::Postgres => RuntimeProfile::Development,
            });
            if let Some(profile) = profile {
                requirements.push(OperationPrerequisite::RuntimeConfiguration { profile });
                if database == DatabaseAdapter::Postgres {
                    requirements.push(OperationPrerequisite::PostgresService);
                }
            }
            Ok((
                if development {
                    OperationEffect::DevelopmentRuntime
                } else {
                    OperationEffect::ProductionBundle
                },
                requirements,
                vec![if development {
                    let mut step = tool(arguments, profile);
                    if let OperationStep::Tool { environment, .. } = &mut step {
                        environment.extend([
                            (
                                "APP__DATABASE__BACKEND".to_owned(),
                                match database {
                                    DatabaseAdapter::Sqlite => "sqlite",
                                    DatabaseAdapter::Postgres => "postgres",
                                }
                                .to_owned(),
                            ),
                            ("APP__SERVER__ADDR".to_owned(), "127.0.0.1:3000".to_owned()),
                            ("LEPTOS_SITE_ADDR".to_owned(), "127.0.0.1:3000".to_owned()),
                            ("LEPTOS_RELOAD_PORT".to_owned(), "3001".to_owned()),
                            ("LEPTOS_BIN_CARGO_COMMAND".to_owned(), "cargo".to_owned()),
                            (
                                "LEPTOS_STYLE_FILE".to_owned(),
                                "../web/src/style/main.css".to_owned(),
                            ),
                        ]);
                    }
                    step
                } else {
                    let mut step = tool(arguments, profile);
                    if let OperationStep::Tool { environment, .. } = &mut step {
                        environment.extend([
                            (
                                "CARGO_TARGET_DIR".to_owned(),
                                "target/hegira/release-build".to_owned(),
                            ),
                            (
                                "CARGO_BUILD_TARGET_DIR".to_owned(),
                                "target/hegira/release-build".to_owned(),
                            ),
                            (
                                "LEPTOS_BIN_TARGET_DIR".to_owned(),
                                "target/hegira/release-build".to_owned(),
                            ),
                            (
                                "LEPTOS_SITE_ROOT".to_owned(),
                                "CARGO_TARGET_DIR/site".to_owned(),
                            ),
                            ("LEPTOS_SITE_PKG_DIR".to_owned(), "pkg".to_owned()),
                            ("LEPTOS_OUTPUT_NAME".to_owned(), "app".to_owned()),
                            ("LEPTOS_BIN_EXE_NAME".to_owned(), "app_server".to_owned()),
                            ("LEPTOS_BIN_TARGET".to_owned(), "app_server".to_owned()),
                            ("LEPTOS_BIN_CARGO_COMMAND".to_owned(), "cargo".to_owned()),
                            (
                                "LEPTOS_STYLE_FILE".to_owned(),
                                "../web/src/style/main.css".to_owned(),
                            ),
                            (
                                "LEPTOS_ASSETS_DIR".to_owned(),
                                "../web/src/public".to_owned(),
                            ),
                            ("LEPTOS_HASH_FILES".to_owned(), "false".to_owned()),
                        ]);
                    }
                    step
                }],
            ))
        }
        OperationIntent::DatabaseStatus { profile }
        | OperationIntent::DatabaseMigrate { profile } => {
            // The Identity template's test profile uses PostgreSQL; the minimal
            // template (including additive Identity) retains SQLite for tests.
            // These are baseline requirements, not validation of user overrides.
            let expected = match profile {
                RuntimeProfile::Sqlite => DatabaseAdapter::Sqlite,
                RuntimeProfile::Development | RuntimeProfile::Production => {
                    DatabaseAdapter::Postgres
                }
                RuntimeProfile::Test if default_identity => DatabaseAdapter::Postgres,
                RuntimeProfile::Test => DatabaseAdapter::Sqlite,
            };
            let compatible = database == expected;
            if !compatible {
                return Err(OperationError::new(
                    OperationErrorKind::Validation,
                    "runtime-profile",
                    "Runtime profile does not match the selected provider's supported profile contract.",
                ));
            }
            requirements.extend([
                OperationPrerequisite::ApplicationDatabaseEntryPoint,
                OperationPrerequisite::RuntimeConfiguration { profile },
            ]);
            if database == DatabaseAdapter::Postgres {
                requirements.push(OperationPrerequisite::PostgresService);
            }
            let status = matches!(intent, OperationIntent::DatabaseStatus { .. });
            Ok((
                if status {
                    OperationEffect::DatabaseRead
                } else {
                    OperationEffect::DatabaseWrite
                },
                requirements,
                vec![OperationStep::ApplicationDatabase {
                    operation: if status {
                        DatabaseOperation::Status
                    } else {
                        DatabaseOperation::Migrate
                    },
                    profile,
                    database,
                }],
            ))
        }
    }
}

fn tool(arguments: Vec<String>, profile: Option<RuntimeProfile>) -> OperationStep {
    OperationStep::Tool {
        program: OperationProgram::Cargo,
        arguments,
        environment: profile
            .map(|profile| ("APP_ENV".to_owned(), profile.name().to_owned()))
            .into_iter()
            .collect(),
    }
}
