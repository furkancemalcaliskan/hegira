use std::{
    fmt::{Display, Formatter},
    path::{Component, Path, PathBuf},
};

use application_manifest::{
    ApplicationManifest, MutationCompatibility, MutationCompatibilityPolicy,
    assess_mutation_compatibility,
};

const MANIFEST_NAME: &str = "hegira.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationContextRequest {
    pub working_directory: PathBuf,
    pub application_root: Option<PathBuf>,
}

impl ApplicationContextRequest {
    pub fn discover_from(working_directory: impl Into<PathBuf>) -> Self {
        Self {
            working_directory: working_directory.into(),
            application_root: None,
        }
    }

    pub fn explicit(
        working_directory: impl Into<PathBuf>,
        application_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            working_directory: working_directory.into(),
            application_root: Some(application_root.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationPaths {
    pub manifest: PathBuf,
    pub apps: PathBuf,
    pub crates: PathBuf,
    pub config: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationContext {
    pub root: PathBuf,
    pub paths: ApplicationPaths,
    pub manifest: Option<ApplicationManifest>,
    pub compatibility: MutationCompatibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationContextErrorKind {
    Validation,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationContextError {
    kind: ApplicationContextErrorKind,
    message: String,
}

impl ApplicationContextError {
    pub fn kind(&self) -> ApplicationContextErrorKind {
        self.kind
    }

    fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: ApplicationContextErrorKind::Validation,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: ApplicationContextErrorKind::Conflict,
            message: message.into(),
        }
    }
}

impl Display for ApplicationContextError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApplicationContextError {}

pub fn resolve_application_context(
    request: &ApplicationContextRequest,
    policy: &MutationCompatibilityPolicy,
) -> Result<ApplicationContext, ApplicationContextError> {
    platform::resolve(request, policy)
}

fn absolute_path(
    working_directory: &Path,
    candidate: &Path,
) -> Result<PathBuf, ApplicationContextError> {
    if working_directory.to_str().is_none() || candidate.to_str().is_none() {
        return Err(ApplicationContextError::validation(
            "application paths must be valid UTF-8",
        ));
    }
    let combined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if working_directory.is_absolute() {
        working_directory.join(candidate)
    } else {
        return Err(ApplicationContextError::validation(
            "working directory must be an absolute path",
        ));
    };
    let mut normalized = PathBuf::new();
    for component in combined.components() {
        match component {
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(_) => {
                return Err(ApplicationContextError::validation(
                    "unsupported application path prefix",
                ));
            }
        }
    }
    if !normalized.is_absolute() {
        return Err(ApplicationContextError::validation(
            "application path could not be resolved absolutely",
        ));
    }
    Ok(normalized)
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod platform {
    use super::*;
    use rustix::{
        fd::OwnedFd,
        fs::{self, Mode, OFlags},
    };
    use std::io::Read;

    const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const FILE: OFlags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

    struct OpenDirectory {
        path: PathBuf,
        fd: OwnedFd,
    }

    pub(super) fn resolve(
        request: &ApplicationContextRequest,
        policy: &MutationCompatibilityPolicy,
    ) -> Result<ApplicationContext, ApplicationContextError> {
        let working_directory = absolute_path(&request.working_directory, Path::new("."))?;
        let root = match &request.application_root {
            Some(root) => {
                let root = absolute_path(&working_directory, root)?;
                let directory = open_directory(&root, "application root")?;
                if !has_manifest(&directory)? {
                    return Err(ApplicationContextError::validation(format!(
                        "explicit application root does not contain {MANIFEST_NAME}"
                    )));
                }
                directory
            }
            None => discover_root(open_directory(&working_directory, "working directory")?)?,
        };

        let manifest_source = read_manifest(&root)?;
        let compatibility =
            assess_mutation_compatibility(&manifest_source, policy).map_err(|error| {
                ApplicationContextError::validation(format!(
                    "cannot validate {MANIFEST_NAME}: {error}"
                ))
            })?;
        let manifest = ApplicationManifest::from_toml(&manifest_source).ok();
        validate_owned_roots(&root)?;
        verify_directory_identity(&root)?;

        Ok(ApplicationContext {
            paths: ApplicationPaths {
                manifest: root.path.join(MANIFEST_NAME),
                apps: root.path.join("apps"),
                crates: root.path.join("crates"),
                config: root.path.join("config"),
            },
            root: root.path,
            manifest,
            compatibility,
        })
    }

    fn discover_root(mut current: OpenDirectory) -> Result<OpenDirectory, ApplicationContextError> {
        let mut found: Option<OpenDirectory> = None;
        loop {
            if has_manifest(&current)? {
                if found.is_some() {
                    return Err(ApplicationContextError::conflict(format!(
                        "multiple {MANIFEST_NAME} files make the application root ambiguous; pass an explicit application root"
                    )));
                }
                found = Some(OpenDirectory {
                    path: current.path.clone(),
                    fd: fs::openat(&current.fd, ".", DIRECTORY, Mode::empty()).map_err(|_| {
                        ApplicationContextError::validation(
                            "cannot retain the discovered application root",
                        )
                    })?,
                });
            }
            if current.path.parent().is_none() {
                break;
            }
            current.fd = fs::openat(&current.fd, "..", DIRECTORY, Mode::empty()).map_err(|_| {
                ApplicationContextError::validation(
                    "cannot inspect an ancestor while discovering the application root",
                )
            })?;
            current.path.pop();
        }
        found.ok_or_else(|| {
            ApplicationContextError::validation(format!(
                "no {MANIFEST_NAME} was found in the working directory or its ancestors"
            ))
        })
    }

    fn open_directory(path: &Path, kind: &str) -> Result<OpenDirectory, ApplicationContextError> {
        let fd = open_directory_fd(path, kind)?;
        let canonical = std::fs::canonicalize(path).map_err(|_| {
            ApplicationContextError::validation(format!("cannot canonicalize {kind}"))
        })?;
        let canonical_fd = open_directory_fd(&canonical, kind)?;
        if !same_directory(&fd, &canonical_fd)? {
            return Err(ApplicationContextError::conflict(format!(
                "{kind} changed during resolution; retry with a stable directory"
            )));
        }
        Ok(OpenDirectory {
            path: canonical,
            fd: canonical_fd,
        })
    }

    fn open_directory_fd(path: &Path, kind: &str) -> Result<OwnedFd, ApplicationContextError> {
        let mut fd = fs::open("/", DIRECTORY, Mode::empty())
            .map_err(|_| ApplicationContextError::validation("cannot open the filesystem root"))?;
        for component in path.components() {
            let Component::Normal(part) = component else {
                continue;
            };
            fd = fs::openat(&fd, part, DIRECTORY, Mode::empty()).map_err(|_| {
                ApplicationContextError::validation(format!(
                    "cannot open {kind}; every path component must be an accessible real directory"
                ))
            })?;
        }
        Ok(fd)
    }

    fn same_directory(first: &OwnedFd, second: &OwnedFd) -> Result<bool, ApplicationContextError> {
        let first = fs::fstat(first).map_err(|_| {
            ApplicationContextError::validation("cannot inspect an open application directory")
        })?;
        let second = fs::fstat(second).map_err(|_| {
            ApplicationContextError::validation("cannot inspect an open application directory")
        })?;
        Ok(first.st_dev == second.st_dev && first.st_ino == second.st_ino)
    }

    fn has_manifest(directory: &OpenDirectory) -> Result<bool, ApplicationContextError> {
        match fs::openat(&directory.fd, MANIFEST_NAME, FILE, Mode::empty()) {
            Ok(_) => Ok(true),
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(_) => Err(ApplicationContextError::validation(format!(
                "cannot inspect {MANIFEST_NAME} while resolving the application root"
            ))),
        }
    }

    fn read_manifest(directory: &OpenDirectory) -> Result<String, ApplicationContextError> {
        let fd = fs::openat(&directory.fd, MANIFEST_NAME, FILE, Mode::empty()).map_err(|_| {
            ApplicationContextError::validation(format!(
                "cannot open {MANIFEST_NAME} as an accessible real file"
            ))
        })?;
        let metadata = fs::fstat(&fd).map_err(|_| {
            ApplicationContextError::validation(format!("cannot inspect {MANIFEST_NAME}"))
        })?;
        if !rustix::fs::FileType::from_raw_mode(metadata.st_mode).is_file() {
            return Err(ApplicationContextError::validation(format!(
                "{MANIFEST_NAME} must be a regular file"
            )));
        }
        if metadata.st_size < 0 || metadata.st_size as u64 > MAX_MANIFEST_BYTES {
            return Err(ApplicationContextError::validation(format!(
                "{MANIFEST_NAME} exceeds the 1 MiB safety limit"
            )));
        }
        let mut source = String::new();
        std::fs::File::from(fd)
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_string(&mut source)
            .map_err(|_| {
                ApplicationContextError::validation(format!(
                    "cannot read {MANIFEST_NAME} as UTF-8 text"
                ))
            })?;
        if source.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(ApplicationContextError::validation(format!(
                "{MANIFEST_NAME} exceeds the 1 MiB safety limit"
            )));
        }
        Ok(source)
    }

    fn validate_owned_roots(root: &OpenDirectory) -> Result<(), ApplicationContextError> {
        for name in ["apps", "crates", "config"] {
            fs::openat(&root.fd, name, DIRECTORY, Mode::empty()).map_err(|_| {
                ApplicationContextError::validation(format!(
                    "expected application-owned root `{name}` must be an accessible real directory inside the application root"
                ))
            })?;
        }
        Ok(())
    }

    fn verify_directory_identity(root: &OpenDirectory) -> Result<(), ApplicationContextError> {
        let current = open_directory(&root.path, "resolved application root")?;
        let original = fs::fstat(&root.fd).map_err(|_| {
            ApplicationContextError::validation("cannot inspect the discovered application root")
        })?;
        let current = fs::fstat(&current.fd).map_err(|_| {
            ApplicationContextError::validation("cannot recheck the discovered application root")
        })?;
        if original.st_dev != current.st_dev || original.st_ino != current.st_ino {
            return Err(ApplicationContextError::conflict(
                "application root changed during resolution; retry with a stable directory",
            ));
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
mod platform {
    use super::*;

    pub(super) fn resolve(
        _: &ApplicationContextRequest,
        _: &MutationCompatibilityPolicy,
    ) -> Result<ApplicationContext, ApplicationContextError> {
        Err(ApplicationContextError::validation(
            "safe existing-application resolution is supported on Linux and Apple platforms",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn manifest(version: &str) -> String {
        format!(
            r#"schema = 1
application = "context-app"

[framework]
repository = "https://github.com/furkancemalcaliskan/hegira.git"
version = "{version}"

[selection]
components = ["layered-base", "layered-leptos-identity"]
databases = ["sqlite"]
clients = ["leptos"]
"#
        )
    }

    fn application(name: &str, version: &str) -> TestDirectory {
        let fixture = TestDirectory::new(name);
        for directory in ["apps", "crates", "config", "apps/web/src"] {
            fs::create_dir_all(fixture.path.join(directory)).unwrap();
        }
        fs::write(fixture.path.join(MANIFEST_NAME), manifest(version)).unwrap();
        fixture
    }

    fn policy(version: &str) -> MutationCompatibilityPolicy {
        MutationCompatibilityPolicy::for_framework_version(version).unwrap()
    }

    #[test]
    fn discovered_and_explicit_roots_produce_the_same_context() {
        let fixture = application("equivalent", "v0.5.0");
        let nested = fixture.path.join("apps/web/src");

        let discovered = resolve_application_context(
            &ApplicationContextRequest::discover_from(&nested),
            &policy("v0.5.0"),
        )
        .unwrap();
        let explicit = resolve_application_context(
            &ApplicationContextRequest::explicit(&nested, &fixture.path),
            &policy("v0.5.0"),
        )
        .unwrap();

        assert_eq!(discovered, explicit);
        assert_eq!(discovered.root, fs::canonicalize(&fixture.path).unwrap());
        assert_eq!(
            discovered.paths.manifest,
            discovered.root.join(MANIFEST_NAME)
        );
        assert_eq!(discovered.compatibility, MutationCompatibility::Compatible);
        assert_eq!(
            discovered.manifest.as_ref().unwrap().application,
            "context-app"
        );
    }

    #[test]
    fn relative_explicit_root_is_resolved_from_the_supplied_working_directory() {
        let fixture = application("relative", "v0.5.0");
        let working_directory = fixture.path.join("apps/web");
        let context = resolve_application_context(
            &ApplicationContextRequest::explicit(&working_directory, "../.."),
            &policy("v0.5.0"),
        )
        .unwrap();
        assert_eq!(context.root, fs::canonicalize(&fixture.path).unwrap());
    }

    #[test]
    fn unsupported_release_is_loaded_as_typed_compatibility() {
        let fixture = application("unsupported", "v0.4.0");
        let context = resolve_application_context(
            &ApplicationContextRequest::discover_from(&fixture.path),
            &policy("v0.5.0"),
        )
        .unwrap();
        assert!(context.manifest.is_some());
        assert!(matches!(
            context.compatibility,
            MutationCompatibility::Unsupported(_)
        ));
    }

    #[test]
    fn missing_and_ambiguous_roots_fail_predictably() {
        let missing = TestDirectory::new("missing");
        let error = resolve_application_context(
            &ApplicationContextRequest::discover_from(&missing.path),
            &policy("v0.5.0"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ApplicationContextErrorKind::Validation);
        assert!(error.to_string().contains("no hegira.toml"));

        let outer = application("ambiguous", "v0.5.0");
        let inner = outer.path.join("apps/nested");
        for directory in ["apps", "crates", "config", "work"] {
            fs::create_dir_all(inner.join(directory)).unwrap();
        }
        fs::write(inner.join(MANIFEST_NAME), manifest("v0.5.0")).unwrap();
        let error = resolve_application_context(
            &ApplicationContextRequest::discover_from(inner.join("work")),
            &policy("v0.5.0"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ApplicationContextErrorKind::Conflict);
        assert!(error.to_string().contains("ambiguous"));
    }

    #[test]
    fn missing_owned_root_is_rejected() {
        let fixture = application("missing-owned-root", "v0.5.0");
        fs::remove_dir(fixture.path.join("config")).unwrap();
        let error = resolve_application_context(
            &ApplicationContextRequest::discover_from(&fixture.path),
            &policy("v0.5.0"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ApplicationContextErrorKind::Validation);
        assert!(error.to_string().contains("`config`"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_roots_manifests_and_owned_roots_are_rejected() {
        use std::os::unix::fs::symlink;

        let fixture = application("symlinks", "v0.5.0");
        let alias = fixture.path.with_extension("alias");
        symlink(&fixture.path, &alias).unwrap();
        let error = resolve_application_context(
            &ApplicationContextRequest::explicit(&fixture.path, &alias),
            &policy("v0.5.0"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), ApplicationContextErrorKind::Validation);
        fs::remove_file(alias).unwrap();

        let source = fixture.path.join(MANIFEST_NAME);
        let moved = fixture.path.join("manifest.real");
        fs::rename(&source, &moved).unwrap();
        symlink(&moved, &source).unwrap();
        assert!(
            resolve_application_context(
                &ApplicationContextRequest::discover_from(&fixture.path),
                &policy("v0.5.0")
            )
            .is_err()
        );
        fs::remove_file(&source).unwrap();
        fs::rename(&moved, &source).unwrap();

        let apps = fixture.path.join("apps");
        let real_apps = fixture.path.join("apps.real");
        fs::rename(&apps, &real_apps).unwrap();
        symlink(&real_apps, &apps).unwrap();
        assert!(
            resolve_application_context(
                &ApplicationContextRequest::discover_from(&fixture.path),
                &policy("v0.5.0")
            )
            .is_err()
        );
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "hegira-application-context-{name}-{}-{sequence}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
