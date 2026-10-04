//! Typed, deterministic plans for changing an existing Hegira application.
//!
//! Callers observe source, construct every change, and validate the complete
//! plan before invoking the failure-safe publisher. This crate performs no
//! repository-only dependency rewriting.

use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    path::{Component, Path},
};

use application_manifest::{SourceOwnership, SourceOwnershipClass};
use serde::Serialize;
use sha2::{Digest, Sha256};

mod editor;
mod installation;
mod publisher;

pub use editor::{
    CargoDependency, CargoDependencySection, CargoDependencySource, RUST_MODULES_END,
    RUST_MODULES_START, StructuredEditError, StructuredEditErrorKind, StructuredEditKind,
    StructuredEditOutcome, plan_cargo_dependency, plan_cargo_dependency_transition,
    plan_rust_managed_entry, plan_rust_module, plan_toml_array_string, plan_toml_identity_entry,
    plan_toml_table_string,
};
pub use installation::{
    ApplicationFileOwner, COMPONENT_INSTALLATION_SUMMARY_SCHEMA, ComponentArtifact,
    ComponentCargoManifest, ComponentConfigurationTarget, ComponentContribution,
    ComponentEditError, ComponentEditOutcome, ComponentInstallationError,
    ComponentInstallationErrorKind, ComponentInstallationPlan, ComponentInstallationSummary,
    ComponentIntegration, ComponentManagedRustTarget, ComponentRustModuleTarget,
    OwnedChangeSummary, compose_component_contributions, plan_component_cargo_dependency,
    plan_component_cargo_feature, plan_component_composition_identity,
    plan_component_configuration_entry, plan_component_installation,
    plan_component_managed_rust_entry, plan_component_rust_module,
};
pub use publisher::{
    MUTATION_MARKER, MutationError, MutationErrorKind, MutationReceipt, publish_change_plan,
};

pub const CHANGE_PLAN_SUMMARY_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChangePath(String);

impl ChangePath {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, ChangePlanError> {
        let path = path.as_ref();
        let Some(text) = path.to_str() else {
            return Err(ChangePlanError::invalid_path(
                "change paths must be valid UTF-8",
            ));
        };
        if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
            return Err(ChangePlanError::invalid_path(
                "change paths must contain 1–4096 visible UTF-8 bytes",
            ));
        }
        if text.contains('\\') || path.is_absolute() {
            return Err(ChangePlanError::invalid_path(
                "change paths must be relative and use forward slashes",
            ));
        }

        let mut normalized = Vec::new();
        for component in path.components() {
            let Component::Normal(segment) = component else {
                return Err(ChangePlanError::invalid_path(
                    "change paths may not contain dot, dot-dot, prefixes, or root components",
                ));
            };
            let Some(segment) = segment.to_str() else {
                return Err(ChangePlanError::invalid_path(
                    "change path segments must be valid UTF-8",
                ));
            };
            if segment.is_empty()
                || segment.len() > 255
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            {
                return Err(ChangePlanError::invalid_path(
                    "change path segments must use 1–255 ASCII letters, digits, dots, hyphens, or underscores",
                ));
            }
            normalized.push(segment);
        }
        if normalized.join("/") != text {
            return Err(ChangePlanError::invalid_path(
                "change paths must use one canonical spelling",
            ));
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ChangePath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentDigest([u8; 32]);

impl ContentDigest {
    pub fn calculate(content: &[u8]) -> Self {
        Self(Sha256::digest(content).into())
    }

    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            write!(output, "{byte:02x}").expect("writing into a String cannot fail");
        }
        output
    }
}

impl Display for ContentDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeOperation {
    Create,
    Edit,
    Retire,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePrecondition {
    Absent,
    MatchesDigest(ContentDigest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCreation {
    path: ChangePath,
    content: Vec<u8>,
    result_digest: ContentDigest,
}

impl FileCreation {
    pub fn new(
        path: impl AsRef<Path>,
        content: impl Into<Vec<u8>>,
    ) -> Result<Self, ChangePlanError> {
        let path = ChangePath::new(path)?;
        let content = content.into();
        let result_digest = ContentDigest::calculate(&content);
        Ok(Self {
            path,
            content,
            result_digest,
        })
    }

    pub fn path(&self) -> &ChangePath {
        &self.path
    }

    pub fn resulting_content(&self) -> &[u8] {
        &self.content
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredFileEdit {
    path: ChangePath,
    original_digest: ContentDigest,
    content: Vec<u8>,
    result_digest: ContentDigest,
}

impl StructuredFileEdit {
    pub fn new(
        path: impl AsRef<Path>,
        observed_content: &[u8],
        resulting_content: impl Into<Vec<u8>>,
    ) -> Result<Self, ChangePlanError> {
        let path = ChangePath::new(path)?;
        let content = resulting_content.into();
        let original_digest = ContentDigest::calculate(observed_content);
        let result_digest = ContentDigest::calculate(&content);
        if original_digest == result_digest {
            return Err(ChangePlanError::unchanged(path));
        }
        Ok(Self {
            path,
            original_digest,
            content,
            result_digest,
        })
    }

    pub fn resulting_content(&self) -> &[u8] {
        &self.content
    }

    pub fn path(&self) -> &ChangePath {
        &self.path
    }
}

/// A digest-preconditioned retirement of one explicitly managed integration.
///
/// The path and owner are taken from an application-manifest claim so callers
/// cannot independently name a file and assert unrelated ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedFileRetirement {
    path: ChangePath,
    original_digest: ContentDigest,
    integration: String,
}

impl ManagedFileRetirement {
    pub fn new(
        ownership: &SourceOwnership,
        path: impl AsRef<Path>,
        integration: &str,
        observed_content: &[u8],
    ) -> Result<Self, ChangePlanError> {
        let path = ChangePath::new(path)?;
        ownership.validate().map_err(|_| {
            ChangePlanError::invalid_ownership(
                path.clone(),
                "file retirement requires a valid source-ownership contract",
            )
        })?;
        let declared = ownership.claims.iter().any(|claim| {
            claim.path == path.as_str()
                && claim.class == SourceOwnershipClass::ManagedIntegration
                && claim.integration.as_deref() == Some(integration)
        });
        if !declared {
            return Err(ChangePlanError::invalid_ownership(
                path,
                "file retirement requires an exact managed-integration ownership claim",
            ));
        }
        Ok(Self {
            path,
            original_digest: ContentDigest::calculate(observed_content),
            integration: integration.to_owned(),
        })
    }

    pub fn path(&self) -> &ChangePath {
        &self.path
    }

    pub fn integration(&self) -> &str {
        &self.integration
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedFileChange {
    Create(FileCreation),
    Edit(StructuredFileEdit),
    Retire(ManagedFileRetirement),
}

impl PlannedFileChange {
    pub fn path(&self) -> &ChangePath {
        match self {
            Self::Create(change) => &change.path,
            Self::Edit(change) => &change.path,
            Self::Retire(change) => &change.path,
        }
    }

    pub fn operation(&self) -> ChangeOperation {
        match self {
            Self::Create(_) => ChangeOperation::Create,
            Self::Edit(_) => ChangeOperation::Edit,
            Self::Retire(_) => ChangeOperation::Retire,
        }
    }

    pub fn precondition(&self) -> FilePrecondition {
        match self {
            Self::Create(_) => FilePrecondition::Absent,
            Self::Edit(change) => FilePrecondition::MatchesDigest(change.original_digest),
            Self::Retire(change) => FilePrecondition::MatchesDigest(change.original_digest),
        }
    }

    pub fn resulting_content(&self) -> Option<&[u8]> {
        match self {
            Self::Create(change) => Some(&change.content),
            Self::Edit(change) => Some(&change.content),
            Self::Retire(_) => None,
        }
    }

    pub fn result_digest(&self) -> Option<ContentDigest> {
        match self {
            Self::Create(change) => Some(change.result_digest),
            Self::Edit(change) => Some(change.result_digest),
            Self::Retire(_) => None,
        }
    }

    fn result_precondition(&self) -> FilePrecondition {
        self.result_digest()
            .map_or(FilePrecondition::Absent, FilePrecondition::MatchesDigest)
    }

    fn managed_by(&self) -> Option<&str> {
        match self {
            Self::Retire(change) => Some(&change.integration),
            Self::Create(_) | Self::Edit(_) => None,
        }
    }
}

impl From<FileCreation> for PlannedFileChange {
    fn from(change: FileCreation) -> Self {
        Self::Create(change)
    }
}

impl From<StructuredFileEdit> for PlannedFileChange {
    fn from(change: StructuredFileEdit) -> Self {
        Self::Edit(change)
    }
}

impl From<ManagedFileRetirement> for PlannedFileChange {
    fn from(change: ManagedFileRetirement) -> Self {
        Self::Retire(change)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlan {
    changes: Vec<PlannedFileChange>,
}

impl ChangePlan {
    pub fn new(
        changes: impl IntoIterator<Item = PlannedFileChange>,
    ) -> Result<Self, ChangePlanError> {
        let mut ordered: BTreeMap<ChangePath, PlannedFileChange> = BTreeMap::new();
        for change in changes {
            let path = change.path().clone();
            if let Some(existing) = ordered.get(&path) {
                let kind = if existing.operation() == change.operation() {
                    ChangePlanErrorKind::DuplicatePath
                } else {
                    ChangePlanErrorKind::ConflictingOperations
                };
                return Err(ChangePlanError::at_path(kind, path));
            }
            ordered.insert(path, change);
        }
        let plan = Self {
            changes: ordered.into_values().collect(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn changes(&self) -> &[PlannedFileChange] {
        &self.changes
    }

    /// Compose ordered plans into one atomic plan while preserving the first
    /// observed precondition and the final resulting content for each path.
    pub fn compose(plans: impl IntoIterator<Item = ChangePlan>) -> Result<Self, ChangePlanError> {
        let mut composed: BTreeMap<ChangePath, PlannedFileChange> = BTreeMap::new();
        for plan in plans {
            plan.validate()?;
            for change in plan.changes {
                let path = change.path().clone();
                let Some(previous) = composed.remove(&path) else {
                    composed.insert(path, change);
                    continue;
                };
                if change.precondition() != previous.result_precondition() {
                    return Err(ChangePlanError::at_path(
                        ChangePlanErrorKind::ConflictingOperations,
                        path,
                    ));
                }
                let chained = match (previous, change) {
                    (PlannedFileChange::Create(mut creation), PlannedFileChange::Edit(edit)) => {
                        creation.content = edit.content;
                        creation.result_digest = edit.result_digest;
                        Some(PlannedFileChange::Create(creation))
                    }
                    (PlannedFileChange::Create(_), PlannedFileChange::Retire(_)) => None,
                    (PlannedFileChange::Edit(mut first), PlannedFileChange::Edit(second)) => {
                        first.content = second.content;
                        first.result_digest = second.result_digest;
                        Some(PlannedFileChange::Edit(first))
                    }
                    (PlannedFileChange::Edit(first), PlannedFileChange::Retire(mut retirement)) => {
                        retirement.original_digest = first.original_digest;
                        Some(PlannedFileChange::Retire(retirement))
                    }
                    (
                        PlannedFileChange::Retire(retirement),
                        PlannedFileChange::Create(creation),
                    ) => Some(PlannedFileChange::Edit(StructuredFileEdit {
                        path: retirement.path,
                        original_digest: retirement.original_digest,
                        content: creation.content,
                        result_digest: creation.result_digest,
                    })),
                    _ => {
                        return Err(ChangePlanError::at_path(
                            ChangePlanErrorKind::ConflictingOperations,
                            path,
                        ));
                    }
                };
                if let Some(chained) = chained {
                    composed.insert(path, chained);
                }
            }
        }
        let plan = Self {
            changes: composed.into_values().collect(),
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn validate(&self) -> Result<(), ChangePlanError> {
        let mut previous: Option<&ChangePath> = None;
        for change in &self.changes {
            if previous.is_some_and(|path| path >= change.path()) {
                return Err(ChangePlanError::at_path(
                    ChangePlanErrorKind::NonDeterministicOrder,
                    change.path().clone(),
                ));
            }
            if let (Some(content), Some(digest)) =
                (change.resulting_content(), change.result_digest())
            {
                if ContentDigest::calculate(content) != digest {
                    return Err(ChangePlanError::at_path(
                        ChangePlanErrorKind::InvalidDigest,
                        change.path().clone(),
                    ));
                }
            } else if change.operation() != ChangeOperation::Retire {
                return Err(ChangePlanError::at_path(
                    ChangePlanErrorKind::InvalidDigest,
                    change.path().clone(),
                ));
            }
            previous = Some(change.path());
        }
        Ok(())
    }

    pub fn summary(&self) -> ChangePlanSummary {
        ChangePlanSummary {
            schema: CHANGE_PLAN_SUMMARY_SCHEMA,
            changes: self
                .changes
                .iter()
                .map(|change| PlannedChangeSummary {
                    path: change.path().as_str().to_owned(),
                    operation: change.operation(),
                    precondition: match change.precondition() {
                        FilePrecondition::Absent => PreconditionSummary::Absent,
                        FilePrecondition::MatchesDigest(digest) => {
                            PreconditionSummary::MatchesDigest {
                                sha256: digest.to_hex(),
                            }
                        }
                    },
                    result_sha256: change.result_digest().map(ContentDigest::to_hex),
                    managed_by: change.managed_by().map(str::to_owned),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangePlanSummary {
    pub schema: u32,
    pub changes: Vec<PlannedChangeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedChangeSummary {
    pub path: String,
    pub operation: ChangeOperation,
    pub precondition: PreconditionSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PreconditionSummary {
    Absent,
    MatchesDigest { sha256: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangePlanErrorKind {
    InvalidPath,
    InvalidOwnership,
    DuplicatePath,
    ConflictingOperations,
    UnchangedEdit,
    NonDeterministicOrder,
    InvalidDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlanError {
    kind: ChangePlanErrorKind,
    path: Option<ChangePath>,
    message: String,
}

impl ChangePlanError {
    pub fn kind(&self) -> ChangePlanErrorKind {
        self.kind
    }

    pub fn path(&self) -> Option<&ChangePath> {
        self.path.as_ref()
    }

    fn invalid_path(message: impl Into<String>) -> Self {
        Self {
            kind: ChangePlanErrorKind::InvalidPath,
            path: None,
            message: message.into(),
        }
    }

    fn unchanged(path: ChangePath) -> Self {
        Self {
            kind: ChangePlanErrorKind::UnchangedEdit,
            message: format!("structured edit for `{path}` does not change the observed content"),
            path: Some(path),
        }
    }

    fn invalid_ownership(path: ChangePath, message: impl Into<String>) -> Self {
        Self {
            kind: ChangePlanErrorKind::InvalidOwnership,
            path: Some(path),
            message: message.into(),
        }
    }

    fn at_path(kind: ChangePlanErrorKind, path: ChangePath) -> Self {
        let message = match kind {
            ChangePlanErrorKind::DuplicatePath => {
                format!("change plan contains duplicate operations for `{path}`")
            }
            ChangePlanErrorKind::ConflictingOperations => {
                format!("change plan contains conflicting operations for `{path}`")
            }
            ChangePlanErrorKind::NonDeterministicOrder => {
                format!("change plan is not in deterministic path order at `{path}`")
            }
            ChangePlanErrorKind::InvalidDigest => {
                format!("change plan result digest does not match content for `{path}`")
            }
            ChangePlanErrorKind::InvalidPath
            | ChangePlanErrorKind::InvalidOwnership
            | ChangePlanErrorKind::UnchangedEdit => {
                unreachable!("these errors use their dedicated constructors")
            }
        };
        Self {
            kind,
            path: Some(path),
            message,
        }
    }
}

impl Display for ChangePlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ChangePlanError {}

#[cfg(test)]
mod tests {
    use super::*;
    use application_manifest::SourceOwnershipClaim;

    fn create(path: &str, content: &str) -> PlannedFileChange {
        FileCreation::new(path, content.as_bytes().to_vec())
            .unwrap()
            .into()
    }

    fn edit(path: &str, original: &str, result: &str) -> PlannedFileChange {
        StructuredFileEdit::new(path, original.as_bytes(), result.as_bytes().to_vec())
            .unwrap()
            .into()
    }

    fn ownership(path: &str, class: SourceOwnershipClass) -> SourceOwnership {
        SourceOwnership {
            default: SourceOwnershipClass::ApplicationOwned,
            claims: vec![SourceOwnershipClaim {
                path: path.to_owned(),
                class,
                integration: (class == SourceOwnershipClass::ManagedIntegration)
                    .then(|| "server-routes".to_owned()),
            }],
        }
    }

    fn retire(path: &str, content: &str) -> PlannedFileChange {
        ManagedFileRetirement::new(
            &ownership(path, SourceOwnershipClass::ManagedIntegration),
            path,
            "server-routes",
            content.as_bytes(),
        )
        .unwrap()
        .into()
    }

    #[test]
    fn plan_and_summary_order_are_deterministic() {
        let first = ChangePlan::new([
            edit("crates/domain/src/lib.rs", "old", "new"),
            create("crates/application/src/orders.rs", "created"),
        ])
        .unwrap();
        let second = ChangePlan::new([
            create("crates/application/src/orders.rs", "created"),
            edit("crates/domain/src/lib.rs", "old", "new"),
        ])
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.summary(), second.summary());
        assert_eq!(first.changes()[0].operation(), ChangeOperation::Create);
        assert_eq!(first.changes()[1].operation(), ChangeOperation::Edit);
        assert_eq!(first.summary().schema, CHANGE_PLAN_SUMMARY_SCHEMA);
    }

    #[test]
    fn create_and_edit_preconditions_are_explicit() {
        let creation = create("crates/domain/src/order.rs", "pub struct Order;");
        assert_eq!(creation.precondition(), FilePrecondition::Absent);

        let original = b"pub mod existing;";
        let edit = StructuredFileEdit::new(
            "crates/domain/src/lib.rs",
            original,
            b"pub mod existing;\npub mod order;".to_vec(),
        )
        .unwrap();
        assert_eq!(
            PlannedFileChange::from(edit).precondition(),
            FilePrecondition::MatchesDigest(ContentDigest::calculate(original))
        );
    }

    #[test]
    fn summaries_never_expose_source_or_result_content() {
        let plan = ChangePlan::new([edit(
            "config/application.toml",
            "password = super-secret-original",
            "password = super-secret-result",
        )])
        .unwrap();
        let summary = serde_json::to_string(&plan.summary()).unwrap();

        assert!(!summary.contains("super-secret-original"));
        assert!(!summary.contains("super-secret-result"));
        assert!(summary.contains("result_sha256"));
        assert!(summary.contains("matches-digest"));
    }

    #[test]
    fn retirement_requires_exact_declared_managed_ownership() {
        for class in [
            SourceOwnershipClass::ApplicationOwned,
            SourceOwnershipClass::GeneratedOnce,
            SourceOwnershipClass::ImmutableHistory,
        ] {
            let error = ManagedFileRetirement::new(
                &ownership("apps/server/src/routes.rs", class),
                "apps/server/src/routes.rs",
                "server-routes",
                b"managed source",
            )
            .unwrap_err();
            assert_eq!(error.kind(), ChangePlanErrorKind::InvalidOwnership);
        }

        let managed = ownership(
            "apps/server/src/routes.rs",
            SourceOwnershipClass::ManagedIntegration,
        );
        for (path, integration) in [
            ("apps/server/src/other.rs", "server-routes"),
            ("apps/server/src/routes.rs", "other-integration"),
        ] {
            let error = ManagedFileRetirement::new(&managed, path, integration, b"managed source")
                .unwrap_err();
            assert_eq!(error.kind(), ChangePlanErrorKind::InvalidOwnership);
        }
    }

    #[test]
    fn retirement_summary_is_deterministic_and_content_redacted() {
        let plan = ChangePlan::new([
            create("crates/domain/src/new.rs", "created source"),
            retire("apps/server/src/routes.rs", "retired super-secret source"),
        ])
        .unwrap();
        let summary = serde_json::to_string(&plan.summary()).unwrap();

        assert_eq!(plan.changes()[0].operation(), ChangeOperation::Retire);
        assert_eq!(plan.changes()[0].result_digest(), None);
        assert!(summary.contains("\"operation\":\"retire\""));
        assert!(summary.contains("\"managed_by\":\"server-routes\""));
        assert!(!summary.contains("retired super-secret source"));
        assert!(!summary.contains("result_sha256\":null"));
    }

    #[test]
    fn duplicate_and_conflicting_paths_have_distinct_diagnostics() {
        let duplicate = ChangePlan::new([
            create("crates/domain/src/order.rs", "one"),
            create("crates/domain/src/order.rs", "two"),
        ])
        .unwrap_err();
        assert_eq!(duplicate.kind(), ChangePlanErrorKind::DuplicatePath);

        let conflict = ChangePlan::new([
            create("crates/domain/src/lib.rs", "created"),
            edit("crates/domain/src/lib.rs", "old", "edited"),
        ])
        .unwrap_err();
        assert_eq!(conflict.kind(), ChangePlanErrorKind::ConflictingOperations);
    }

    #[test]
    fn ordered_plans_compose_created_and_edited_content_atomically() {
        let created = ChangePlan::new([create("crates/domain/src/order.rs", "domain")]).unwrap();
        let extended = ChangePlan::new([edit(
            "crates/domain/src/order.rs",
            "domain",
            "domain with adapter",
        )])
        .unwrap();

        let composed = ChangePlan::compose([created, extended]).unwrap();

        assert_eq!(composed.changes().len(), 1);
        assert_eq!(composed.changes()[0].operation(), ChangeOperation::Create);
        assert_eq!(
            composed.changes()[0].precondition(),
            FilePrecondition::Absent
        );
        assert_eq!(
            composed.changes()[0].resulting_content(),
            Some(b"domain with adapter".as_slice())
        );
    }

    #[test]
    fn ordered_plans_preserve_the_first_edit_precondition() {
        let first = ChangePlan::new([edit("apps/server/src/server.rs", "base", "http")]).unwrap();
        let second =
            ChangePlan::new([edit("apps/server/src/server.rs", "http", "http and web")]).unwrap();

        let composed = ChangePlan::compose([first, second]).unwrap();

        assert_eq!(composed.changes().len(), 1);
        assert_eq!(composed.changes()[0].operation(), ChangeOperation::Edit);
        assert_eq!(
            composed.changes()[0].precondition(),
            FilePrecondition::MatchesDigest(ContentDigest::calculate(b"base"))
        );
        assert_eq!(
            composed.changes()[0].resulting_content(),
            Some(b"http and web".as_slice())
        );
    }

    #[test]
    fn ordered_plans_compose_retirement_to_their_net_effect() {
        let edited = ChangePlan::new([edit(
            "apps/server/src/routes.rs",
            "original",
            "managed revision",
        )])
        .unwrap();
        let retired =
            ChangePlan::new([retire("apps/server/src/routes.rs", "managed revision")]).unwrap();
        let composed = ChangePlan::compose([edited, retired]).unwrap();
        assert_eq!(composed.changes()[0].operation(), ChangeOperation::Retire);
        assert_eq!(
            composed.changes()[0].precondition(),
            FilePrecondition::MatchesDigest(ContentDigest::calculate(b"original"))
        );

        let created = ChangePlan::new([create("apps/server/src/routes.rs", "temporary")]).unwrap();
        let retired = ChangePlan::new([retire("apps/server/src/routes.rs", "temporary")]).unwrap();
        assert!(ChangePlan::compose([created, retired]).unwrap().is_empty());

        let retired = ChangePlan::new([retire("apps/server/src/routes.rs", "original")]).unwrap();
        let replacement =
            ChangePlan::new([create("apps/server/src/routes.rs", "replacement")]).unwrap();
        let composed = ChangePlan::compose([retired, replacement]).unwrap();
        assert_eq!(composed.changes()[0].operation(), ChangeOperation::Edit);
        assert_eq!(
            composed.changes()[0].resulting_content(),
            Some(b"replacement".as_slice())
        );
    }

    #[test]
    fn incompatible_plan_chains_fail_instead_of_guessing() {
        let first = ChangePlan::new([create("crates/domain/src/order.rs", "one")]).unwrap();
        let second =
            ChangePlan::new([edit("crates/domain/src/order.rs", "different", "two")]).unwrap();

        let error = ChangePlan::compose([first, second]).unwrap_err();

        assert_eq!(error.kind(), ChangePlanErrorKind::ConflictingOperations);
        assert_eq!(
            error.path().map(ChangePath::as_str),
            Some("crates/domain/src/order.rs")
        );
    }

    #[test]
    fn unsafe_or_noncanonical_paths_are_rejected() {
        for path in [
            "",
            "/absolute.rs",
            "../escape.rs",
            "crates/../escape.rs",
            "crates//domain.rs",
            "crates/./domain.rs",
            "crates\\domain.rs",
            "crates/domain name.rs",
            "crates/domain\nname.rs",
        ] {
            let error = FileCreation::new(path, Vec::new()).unwrap_err();
            assert_eq!(error.kind(), ChangePlanErrorKind::InvalidPath, "{path:?}");
        }
    }

    #[test]
    fn unchanged_structured_edits_are_rejected() {
        let error = StructuredFileEdit::new(
            "crates/domain/src/lib.rs",
            b"unchanged",
            b"unchanged".to_vec(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ChangePlanErrorKind::UnchangedEdit);
        assert_eq!(
            error.path().map(ChangePath::as_str),
            Some("crates/domain/src/lib.rs")
        );
    }

    #[test]
    fn empty_plan_is_a_valid_deterministic_no_op() {
        let plan = ChangePlan::new([]).unwrap();
        assert!(plan.is_empty());
        assert!(plan.changes().is_empty());
        assert_eq!(plan.summary().changes, []);
    }
}
