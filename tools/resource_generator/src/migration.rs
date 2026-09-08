use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    fs,
    path::{Path, PathBuf},
};

use application_mutator::{ChangePlan, FileCreation, PlannedFileChange, StructuredFileEdit};
use serde::{Deserialize, Serialize};

use crate::SelectedDatabase;

pub const MIGRATION_STATE_SCHEMA: u32 = 1;
const MIGRATION_ROOT: &str = "crates/infrastructure/migrations";
const MIGRATION_STATE: &str = "crates/infrastructure/migrations/.hegira-generator.toml";
const MAX_IDENTITY_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationIdentity(String);

impl MigrationIdentity {
    pub fn new(value: impl Into<String>) -> Result<Self, MigrationError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_IDENTITY_BYTES
            && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            && !value.as_bytes().windows(2).any(|pair| pair == b"__");
        if !valid {
            return Err(MigrationError::new(
                MigrationErrorKind::InvalidIdentity,
                "migration identities must use 1–64 bytes of lowercase ASCII snake_case",
            ));
        }
        if matches!(value.as_str(), "migration" | "up" | "down" | "hegira") {
            return Err(MigrationError::new(
                MigrationErrorKind::InvalidIdentity,
                format!("migration identity `{value}` is reserved"),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMigration {
    version: i64,
    identity: MigrationIdentity,
    path: String,
    plan: ChangePlan,
}

impl PlannedMigration {
    pub const fn version(&self) -> i64 {
        self.version
    }

    pub fn identity(&self) -> &MigrationIdentity {
        &self.identity
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn plan(&self) -> &ChangePlan {
        &self.plan
    }

    pub fn into_plan(self) -> ChangePlan {
        self.plan
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationErrorKind {
    InvalidIdentity,
    UnsafeApplication,
    InvalidHistory,
    IdentityCollision,
    VersionExhausted,
    InvalidState,
    Planning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationError {
    kind: MigrationErrorKind,
    message: String,
}

impl MigrationError {
    fn new(kind: MigrationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> MigrationErrorKind {
        self.kind
    }
}

impl Display for MigrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MigrationError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationState {
    schema: u32,
    database: StateDatabase,
    next_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StateDatabase {
    Postgres,
    Sqlite,
}

impl From<SelectedDatabase> for StateDatabase {
    fn from(database: SelectedDatabase) -> Self {
        match database {
            SelectedDatabase::Postgres => Self::Postgres,
            SelectedDatabase::Sqlite => Self::Sqlite,
        }
    }
}

struct ObservedHistory {
    highest_version: i64,
    identities: BTreeMap<String, i64>,
    state: Option<Vec<u8>>,
}

pub fn plan_application_migration(
    application_root: &Path,
    database: SelectedDatabase,
    identity: MigrationIdentity,
) -> Result<PlannedMigration, MigrationError> {
    let source = migration_source(database, &identity);
    plan_application_migration_with_source(application_root, database, identity, source)
}

pub(crate) fn plan_application_migration_with_source(
    application_root: &Path,
    database: SelectedDatabase,
    identity: MigrationIdentity,
    source: Vec<u8>,
) -> Result<PlannedMigration, MigrationError> {
    let history = observe_history(application_root, database)?;
    if let Some(version) = history.identities.get(identity.as_str()) {
        return Err(MigrationError::new(
            MigrationErrorKind::IdentityCollision,
            format!(
                "migration identity `{}` already exists at version {version}",
                identity.as_str()
            ),
        ));
    }

    let database_state = StateDatabase::from(database);
    let observed_state = history.state.as_deref().map(parse_state).transpose()?;
    if let Some(state) = &observed_state
        && state.database != database_state
    {
        return Err(MigrationError::new(
            MigrationErrorKind::InvalidState,
            "migration generator state belongs to a different database adapter",
        ));
    }
    let state_next = observed_state
        .as_ref()
        .map_or(1, |state| state.next_version);
    let history_next = history.highest_version.checked_add(1).ok_or_else(|| {
        MigrationError::new(
            MigrationErrorKind::VersionExhausted,
            "migration version space is exhausted",
        )
    })?;
    let version = state_next.max(history_next);
    if version <= 0 {
        return Err(MigrationError::new(
            MigrationErrorKind::InvalidState,
            "migration generator state must contain a positive next version",
        ));
    }
    let next_version = version.checked_add(1).ok_or_else(|| {
        MigrationError::new(
            MigrationErrorKind::VersionExhausted,
            "migration version space is exhausted",
        )
    })?;

    let provider = database_directory(database);
    let filename = format!("{version:03}_{}.sql", identity.as_str());
    let path = format!("{MIGRATION_ROOT}/{provider}/{filename}");
    let migration = FileCreation::new(&path, source)
        .map_err(|error| MigrationError::new(MigrationErrorKind::Planning, error.to_string()))?;
    let state = MigrationState {
        schema: MIGRATION_STATE_SCHEMA,
        database: database_state,
        next_version,
    };
    let state_source = serialize_state(&state)?;
    let state_change = match history.state {
        Some(observed) => PlannedFileChange::from(
            StructuredFileEdit::new(MIGRATION_STATE, &observed, state_source).map_err(|error| {
                MigrationError::new(MigrationErrorKind::Planning, error.to_string())
            })?,
        ),
        None => PlannedFileChange::from(FileCreation::new(MIGRATION_STATE, state_source).map_err(
            |error| MigrationError::new(MigrationErrorKind::Planning, error.to_string()),
        )?),
    };
    let plan = ChangePlan::new([PlannedFileChange::from(migration), state_change])
        .map_err(|error| MigrationError::new(MigrationErrorKind::Planning, error.to_string()))?;

    Ok(PlannedMigration {
        version,
        identity,
        path,
        plan,
    })
}

fn observe_history(
    application_root: &Path,
    database: SelectedDatabase,
) -> Result<ObservedHistory, MigrationError> {
    let root = safe_application_root(application_root)?;
    let migration_root = checked_directory(&root, MIGRATION_ROOT)?;
    let provider = database_directory(database);
    let provider_directory = checked_directory(&migration_root, provider)?;
    let mut versions = BTreeMap::new();
    let mut identities = BTreeMap::new();

    let entries = fs::read_dir(&provider_directory).map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::UnsafeApplication,
            "cannot read the selected application migration directory",
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|_| {
            MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "cannot inspect the selected application migration directory",
            )
        })?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|_| {
            MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "cannot inspect an application migration entry",
            )
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "application migration directories may contain only regular files",
            ));
        }
        let name = entry.file_name().into_string().map_err(|_| {
            MigrationError::new(
                MigrationErrorKind::InvalidHistory,
                "application migration filenames must be valid UTF-8",
            )
        })?;
        if !name.ends_with(".sql") {
            return Err(MigrationError::new(
                MigrationErrorKind::InvalidHistory,
                "selected application migration directory contains a non-SQL file",
            ));
        }
        let (version, identity) = parse_migration_filename(&name)?;
        if let Some(existing) = versions.insert(version, identity.clone()) {
            return Err(MigrationError::new(
                MigrationErrorKind::InvalidHistory,
                format!(
                    "migration version {version} is used by both `{existing}` and `{identity}`"
                ),
            ));
        }
        if let Some(existing) = identities.insert(identity.clone(), version) {
            return Err(MigrationError::new(
                MigrationErrorKind::InvalidHistory,
                format!(
                    "migration identity `{identity}` is used by versions {existing} and {version}"
                ),
            ));
        }
    }

    let state_path = root.join(MIGRATION_STATE);
    let state = match fs::symlink_metadata(&state_path) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            Some(fs::read(state_path).map_err(|_| {
                MigrationError::new(
                    MigrationErrorKind::UnsafeApplication,
                    "cannot read application migration generator state",
                )
            })?)
        }
        Ok(_) => {
            return Err(MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "application migration generator state must be a regular file",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            return Err(MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "cannot inspect application migration generator state",
            ));
        }
    };

    Ok(ObservedHistory {
        highest_version: versions.last_key_value().map_or(0, |(version, _)| *version),
        identities,
        state,
    })
}

fn safe_application_root(root: &Path) -> Result<PathBuf, MigrationError> {
    if !root.is_absolute() {
        return Err(MigrationError::new(
            MigrationErrorKind::UnsafeApplication,
            "application root must be absolute before migration planning",
        ));
    }
    let canonical = fs::canonicalize(root).map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::UnsafeApplication,
            "application root must be an existing real directory",
        )
    })?;
    if canonical != root {
        return Err(MigrationError::new(
            MigrationErrorKind::UnsafeApplication,
            "application root may not contain symlinks or a non-canonical spelling",
        ));
    }
    let metadata = fs::symlink_metadata(root).map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::UnsafeApplication,
            "cannot inspect the application root",
        )
    })?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(MigrationError::new(
            MigrationErrorKind::UnsafeApplication,
            "application root must be a real directory",
        ));
    }
    Ok(canonical)
}

fn checked_directory(parent: &Path, child: &str) -> Result<PathBuf, MigrationError> {
    let mut path = parent.to_path_buf();
    for component in Path::new(child).components() {
        let std::path::Component::Normal(component) = component else {
            return Err(MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "application migration paths must contain only normal components",
            ));
        };
        path.push(component);
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "required application migration directory is missing",
            )
        })?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(MigrationError::new(
                MigrationErrorKind::UnsafeApplication,
                "application migration path components must be real directories",
            ));
        }
    }
    Ok(path)
}

fn parse_migration_filename(name: &str) -> Result<(i64, String), MigrationError> {
    let source = name.strip_suffix(".sql").ok_or_else(|| {
        MigrationError::new(
            MigrationErrorKind::InvalidHistory,
            "application migration filenames must end in .sql",
        )
    })?;
    let (version, identity) = source.split_once('_').ok_or_else(|| {
        MigrationError::new(
            MigrationErrorKind::InvalidHistory,
            "application migration filenames must use <version>_<identity>.sql",
        )
    })?;
    if version.len() < 3
        || !version.bytes().all(|byte| byte.is_ascii_digit())
        || (version.len() > 3 && version.starts_with('0'))
    {
        return Err(MigrationError::new(
            MigrationErrorKind::InvalidHistory,
            "application migration versions must be canonical integers padded to at least three digits",
        ));
    }
    let version = version.parse::<i64>().map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::InvalidHistory,
            "application migration version is outside the supported integer range",
        )
    })?;
    if version <= 0 || format!("{version:03}") != source.split_once('_').unwrap().0 {
        return Err(MigrationError::new(
            MigrationErrorKind::InvalidHistory,
            "application migration version is not in canonical positive form",
        ));
    }
    let identity = MigrationIdentity::new(identity.to_owned()).map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::InvalidHistory,
            "application migration filename contains an invalid identity",
        )
    })?;
    Ok((version, identity.0))
}

fn parse_state(source: &[u8]) -> Result<MigrationState, MigrationError> {
    let source = std::str::from_utf8(source).map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::InvalidState,
            "migration generator state must be UTF-8 TOML",
        )
    })?;
    let state: MigrationState = toml::from_str(source).map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::InvalidState,
            "migration generator state is invalid",
        )
    })?;
    if state.schema != MIGRATION_STATE_SCHEMA || state.next_version <= 0 {
        return Err(MigrationError::new(
            MigrationErrorKind::InvalidState,
            "migration generator state has an unsupported schema or next version",
        ));
    }
    Ok(state)
}

fn serialize_state(state: &MigrationState) -> Result<Vec<u8>, MigrationError> {
    let mut source = toml::to_string(state).map_err(|_| {
        MigrationError::new(
            MigrationErrorKind::Planning,
            "cannot serialize migration generator state",
        )
    })?;
    if !source.ends_with('\n') {
        source.push('\n');
    }
    Ok(source.into_bytes())
}

fn migration_source(database: SelectedDatabase, identity: &MigrationIdentity) -> Vec<u8> {
    let provider = match database {
        SelectedDatabase::Postgres => "PostgreSQL",
        SelectedDatabase::Sqlite => "SQLite",
    };
    format!(
        "-- Application-owned {provider} migration: {}\n-- Add forward-only {provider} SQL below this line.\n",
        identity.as_str()
    )
    .into_bytes()
}

const fn database_directory(database: SelectedDatabase) -> &'static str {
    match database {
        SelectedDatabase::Postgres => "postgres",
        SelectedDatabase::Sqlite => "sqlite",
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use application_mutator::{MutationErrorKind, publish_change_plan};

    use super::*;

    struct TestApplication(PathBuf);

    impl TestApplication {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "hegira-migration-planner-{}-{nonce}",
                std::process::id()
            ));
            for provider in ["sqlite", "postgres"] {
                fs::create_dir_all(root.join(MIGRATION_ROOT).join(provider)).unwrap();
            }
            fs::write(
                root.join(MIGRATION_ROOT).join("sqlite/009_existing.sql"),
                "-- preserved sqlite\n",
            )
            .unwrap();
            fs::write(
                root.join(MIGRATION_ROOT).join("postgres/022_existing.sql"),
                "-- preserved postgres\n",
            )
            .unwrap();
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestApplication {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn planned_source<'a>(plan: &'a ChangePlan, path: &str) -> &'a [u8] {
        plan.changes()
            .iter()
            .find(|change| change.path().as_str() == path)
            .unwrap()
            .resulting_content()
    }

    #[test]
    fn selected_provider_produces_one_deterministic_append_only_migration() {
        let first_root = TestApplication::new();
        let second_root = TestApplication::new();
        let first = plan_application_migration(
            first_root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("add_orders").unwrap(),
        )
        .unwrap();
        let second = plan_application_migration(
            second_root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("add_orders").unwrap(),
        )
        .unwrap();

        assert_eq!(first.version(), 10);
        assert_eq!(
            first.path(),
            "crates/infrastructure/migrations/sqlite/010_add_orders.sql"
        );
        assert_eq!(first.plan().summary(), second.plan().summary());
        assert_eq!(
            planned_source(first.plan(), first.path()),
            b"-- Application-owned SQLite migration: add_orders\n-- Add forward-only SQLite SQL below this line.\n"
        );
        assert_eq!(first.plan().changes().len(), 2);
    }

    #[test]
    fn postgres_uses_its_own_history_path_and_source_contract() {
        let root = TestApplication::new();
        let migration = plan_application_migration(
            root.path(),
            SelectedDatabase::Postgres,
            MigrationIdentity::new("add_orders").unwrap(),
        )
        .unwrap();

        assert_eq!(migration.version(), 23);
        assert_eq!(
            migration.path(),
            "crates/infrastructure/migrations/postgres/023_add_orders.sql"
        );
        let source =
            std::str::from_utf8(planned_source(migration.plan(), migration.path())).unwrap();
        assert!(source.contains("PostgreSQL"));
        assert!(!source.contains("SQLite"));
    }

    #[test]
    fn publication_preserves_history_and_repeated_identity_is_a_conflict() {
        let root = TestApplication::new();
        let sqlite_history = fs::read(
            root.path()
                .join(MIGRATION_ROOT)
                .join("sqlite/009_existing.sql"),
        )
        .unwrap();
        let migration = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("add_orders").unwrap(),
        )
        .unwrap();

        publish_change_plan(root.path(), migration.plan()).unwrap();

        assert_eq!(
            fs::read(
                root.path()
                    .join(MIGRATION_ROOT)
                    .join("sqlite/009_existing.sql")
            )
            .unwrap(),
            sqlite_history
        );
        assert!(root.path().join(migration.path()).is_file());
        let repeated = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("add_orders").unwrap(),
        )
        .unwrap_err();
        assert_eq!(repeated.kind(), MigrationErrorKind::IdentityCollision);
    }

    #[test]
    fn generator_state_serializes_competing_plans_without_partial_output() {
        let root = TestApplication::new();
        let first = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("first_change").unwrap(),
        )
        .unwrap();
        let stale = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("second_change").unwrap(),
        )
        .unwrap();

        publish_change_plan(root.path(), first.plan()).unwrap();
        let error = publish_change_plan(root.path(), stale.plan()).unwrap_err();

        assert_eq!(error.kind(), MutationErrorKind::PreconditionFailed);
        assert!(!root.path().join(stale.path()).exists());
        assert!(root.path().join(first.path()).exists());
    }

    #[test]
    fn state_advances_versions_and_never_embeds_environment_or_credentials() {
        let root = TestApplication::new();
        let first = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("first_change").unwrap(),
        )
        .unwrap();
        publish_change_plan(root.path(), first.plan()).unwrap();
        let second = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("second_change").unwrap(),
        )
        .unwrap();

        assert_eq!(second.version(), 11);
        let state = planned_source(second.plan(), MIGRATION_STATE);
        let state = std::str::from_utf8(state).unwrap();
        assert!(state.contains("next_version = 12"));
        assert!(!state.contains("DATABASE_URL"));
        assert!(!state.to_ascii_lowercase().contains("password"));
        let source = std::str::from_utf8(planned_source(second.plan(), second.path())).unwrap();
        assert!(!source.contains("DATABASE_URL"));
        assert!(!source.to_ascii_lowercase().contains("password"));
    }

    #[test]
    fn invalid_identity_history_and_state_fail_before_planning() {
        assert_eq!(
            MigrationIdentity::new("../escape").unwrap_err().kind(),
            MigrationErrorKind::InvalidIdentity
        );

        let malformed = TestApplication::new();
        fs::write(
            malformed
                .path()
                .join(MIGRATION_ROOT)
                .join("sqlite/not-valid.sql"),
            "-- invalid\n",
        )
        .unwrap();
        let error = plan_application_migration(
            malformed.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("safe_change").unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), MigrationErrorKind::InvalidHistory);

        let invalid_state = TestApplication::new();
        fs::write(invalid_state.path().join(MIGRATION_STATE), "schema = 999\n").unwrap();
        let error = plan_application_migration(
            invalid_state.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("safe_change").unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), MigrationErrorKind::InvalidState);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_migration_directories_fail_closed() {
        use std::os::unix::fs::symlink;

        let root = TestApplication::new();
        let outside = TestApplication::new();
        fs::remove_dir_all(root.path().join(MIGRATION_ROOT).join("sqlite")).unwrap();
        symlink(
            outside.path().join(MIGRATION_ROOT).join("sqlite"),
            root.path().join(MIGRATION_ROOT).join("sqlite"),
        )
        .unwrap();

        let error = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("safe_change").unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), MigrationErrorKind::UnsafeApplication);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_migration_ancestor_fails_closed() {
        use std::os::unix::fs::symlink;

        let root = TestApplication::new();
        let outside = TestApplication::new();
        fs::remove_dir_all(root.path().join("crates/infrastructure")).unwrap();
        symlink(
            outside.path().join("crates/infrastructure"),
            root.path().join("crates/infrastructure"),
        )
        .unwrap();

        let error = plan_application_migration(
            root.path(),
            SelectedDatabase::Sqlite,
            MigrationIdentity::new("safe_change").unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), MigrationErrorKind::UnsafeApplication);
    }
}
