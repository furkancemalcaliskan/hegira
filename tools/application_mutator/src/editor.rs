use std::{
    collections::BTreeSet,
    fmt::{Display, Formatter},
    path::Path,
};

use toml_edit::{DocumentMut, Item, TableLike, value};

use crate::{ChangePath, ChangePlanError, StructuredFileEdit};

pub const RUST_MODULES_START: &str = "// hegira:generated-modules:start";
pub const RUST_MODULES_END: &str = "// hegira:generated-modules:end";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredEditKind {
    RustModule,
    TomlArrayString,
    TomlTableString,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredEditOutcome {
    Planned {
        kind: StructuredEditKind,
        edit: StructuredFileEdit,
    },
    AlreadyPresent {
        kind: StructuredEditKind,
        path: ChangePath,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredEditErrorKind {
    InvalidPath,
    InvalidInput,
    InvalidSource,
    MissingIntegrationPoint,
    AmbiguousIntegrationPoint,
    DuplicateRegistration,
    ReorderedRegistrations,
    ExistingOutsideManagedBlock,
    TomlConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredEditError {
    kind: StructuredEditErrorKind,
    path: Option<ChangePath>,
    message: String,
}

impl StructuredEditError {
    pub fn kind(&self) -> StructuredEditErrorKind {
        self.kind
    }

    pub fn path(&self) -> Option<&ChangePath> {
        self.path.as_ref()
    }

    fn new(
        kind: StructuredEditErrorKind,
        path: Option<ChangePath>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path,
            message: message.into(),
        }
    }

    fn at_path(
        kind: StructuredEditErrorKind,
        path: &ChangePath,
        message: impl Into<String>,
    ) -> Self {
        Self::new(kind, Some(path.clone()), message)
    }
}

impl Display for StructuredEditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StructuredEditError {}

impl From<ChangePlanError> for StructuredEditError {
    fn from(error: ChangePlanError) -> Self {
        Self::new(
            StructuredEditErrorKind::InvalidPath,
            error.path().cloned(),
            error.to_string(),
        )
    }
}

pub fn plan_rust_module(
    path: impl AsRef<Path>,
    observed_source: &[u8],
    module: &str,
) -> Result<StructuredEditOutcome, StructuredEditError> {
    let path = ChangePath::new(path)?;
    validate_rust_identifier(module).map_err(|()| {
        StructuredEditError::at_path(
            StructuredEditErrorKind::InvalidInput,
            &path,
            "Rust module identifiers must be non-keyword lowercase ASCII snake_case",
        )
    })?;
    let source = std::str::from_utf8(observed_source).map_err(|_| {
        StructuredEditError::at_path(
            StructuredEditErrorKind::InvalidSource,
            &path,
            "Rust integration source must be UTF-8",
        )
    })?;
    let (normalized, newline, final_newline) = normalize_newlines(source).map_err(|()| {
        StructuredEditError::at_path(
            StructuredEditErrorKind::InvalidSource,
            &path,
            "Rust integration source must use consistent LF or CRLF newlines",
        )
    })?;
    let mut lines = normalized.lines().map(str::to_owned).collect::<Vec<_>>();
    let starts = marker_positions(&lines, RUST_MODULES_START);
    let ends = marker_positions(&lines, RUST_MODULES_END);
    if starts.is_empty() || ends.is_empty() {
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::MissingIntegrationPoint,
            &path,
            "Rust source is missing the generated-module integration block",
        ));
    }
    if starts.len() != 1 || ends.len() != 1 || starts[0] >= ends[0] {
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::AmbiguousIntegrationPoint,
            &path,
            "Rust source has an ambiguous generated-module integration block",
        ));
    }
    let start = starts[0];
    let end = ends[0];
    let expected = format!("pub mod {module};");

    if lines[..start]
        .iter()
        .chain(&lines[end + 1..])
        .any(|line| line.trim() == expected)
    {
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::ExistingOutsideManagedBlock,
            &path,
            "Rust module is already registered outside the managed integration block",
        ));
    }

    let mut modules = Vec::new();
    for line in &lines[start + 1..end] {
        let Some(module) = line
            .strip_prefix("pub mod ")
            .and_then(|line| line.strip_suffix(';'))
        else {
            return Err(StructuredEditError::at_path(
                StructuredEditErrorKind::AmbiguousIntegrationPoint,
                &path,
                "managed Rust module block contains an unsupported registration",
            ));
        };
        validate_rust_identifier(module).map_err(|()| {
            StructuredEditError::at_path(
                StructuredEditErrorKind::AmbiguousIntegrationPoint,
                &path,
                "managed Rust module block contains an invalid registration",
            )
        })?;
        modules.push(module.to_owned());
    }
    let unique = modules.iter().collect::<BTreeSet<_>>();
    if unique.len() != modules.len() {
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::DuplicateRegistration,
            &path,
            "managed Rust module block contains duplicate registrations",
        ));
    }
    if !modules.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::ReorderedRegistrations,
            &path,
            "managed Rust module registrations are not in deterministic order",
        ));
    }
    if modules
        .binary_search_by(|candidate| candidate.as_str().cmp(module))
        .is_ok()
    {
        return Ok(StructuredEditOutcome::AlreadyPresent {
            kind: StructuredEditKind::RustModule,
            path,
        });
    }
    modules.push(module.to_owned());
    modules.sort();
    lines.splice(
        start + 1..end,
        modules
            .into_iter()
            .map(|module| format!("pub mod {module};")),
    );
    let resulting_source = restore_newlines(&lines, newline, final_newline);
    planned(
        StructuredEditKind::RustModule,
        path,
        observed_source,
        resulting_source.into_bytes(),
    )
}

pub fn plan_toml_array_string(
    path: impl AsRef<Path>,
    observed_source: &[u8],
    table_path: &[&str],
    key: &str,
    entry: &str,
) -> Result<StructuredEditOutcome, StructuredEditError> {
    let path = ChangePath::new(path)?;
    validate_toml_address(table_path, key, entry, &path)?;
    let source = std::str::from_utf8(observed_source).map_err(|_| {
        StructuredEditError::at_path(
            StructuredEditErrorKind::InvalidSource,
            &path,
            "TOML integration source must be UTF-8",
        )
    })?;
    let mut document = parse_toml(source, &path)?;
    let table = find_table(&mut document, table_path, &path)?;
    let array = table
        .get_mut(key)
        .and_then(Item::as_value_mut)
        .and_then(toml_edit::Value::as_array_mut)
        .ok_or_else(|| {
            StructuredEditError::at_path(
                StructuredEditErrorKind::MissingIntegrationPoint,
                &path,
                "declared TOML array integration point is missing or has another type",
            )
        })?;
    let entries = array
        .iter()
        .map(|value| value.as_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            StructuredEditError::at_path(
                StructuredEditErrorKind::TomlConflict,
                &path,
                "declared TOML array integration point contains a non-string entry",
            )
        })?;
    if entries.iter().collect::<BTreeSet<_>>().len() != entries.len() {
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::DuplicateRegistration,
            &path,
            "declared TOML array integration point contains duplicate entries",
        ));
    }
    if entries.contains(&entry) {
        return Ok(StructuredEditOutcome::AlreadyPresent {
            kind: StructuredEditKind::TomlArrayString,
            path,
        });
    }
    array.push(entry);
    planned(
        StructuredEditKind::TomlArrayString,
        path,
        observed_source,
        document.to_string().into_bytes(),
    )
}

pub fn plan_toml_table_string(
    path: impl AsRef<Path>,
    observed_source: &[u8],
    table_path: &[&str],
    key: &str,
    entry: &str,
) -> Result<StructuredEditOutcome, StructuredEditError> {
    let path = ChangePath::new(path)?;
    validate_toml_address(table_path, key, entry, &path)?;
    let source = std::str::from_utf8(observed_source).map_err(|_| {
        StructuredEditError::at_path(
            StructuredEditErrorKind::InvalidSource,
            &path,
            "TOML integration source must be UTF-8",
        )
    })?;
    let mut document = parse_toml(source, &path)?;
    let table = find_table(&mut document, table_path, &path)?;
    if let Some(existing) = table.get(key) {
        if existing.as_str() == Some(entry) {
            return Ok(StructuredEditOutcome::AlreadyPresent {
                kind: StructuredEditKind::TomlTableString,
                path,
            });
        }
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::TomlConflict,
            &path,
            "declared TOML key already exists with a different value or type",
        ));
    }
    table.insert(key, value(entry));
    planned(
        StructuredEditKind::TomlTableString,
        path,
        observed_source,
        document.to_string().into_bytes(),
    )
}

fn planned(
    kind: StructuredEditKind,
    path: ChangePath,
    observed_source: &[u8],
    resulting_source: Vec<u8>,
) -> Result<StructuredEditOutcome, StructuredEditError> {
    let edit = StructuredFileEdit::new(path.as_str(), observed_source, resulting_source)?;
    Ok(StructuredEditOutcome::Planned { kind, edit })
}

fn validate_toml_address(
    table_path: &[&str],
    key: &str,
    entry: &str,
    path: &ChangePath,
) -> Result<(), StructuredEditError> {
    if table_path
        .iter()
        .copied()
        .chain([key])
        .any(|part| !valid_toml_key(part))
        || entry.is_empty()
        || entry.chars().any(char::is_control)
    {
        return Err(StructuredEditError::at_path(
            StructuredEditErrorKind::InvalidInput,
            path,
            "TOML integration addresses and string entries must use validated non-empty values",
        ));
    }
    Ok(())
}

fn valid_toml_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn parse_toml(source: &str, path: &ChangePath) -> Result<DocumentMut, StructuredEditError> {
    source.parse().map_err(|_| {
        StructuredEditError::at_path(
            StructuredEditErrorKind::InvalidSource,
            path,
            "TOML integration source is not a supported document",
        )
    })
}

fn find_table<'a>(
    document: &'a mut DocumentMut,
    table_path: &[&str],
    path: &ChangePath,
) -> Result<&'a mut dyn TableLike, StructuredEditError> {
    let mut table: &mut dyn TableLike = document.as_table_mut();
    for segment in table_path {
        table = table
            .get_mut(segment)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| {
                StructuredEditError::at_path(
                    StructuredEditErrorKind::MissingIntegrationPoint,
                    path,
                    "declared TOML table integration point is missing or ambiguous",
                )
            })?;
    }
    Ok(table)
}

fn marker_positions(lines: &[String], marker: &str) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line == marker).then_some(index))
        .collect()
}

fn normalize_newlines(source: &str) -> Result<(String, &'static str, bool), ()> {
    if source.contains('\r') {
        let without_crlf = source.replace("\r\n", "");
        if without_crlf.contains(['\r', '\n']) {
            return Err(());
        }
        Ok((
            source.replace("\r\n", "\n"),
            "\r\n",
            source.ends_with("\r\n"),
        ))
    } else {
        Ok((source.to_owned(), "\n", source.ends_with('\n')))
    }
}

fn restore_newlines(lines: &[String], newline: &str, final_newline: bool) -> String {
    let mut source = lines.join(newline);
    if final_newline {
        source.push_str(newline);
    }
    source
}

fn validate_rust_identifier(identifier: &str) -> Result<(), ()> {
    let valid = !identifier.is_empty()
        && identifier.len() <= 128
        && identifier.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || (byte == b'_' && index > 0 && index + 1 < identifier.len())
                || (byte.is_ascii_digit() && index > 0)
        })
        && !identifier.contains("__")
        && !matches!(
            identifier,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        );
    valid.then_some(()).ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FilePrecondition, PlannedFileChange};

    fn planned(outcome: StructuredEditOutcome) -> StructuredFileEdit {
        match outcome {
            StructuredEditOutcome::Planned { edit, .. } => edit,
            StructuredEditOutcome::AlreadyPresent { .. } => panic!("expected a planned edit"),
        }
    }

    #[test]
    fn rust_modules_are_inserted_in_order_without_changing_surrounding_source() {
        let source = format!(
            "//! User documentation.\n\n{RUST_MODULES_START}\npub mod account;\npub mod zebra;\n{RUST_MODULES_END}\n\npub const USER_VALUE: u8 = 7;\n"
        );
        let edit = planned(
            plan_rust_module("crates/domain/src/lib.rs", source.as_bytes(), "order").unwrap(),
        );
        let result = std::str::from_utf8(edit.resulting_content()).unwrap();

        assert_eq!(
            result,
            format!(
                "//! User documentation.\n\n{RUST_MODULES_START}\npub mod account;\npub mod order;\npub mod zebra;\n{RUST_MODULES_END}\n\npub const USER_VALUE: u8 = 7;\n"
            )
        );
        assert!(matches!(
            PlannedFileChange::from(edit).precondition(),
            FilePrecondition::MatchesDigest(_)
        ));
    }

    #[test]
    fn repeated_rust_registration_is_an_explicit_existing_result() {
        let source = format!("{RUST_MODULES_START}\npub mod order;\n{RUST_MODULES_END}\n");
        assert!(matches!(
            plan_rust_module("crates/domain/src/lib.rs", source.as_bytes(), "order").unwrap(),
            StructuredEditOutcome::AlreadyPresent {
                kind: StructuredEditKind::RustModule,
                ..
            }
        ));
    }

    #[test]
    fn rust_marker_and_registration_conflicts_are_typed() {
        let cases = [
            (
                "pub mod order;\n",
                StructuredEditErrorKind::MissingIntegrationPoint,
            ),
            (
                "// hegira:generated-modules:start\n// hegira:generated-modules:start\n// hegira:generated-modules:end\n",
                StructuredEditErrorKind::AmbiguousIntegrationPoint,
            ),
            (
                "pub mod order;\n// hegira:generated-modules:start\n// hegira:generated-modules:end\n",
                StructuredEditErrorKind::ExistingOutsideManagedBlock,
            ),
            (
                "// hegira:generated-modules:start\npub mod order;\npub mod order;\n// hegira:generated-modules:end\n",
                StructuredEditErrorKind::DuplicateRegistration,
            ),
            (
                "// hegira:generated-modules:start\npub mod zebra;\npub mod account;\n// hegira:generated-modules:end\n",
                StructuredEditErrorKind::ReorderedRegistrations,
            ),
        ];
        for (source, kind) in cases {
            assert_eq!(
                plan_rust_module("crates/domain/src/lib.rs", source.as_bytes(), "order")
                    .unwrap_err()
                    .kind(),
                kind
            );
        }
    }

    #[test]
    fn rust_editor_preserves_consistent_crlf() {
        let source = format!("{RUST_MODULES_START}\r\n{RUST_MODULES_END}\r\n");
        let edit = planned(
            plan_rust_module("crates/domain/src/lib.rs", source.as_bytes(), "order").unwrap(),
        );
        let result = std::str::from_utf8(edit.resulting_content()).unwrap();
        assert_eq!(
            result,
            format!("{RUST_MODULES_START}\r\npub mod order;\r\n{RUST_MODULES_END}\r\n")
        );
    }

    #[test]
    fn invalid_identifiers_cannot_expand_rust_source() {
        for module in [
            "Order",
            "order; use secret",
            "../order",
            "mod",
            "order__item",
        ] {
            assert_eq!(
                plan_rust_module(
                    "crates/domain/src/lib.rs",
                    format!("{RUST_MODULES_START}\n{RUST_MODULES_END}\n").as_bytes(),
                    module,
                )
                .unwrap_err()
                .kind(),
                StructuredEditErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn toml_array_edit_preserves_comments_and_existing_order() {
        let source = b"# user comment\n[workspace]\nmembers = [\"apps/server\", \"crates/domain\"] # retained\nexclude = [\"user-owned\"]\n";
        let edit = planned(
            plan_toml_array_string(
                "Cargo.toml",
                source,
                &["workspace"],
                "members",
                "crates/application",
            )
            .unwrap(),
        );
        let result = std::str::from_utf8(edit.resulting_content()).unwrap();

        assert!(result.starts_with("# user comment\n"));
        assert!(result.contains("# retained"));
        assert!(result.contains("exclude = [\"user-owned\"]"));
        assert!(
            result
                .contains("members = [\"apps/server\", \"crates/domain\", \"crates/application\"]")
        );
    }

    #[test]
    fn toml_table_edit_preserves_unrelated_values_and_is_idempotent() {
        let source = b"[workspace.dependencies]\n# retained dependency\nserde = \"1\"\n";
        let edit = planned(
            plan_toml_table_string(
                "Cargo.toml",
                source,
                &["workspace", "dependencies"],
                "uuid",
                "1",
            )
            .unwrap(),
        );
        let result = edit.resulting_content();
        assert!(
            std::str::from_utf8(result)
                .unwrap()
                .contains("# retained dependency")
        );
        assert!(matches!(
            plan_toml_table_string(
                "Cargo.toml",
                result,
                &["workspace", "dependencies"],
                "uuid",
                "1",
            )
            .unwrap(),
            StructuredEditOutcome::AlreadyPresent { .. }
        ));
    }

    #[test]
    fn toml_conflicts_do_not_guess_at_user_intent() {
        let duplicate = b"[workspace]\nmembers = [\"apps/server\", \"apps/server\"]\n";
        assert_eq!(
            plan_toml_array_string(
                "Cargo.toml",
                duplicate,
                &["workspace"],
                "members",
                "crates/domain",
            )
            .unwrap_err()
            .kind(),
            StructuredEditErrorKind::DuplicateRegistration
        );

        let occupied = b"[workspace.dependencies]\nuuid = \"0\"\n";
        assert_eq!(
            plan_toml_table_string(
                "Cargo.toml",
                occupied,
                &["workspace", "dependencies"],
                "uuid",
                "1",
            )
            .unwrap_err()
            .kind(),
            StructuredEditErrorKind::TomlConflict
        );
    }

    #[test]
    fn canonical_layered_integration_points_are_supported_without_writes() {
        let application =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/applications/layered");
        for relative in [
            "crates/domain_shared/src/lib.rs",
            "crates/domain/src/lib.rs",
            "crates/application_contracts/src/lib.rs",
            "crates/application/src/lib.rs",
            "crates/infrastructure/src/lib.rs",
            "crates/presentation/src/lib.rs",
            "apps/web/src/lib.rs",
        ] {
            let source = std::fs::read(application.join(relative)).unwrap();
            assert!(matches!(
                plan_rust_module(relative, &source, "generated_probe").unwrap(),
                StructuredEditOutcome::Planned {
                    kind: StructuredEditKind::RustModule,
                    ..
                }
            ));
        }

        let manifest = std::fs::read(application.join("Cargo.toml")).unwrap();
        assert!(matches!(
            plan_toml_array_string(
                "Cargo.toml",
                &manifest,
                &["workspace"],
                "members",
                "crates/generated_probe",
            )
            .unwrap(),
            StructuredEditOutcome::Planned {
                kind: StructuredEditKind::TomlArrayString,
                ..
            }
        ));
    }
}
