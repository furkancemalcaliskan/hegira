use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};

use application_manifest::{
    ApplicationCapability, ClientAdapter, DatabaseAdapter, InstalledComponent, InstalledModule,
    MutationCompatibilityPolicy, PackageIdentity,
};
use clap::{Args, Subcommand};
use serde::Serialize;
use template_renderer::{
    CompositionRequest, ManifestCatalog, UpgradeAuthenticationDiagnosticKind,
    UpgradeReleaseIdentity, upgrade_recovery_pending,
};

use crate::{
    ApplicationContextErrorKind, ApplicationContextRequest, CliExit, resolve_application_context,
};

pub const UPGRADE_STATUS_OUTPUT_SCHEMA: u32 = 1;

#[derive(Debug, Args)]
pub(crate) struct UpgradeCommand {
    #[command(subcommand)]
    command: UpgradeSubcommand,
}

#[derive(Debug, Subcommand)]
enum UpgradeSubcommand {
    /// Report authenticated direct upgrade readiness without writing files.
    Status(StatusCommand),
}

#[derive(Debug, Args)]
struct StatusCommand {
    /// Application root; defaults to discovery from the working directory.
    #[arg(long, value_name = "PATH")]
    application_root: Option<PathBuf>,
    /// Emit the versioned, content-redacted assessment as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Ready,
    NoUpgrade,
    Unsupported,
    Incompatible,
    Conflict,
    RecoveryBlocked,
    InvalidInput,
    InternalError,
}

impl Status {
    fn exit(self) -> CliExit {
        match self {
            Self::Ready | Self::NoUpgrade => CliExit::Success,
            Self::Unsupported | Self::Incompatible | Self::InvalidInput => CliExit::Validation,
            Self::Conflict | Self::RecoveryBlocked => CliExit::Conflict,
            Self::InternalError => CliExit::Internal,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NoUpgrade => "no-upgrade",
            Self::Unsupported => "unsupported",
            Self::Incompatible => "incompatible",
            Self::Conflict => "conflict",
            Self::RecoveryBlocked => "recovery-blocked",
            Self::InvalidInput => "invalid-input",
            Self::InternalError => "internal-error",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Preflight {
    Pass,
    Blocked,
    NotApplicable,
    Unverified,
}

impl Preflight {
    fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Blocked => "blocked",
            Self::NotApplicable => "not-applicable",
            Self::Unverified => "unverified",
        }
    }
}

#[derive(Serialize)]
struct Release {
    framework_version: String,
    package: PackageIdentity,
}

impl From<&UpgradeReleaseIdentity> for Release {
    fn from(identity: &UpgradeReleaseIdentity) -> Self {
        Self {
            framework_version: identity.framework.version.clone(),
            package: identity.package.clone(),
        }
    }
}

#[derive(Serialize)]
struct Composition {
    components: Vec<InstalledComponent>,
    modules: Vec<InstalledModule>,
    capabilities: BTreeSet<ApplicationCapability>,
    databases: BTreeSet<DatabaseAdapter>,
    clients: BTreeSet<ClientAdapter>,
}

#[derive(Serialize)]
struct Diagnostic {
    code: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct Report {
    output_schema: u32,
    status: Status,
    source: Option<Release>,
    target: Option<Release>,
    composition: Option<Composition>,
    recovery: Preflight,
    managed_boundaries: Preflight,
    diagnostics: Vec<Diagnostic>,
}

impl Report {
    fn new() -> Self {
        Self {
            output_schema: UPGRADE_STATUS_OUTPUT_SCHEMA,
            status: Status::InvalidInput,
            source: None,
            target: None,
            composition: None,
            recovery: Preflight::Unverified,
            managed_boundaries: Preflight::Unverified,
            diagnostics: Vec::new(),
        }
    }
    fn fail(&mut self, status: Status, code: &'static str, message: &'static str) {
        self.status = status;
        self.diagnostics.push(Diagnostic { code, message });
        self.diagnostics.sort_by_key(|diagnostic| diagnostic.code);
    }
}

pub(crate) fn run(
    command: UpgradeCommand,
    repository: PathBuf,
    working_directory: PathBuf,
    output: &mut impl Write,
) -> CliExit {
    let UpgradeSubcommand::Status(command) = command.command;
    let report = assess(&repository, working_directory, command.application_root);
    let rendered = if command.json {
        serde_json::to_string_pretty(&report).ok()
    } else {
        Some(render_human(&report))
    };
    match rendered {
        Some(rendered) if writeln!(output, "{rendered}").is_ok() => report.status.exit(),
        _ => CliExit::Internal,
    }
}

fn assess(repository: &Path, working_directory: PathBuf, root: Option<PathBuf>) -> Report {
    let mut report = Report::new();
    let Ok(policy) = MutationCompatibilityPolicy::for_current_release() else {
        report.fail(
            Status::InternalError,
            "compatibility-policy",
            "Cannot establish the CLI release contract.",
        );
        return report;
    };
    let request = ApplicationContextRequest {
        working_directory,
        application_root: root,
    };
    let context = match resolve_application_context(&request, &policy) {
        Ok(context) => context,
        Err(error) => {
            let status = match error.kind() {
                ApplicationContextErrorKind::Validation => Status::InvalidInput,
                ApplicationContextErrorKind::Conflict => Status::Conflict,
            };
            report.fail(
                status,
                "application-context",
                "Cannot safely discover or validate the application manifest.",
            );
            return report;
        }
    };
    let Some(manifest) = &context.manifest else {
        report.fail(
            Status::Unsupported,
            "manifest-schema",
            "The recorded manifest schema has no supported direct upgrade.",
        );
        return report;
    };
    let Some(composition) = &manifest.composition else {
        report.fail(
            Status::Unsupported,
            "manifest-composition",
            "The manifest has no supported recorded composition.",
        );
        return report;
    };
    let source = UpgradeReleaseIdentity {
        framework: manifest.framework.clone(),
        package: composition.package.clone(),
    };
    report.source = Some(Release::from(&source));
    report.composition = Some(Composition {
        components: composition.components.clone(),
        modules: composition.modules.clone(),
        capabilities: composition.capabilities.clone(),
        databases: manifest.selection.databases.clone(),
        clients: manifest.selection.clients.clone(),
    });
    if let Some(recorded) = &mut report.composition {
        recorded
            .components
            .sort_by(|left, right| left.id.cmp(&right.id));
        recorded
            .modules
            .sort_by(|left, right| left.id.cmp(&right.id));
    }
    let catalog = match ManifestCatalog::load(repository, "layered") {
        Ok(catalog) => catalog,
        Err(_) => {
            report.fail(
                Status::InternalError,
                "package-authentication",
                "The bundled package could not be authenticated.",
            );
            return report;
        }
    };
    report.target = catalog
        .upgrade_edges()
        .iter()
        .find(|edge| edge.source == source)
        .map(|edge| Release::from(&edge.target));
    if !check_recovery(&context.root, &mut report) {
        return report;
    }
    let package = catalog
        .package()
        .expect("the bundled catalog contains a package");
    if source.framework.repository != package.framework.repository {
        report.fail(
            Status::Incompatible,
            "framework-source",
            "The recorded framework source is incompatible with the bundled package.",
        );
        return report;
    }
    if manifest.selection.databases.len() != 1 || manifest.selection.clients.len() != 1 {
        report.fail(
            Status::Incompatible,
            "composition-adapters",
            "Upgrade assessment requires exactly one supported database and client adapter.",
        );
        return report;
    }
    if source.framework == package.framework
        && source.package.id == package.id
        && source.package.version == package.version
    {
        if context.compatibility != application_manifest::MutationCompatibility::Compatible {
            report.fail(
                Status::Incompatible,
                "current-manifest",
                "The current release requires its supported manifest schema and selection contract.",
            );
            return report;
        }
        let request = CompositionRequest::new(
            source.framework.clone(),
            source.package.clone(),
            manifest.installed_component_ids(),
        )
        .with_recorded_state(
            composition.modules.clone(),
            composition.capabilities.clone(),
        );
        if catalog.resolve_composition(&request).is_err() {
            report.fail(
                Status::Incompatible,
                "composition",
                "The recorded composition is incompatible with the bundled package.",
            );
            return report;
        }
        report.status = Status::NoUpgrade;
        report.managed_boundaries = Preflight::NotApplicable;
    } else {
        let Some(edge) = catalog
            .upgrade_edges()
            .iter()
            .find(|edge| edge.source == source)
        else {
            report.fail(
                Status::Unsupported,
                "direct-edge",
                "No supported direct upgrade exists for the recorded release.",
            );
            return report;
        };
        report.target = Some(Release::from(&edge.target));
        match catalog.authenticate_upgrade_source(&context.root) {
            Ok(boundary) if boundary.application() == manifest => {
                if catalog.validate_upgrade_transition(&boundary).is_err() {
                    report.fail(
                        Status::Incompatible,
                        "manifest-transition",
                        "The recorded manifest cannot follow the declared direct upgrade transition.",
                    );
                    return report;
                }
                report.status = Status::Ready;
                report.managed_boundaries = Preflight::Pass;
            }
            Ok(_) => {
                report.fail(
                    Status::Conflict,
                    "manifest-changed",
                    "The application manifest changed during assessment.",
                );
                return report;
            }
            Err(error) => {
                authentication_failure(error.diagnostic().kind, &mut report);
                return report;
            }
        }
    }
    // Revalidate discovery and recovery after inspection. This assessment never
    // grants mutation authority; a later plan/apply must reauthenticate source.
    match resolve_application_context(&request, &policy) {
        Ok(final_context)
            if final_context.root == context.root
                && final_context.manifest.as_ref() == Some(manifest) =>
        {
            check_recovery(&context.root, &mut report);
        }
        _ => report.fail(
            Status::Conflict,
            "manifest-changed",
            "The application changed during assessment.",
        ),
    }
    report
}

fn check_recovery(root: &Path, report: &mut Report) -> bool {
    match upgrade_recovery_pending(root) {
        Ok(false) => {
            report.recovery = Preflight::Pass;
            true
        }
        Ok(true) => {
            report.recovery = Preflight::Blocked;
            report.fail(
                Status::RecoveryBlocked,
                "recovery-pending",
                "Inspect pending mutation recovery state before another operation.",
            );
            false
        }
        Err(_) => {
            report.fail(
                Status::Conflict,
                "recovery-inspection",
                "Application recovery state could not be safely inspected.",
            );
            false
        }
    }
}

fn authentication_failure(kind: UpgradeAuthenticationDiagnosticKind, report: &mut Report) {
    use UpgradeAuthenticationDiagnosticKind::*;
    let (status, code, message) = match kind {
        UnsupportedRelease => (
            Status::Unsupported,
            "direct-edge",
            "The recorded release has no supported direct upgrade.",
        ),
        UnsupportedComposition => (
            Status::Incompatible,
            "composition",
            "The direct upgrade does not support this composition or adapter selection.",
        ),
        InvalidManifest => (
            Status::InvalidInput,
            "manifest",
            "The recorded manifest is invalid for upgrade assessment.",
        ),
        OwnershipMismatch => (
            Status::Conflict,
            "ownership",
            "Recorded source ownership conflicts with the authenticated upgrade boundary.",
        ),
        SourceDigestMismatch => (
            Status::Conflict,
            "managed-source-digest",
            "A managed source file differs from the supported release boundary.",
        ),
        MissingManagedSource => (
            Status::Conflict,
            "managed-source-missing",
            "A required managed source file is missing.",
        ),
        UnexpectedManagedSource => (
            Status::Conflict,
            "managed-source-occupied",
            "A managed creation path is already occupied.",
        ),
        UnsupportedSourceType | UnsafeApplicationRoot => (
            Status::Conflict,
            "unsafe-source",
            "The application source cannot be inspected without following unsafe paths.",
        ),
        SourceLimitExceeded => (
            Status::Conflict,
            "source-limit",
            "A managed source exceeds the supported inspection limit.",
        ),
        PackageContract => (
            Status::InternalError,
            "package-contract",
            "The authenticated package has an invalid upgrade contract.",
        ),
    };
    report.managed_boundaries = Preflight::Blocked;
    report.fail(status, code, message);
}

fn render_human(report: &Report) -> String {
    let mut lines = vec![format!("Upgrade status: {}", report.status.name())];
    for (label, release) in [
        ("Source", &report.source),
        ("Direct target", &report.target),
    ] {
        lines.push(match release {
            Some(release) => format!(
                "{label}: {} ({} {})",
                release.framework_version, release.package.id, release.package.version
            ),
            None => format!("{label}: none"),
        });
    }
    if let Some(composition) = &report.composition {
        lines.push(format!(
            "Components: {}",
            crate::format_named_versions(
                composition
                    .components
                    .iter()
                    .map(|item| (item.id.as_str(), Some(item.version.as_str())))
            )
        ));
        lines.push(format!(
            "Modules: {}",
            crate::format_installed_modules(&composition.modules)
        ));
        lines.push(format!(
            "Capabilities: {}",
            crate::format_capabilities(&composition.capabilities)
        ));
        lines.push(format!(
            "Databases: {}",
            crate::format_databases(&composition.databases)
        ));
        lines.push(format!(
            "Clients: {}",
            crate::format_clients(&composition.clients)
        ));
    }
    lines.push(format!("Recovery: {}", report.recovery.name()));
    lines.push(format!(
        "Managed boundaries: {}",
        report.managed_boundaries.name()
    ));
    for diagnostic in &report.diagnostics {
        lines.push(format!("{}: {}", diagnostic.code, diagnostic.message));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use upgrade_test_support::{
        BaselineCatalog, BaselineComposition, BaselineDatabase, BaselineRequest,
    };

    #[test]
    fn unauthenticated_package_is_a_redacted_internal_outcome() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let root =
            std::env::temp_dir().join(format!("hegira-status-package-{}", std::process::id()));
        BaselineCatalog::from_repository(repository)
            .unwrap()
            .snapshot(BaselineRequest::new(
                BaselineComposition::Default,
                BaselineDatabase::Sqlite,
            ))
            .unwrap()
            .materialize(&root)
            .unwrap();
        let report = assess(&root, root.clone(), None);
        assert_eq!(report.status.name(), "internal-error");
        assert_eq!(report.status.exit(), CliExit::Internal);
        assert_eq!(report.diagnostics[0].code, "package-authentication");
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains(root.to_str().unwrap()));
        assert!(!root.join(application_mutator::MUTATION_MARKER).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
