use std::path::{Component, Path, PathBuf};

use crate::{RendererError, RendererErrorKind, Result};

fn safety(message: impl Into<String>) -> RendererError {
    RendererError::with_kind(RendererErrorKind::Safety, message)
}

fn conflict() -> RendererError {
    RendererError::with_kind(
        RendererErrorKind::Conflict,
        "destination already exists; choose a new directory (overwrite is not supported)",
    )
}

pub(crate) fn checked_path(path: &Path) -> Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| safety("destination must be valid UTF-8"))?;
    if text.is_empty() || text.chars().any(char::is_control) || text.contains('\\') {
        return Err(safety(
            "destination must be non-empty and contain no controls or backslashes",
        ));
    }
    if matches!(
        text.rsplit('/').find(|part| !part.is_empty()),
        Some("." | "..")
    ) {
        return Err(safety(
            "destination must end in a new directory name, not dot or dot-dot",
        ));
    }
    let mut normal_seen = false;
    for component in path.components() {
        match component {
            Component::ParentDir if normal_seen || path.is_absolute() => {
                return Err(safety(
                    "destination may not traverse back through a named directory",
                ));
            }
            Component::Normal(_) => normal_seen = true,
            _ => {}
        }
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| safety("destination must name a new application directory"))?;
    if name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || application_manifest::reserved_project_name(name)
    {
        return Err(safety(
            "destination directory must use 1–64 ASCII letters, digits, hyphens or underscores and must not be reserved",
        ));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| safety("cannot resolve current directory"))?
            .join(path)
    };
    let mut resolved = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    Ok(resolved)
}

/// Validate without creating directories. Existing parents are required.
pub fn validate_destination(path: &Path) -> Result<()> {
    let path = checked_path(path)?;
    platform::Parent::open(&path)?.ensure_absent(&path)
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
pub(crate) mod platform {
    use super::*;
    use crate::render::RenderPlan;
    use rustix::{
        fd::OwnedFd,
        fs::{self, AtFlags, Mode, OFlags, RenameFlags},
        io::Errno,
    };
    use std::{
        collections::BTreeSet,
        fs::File,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    pub(super) struct Parent {
        fd: OwnedFd,
    }

    impl Parent {
        pub(super) fn open(path: &Path) -> Result<Self> {
            let parent = path
                .parent()
                .ok_or_else(|| safety("destination has no parent"))?;
            let mut fd = fs::open("/", DIRECTORY, Mode::empty())
                .map_err(|_| safety("cannot open filesystem root"))?;
            for component in parent.components() {
                let name = match component {
                    Component::RootDir | Component::CurDir => continue,
                    Component::Normal(name) => name,
                    Component::ParentDir => std::ffi::OsStr::new(".."),
                    _ => return Err(safety("unsupported destination prefix")),
                };
                fd = fs::openat(&fd, name, DIRECTORY, Mode::empty())
                    .map_err(|_| safety("destination parent must already exist and contain no symlinks; choose or create a real parent directory"))?;
            }
            Ok(Self { fd })
        }

        pub(super) fn ensure_absent(&self, path: &Path) -> Result<()> {
            match fs::statat(
                &self.fd,
                path.file_name().unwrap(),
                AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Err(Errno::NOENT) => Ok(()),
                Ok(_) => Err(conflict()),
                Err(_) => Err(safety(
                    "cannot inspect destination; check parent permissions",
                )),
            }
        }
    }

    fn same_directory(a: &OwnedFd, b: &OwnedFd) -> Result<bool> {
        let a = fs::fstat(a).map_err(|_| safety("cannot inspect open directory"))?;
        let b = fs::fstat(b).map_err(|_| safety("cannot inspect open directory"))?;
        Ok(a.st_dev == b.st_dev && a.st_ino == b.st_ino)
    }

    struct Staging<'a> {
        parent: &'a Parent,
        name: String,
        fd: OwnedFd,
        files: Vec<PathBuf>,
        directories: BTreeSet<PathBuf>,
        published: bool,
    }

    impl Drop for Staging<'_> {
        fn drop(&mut self) {
            if self.published {
                return;
            }
            // Resolve cleanup from the open staging descriptor, never a replaced parent path.
            for file in &self.files {
                if let Some(parent) = file.parent()
                    && let Ok(fd) = descend(&self.fd, parent)
                {
                    let _ = fs::unlinkat(fd, file.file_name().unwrap(), AtFlags::empty());
                }
            }
            for directory in self.directories.iter().rev() {
                if let Ok(fd) = descend(&self.fd, directory.parent().unwrap()) {
                    let _ = fs::unlinkat(fd, directory.file_name().unwrap(), AtFlags::REMOVEDIR);
                }
            }
            if let Ok(fd) = fs::openat(&self.parent.fd, &self.name, DIRECTORY, Mode::empty())
                && same_directory(&fd, &self.fd).unwrap_or(false)
            {
                let _ = fs::unlinkat(&self.parent.fd, &self.name, AtFlags::REMOVEDIR);
            }
        }
    }

    fn descend(root: &OwnedFd, path: &Path) -> Result<OwnedFd> {
        let mut fd = fs::openat(root, ".", DIRECTORY, Mode::empty())
            .map_err(|_| safety("cannot open staging directory"))?;
        for component in path.components() {
            let Component::Normal(name) = component else {
                return Err(safety(
                    "rendered component paths must be relative without traversal",
                ));
            };
            fd = fs::openat(fd, name, DIRECTORY, Mode::empty())
                .map_err(|_| safety("staging directory changed during rendering"))?;
        }
        Ok(fd)
    }

    pub(crate) fn publish(output: &Path, plan: &RenderPlan) -> Result<()> {
        publish_with(output, plan, || Ok(()), || Ok(()))
    }

    fn publish_with(
        output: &Path,
        plan: &RenderPlan,
        before_publish: impl FnOnce() -> Result<()>,
        before_rename: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let output = checked_path(output)?;
        let parent = Parent::open(&output)?;
        parent.ensure_absent(&output)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| safety("cannot read system clock"))?
            .as_nanos();
        let name = format!(".hegira-render-{}-{nonce}", std::process::id());
        fs::mkdirat(&parent.fd, &name, Mode::RWXU)
            .map_err(|_| safety("cannot allocate private staging directory"))?;
        let fd = fs::openat(&parent.fd, &name, DIRECTORY, Mode::empty())
            .map_err(|_| safety("cannot open private staging directory"))?;
        let mut staging = Staging {
            parent: &parent,
            name,
            fd,
            files: vec![],
            directories: BTreeSet::new(),
            published: false,
        };
        for (path, file) in &plan.files {
            if path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
                || path.as_os_str().is_empty()
            {
                return Err(safety(
                    "rendered component paths must be relative without traversal",
                ));
            }
            let mut directory = PathBuf::new();
            for part in path.parent().unwrap().components() {
                let fd = descend(&staging.fd, &directory)?;
                directory.push(part);
                if !staging.directories.contains(&directory) {
                    fs::mkdirat(fd, part.as_os_str(), Mode::RWXU)
                        .map_err(|_| safety("cannot create private rendered directory"))?;
                    staging.directories.insert(directory.clone());
                }
            }
            let fd = descend(&staging.fd, path.parent().unwrap())?;
            let file_fd = fs::openat(
                fd,
                path.file_name().unwrap(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(|_| safety("cannot exclusively create rendered file"))?;
            staging.files.push(path.clone());
            File::from(file_fd).write_all(&file.bytes).map_err(|_| {
                safety("failed to write rendered file; check disk space and permissions")
            })?;
        }
        before_publish()?;
        let current_parent = Parent::open(&output)?;
        if !same_directory(&parent.fd, &current_parent.fd)? {
            return Err(safety(
                "destination parent changed during rendering; retry with a stable directory",
            ));
        }
        parent.ensure_absent(&output)?;
        let current_staging = fs::openat(&parent.fd, &staging.name, DIRECTORY, Mode::empty())
            .map_err(|_| safety("staging directory changed before publication"))?;
        if !same_directory(&staging.fd, &current_staging)? {
            return Err(safety("staging directory changed before publication"));
        }
        before_rename()?;
        fs::renameat_with(&parent.fd, &staging.name, &parent.fd, output.file_name().unwrap(), RenameFlags::NOREPLACE)
            .map_err(|error| if error == Errno::EXIST { conflict() } else {
                safety("atomic no-overwrite publication failed; the filesystem must support exclusive rename")
            })?;
        staging.published = true;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::render::PlannedFile;
        use std::{collections::BTreeMap, fs as stdfs, os::unix::fs::symlink};

        fn fixture() -> (PathBuf, RenderPlan) {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("hegira-safe-{}-{nonce}", std::process::id()));
            stdfs::create_dir(&root).unwrap();
            let plan = RenderPlan {
                package: None,
                components: vec![],
                files: BTreeMap::from([(
                    PathBuf::from("nested/file.txt"),
                    PlannedFile {
                        bytes: b"generated".to_vec(),
                        owner: "test".into(),
                    },
                )]),
            };
            (root, plan)
        }

        #[test]
        fn destination_created_after_final_check_is_never_replaced() {
            let (root, plan) = fixture();
            let output = root.join("application");
            let error = publish_with(
                &output,
                &plan,
                || Ok(()),
                || {
                    stdfs::create_dir(&output).unwrap();
                    Ok(())
                },
            )
            .unwrap_err();
            assert_eq!(error.kind(), RendererErrorKind::Conflict);
            assert!(output.is_dir());
            assert_eq!(stdfs::read_dir(&root).unwrap().count(), 1);
            assert_eq!(stdfs::read_dir(&output).unwrap().count(), 0);
            stdfs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn replaced_parent_cannot_redirect_publication_or_cleanup() {
            let (root, plan) = fixture();
            let parent = root.join("parent");
            let moved = root.join("moved");
            let outside = root.join("outside");
            stdfs::create_dir(&parent).unwrap();
            stdfs::create_dir(&outside).unwrap();
            stdfs::write(outside.join("sentinel"), "preserved").unwrap();
            let error = publish_with(
                &parent.join("application"),
                &plan,
                || {
                    stdfs::rename(&parent, &moved).unwrap();
                    symlink(&outside, &parent).unwrap();
                    Ok(())
                },
                || Ok(()),
            )
            .unwrap_err();
            assert_eq!(error.kind(), RendererErrorKind::Safety);
            assert_eq!(stdfs::read_dir(&outside).unwrap().count(), 1);
            assert_eq!(
                stdfs::read_to_string(outside.join("sentinel")).unwrap(),
                "preserved"
            );
            assert_eq!(stdfs::read_dir(&moved).unwrap().count(), 0);
            stdfs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn failure_after_writes_removes_only_our_staging() {
            let (root, plan) = fixture();
            let output = root.join("application");
            stdfs::write(root.join("sentinel"), "preserved").unwrap();
            assert!(
                publish_with(
                    &output,
                    &plan,
                    || Err(safety("injected failure")),
                    || Ok(())
                )
                .is_err()
            );
            assert!(!output.exists());
            assert_eq!(stdfs::read_dir(&root).unwrap().count(), 1);
            assert_eq!(
                stdfs::read_to_string(root.join("sentinel")).unwrap(),
                "preserved"
            );
            stdfs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn failure_while_writing_tree_cleans_partial_output() {
            let (root, mut plan) = fixture();
            plan.files.insert(
                PathBuf::from("nested/file.txt/child"),
                PlannedFile {
                    bytes: vec![],
                    owner: "test".into(),
                },
            );
            assert!(publish(&root.join("application"), &plan).is_err());
            assert_eq!(stdfs::read_dir(&root).unwrap().count(), 0);
            stdfs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn rejects_symlink_parents_and_dangling_destinations() {
            let (root, _) = fixture();
            symlink(root.join("missing"), root.join("dangling")).unwrap();
            symlink(&root, root.join("alias")).unwrap();
            assert_eq!(
                validate_destination(&root.join("dangling"))
                    .unwrap_err()
                    .kind(),
                RendererErrorKind::Conflict
            );
            assert_eq!(
                validate_destination(&root.join("alias/application"))
                    .unwrap_err()
                    .kind(),
                RendererErrorKind::Safety
            );
            assert!(
                stdfs::symlink_metadata(root.join("dangling"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            stdfs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
pub(crate) mod platform {
    use super::*;
    pub(super) struct Parent;
    impl Parent {
        pub(super) fn open(_: &Path) -> Result<Self> {
            Err(safety(
                "safe application publication is supported on Linux and Apple platforms",
            ))
        }
        pub(super) fn ensure_absent(&self, _: &Path) -> Result<()> {
            unreachable!()
        }
    }
    pub(crate) fn publish(_: &Path, _: &crate::render::RenderPlan) -> Result<()> {
        Err(safety(
            "safe application publication is supported on Linux and Apple platforms",
        ))
    }
}
