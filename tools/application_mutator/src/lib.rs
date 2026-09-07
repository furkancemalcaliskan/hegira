//! Typed, deterministic plans for changing an existing Hegira application.
//!
//! This crate owns no filesystem publication and performs no repository-only
//! dependency rewriting. Callers observe source, construct every change, and
//! validate the complete plan before a separate publisher is invoked.

use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    path::{Component, Path},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

mod editor;

pub use editor::{
    RUST_MODULES_END, RUST_MODULES_START, StructuredEditError, StructuredEditErrorKind,
    StructuredEditKind, StructuredEditOutcome, plan_rust_module, plan_toml_array_string,
    plan_toml_table_string,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedFileChange {
    Create(FileCreation),
    Edit(StructuredFileEdit),
}

impl PlannedFileChange {
    pub fn path(&self) -> &ChangePath {
        match self {
            Self::Create(change) => &change.path,
            Self::Edit(change) => &change.path,
        }
    }

    pub fn operation(&self) -> ChangeOperation {
        match self {
            Self::Create(_) => ChangeOperation::Create,
            Self::Edit(_) => ChangeOperation::Edit,
        }
    }

    pub fn precondition(&self) -> FilePrecondition {
        match self {
            Self::Create(_) => FilePrecondition::Absent,
            Self::Edit(change) => FilePrecondition::MatchesDigest(change.original_digest),
        }
    }

    pub fn resulting_content(&self) -> &[u8] {
        match self {
            Self::Create(change) => &change.content,
            Self::Edit(change) => &change.content,
        }
    }

    pub fn result_digest(&self) -> ContentDigest {
        match self {
            Self::Create(change) => change.result_digest,
            Self::Edit(change) => change.result_digest,
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
            if ContentDigest::calculate(change.resulting_content()) != change.result_digest() {
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
                    result_sha256: change.result_digest().to_hex(),
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
    pub result_sha256: String,
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
            ChangePlanErrorKind::InvalidPath | ChangePlanErrorKind::UnchangedEdit => {
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
