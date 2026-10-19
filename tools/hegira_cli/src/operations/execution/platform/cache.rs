//! Explicitly owned validation storage. All mutation is under the application's
//! existing exclusive operation lock; read-only maintenance never creates state.

use super::*;
use crate::artifacts::{CacheAction, CacheEntry, CacheReport};
use std::{
    collections::BTreeMap,
    ffi::CString,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const ROOT: &str = "target/hegira/validation";
pub(super) const MARKER: &str = ".hegira-cache.json";
const ACCESS: &str = ".last-used";
const KINDS: [&str; 3] = ["build", "check", "test"];
const LIMIT: usize = 200_000;

fn invalid() -> OperationError {
    failure(
        OperationErrorKind::Conflict,
        "validation-cache-ownership",
        "Validation storage is unclaimed, unsafe, changed or exceeds scan limits (200000 entries/64 levels). Inspect target/hegira/validation manually; never forge claims or remove recovery state.",
    )
}

fn budget_error() -> OperationError {
    failure(
        OperationErrorKind::Validation,
        "validation-cache-budget-exceeded",
        "Owned validation storage exceeds its budget and no inactive eligible cache can meet it. Finish active work, review artifacts prune --dry-run, or explicitly raise --cache-budget-mib. Developer output was not reclaimed.",
    )
}

pub(super) fn cache_marker(plan: &OperationPlan, kind: &str) -> Result<Vec<u8>, OperationError> {
    serde_json::to_vec(&serde_json::json!({
        "output_schema": 1,
        "owner": "hegira-validation-cache",
        "application": plan.summary.application,
        "cache": kind,
    }))
    .map_err(|_| invalid())
}

fn write_new(directory: &Directory, name: &str, bytes: &[u8]) -> Result<(), OperationError> {
    directory.verify().map_err(|_| invalid())?;
    let file = fs::openat(
        &directory.fd,
        name,
        fs::OFlags::WRONLY
            | fs::OFlags::CREATE
            | fs::OFlags::EXCL
            | fs::OFlags::NOFOLLOW
            | fs::OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|_| invalid())?;
    let mut file = std::fs::File::from(file);
    file.write_all(bytes).map_err(|_| invalid())?;
    file.sync_all().map_err(|_| invalid())
}

fn claim(directory: &Directory, expected: &[u8]) -> Result<(), OperationError> {
    directory.verify().map_err(|_| invalid())?;
    let file = regular(directory, &CString::new(MARKER).unwrap())?;
    if fs::fstat(&file).map_err(|_| invalid())?.st_nlink != 1
        || read_file(&directory.fd, Path::new(MARKER), 4096).map_err(|_| invalid())? != expected
    {
        return Err(invalid());
    }
    Ok(())
}

fn regular(directory: &Directory, name: &std::ffi::CStr) -> Result<OwnedFd, OperationError> {
    let fd = fs::openat2(
        &directory.fd,
        name,
        fs::OFlags::RDONLY | fs::OFlags::NOFOLLOW | fs::OFlags::NONBLOCK | fs::OFlags::CLOEXEC,
        Mode::empty(),
        fs::ResolveFlags::BENEATH | fs::ResolveFlags::NO_SYMLINKS | fs::ResolveFlags::NO_XDEV,
    )
    .map_err(|_| invalid())?;
    if !fs::FileType::from_raw_mode(fs::fstat(&fd).map_err(|_| invalid())?.st_mode).is_file() {
        return Err(invalid());
    }
    Ok(fd)
}

fn names(directory: &Directory, remaining: &mut usize) -> Result<Vec<CString>, OperationError> {
    directory.verify().map_err(|_| invalid())?;
    let mut names = Vec::new();
    for entry in fs::Dir::read_from(&directory.fd).map_err(|_| invalid())? {
        let entry = entry.map_err(|_| invalid())?;
        if matches!(entry.file_name().to_bytes(), b"." | b"..") {
            continue;
        }
        *remaining = remaining.checked_sub(1).ok_or_else(invalid)?;
        names.push(entry.file_name().to_owned());
    }
    names.sort();
    Ok(names)
}

fn stat(directory: &Directory, name: &CString) -> Result<fs::Stat, OperationError> {
    fs::statat(&directory.fd, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| invalid())
}

fn stable(left: &fs::Stat, right: &fs::Stat) -> bool {
    left.st_dev == right.st_dev
        && left.st_ino == right.st_ino
        && left.st_mode == right.st_mode
        && left.st_nlink == right.st_nlink
        && left.st_size == right.st_size
        && left.st_blocks == right.st_blocks
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
}

struct FileEntry {
    parent: Directory,
    name: CString,
    observed: fs::Stat,
}
struct Tree {
    files: Vec<FileEntry>,
    directories: Vec<(Directory, CString, Directory)>,
    allocated: u64,
    logical: u64,
    unique_files: u64,
}

impl Tree {
    fn scan(root: &Directory, device: u64, remaining: &mut usize) -> Result<Self, OperationError> {
        let mut tree = Self {
            files: Vec::new(),
            directories: Vec::new(),
            allocated: 0,
            logical: 0,
            unique_files: 0,
        };
        let mut links = BTreeMap::new();
        tree.walk(root, device, remaining, &mut links, 0)?;
        if links
            .values()
            .any(|&(observed, expected)| observed != expected)
        {
            return Err(invalid());
        }
        tree.unique_files = links.len() as u64;
        Ok(tree)
    }

    fn walk(
        &mut self,
        dir: &Directory,
        device: u64,
        remaining: &mut usize,
        links: &mut BTreeMap<(u64, u64), (u64, u64)>,
        depth: usize,
    ) -> Result<(), OperationError> {
        if depth > 64 {
            return Err(invalid());
        }
        let before = fs::fstat(&dir.fd).map_err(|_| invalid())?;
        self.add_allocated(&before)?;
        for name in names(dir, remaining)? {
            let observed = stat(dir, &name)?;
            if observed.st_dev != device {
                return Err(invalid());
            }
            match fs::FileType::from_raw_mode(observed.st_mode) {
                fs::FileType::Directory => {
                    let child = dir.open_child(&name).map_err(|_| invalid())?;
                    if !stable(&observed, &fs::fstat(&child.fd).map_err(|_| invalid())?) {
                        return Err(invalid());
                    }
                    self.walk(&child, device, remaining, links, depth + 1)?;
                    self.directories.push((dir.clone(), name, child));
                }
                fs::FileType::RegularFile => {
                    // A same-device bind-mounted file also crosses ownership.
                    let opened = regular(dir, &name)?;
                    if !stable(&observed, &fs::fstat(&opened).map_err(|_| invalid())?) {
                        return Err(invalid());
                    }
                    let record = links
                        .entry((observed.st_dev, observed.st_ino))
                        .or_insert((0, observed.st_nlink));
                    if record.1 != observed.st_nlink {
                        return Err(invalid());
                    }
                    if record.0 == 0 {
                        self.add_allocated(&observed)?;
                        self.logical = self
                            .logical
                            .checked_add(u64::try_from(observed.st_size).map_err(|_| invalid())?)
                            .ok_or_else(invalid)?;
                    }
                    record.0 += 1;
                    self.files.push(FileEntry {
                        parent: dir.clone(),
                        name,
                        observed,
                    });
                }
                _ => return Err(invalid()),
            }
        }
        dir.verify().map_err(|_| invalid())?;
        if !stable(&before, &fs::fstat(&dir.fd).map_err(|_| invalid())?) {
            return Err(invalid());
        }
        Ok(())
    }

    fn add_allocated(&mut self, stat: &fs::Stat) -> Result<(), OperationError> {
        let bytes = u64::try_from(stat.st_blocks)
            .ok()
            .and_then(|n| n.checked_mul(512))
            .ok_or_else(invalid)?;
        self.allocated = self.allocated.checked_add(bytes).ok_or_else(invalid)?;
        Ok(())
    }

    fn verify(&self) -> Result<(), OperationError> {
        for file in &self.files {
            file.parent.verify().map_err(|_| invalid())?;
            if !stable(&file.observed, &stat(&file.parent, &file.name)?) {
                return Err(invalid());
            }
        }
        for (_, _, child) in &self.directories {
            child.verify().map_err(|_| invalid())?;
        }
        Ok(())
    }

    fn remove(
        &self,
        plan: &OperationPlan,
        storage: &Directory,
        root: &Directory,
        kind: &str,
    ) -> Result<(), OperationError> {
        plan.anchor.verify()?;
        claim(storage, &cache_marker(plan, "root")?)?;
        claim(root, &cache_marker(plan, kind)?)?;
        self.verify()?;
        // Keep the ownership marker until all other files/directories are gone.
        for file in self
            .files
            .iter()
            .filter(|file| file.parent != *root || file.name.to_bytes() != MARKER.as_bytes())
        {
            plan.anchor.verify()?;
            storage.verify().map_err(|_| invalid())?;
            root.verify().map_err(|_| invalid())?;
            file.parent.verify().map_err(|_| invalid())?;
            let now = stat(&file.parent, &file.name)?;
            // Unlinking an internal hard link changes ctime/nlink for its aliases.
            if now.st_dev != file.observed.st_dev
                || now.st_ino != file.observed.st_ino
                || now.st_mode != file.observed.st_mode
                || now.st_size != file.observed.st_size
            {
                return Err(invalid());
            }
            fs::unlinkat(&file.parent.fd, &file.name, AtFlags::empty()).map_err(|_| invalid())?;
        }
        for (parent, name, child) in &self.directories {
            root.verify().map_err(|_| invalid())?;
            parent.verify().map_err(|_| invalid())?;
            child.verify().map_err(|_| invalid())?;
            fs::unlinkat(&parent.fd, name, AtFlags::REMOVEDIR).map_err(|_| invalid())?;
        }
        claim(root, &cache_marker(plan, kind)?)?;
        fs::unlinkat(&root.fd, MARKER, AtFlags::empty()).map_err(|_| invalid())?;
        storage.verify().map_err(|_| invalid())?;
        root.verify().map_err(|_| invalid())?;
        fs::unlinkat(&storage.fd, kind, AtFlags::REMOVEDIR).map_err(|_| invalid())
    }
}

struct Cache {
    kind: &'static str,
    directory: Directory,
    used: u64,
    tree: Tree,
    active: bool,
    _lease: Option<OwnedFd>,
}
struct Storage {
    root: Directory,
    caches: Vec<Cache>,
    allocated: u64,
    logical: u64,
}

impl Storage {
    fn inspect(plan: &OperationPlan) -> Result<Option<Self>, OperationError> {
        plan.anchor.verify()?;
        let mut dir = plan.anchor.directory.clone();
        for name in ["target", "hegira", "validation"] {
            match fs::statat(&dir.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Err(rustix::io::Errno::NOENT) => return Ok(None),
                Ok(stat)
                    if fs::FileType::from_raw_mode(stat.st_mode).is_dir()
                        && stat.st_dev
                            == fs::fstat(&plan.anchor.directory.fd)
                                .map_err(|_| invalid())?
                                .st_dev =>
                {
                    dir = dir
                        .open_child(&CString::new(name).unwrap())
                        .map_err(|_| invalid())?;
                    if identity(&dir.fd)? != (stat.st_dev, stat.st_ino) {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
        }
        claim(&dir, &cache_marker(plan, "root")?)?;
        let device = fs::fstat(&dir.fd).map_err(|_| invalid())?.st_dev;
        let mut remaining = LIMIT;
        let mut caches = Vec::new();
        // Root overhead is measured as well; no unknown children can be adopted.
        let mut allocated = u64::try_from(fs::fstat(&dir.fd).map_err(|_| invalid())?.st_blocks)
            .ok()
            .and_then(|n| n.checked_mul(512))
            .ok_or_else(invalid)?;
        let marker_stat =
            fs::statat(&dir.fd, MARKER, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| invalid())?;
        let mut logical = u64::try_from(marker_stat.st_size).map_err(|_| invalid())?;
        allocated = allocated
            .checked_add(
                u64::try_from(marker_stat.st_blocks)
                    .map_err(|_| invalid())?
                    .checked_mul(512)
                    .ok_or_else(invalid)?,
            )
            .ok_or_else(invalid)?;
        for name in names(&dir, &mut remaining)? {
            if name.to_bytes() == MARKER.as_bytes() {
                continue;
            }
            let kind = KINDS
                .into_iter()
                .find(|kind| kind.as_bytes() == name.to_bytes())
                .ok_or_else(invalid)?;
            let observed = stat(&dir, &name)?;
            let directory = dir.open_child(&name).map_err(|_| invalid())?;
            if observed.st_dev != device
                || identity(&directory.fd)? != (observed.st_dev, observed.st_ino)
            {
                return Err(invalid());
            }
            claim(&directory, &cache_marker(plan, kind)?)?;
            let lease =
                fs::openat(&directory.fd, ".", DIRECTORY, Mode::empty()).map_err(|_| invalid())?;
            let active = match fs::flock(&lease, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => false,
                Err(rustix::io::Errno::WOULDBLOCK) => true,
                Err(_) => return Err(invalid()),
            };
            let bytes = read_file(&directory.fd, Path::new(ACCESS), 32).map_err(|_| invalid())?;
            let used = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(invalid)?;
            let tree = Tree::scan(&directory, device, &mut remaining)?;
            allocated = allocated.checked_add(tree.allocated).ok_or_else(invalid)?;
            logical = logical.checked_add(tree.logical).ok_or_else(invalid)?;
            caches.push(Cache {
                kind,
                directory,
                used,
                tree,
                active,
                _lease: (!active).then_some(lease),
            });
        }
        claim(&dir, &cache_marker(plan, "root")?)?;
        plan.anchor.verify()?;
        Ok(Some(Self {
            root: dir,
            caches,
            allocated,
            logical,
        }))
    }

    fn reclaim(
        &self,
        plan: &OperationPlan,
        budget: u64,
        active: Option<&str>,
        execute: bool,
    ) -> Result<(Vec<&'static str>, u64), OperationError> {
        let mut candidates: Vec<_> = self
            .caches
            .iter()
            .filter(|cache| Some(cache.kind) != active && !cache.active)
            .collect();
        candidates.sort_by_key(|cache| (cache.used, cache.kind));
        let mut total = self.allocated;
        let mut selected = Vec::new();
        for cache in candidates {
            if total <= budget {
                break;
            }
            selected.push(cache.kind);
            total = total
                .checked_sub(cache.tree.allocated)
                .ok_or_else(invalid)?;
        }
        // No mutation until every observed tree/claim and candidate has passed.
        for cache in &self.caches {
            claim(&cache.directory, &cache_marker(plan, cache.kind)?)?;
            cache.tree.verify()?;
        }
        if execute {
            for kind in &selected {
                let cache = self
                    .caches
                    .iter()
                    .find(|cache| cache.kind == *kind)
                    .unwrap();
                cache
                    .tree
                    .remove(plan, &self.root, &cache.directory, kind)?;
            }
        }
        Ok((selected, total))
    }
}

fn create(parent: &Directory, name: &str) -> Result<(Directory, bool), OperationError> {
    parent.verify().map_err(|_| invalid())?;
    let fresh = match fs::mkdirat(&parent.fd, name, Mode::from_raw_mode(0o700)) {
        Ok(()) => true,
        Err(rustix::io::Errno::EXIST) => false,
        Err(_) => return Err(invalid()),
    };
    let directory = parent
        .open_child(&CString::new(name).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    if fs::fstat(&directory.fd).map_err(|_| invalid())?.st_dev
        != fs::fstat(&parent.fd).map_err(|_| invalid())?.st_dev
    {
        return Err(invalid());
    }
    Ok((directory, fresh))
}

fn touch(directory: &Directory) -> Result<(), OperationError> {
    directory.verify().map_err(|_| invalid())?;
    let fd = open_file(&directory.fd, Path::new(ACCESS)).map_err(|_| invalid())?;
    if fs::fstat(&fd).map_err(|_| invalid())?.st_nlink != 1 {
        return Err(invalid());
    }
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid())?
        .as_secs()
        .to_string();
    // Replace rather than truncate: even a concurrently introduced hard link
    // cannot redirect a write into existing external data.
    const NEXT_ACCESS: &str = ".hegira-access.next";
    write_new(directory, NEXT_ACCESS, value.as_bytes())?;
    directory.verify().map_err(|_| invalid())?;
    let current = open_file(&directory.fd, Path::new(ACCESS)).map_err(|_| invalid())?;
    if identity(&current)? != identity(&fd)? {
        return Err(invalid());
    }
    fs::renameat(&directory.fd, NEXT_ACCESS, &directory.fd, ACCESS).map_err(|_| invalid())?;
    fs::fsync(&directory.fd).map_err(|_| invalid())
}

pub(super) struct Session {
    kind: &'static str,
    root: Directory,
    directory: Directory,
    lease: Arc<OwnedFd>,
}

impl Session {
    pub(super) fn prepare(plan: &OperationPlan) -> Result<Self, OperationError> {
        let budget = plan
            .summary
            .validation_cache
            .ok_or_else(invalid)?
            .budget_bytes;
        let kind = match plan.summary.intent {
            OperationIntent::Check => "check",
            OperationIntent::Test => "test",
            OperationIntent::ReleaseBuild => "build",
            _ => return Err(invalid()),
        };
        if let Some(storage) = Storage::inspect(plan)? {
            let (_, total) = storage.reclaim(plan, budget, Some(kind), true)?;
            if total > budget {
                return Err(budget_error());
            }
        }
        let (target, _) = create(&plan.anchor.directory, "target")?;
        let (hegira, _) = create(&target, "hegira")?;
        let (root, fresh) = create(&hegira, "validation")?;
        if fresh {
            write_new(&root, MARKER, &cache_marker(plan, "root")?)?;
        }
        claim(&root, &cache_marker(plan, "root")?)?;
        let (directory, fresh) = create(&root, kind)?;
        if fresh {
            write_new(&directory, MARKER, &cache_marker(plan, kind)?)?;
            write_new(&directory, ACCESS, b"0")?;
        }
        claim(&directory, &cache_marker(plan, kind)?)?;
        // Keep CLOEXEC in the parent so unrelated concurrent spawns cannot
        // retain the lease. configure clears it only in the approved child.
        let lease =
            fs::openat(&directory.fd, ".", DIRECTORY, Mode::empty()).map_err(|_| invalid())?;
        fs::flock(&lease, FlockOperation::NonBlockingLockExclusive).map_err(|_| failure(OperationErrorKind::Conflict, "validation-cache-active", "The selected validation cache is still active, possibly in a surviving child. Finish that work before retrying."))?;
        touch(&directory)?;
        let session = Self {
            kind,
            root,
            directory,
            lease: Arc::new(lease),
        };
        session.verify(plan)?;
        session.enforce(plan)?;
        Ok(session)
    }

    pub(super) fn verify(&self, plan: &OperationPlan) -> Result<(), OperationError> {
        self.root.verify().map_err(|_| invalid())?;
        claim(&self.root, &cache_marker(plan, "root")?)?;
        self.directory.verify().map_err(|_| invalid())?;
        let storage = Storage::inspect(plan)?.ok_or_else(invalid)?;
        if storage
            .caches
            .iter()
            .any(|cache| cache.kind == self.kind && cache.directory == self.directory)
        {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    pub(super) fn configure(&self, command: &mut Command) {
        let lease = Arc::clone(&self.lease);
        // SAFETY: the descriptor stays alive through the captured Arc. After
        // fork this closure only borrows it and performs async-signal-safe
        // fcntl, returning an OS error without allocation, locks or destruction.
        // Clearing CLOEXEC in the child cannot change the parent's fd flags.
        unsafe {
            command.pre_exec(move || {
                rustix::io::fcntl_setfd(&*lease, rustix::io::FdFlags::empty())
                    .map_err(std::io::Error::from)
            });
        }
    }

    fn enforce(&self, plan: &OperationPlan) -> Result<(), OperationError> {
        self.root.verify().map_err(|_| invalid())?;
        let storage = Storage::inspect(plan)?.ok_or_else(invalid)?;
        let budget = plan
            .summary
            .validation_cache
            .ok_or_else(invalid)?
            .budget_bytes;
        storage.reclaim(plan, budget, Some(self.kind), true)?;
        let after = Storage::inspect(plan)?.ok_or_else(invalid)?;
        if after.allocated > budget {
            return Err(budget_error());
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, plan: &OperationPlan) -> Result<(), OperationError> {
        self.verify(plan)?;
        touch(&self.directory)?;
        self.enforce(plan)
    }
}

pub(crate) fn maintain(
    plan: &OperationPlan,
    budget: ValidationCache,
    action: CacheAction,
) -> Result<CacheReport, OperationError> {
    plan.anchor.verify()?;
    let lock = fs::openat(&plan.anchor.directory.fd, ".", DIRECTORY, Mode::empty())
        .map_err(|_| invalid())?;
    let activity = match fs::flock(
        &lock,
        if matches!(action, CacheAction::Prune) {
            FlockOperation::NonBlockingLockExclusive
        } else {
            FlockOperation::NonBlockingLockShared
        },
    ) {
        Ok(()) => "idle",
        Err(rustix::io::Errno::WOULDBLOCK) if !matches!(action, CacheAction::Prune) => "active",
        Err(rustix::io::Errno::WOULDBLOCK) => {
            return Err(failure(
                OperationErrorKind::Conflict,
                "execution-active",
                "Another application operation is active; no cache was removed.",
            ));
        }
        Err(_) => return Err(invalid()),
    };
    if matches!(action, CacheAction::Prune) {
        ensure_no_recovery(plan)?;
    }
    let storage = Storage::inspect(plan)?;
    let mut report = CacheReport {
        output_schema: 1,
        mode: action.name(),
        application: plan.summary.application.clone(),
        path: ROOT,
        operation_activity: activity,
        budget_bytes: budget.budget_bytes,
        allocated_bytes: storage.as_ref().map_or(0, |s| s.allocated),
        logical_bytes: storage.as_ref().map_or(0, |s| s.logical),
        remaining_bytes: 0,
        budget_satisfied: true,
        caches: Vec::new(),
        selected: Vec::new(),
        guidance: "Only claimed inactive validation caches are eligible. Developer output, ordinary release bundles, global Cargo/Git/toolchain caches, frontend dependencies and Docker are excluded. Explicit execution reauthenticates; previews grant no deletion authority.",
    };
    if let Some(storage) = storage {
        for cache in &storage.caches {
            report.caches.push(CacheEntry {
                cache: cache.kind,
                logical_bytes: cache.tree.logical,
                allocated_bytes: cache.tree.allocated,
                unique_files: cache.tree.unique_files,
                last_used: cache.used,
                active: activity != "idle" || cache.active,
                cleanup_eligible: activity == "idle" && !cache.active,
            });
        }
        if !matches!(action, CacheAction::Status) && activity == "idle" {
            let (selected, total) = storage.reclaim(
                plan,
                budget.budget_bytes,
                None,
                matches!(action, CacheAction::Prune),
            )?;
            report.selected = selected;
            report.remaining_bytes = if matches!(action, CacheAction::Prune) {
                Storage::inspect(plan)?.ok_or_else(invalid)?.allocated
            } else {
                total
            };
        } else {
            report.remaining_bytes = report.allocated_bytes;
        }
    }
    report.budget_satisfied = report.remaining_bytes <= budget.budget_bytes;
    plan.anchor.verify()?;
    Ok(report)
}
