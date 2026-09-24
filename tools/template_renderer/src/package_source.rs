use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{RendererError, RendererErrorKind, Result};

const MAX_PACKAGE_FILES: usize = 10_000;
const MAX_PACKAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;

fn safety(message: impl Into<String>) -> RendererError {
    RendererError::with_kind(RendererErrorKind::Safety, message)
}

/// Immutable, directory-anchored snapshot of a bundled component package.
///
/// Callers parse manifests, verify declarations, and consume source from the
/// same bytes. Package paths are intentionally not exposed in diagnostics.
#[derive(Debug)]
pub(crate) struct PackageSource {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl PackageSource {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        platform::snapshot(root)
    }

    pub(crate) fn file(&self, path: &Path) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    pub(crate) fn files(&self) -> impl ExactSizeIterator<Item = (&Path, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_path(), bytes.as_slice()))
    }

    pub(crate) fn files_below<'a>(
        &'a self,
        root: &Path,
    ) -> impl Iterator<Item = (&'a Path, &'a [u8])> + 'a {
        let root = root.to_path_buf();
        self.files()
            .filter(move |(path, _)| path.starts_with(&root))
    }
}

fn insert_file(
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    total_bytes: &mut u64,
    path: PathBuf,
    bytes: Vec<u8>,
) -> Result<()> {
    if files.len() >= MAX_PACKAGE_FILES {
        return Err(safety(
            "component package exceeds the file-count safety limit",
        ));
    }
    *total_bytes = total_bytes
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| safety("component package exceeds the byte-size safety limit"))?;
    if *total_bytes > MAX_PACKAGE_BYTES {
        return Err(safety(
            "component package exceeds the byte-size safety limit",
        ));
    }
    if files.insert(path, bytes).is_some() {
        return Err(safety("component package contains a duplicate source path"));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod platform {
    use super::*;
    use rustix::{
        fd::OwnedFd,
        fs::{self, Dir, FileType, Mode, OFlags},
    };
    use std::{
        ffi::{OsStr, OsString},
        io::Read,
        os::unix::ffi::{OsStrExt, OsStringExt},
    };

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const ENTRY: OFlags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC)
        .union(OFlags::NONBLOCK);

    pub(super) fn snapshot(root: &Path) -> Result<PackageSource> {
        snapshot_with(root, || Ok(()))
    }

    fn snapshot_with(
        root: &Path,
        after_open: impl FnOnce() -> Result<()>,
    ) -> Result<PackageSource> {
        let root = absolute_lexical(root)?;
        let fd = open_directory(&root)?;
        let identity =
            fs::fstat(&fd).map_err(|_| safety("cannot inspect the open component package root"))?;
        after_open()?;
        let mut files = BTreeMap::new();
        let mut total_bytes = 0;
        walk(&fd, Path::new(""), &mut files, &mut total_bytes)?;
        let current = open_directory(&root).map_err(|_| {
            safety("component package root changed during verification; retry with a stable source")
        })?;
        let current =
            fs::fstat(&current).map_err(|_| safety("cannot recheck the component package root"))?;
        if identity.st_dev != current.st_dev || identity.st_ino != current.st_ino {
            return Err(safety(
                "component package root changed during verification; retry with a stable source",
            ));
        }
        Ok(PackageSource { files })
    }

    fn absolute_lexical(path: &Path) -> Result<PathBuf> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| safety("cannot resolve the component package root"))?
                .join(path)
        };
        let mut resolved = PathBuf::new();
        for component in absolute.components() {
            match component {
                std::path::Component::RootDir => resolved.push(Path::new("/")),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    resolved.pop();
                }
                std::path::Component::Normal(part) => resolved.push(part),
                std::path::Component::Prefix(_) => {
                    return Err(safety("unsupported component package path prefix"));
                }
            }
        }
        Ok(resolved)
    }

    fn open_directory(path: &Path) -> Result<OwnedFd> {
        let mut fd = fs::open("/", DIRECTORY, Mode::empty())
            .map_err(|_| safety("cannot open the filesystem root"))?;
        for component in path.components() {
            let std::path::Component::Normal(part) = component else {
                continue;
            };
            fd = fs::openat(&fd, part, DIRECTORY, Mode::empty()).map_err(|_| {
                safety(
                    "component package root must be an accessible real directory without symlinks",
                )
            })?;
        }
        Ok(fd)
    }

    fn walk(
        directory: &OwnedFd,
        relative: &Path,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
        total_bytes: &mut u64,
    ) -> Result<()> {
        let entries = Dir::read_from(directory)
            .map_err(|_| safety("cannot enumerate the component package"))?
            .map(|entry| {
                let entry = entry.map_err(|_| safety("cannot enumerate the component package"))?;
                let name = OsString::from_vec(entry.file_name().to_bytes().to_vec());
                if name.as_os_str() == OsStr::from_bytes(b".")
                    || name.as_os_str() == OsStr::from_bytes(b"..")
                {
                    return Ok(None);
                }
                Ok(Some(name))
            })
            .filter_map(|entry: Result<Option<OsString>>| match entry {
                Ok(Some(name)) => Some(Ok(name)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut entries = entries;
        entries.sort();

        for name in entries {
            let fd = fs::openat(directory, &name, ENTRY, Mode::empty())
                .map_err(|_| safety("component package entries must not be symbolic links"))?;
            let metadata =
                fs::fstat(&fd).map_err(|_| safety("cannot inspect a component package entry"))?;
            let file_type = FileType::from_raw_mode(metadata.st_mode);
            let path = relative.join(&name);
            if file_type.is_dir() {
                walk(&fd, &path, files, total_bytes)?;
            } else if file_type.is_file() {
                if metadata.st_size < 0 || metadata.st_size as u64 > MAX_PACKAGE_FILE_BYTES {
                    return Err(safety(
                        "component package entry exceeds the file-size safety limit",
                    ));
                }
                let mut bytes = Vec::with_capacity(metadata.st_size as usize);
                std::fs::File::from(fd)
                    .take(MAX_PACKAGE_FILE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| safety("cannot read a component package entry"))?;
                if bytes.len() as u64 > MAX_PACKAGE_FILE_BYTES {
                    return Err(safety(
                        "component package entry exceeds the file-size safety limit",
                    ));
                }
                insert_file(files, total_bytes, path, bytes)?;
            } else {
                return Err(safety(
                    "component package entries must be regular files or directories",
                ));
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{
            fs as stdfs,
            os::unix::fs::symlink,
            sync::atomic::{AtomicU64, Ordering},
        };

        static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

        #[test]
        fn package_root_replacement_is_rejected_after_anchored_reads() {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let fixture = std::env::temp_dir().join(format!(
                "hegira-package-source-race-{}-{sequence}",
                std::process::id()
            ));
            let package = fixture.join("package");
            let moved = fixture.join("moved-package");
            let outside = fixture.join("outside");
            stdfs::create_dir_all(&package).unwrap();
            stdfs::create_dir_all(&outside).unwrap();
            stdfs::write(package.join("safe.txt"), "safe").unwrap();
            stdfs::write(outside.join("secret.txt"), "secret").unwrap();

            let error = snapshot_with(&package, || {
                stdfs::rename(&package, &moved).unwrap();
                symlink(&outside, &package).unwrap();
                Ok(())
            })
            .expect_err("replaced package root should fail");

            assert!(error.to_string().contains("changed during verification"));
            assert!(!error.to_string().contains("secret"));
            let _ = stdfs::remove_file(&package);
            let _ = stdfs::remove_dir_all(&fixture);
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
mod platform {
    use super::*;

    pub(super) fn snapshot(_: &Path) -> Result<PackageSource> {
        Err(safety(
            "safe component package access is supported on Linux and Apple platforms",
        ))
    }
}
