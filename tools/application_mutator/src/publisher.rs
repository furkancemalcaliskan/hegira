use std::{
    fmt::{Display, Formatter},
    path::Path,
};

use crate::{ChangePath, ChangePlan, ChangePlanError, PlannedFileChange};

pub const MUTATION_MARKER: &str = ".hegira-mutation.lock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationErrorKind {
    InvalidPlan,
    UnsupportedPlatform,
    UnsafeRoot,
    RecoveryRequired,
    PreconditionFailed,
    PublicationFailed,
    RollbackIncomplete,
}

#[derive(Debug)]
pub struct MutationError {
    kind: MutationErrorKind,
    path: Option<ChangePath>,
    message: String,
}

impl MutationError {
    pub fn kind(&self) -> MutationErrorKind {
        self.kind
    }

    pub fn path(&self) -> Option<&ChangePath> {
        self.path.as_ref()
    }

    fn new(kind: MutationErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: None,
            message: message.into(),
        }
    }

    fn at_path(kind: MutationErrorKind, path: &ChangePath, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: Some(path.clone()),
            message: message.into(),
        }
    }
}

impl Display for MutationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MutationError {}

impl From<ChangePlanError> for MutationError {
    fn from(error: ChangePlanError) -> Self {
        Self {
            kind: MutationErrorKind::InvalidPlan,
            path: error.path().cloned(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationReceipt {
    changed_files: usize,
}

impl MutationReceipt {
    pub fn changed_files(self) -> usize {
        self.changed_files
    }
}

pub fn publish_change_plan(
    application_root: &Path,
    plan: &ChangePlan,
) -> Result<MutationReceipt, MutationError> {
    plan.validate()?;
    if plan.is_empty() {
        return Ok(MutationReceipt { changed_files: 0 });
    }
    platform::publish(application_root, plan)
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod platform {
    use std::{
        collections::BTreeSet,
        ffi::{OsStr, OsString},
        fs::File,
        io::{Read, Write},
        os::unix::fs::MetadataExt,
        path::{Component, Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use rustix::{
        fd::OwnedFd,
        fs::{self, AtFlags, Mode, OFlags, RenameFlags},
        io::Errno,
    };

    use super::*;
    use crate::{ChangeOperation, ContentDigest, FilePrecondition};

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const READ_FILE: OFlags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const PRIVATE_FILE: OFlags = OFlags::WRONLY
        .union(OFlags::CREATE)
        .union(OFlags::EXCL)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const PRIVATE_MODE: Mode = Mode::RUSR.union(Mode::WUSR);
    const CREATED_MODE: Mode = Mode::RUSR
        .union(Mode::WUSR)
        .union(Mode::RGRP)
        .union(Mode::ROTH);

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    struct Identity {
        device: u64,
        inode: u64,
    }

    struct ObservedFile {
        identity: Identity,
        digest: ContentDigest,
        mode: u32,
    }

    struct Root {
        path: PathBuf,
        fd: OwnedFd,
        identity: Identity,
    }

    impl Root {
        fn open(path: &Path) -> Result<Self, MutationError> {
            let path = checked_root(path)?;
            let fd = open_directory_path(&path)?;
            let identity = directory_identity(&fd)?;
            Ok(Self { path, fd, identity })
        }

        fn verify_namespace(&self) -> Result<(), MutationError> {
            let current = open_directory_path(&self.path).map_err(|_| {
                MutationError::new(
                    MutationErrorKind::UnsafeRoot,
                    "application root changed during mutation; manual inspection may be required",
                )
            })?;
            if directory_identity(&current)? != self.identity {
                return Err(MutationError::new(
                    MutationErrorKind::UnsafeRoot,
                    "application root changed during mutation; manual inspection may be required",
                ));
            }
            Ok(())
        }
    }

    struct Marker {
        identity: Identity,
    }

    struct PreparedChange {
        path: ChangePath,
        parent_path: PathBuf,
        name: OsString,
        parent: OwnedFd,
        temp_name: OsString,
        operation: ChangeOperation,
        expected: FilePrecondition,
        result_digest: ContentDigest,
        original: Option<ObservedFile>,
        staged_identity: Identity,
        published: bool,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum HookPoint {
        BeforePublication,
        BeforeChange(usize),
        AfterChange(usize),
    }

    pub(super) fn publish(
        application_root: &Path,
        plan: &ChangePlan,
    ) -> Result<MutationReceipt, MutationError> {
        publish_with(application_root, plan, |_| Ok(()))
    }

    fn publish_with(
        application_root: &Path,
        plan: &ChangePlan,
        mut hook: impl FnMut(HookPoint) -> Result<(), MutationError>,
    ) -> Result<MutationReceipt, MutationError> {
        let root = Root::open(application_root)?;
        if plan
            .changes()
            .iter()
            .any(|change| change.path().as_str() == MUTATION_MARKER)
        {
            return Err(MutationError::new(
                MutationErrorKind::InvalidPlan,
                "change plans may not target the reserved mutation marker",
            ));
        }

        let transaction = transaction_id()?;
        let marker = create_marker(&root, plan, &transaction)?;
        let mut prepared = Vec::with_capacity(plan.changes().len());
        if let Err(error) = prepare_changes(&root, plan, &transaction, &mut prepared) {
            return cleanup_before_publication(&root, &marker, &mut prepared, error);
        }

        let result = (|| {
            preflight_filesystem_semantics(&prepared, &transaction)?;
            root.verify_namespace()?;
            verify_marker(&root, &marker)?;
            verify_all_preconditions(&root, &prepared)?;
            hook(HookPoint::BeforePublication)?;

            for (index, change) in prepared.iter_mut().enumerate() {
                root.verify_namespace()?;
                verify_marker(&root, &marker)?;
                verify_parent_namespace(&root, change)?;
                hook(HookPoint::BeforeChange(index))?;
                verify_parent_namespace(&root, change)?;
                verify_precondition(change)?;
                publish_one(change)?;
                change.published = true;
                verify_published(change)?;
                sync_directory(&change.parent)?;
                hook(HookPoint::AfterChange(index))?;
                root.verify_namespace()?;
                verify_marker(&root, &marker)?;
                verify_parent_namespace(&root, change)?;
            }
            root.verify_namespace()?;
            verify_marker(&root, &marker)?;
            verify_all_parent_namespaces(&root, &prepared)?;
            Ok(())
        })();

        if let Err(error) = result {
            return rollback_or_report(&root, &marker, &mut prepared, error);
        }

        for change in &prepared {
            if change.operation == ChangeOperation::Edit {
                remove_backup(change)?;
            }
        }
        sync_unique_parents(&prepared)?;
        remove_marker(&root, &marker).map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "mutation completed but its recovery marker could not be removed; inspect the application before another mutation",
            )
        })?;
        sync_directory(&root.fd).map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "mutation completed but marker cleanup durability is uncertain; inspect the application before another mutation",
            )
        })?;
        Ok(MutationReceipt {
            changed_files: prepared.len(),
        })
    }

    fn checked_root(path: &Path) -> Result<PathBuf, MutationError> {
        let text = path.to_str().ok_or_else(|| {
            MutationError::new(
                MutationErrorKind::UnsafeRoot,
                "application root must be valid UTF-8",
            )
        })?;
        if text.is_empty() || text.contains('\\') || text.chars().any(char::is_control) {
            return Err(MutationError::new(
                MutationErrorKind::UnsafeRoot,
                "application root must be non-empty and contain no controls or backslashes",
            ));
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(MutationError::new(
                MutationErrorKind::UnsafeRoot,
                "application root may not contain parent traversal",
            ));
        }
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| {
                    MutationError::new(
                        MutationErrorKind::UnsafeRoot,
                        "cannot resolve the current directory",
                    )
                })?
                .join(path)
        };
        if absolute.file_name().is_none() {
            return Err(MutationError::new(
                MutationErrorKind::UnsafeRoot,
                "application root must name a directory below the filesystem root",
            ));
        }
        Ok(absolute)
    }

    fn open_directory_path(path: &Path) -> Result<OwnedFd, MutationError> {
        let mut fd = fs::open("/", DIRECTORY, Mode::empty()).map_err(|_| {
            MutationError::new(
                MutationErrorKind::UnsafeRoot,
                "cannot open the filesystem root",
            )
        })?;
        for component in path.components() {
            let name = match component {
                Component::RootDir | Component::CurDir => continue,
                Component::Normal(name) => name,
                _ => {
                    return Err(MutationError::new(
                        MutationErrorKind::UnsafeRoot,
                        "application root contains an unsupported path component",
                    ));
                }
            };
            fd = fs::openat(&fd, name, DIRECTORY, Mode::empty()).map_err(|_| {
                MutationError::new(
                    MutationErrorKind::UnsafeRoot,
                    "application root and its ancestors must be existing real directories",
                )
            })?;
        }
        Ok(fd)
    }

    fn descend(root: &OwnedFd, path: &Path, change: &ChangePath) -> Result<OwnedFd, MutationError> {
        let mut fd = fs::openat(root, ".", DIRECTORY, Mode::empty()).map_err(|_| {
            MutationError::at_path(
                MutationErrorKind::UnsafeRoot,
                change,
                "cannot anchor a change to the application root",
            )
        })?;
        for component in path.components() {
            let Component::Normal(name) = component else {
                return Err(MutationError::at_path(
                    MutationErrorKind::UnsafeRoot,
                    change,
                    "change parent contains an unsupported path component",
                ));
            };
            fd = fs::openat(&fd, name, DIRECTORY, Mode::empty()).map_err(|_| {
                MutationError::at_path(
                    MutationErrorKind::UnsafeRoot,
                    change,
                    "change parents must be existing real directories",
                )
            })?;
        }
        Ok(fd)
    }

    fn directory_identity(fd: &OwnedFd) -> Result<Identity, MutationError> {
        let stat = fs::fstat(fd).map_err(|_| {
            MutationError::new(
                MutationErrorKind::UnsafeRoot,
                "cannot inspect an opened application directory",
            )
        })?;
        Ok(Identity {
            device: stat.st_dev as u64,
            inode: stat.st_ino as u64,
        })
    }

    fn transaction_id() -> Result<String, MutationError> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                MutationError::new(
                    MutationErrorKind::PublicationFailed,
                    "cannot allocate a mutation transaction identifier",
                )
            })?
            .as_nanos();
        Ok(format!("{}-{nonce}", std::process::id()))
    }

    fn create_marker(
        root: &Root,
        plan: &ChangePlan,
        transaction: &str,
    ) -> Result<Marker, MutationError> {
        let fd = match fs::openat(&root.fd, MUTATION_MARKER, PRIVATE_FILE, PRIVATE_MODE) {
            Ok(fd) => fd,
            Err(Errno::EXIST) => {
                return Err(MutationError::new(
                    MutationErrorKind::RecoveryRequired,
                    "an existing mutation marker requires inspection before another mutation",
                ));
            }
            Err(_) => {
                return Err(MutationError::new(
                    MutationErrorKind::PublicationFailed,
                    "cannot create the private mutation marker",
                ));
            }
        };
        let mut file = File::from(fd);
        let identity = file_identity(&file).map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "cannot inspect the private mutation marker; manual recovery is required",
            )
        })?;
        let mut manifest = format!("schema=1\ntransaction={transaction}\nstate=publishing\n");
        for (index, change) in plan.changes().iter().enumerate() {
            let operation = match change.operation() {
                ChangeOperation::Create => "create",
                ChangeOperation::Edit => "edit",
            };
            let precondition = match change.precondition() {
                FilePrecondition::Absent => "absent".to_owned(),
                FilePrecondition::MatchesDigest(digest) => digest.to_hex(),
            };
            manifest.push_str(&format!(
                "change={index}\t{operation}\t{}\t{precondition}\t{}\t{}\n",
                change.path(),
                change.result_digest(),
                temp_name(transaction, index)
                    .to_str()
                    .expect("transaction temp names are ASCII")
            ));
        }
        file.write_all(manifest.as_bytes()).map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "cannot write the private mutation marker; inspect it before another mutation",
            )
        })?;
        file.sync_all().map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "cannot make the mutation marker durable; inspect it before another mutation",
            )
        })?;
        sync_directory(&root.fd).map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "cannot make the mutation marker durable; inspect it before another mutation",
            )
        })?;
        Ok(Marker { identity })
    }

    fn prepare_changes(
        root: &Root,
        plan: &ChangePlan,
        transaction: &str,
        prepared: &mut Vec<PreparedChange>,
    ) -> Result<(), MutationError> {
        for (index, change) in plan.changes().iter().enumerate() {
            let path = Path::new(change.path().as_str());
            let parent_path = path.parent().unwrap_or_else(|| Path::new(""));
            let name = path.file_name().expect("ChangePath always names a file");
            let parent = descend(&root.fd, parent_path, change.path())?;
            let original = verify_initial_precondition(&parent, name, change)?;
            let temp_name = temp_name(transaction, index);
            let result_mode = original.as_ref().map_or(CREATED_MODE, |observed| {
                Mode::from_raw_mode(observed.mode & 0o0777)
            });
            let staged_identity = stage_file(
                &parent,
                &temp_name,
                change.path(),
                change.resulting_content(),
                result_mode,
            )?;
            prepared.push(PreparedChange {
                path: change.path().clone(),
                parent_path: parent_path.to_path_buf(),
                name: name.to_os_string(),
                parent,
                temp_name,
                operation: change.operation(),
                expected: change.precondition(),
                result_digest: change.result_digest(),
                original,
                staged_identity,
                published: false,
            });
        }
        sync_unique_parents(prepared)?;
        Ok(())
    }

    fn stage_file(
        parent: &OwnedFd,
        temp_name: &OsStr,
        path: &ChangePath,
        content: &[u8],
        mode: Mode,
    ) -> Result<Identity, MutationError> {
        let fd = fs::openat(parent, temp_name, PRIVATE_FILE, PRIVATE_MODE).map_err(|_| {
            MutationError::at_path(
                MutationErrorKind::PublicationFailed,
                path,
                "cannot create a private staged mutation file",
            )
        })?;
        let mut staged = File::from(fd);
        let identity = file_identity(&staged).map_err(|_| {
            MutationError::at_path(
                MutationErrorKind::RecoveryRequired,
                path,
                "cannot inspect a staged mutation file; manual cleanup is required",
            )
        })?;
        let result = staged
            .write_all(content)
            .map_err(|_| "cannot write a private staged mutation file")
            .and_then(|()| {
                fs::fchmod(&staged, mode).map_err(|_| "cannot set staged mutation file permissions")
            })
            .and_then(|()| {
                staged
                    .sync_all()
                    .map_err(|_| "cannot make a staged mutation file durable")
            });
        if let Err(message) = result {
            drop(staged);
            return if remove_named_if_identity(parent, temp_name, identity).is_ok() {
                Err(MutationError::at_path(
                    MutationErrorKind::PublicationFailed,
                    path,
                    message,
                ))
            } else {
                Err(MutationError::at_path(
                    MutationErrorKind::RecoveryRequired,
                    path,
                    "staged mutation file cleanup requires manual inspection",
                ))
            };
        }
        Ok(identity)
    }

    fn verify_initial_precondition(
        parent: &OwnedFd,
        name: &OsStr,
        change: &PlannedFileChange,
    ) -> Result<Option<ObservedFile>, MutationError> {
        match change.precondition() {
            FilePrecondition::Absent => {
                ensure_absent(parent, name, change.path())?;
                Ok(None)
            }
            FilePrecondition::MatchesDigest(expected) => {
                let observed = read_regular_file(parent, name, change.path())?;
                if observed.digest != expected {
                    return Err(precondition_failed(change.path()));
                }
                Ok(Some(observed))
            }
        }
    }

    fn verify_all_preconditions(
        root: &Root,
        prepared: &[PreparedChange],
    ) -> Result<(), MutationError> {
        for change in prepared {
            verify_parent_namespace(root, change)?;
            verify_precondition(change)?;
        }
        Ok(())
    }

    fn verify_all_parent_namespaces(
        root: &Root,
        prepared: &[PreparedChange],
    ) -> Result<(), MutationError> {
        for change in prepared {
            verify_parent_namespace(root, change)?;
        }
        Ok(())
    }

    fn verify_parent_namespace(root: &Root, change: &PreparedChange) -> Result<(), MutationError> {
        let current = descend(&root.fd, &change.parent_path, &change.path)?;
        if directory_identity(&current)? != directory_identity(&change.parent)? {
            return Err(MutationError::at_path(
                MutationErrorKind::UnsafeRoot,
                &change.path,
                "a change parent was replaced during mutation",
            ));
        }
        Ok(())
    }

    fn verify_precondition(change: &PreparedChange) -> Result<(), MutationError> {
        match (&change.expected, &change.original) {
            (FilePrecondition::Absent, None) => {
                ensure_absent(&change.parent, &change.name, &change.path)
            }
            (FilePrecondition::MatchesDigest(expected), Some(original)) => {
                let observed = read_regular_file(&change.parent, &change.name, &change.path)?;
                if observed.identity != original.identity || observed.digest != *expected {
                    return Err(precondition_failed(&change.path));
                }
                Ok(())
            }
            _ => Err(MutationError::at_path(
                MutationErrorKind::InvalidPlan,
                &change.path,
                "mutation precondition and operation are inconsistent",
            )),
        }
    }

    fn ensure_absent(
        parent: &OwnedFd,
        name: &OsStr,
        path: &ChangePath,
    ) -> Result<(), MutationError> {
        match fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => Ok(()),
            Ok(_) => Err(precondition_failed(path)),
            Err(_) => Err(MutationError::at_path(
                MutationErrorKind::UnsafeRoot,
                path,
                "cannot safely inspect a mutation destination",
            )),
        }
    }

    fn precondition_failed(path: &ChangePath) -> MutationError {
        MutationError::at_path(
            MutationErrorKind::PreconditionFailed,
            path,
            format!("mutation precondition no longer holds for `{path}`"),
        )
    }

    fn read_regular_file(
        parent: &OwnedFd,
        name: &OsStr,
        path: &ChangePath,
    ) -> Result<ObservedFile, MutationError> {
        let fd = fs::openat(parent, name, READ_FILE, Mode::empty())
            .map_err(|_| precondition_failed(path))?;
        let mut file = File::from(fd);
        let metadata = file.metadata().map_err(|_| precondition_failed(path))?;
        if !metadata.is_file() {
            return Err(precondition_failed(path));
        }
        let mut content = Vec::new();
        file.read_to_end(&mut content)
            .map_err(|_| precondition_failed(path))?;
        Ok(ObservedFile {
            identity: Identity {
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            digest: ContentDigest::calculate(&content),
            mode: metadata.mode(),
        })
    }

    fn file_identity(file: &File) -> std::io::Result<Identity> {
        let metadata = file.metadata()?;
        Ok(Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn temp_name(transaction: &str, index: usize) -> OsString {
        OsString::from(format!(".hegira-mutation-{transaction}-{index}"))
    }

    fn probe_name(transaction: &str, index: usize, side: char) -> OsString {
        OsString::from(format!(".hegira-probe-{transaction}-{index}-{side}"))
    }

    fn preflight_filesystem_semantics(
        prepared: &[PreparedChange],
        transaction: &str,
    ) -> Result<(), MutationError> {
        for (index, change) in prepared.iter().enumerate() {
            let first_name = probe_name(transaction, index, 'a');
            let second_name = probe_name(transaction, index, 'b');
            let first = create_probe(&change.parent, &first_name, &change.path)?;
            let second = match create_probe(&change.parent, &second_name, &change.path) {
                Ok(second) => second,
                Err(error) => {
                    return if remove_named_if_identity(&change.parent, &first_name, first).is_ok() {
                        Err(error)
                    } else {
                        Err(MutationError::at_path(
                            MutationErrorKind::RecoveryRequired,
                            &change.path,
                            "filesystem safety probe cleanup requires manual inspection",
                        ))
                    };
                }
            };
            let mut exchanged = false;
            let check = match change.operation {
                ChangeOperation::Create => fs::renameat_with(
                    &change.parent,
                    &first_name,
                    &change.parent,
                    &second_name,
                    RenameFlags::NOREPLACE,
                )
                .map_or_else(
                    |error| {
                        if error == Errno::EXIST {
                            Ok(())
                        } else {
                            Err(unsupported_semantics(&change.path))
                        }
                    },
                    |()| Err(unsupported_semantics(&change.path)),
                ),
                ChangeOperation::Edit => fs::renameat_with(
                    &change.parent,
                    &first_name,
                    &change.parent,
                    &second_name,
                    RenameFlags::EXCHANGE,
                )
                .map(|()| exchanged = true)
                .map_err(|_| unsupported_semantics(&change.path)),
            };
            let (first_expected, second_expected) = if exchanged {
                (second, first)
            } else {
                (first, second)
            };
            let first_cleanup =
                remove_named_if_identity(&change.parent, &first_name, first_expected);
            let second_cleanup =
                remove_named_if_identity(&change.parent, &second_name, second_expected);
            if first_cleanup.is_err() || second_cleanup.is_err() {
                return Err(MutationError::at_path(
                    MutationErrorKind::RecoveryRequired,
                    &change.path,
                    "filesystem safety probes could not be cleaned; inspect the application",
                ));
            }
            check?;
        }
        Ok(())
    }

    fn create_probe(
        parent: &OwnedFd,
        name: &OsStr,
        path: &ChangePath,
    ) -> Result<Identity, MutationError> {
        let fd = fs::openat(parent, name, PRIVATE_FILE, PRIVATE_MODE)
            .map_err(|_| unsupported_semantics(path))?;
        let file = File::from(fd);
        let identity = file_identity(&file).map_err(|_| {
            MutationError::at_path(
                MutationErrorKind::RecoveryRequired,
                path,
                "filesystem safety probe cannot be inspected; manual cleanup is required",
            )
        })?;
        if file.sync_all().is_err() {
            drop(file);
            return if remove_named_if_identity(parent, name, identity).is_ok() {
                Err(unsupported_semantics(path))
            } else {
                Err(MutationError::at_path(
                    MutationErrorKind::RecoveryRequired,
                    path,
                    "filesystem safety probe cleanup requires manual inspection",
                ))
            };
        }
        Ok(identity)
    }

    fn unsupported_semantics(path: &ChangePath) -> MutationError {
        MutationError::at_path(
            MutationErrorKind::UnsupportedPlatform,
            path,
            "the target filesystem does not provide required safe mutation semantics",
        )
    }

    fn publish_one(change: &PreparedChange) -> Result<(), MutationError> {
        let flags = match change.operation {
            ChangeOperation::Create => RenameFlags::NOREPLACE,
            ChangeOperation::Edit => RenameFlags::EXCHANGE,
        };
        fs::renameat_with(
            &change.parent,
            &change.temp_name,
            &change.parent,
            &change.name,
            flags,
        )
        .map_err(|error| {
            if error == Errno::EXIST {
                precondition_failed(&change.path)
            } else {
                MutationError::at_path(
                    MutationErrorKind::PublicationFailed,
                    &change.path,
                    "atomic mutation publication failed",
                )
            }
        })
    }

    fn verify_published(change: &PreparedChange) -> Result<(), MutationError> {
        let result = read_regular_file(&change.parent, &change.name, &change.path)?;
        if result.identity != change.staged_identity || result.digest != change.result_digest {
            return Err(MutationError::at_path(
                MutationErrorKind::PublicationFailed,
                &change.path,
                "published mutation result could not be verified",
            ));
        }
        if let Some(original) = &change.original {
            let backup = read_regular_file(&change.parent, &change.temp_name, &change.path)?;
            if backup.identity != original.identity || backup.digest != original.digest {
                return Err(MutationError::at_path(
                    MutationErrorKind::PublicationFailed,
                    &change.path,
                    "mutation rollback source could not be verified",
                ));
            }
        }
        Ok(())
    }

    fn rollback_or_report(
        root: &Root,
        marker: &Marker,
        prepared: &mut [PreparedChange],
        original_error: MutationError,
    ) -> Result<MutationReceipt, MutationError> {
        let mut complete = true;
        for change in prepared.iter_mut().rev().filter(|change| change.published) {
            if rollback_one(change).is_err() {
                complete = false;
            }
        }
        if complete {
            for change in prepared.iter() {
                if remove_temp_if_present(change).is_err() {
                    complete = false;
                }
            }
        }
        if complete && sync_unique_parents(prepared).is_err() {
            complete = false;
        }
        if complete
            && original_error.kind() != MutationErrorKind::RecoveryRequired
            && remove_marker(root, marker).is_err()
        {
            complete = false;
        }
        if complete {
            let _ = sync_directory(&root.fd);
            Err(original_error)
        } else {
            Err(MutationError::new(
                MutationErrorKind::RollbackIncomplete,
                "mutation rollback could not be completed; the recovery marker and remaining transaction files require manual inspection",
            ))
        }
    }

    fn rollback_one(change: &mut PreparedChange) -> Result<(), MutationError> {
        let result =
            read_regular_file(&change.parent, &change.name, &change.path).map_err(|_| {
                MutationError::at_path(
                    MutationErrorKind::RollbackIncomplete,
                    &change.path,
                    "cannot verify a published file before rollback",
                )
            })?;
        if result.identity != change.staged_identity || result.digest != change.result_digest {
            return Err(MutationError::at_path(
                MutationErrorKind::RollbackIncomplete,
                &change.path,
                "a published file changed before rollback; it was not overwritten",
            ));
        }
        match change.operation {
            ChangeOperation::Create => {
                fs::unlinkat(&change.parent, &change.name, AtFlags::empty()).map_err(|_| {
                    MutationError::at_path(
                        MutationErrorKind::RollbackIncomplete,
                        &change.path,
                        "cannot remove a transaction-owned created file during rollback",
                    )
                })?;
            }
            ChangeOperation::Edit => {
                let original = change.original.as_ref().expect("edits have original state");
                let backup = read_regular_file(&change.parent, &change.temp_name, &change.path)
                    .map_err(|_| {
                        MutationError::at_path(
                            MutationErrorKind::RollbackIncomplete,
                            &change.path,
                            "cannot verify the original file before rollback",
                        )
                    })?;
                if backup.identity != original.identity || backup.digest != original.digest {
                    return Err(MutationError::at_path(
                        MutationErrorKind::RollbackIncomplete,
                        &change.path,
                        "the original rollback file changed; it was not published",
                    ));
                }
                fs::renameat_with(
                    &change.parent,
                    &change.temp_name,
                    &change.parent,
                    &change.name,
                    RenameFlags::EXCHANGE,
                )
                .map_err(|_| {
                    MutationError::at_path(
                        MutationErrorKind::RollbackIncomplete,
                        &change.path,
                        "cannot atomically restore an edited file",
                    )
                })?;
                let restored = read_regular_file(&change.parent, &change.name, &change.path)?;
                if restored.identity != original.identity || restored.digest != original.digest {
                    return Err(MutationError::at_path(
                        MutationErrorKind::RollbackIncomplete,
                        &change.path,
                        "restored file identity could not be verified",
                    ));
                }
                remove_staged_temp(change)?;
            }
        }
        change.published = false;
        sync_directory(&change.parent)?;
        Ok(())
    }

    fn cleanup_before_publication(
        root: &Root,
        marker: &Marker,
        prepared: &mut [PreparedChange],
        error: MutationError,
    ) -> Result<MutationReceipt, MutationError> {
        let mut complete = true;
        for change in prepared.iter() {
            if remove_temp_if_present(change).is_err() {
                complete = false;
            }
        }
        if complete && sync_unique_parents(prepared).is_err() {
            complete = false;
        }
        if complete
            && error.kind() != MutationErrorKind::RecoveryRequired
            && remove_marker(root, marker).is_err()
        {
            complete = false;
        }
        if complete {
            Err(error)
        } else {
            Err(MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "mutation preparation failed and transaction cleanup requires manual inspection",
            ))
        }
    }

    fn verify_marker(root: &Root, marker: &Marker) -> Result<(), MutationError> {
        let observed = read_identity(&root.fd, OsStr::new(MUTATION_MARKER)).map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "mutation marker changed during publication; manual recovery is required",
            )
        })?;
        if observed != marker.identity {
            return Err(MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "mutation marker changed during publication; manual recovery is required",
            ));
        }
        Ok(())
    }

    fn remove_marker(root: &Root, marker: &Marker) -> Result<(), MutationError> {
        verify_marker(root, marker)?;
        fs::unlinkat(&root.fd, MUTATION_MARKER, AtFlags::empty()).map_err(|_| {
            MutationError::new(
                MutationErrorKind::RecoveryRequired,
                "cannot remove the mutation marker",
            )
        })
    }

    fn read_identity(parent: &OwnedFd, name: &OsStr) -> Result<Identity, ()> {
        let fd = fs::openat(parent, name, READ_FILE, Mode::empty()).map_err(|_| ())?;
        let file = File::from(fd);
        file_identity(&file).map_err(|_| ())
    }

    fn remove_staged_temp(change: &PreparedChange) -> Result<(), MutationError> {
        remove_named_if_identity(&change.parent, &change.temp_name, change.staged_identity).map_err(
            |_| {
                MutationError::at_path(
                    MutationErrorKind::RecoveryRequired,
                    &change.path,
                    "cannot safely remove a transaction-owned temporary file",
                )
            },
        )
    }

    fn remove_backup(change: &PreparedChange) -> Result<(), MutationError> {
        let original = change.original.as_ref().expect("only edits have backups");
        remove_named_if_identity(&change.parent, &change.temp_name, original.identity).map_err(
            |_| {
                MutationError::at_path(
                    MutationErrorKind::RecoveryRequired,
                    &change.path,
                    "cannot safely remove a transaction-owned rollback file",
                )
            },
        )
    }

    fn remove_temp_if_present(change: &PreparedChange) -> Result<(), MutationError> {
        match fs::statat(&change.parent, &change.temp_name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => Ok(()),
            Ok(_) => remove_staged_temp(change),
            Err(_) => Err(MutationError::at_path(
                MutationErrorKind::RecoveryRequired,
                &change.path,
                "cannot inspect a transaction-owned temporary file",
            )),
        }
    }

    fn remove_named_if_identity(
        parent: &OwnedFd,
        name: &OsStr,
        expected: Identity,
    ) -> Result<(), ()> {
        if read_identity(parent, name)? != expected {
            return Err(());
        }
        fs::unlinkat(parent, name, AtFlags::empty()).map_err(|_| ())
    }

    fn sync_unique_parents(prepared: &[PreparedChange]) -> Result<(), MutationError> {
        let mut seen = BTreeSet::new();
        for change in prepared {
            let identity = directory_identity(&change.parent)?;
            if seen.insert(identity) {
                sync_directory(&change.parent)?;
            }
        }
        Ok(())
    }

    fn sync_directory(fd: &OwnedFd) -> Result<(), MutationError> {
        let duplicate = fs::openat(fd, ".", DIRECTORY, Mode::empty()).map_err(|_| {
            MutationError::new(
                MutationErrorKind::PublicationFailed,
                "cannot duplicate an application directory handle",
            )
        })?;
        File::from(duplicate).sync_all().map_err(|_| {
            MutationError::new(
                MutationErrorKind::PublicationFailed,
                "cannot make application directory changes durable",
            )
        })
    }

    #[cfg(test)]
    mod tests {
        use std::{
            fs as stdfs,
            os::unix::fs::symlink,
            sync::atomic::{AtomicU64, Ordering},
        };

        use super::*;
        use crate::{FileCreation, StructuredFileEdit};

        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

        struct Fixture {
            root: PathBuf,
        }

        impl Fixture {
            fn new(name: &str) -> Self {
                let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
                let root = std::env::temp_dir().join(format!(
                    "hegira-mutation-test-{}-{id}-{name}",
                    std::process::id()
                ));
                stdfs::create_dir(&root).unwrap();
                stdfs::create_dir_all(root.join("crates/domain/src")).unwrap();
                Self { root }
            }

            fn plan(&self) -> ChangePlan {
                let original = b"pub mod existing;\n";
                stdfs::write(self.root.join("crates/domain/src/lib.rs"), original).unwrap();
                ChangePlan::new([
                    StructuredFileEdit::new(
                        "crates/domain/src/lib.rs",
                        original,
                        b"pub mod existing;\npub mod order;\n".to_vec(),
                    )
                    .unwrap()
                    .into(),
                    FileCreation::new(
                        "crates/domain/src/order.rs",
                        b"pub struct Order;\n".to_vec(),
                    )
                    .unwrap()
                    .into(),
                ])
                .unwrap()
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = stdfs::remove_dir_all(&self.root);
            }
        }

        fn injected() -> MutationError {
            MutationError::new(MutationErrorKind::PublicationFailed, "injected failure")
        }

        #[test]
        fn publishes_edit_and_create_and_removes_transaction_state() {
            let fixture = Fixture::new("success");
            let plan = fixture.plan();
            let receipt = publish_change_plan(&fixture.root, &plan).unwrap();

            assert_eq!(receipt.changed_files(), 2);
            assert_eq!(
                stdfs::read_to_string(fixture.root.join("crates/domain/src/lib.rs")).unwrap(),
                "pub mod existing;\npub mod order;\n"
            );
            assert_eq!(
                stdfs::read_to_string(fixture.root.join("crates/domain/src/order.rs")).unwrap(),
                "pub struct Order;\n"
            );
            assert!(!fixture.root.join(MUTATION_MARKER).exists());
            assert_eq!(
                stdfs::read_dir(fixture.root.join("crates/domain/src"))
                    .unwrap()
                    .count(),
                2
            );
        }

        #[test]
        fn concurrent_change_invalidates_the_plan_before_publication() {
            let fixture = Fixture::new("concurrent");
            let plan = fixture.plan();
            let target = fixture.root.join("crates/domain/src/lib.rs");
            let error = publish_with(&fixture.root, &plan, |point| {
                if point == HookPoint::BeforePublication {
                    stdfs::write(&target, "user change\n").unwrap();
                }
                Ok(())
            })
            .unwrap_err();

            assert_eq!(error.kind(), MutationErrorKind::PreconditionFailed);
            assert_eq!(stdfs::read_to_string(target).unwrap(), "user change\n");
            assert!(!fixture.root.join(MUTATION_MARKER).exists());
            assert!(!fixture.root.join("crates/domain/src/order.rs").exists());
        }

        #[test]
        fn failure_after_a_published_edit_restores_the_complete_fixture() {
            let fixture = Fixture::new("rollback");
            let plan = fixture.plan();
            let error = publish_with(&fixture.root, &plan, |point| {
                if point == HookPoint::AfterChange(0) {
                    return Err(injected());
                }
                Ok(())
            })
            .unwrap_err();

            assert_eq!(error.kind(), MutationErrorKind::PublicationFailed);
            assert_eq!(
                stdfs::read_to_string(fixture.root.join("crates/domain/src/lib.rs")).unwrap(),
                "pub mod existing;\n"
            );
            assert!(!fixture.root.join("crates/domain/src/order.rs").exists());
            assert!(!fixture.root.join(MUTATION_MARKER).exists());
        }

        #[test]
        fn failure_after_all_changes_restores_edits_and_removes_created_files() {
            let fixture = Fixture::new("complete-rollback");
            let plan = fixture.plan();
            let error = publish_with(&fixture.root, &plan, |point| {
                if point == HookPoint::AfterChange(1) {
                    return Err(injected());
                }
                Ok(())
            })
            .unwrap_err();

            assert_eq!(error.kind(), MutationErrorKind::PublicationFailed);
            assert_eq!(
                stdfs::read_to_string(fixture.root.join("crates/domain/src/lib.rs")).unwrap(),
                "pub mod existing;\n"
            );
            assert!(!fixture.root.join("crates/domain/src/order.rs").exists());
            assert!(!fixture.root.join(MUTATION_MARKER).exists());
        }

        #[test]
        fn changed_published_file_is_never_overwritten_during_rollback() {
            let fixture = Fixture::new("incomplete-rollback");
            let plan = fixture.plan();
            let target = fixture.root.join("crates/domain/src/lib.rs");
            let error = publish_with(&fixture.root, &plan, |point| {
                if point == HookPoint::AfterChange(0) {
                    stdfs::write(&target, "concurrent user change\n").unwrap();
                    return Err(injected());
                }
                Ok(())
            })
            .unwrap_err();

            assert_eq!(error.kind(), MutationErrorKind::RollbackIncomplete);
            assert_eq!(
                stdfs::read_to_string(target).unwrap(),
                "concurrent user change\n"
            );
            assert!(fixture.root.join(MUTATION_MARKER).is_file());
        }

        #[test]
        fn destination_race_and_symlink_attempts_fail_closed() {
            let fixture = Fixture::new("destination-race");
            let plan = fixture.plan();
            let destination = fixture.root.join("crates/domain/src/order.rs");
            let error = publish_with(&fixture.root, &plan, |point| {
                if point == HookPoint::BeforeChange(0) {
                    stdfs::write(&destination, "user-owned\n").unwrap();
                }
                Ok(())
            })
            .unwrap_err();
            assert_eq!(error.kind(), MutationErrorKind::PreconditionFailed);
            assert_eq!(stdfs::read_to_string(destination).unwrap(), "user-owned\n");

            let outside = fixture.root.with_extension("outside");
            stdfs::write(&outside, "outside\n").unwrap();
            let target = fixture.root.join("crates/domain/src/lib.rs");
            stdfs::remove_file(&target).unwrap();
            symlink(&outside, &target).unwrap();
            let symlink_plan = ChangePlan::new([StructuredFileEdit::new(
                "crates/domain/src/lib.rs",
                b"pub mod existing;\n",
                b"changed\n".to_vec(),
            )
            .unwrap()
            .into()])
            .unwrap();
            assert_eq!(
                publish_change_plan(&fixture.root, &symlink_plan)
                    .unwrap_err()
                    .kind(),
                MutationErrorKind::PreconditionFailed
            );
            assert_eq!(stdfs::read_to_string(&outside).unwrap(), "outside\n");
            stdfs::remove_file(outside).unwrap();
        }

        #[test]
        fn replaced_ancestor_never_redirects_publication_or_cleanup() {
            let fixture = Fixture::new("ancestor");
            let plan = fixture.plan();
            let moved = fixture.root.with_extension("moved");
            let outside = fixture.root.with_extension("replacement");
            stdfs::create_dir(&outside).unwrap();
            stdfs::write(outside.join("sentinel"), "preserved").unwrap();
            let error = publish_with(&fixture.root, &plan, |point| {
                if point == HookPoint::BeforePublication {
                    stdfs::rename(&fixture.root, &moved).unwrap();
                    symlink(&outside, &fixture.root).unwrap();
                }
                Ok(())
            })
            .unwrap_err();

            assert_eq!(error.kind(), MutationErrorKind::UnsafeRoot);
            assert_eq!(
                stdfs::read_to_string(outside.join("sentinel")).unwrap(),
                "preserved"
            );
            assert_eq!(
                stdfs::read_to_string(moved.join("crates/domain/src/lib.rs")).unwrap(),
                "pub mod existing;\n"
            );
            stdfs::remove_file(&fixture.root).unwrap();
            stdfs::remove_dir_all(outside).unwrap();
            stdfs::rename(moved, &fixture.root).unwrap();
        }

        #[test]
        fn existing_marker_blocks_mutation_and_remains_detectable() {
            let fixture = Fixture::new("marker");
            let plan = fixture.plan();
            stdfs::write(fixture.root.join(MUTATION_MARKER), "incomplete\n").unwrap();

            let error = publish_change_plan(&fixture.root, &plan).unwrap_err();
            assert_eq!(error.kind(), MutationErrorKind::RecoveryRequired);
            assert_eq!(
                stdfs::read_to_string(fixture.root.join(MUTATION_MARKER)).unwrap(),
                "incomplete\n"
            );
            assert_eq!(
                stdfs::read_to_string(fixture.root.join("crates/domain/src/lib.rs")).unwrap(),
                "pub mod existing;\n"
            );
        }

        #[test]
        fn invalid_roots_fail_before_creating_a_marker() {
            let fixture = Fixture::new("invalid-root");
            let plan = fixture.plan();
            let alias = fixture.root.with_extension("alias");
            symlink(&fixture.root, &alias).unwrap();
            let error = publish_change_plan(&alias, &plan).unwrap_err();
            assert_eq!(error.kind(), MutationErrorKind::UnsafeRoot);
            assert!(!fixture.root.join(MUTATION_MARKER).exists());
            stdfs::remove_file(alias).unwrap();
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
mod platform {
    use super::*;

    pub(super) fn publish(_: &Path, _: &ChangePlan) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::new(
            MutationErrorKind::UnsupportedPlatform,
            "safe existing-application mutation is supported only on Linux and Apple platforms",
        ))
    }
}
