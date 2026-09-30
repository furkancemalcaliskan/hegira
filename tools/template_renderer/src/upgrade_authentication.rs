use std::{
    fmt::{Display, Formatter},
    path::{Component, Path, PathBuf},
};

use application_manifest::{ApplicationManifest, SourceOwnershipClass};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    ManagedIntegrationTransition, ManagedIntegrationTransitionKind, ManifestCatalog,
    ResolvedUpgradeEdge, UpgradeCompositionState, UpgradeEdgeRequest, UpgradeReleaseIdentity,
};

pub const UPGRADE_AUTHENTICATION_SCHEMA: u32 = 1;
const MAX_MANAGED_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpgradeAuthenticationDiagnosticKind {
    UnsafeApplicationRoot,
    InvalidManifest,
    UnsupportedRelease,
    UnsupportedComposition,
    OwnershipMismatch,
    MissingManagedSource,
    UnexpectedManagedSource,
    UnsupportedSourceType,
    SourceDigestMismatch,
    SourceLimitExceeded,
    PackageContract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpgradeAuthenticationDiagnostic {
    pub schema: u32,
    pub kind: UpgradeAuthenticationDiagnosticKind,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeAuthenticationError {
    diagnostic: UpgradeAuthenticationDiagnostic,
}

impl UpgradeAuthenticationError {
    pub fn diagnostic(&self) -> &UpgradeAuthenticationDiagnostic {
        &self.diagnostic
    }
}

impl Display for UpgradeAuthenticationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {}",
            diagnostic_name(self.diagnostic.kind),
            self.diagnostic.subject
        )
    }
}

impl std::error::Error for UpgradeAuthenticationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedManagedSource {
    transition: ManagedIntegrationTransition,
    source: Option<Vec<u8>>,
    target: Option<Vec<u8>>,
}

impl AuthenticatedManagedSource {
    pub fn transition(&self) -> &ManagedIntegrationTransition {
        &self.transition
    }

    pub fn source(&self) -> Option<&[u8]> {
        self.source.as_deref()
    }

    pub fn target(&self) -> Option<&[u8]> {
        self.target.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedUpgradeBoundary {
    application: ApplicationManifest,
    manifest_source: Vec<u8>,
    edge: ResolvedUpgradeEdge,
    source_package_digest: String,
    source_baseline_digest: String,
    managed_sources: Vec<AuthenticatedManagedSource>,
}

impl AuthenticatedUpgradeBoundary {
    pub fn application(&self) -> &ApplicationManifest {
        &self.application
    }

    pub fn manifest_source(&self) -> &[u8] {
        &self.manifest_source
    }

    pub fn edge(&self) -> &ResolvedUpgradeEdge {
        &self.edge
    }

    pub fn source_package_digest(&self) -> &str {
        &self.source_package_digest
    }

    pub fn source_baseline_digest(&self) -> &str {
        &self.source_baseline_digest
    }

    pub fn managed_sources(&self) -> &[AuthenticatedManagedSource] {
        &self.managed_sources
    }
}

impl ManifestCatalog {
    /// Authenticate one existing application against one exact package edge.
    ///
    /// The operation is read-only. It observes only `hegira.toml` and managed
    /// paths declared by the resolved, digest-authenticated package edge.
    pub fn authenticate_upgrade_source(
        &self,
        application_root: impl AsRef<Path>,
    ) -> Result<AuthenticatedUpgradeBoundary, UpgradeAuthenticationError> {
        let source = ApplicationSource::open(application_root.as_ref())?;
        let manifest_bytes = source.read_required(Path::new("hegira.toml"))?;
        let manifest_text = std::str::from_utf8(&manifest_bytes).map_err(|_| {
            error(
                UpgradeAuthenticationDiagnosticKind::InvalidManifest,
                "hegira.toml",
            )
        })?;
        let application = ApplicationManifest::from_toml(manifest_text).map_err(|_| {
            error(
                UpgradeAuthenticationDiagnosticKind::InvalidManifest,
                "hegira.toml",
            )
        })?;
        let request = edge_request(&application)?;
        let edge = self.resolve_upgrade_edge(&request).map_err(|edge_error| {
            let diagnostic = edge_error.diagnostic();
            let kind = match diagnostic.kind {
                crate::UpgradeEdgeDiagnosticKind::UnsupportedRelease => {
                    UpgradeAuthenticationDiagnosticKind::UnsupportedRelease
                }
                crate::UpgradeEdgeDiagnosticKind::UnsupportedComposition => {
                    UpgradeAuthenticationDiagnosticKind::UnsupportedComposition
                }
            };
            error(kind, diagnostic.subject.clone())
        })?;
        let package_edge = self
            .upgrade_edges()
            .iter()
            .find(|candidate| candidate.id == edge.id)
            .ok_or_else(|| {
                error(
                    UpgradeAuthenticationDiagnosticKind::PackageContract,
                    "upgrade-edge",
                )
            })?;

        let applicable = edge
            .composition
            .source
            .components
            .iter()
            .chain(&edge.composition.target.components)
            .collect::<std::collections::BTreeSet<_>>();
        let local_ownership = application
            .upgrade
            .as_ref()
            .map(|upgrade| &upgrade.ownership);
        let mut managed_sources = Vec::new();
        for transition in edge
            .managed_integrations
            .iter()
            .filter(|transition| applicable.contains(&transition.component))
        {
            require_ownership(&edge, local_ownership, transition)?;
            let path = Path::new(&transition.path);
            let observed = match transition.kind {
                ManagedIntegrationTransitionKind::Create => {
                    source.ensure_absent(path)?;
                    None
                }
                ManagedIntegrationTransitionKind::Edit
                | ManagedIntegrationTransitionKind::Retire => {
                    let bytes = source.read_required(path)?;
                    verify_source_digest(transition, &bytes)?;
                    Some(bytes)
                }
            };
            let target = match transition.kind {
                ManagedIntegrationTransitionKind::Create
                | ManagedIntegrationTransitionKind::Edit => Some(
                    self.component_file(
                        transition
                            .target_source_component
                            .as_deref()
                            .unwrap_or(&transition.component),
                        path,
                    )
                    .map_err(|_| {
                        error(
                            UpgradeAuthenticationDiagnosticKind::PackageContract,
                            "managed-integration.target",
                        )
                    })?
                    .to_vec(),
                ),
                ManagedIntegrationTransitionKind::Retire => None,
            };
            managed_sources.push(AuthenticatedManagedSource {
                transition: transition.clone(),
                source: observed,
                target,
            });
        }
        source.verify_root()?;

        Ok(AuthenticatedUpgradeBoundary {
            application,
            manifest_source: manifest_bytes,
            edge: edge.clone(),
            source_package_digest: package_edge.source_package_digest.clone(),
            source_baseline_digest: edge.composition.source_baseline_digest.clone(),
            managed_sources,
        })
    }
}

fn edge_request(
    manifest: &ApplicationManifest,
) -> Result<UpgradeEdgeRequest, UpgradeAuthenticationError> {
    let composition = manifest.composition.as_ref().ok_or_else(|| {
        error(
            UpgradeAuthenticationDiagnosticKind::InvalidManifest,
            "composition",
        )
    })?;
    let database = one(
        manifest.selection.databases.iter().copied(),
        "selection.databases",
    )?;
    let client = one(
        manifest.selection.clients.iter().copied(),
        "selection.clients",
    )?;
    Ok(UpgradeEdgeRequest {
        source: UpgradeReleaseIdentity {
            framework: manifest.framework.clone(),
            package: composition.package.clone(),
        },
        composition: UpgradeCompositionState {
            database,
            client,
            components: composition
                .components
                .iter()
                .map(|component| component.id.clone())
                .collect(),
            modules: composition
                .modules
                .iter()
                .map(|module| module.id.clone())
                .collect(),
            capabilities: composition.capabilities.iter().copied().collect(),
        },
    })
}

fn one<T: Copy>(
    values: impl ExactSizeIterator<Item = T>,
    subject: &str,
) -> Result<T, UpgradeAuthenticationError> {
    if values.len() != 1 {
        return Err(error(
            UpgradeAuthenticationDiagnosticKind::InvalidManifest,
            subject,
        ));
    }
    values.into_iter().next().ok_or_else(|| {
        error(
            UpgradeAuthenticationDiagnosticKind::InvalidManifest,
            subject,
        )
    })
}

fn require_ownership(
    edge: &ResolvedUpgradeEdge,
    local: Option<&application_manifest::SourceOwnership>,
    transition: &ManagedIntegrationTransition,
) -> Result<(), UpgradeAuthenticationError> {
    let matches = |ownership: &application_manifest::SourceOwnership| {
        ownership.claims.iter().any(|claim| {
            claim.path == transition.path
                && claim.class == SourceOwnershipClass::ManagedIntegration
                && claim.integration.as_deref() == Some(&transition.integration)
        })
    };
    let edge_matches = match transition.kind {
        ManagedIntegrationTransitionKind::Create => matches(&edge.composition.target_ownership),
        ManagedIntegrationTransitionKind::Edit => {
            matches(&edge.composition.source_ownership)
                && matches(&edge.composition.target_ownership)
        }
        ManagedIntegrationTransitionKind::Retire => matches(&edge.composition.source_ownership),
    };
    let local_matches =
        transition.kind == ManagedIntegrationTransitionKind::Create || local.is_none_or(matches);
    if !edge_matches || !local_matches {
        return Err(error(
            UpgradeAuthenticationDiagnosticKind::OwnershipMismatch,
            transition.path.clone(),
        ));
    }
    Ok(())
}

fn verify_source_digest(
    transition: &ManagedIntegrationTransition,
    bytes: &[u8],
) -> Result<(), UpgradeAuthenticationError> {
    let expected = transition.source_sha256.as_deref().ok_or_else(|| {
        error(
            UpgradeAuthenticationDiagnosticKind::PackageContract,
            "managed-integration.source-sha256",
        )
    })?;
    let actual = format!("sha256:{:x}", Sha256::digest(bytes));
    if actual != expected {
        return Err(error(
            UpgradeAuthenticationDiagnosticKind::SourceDigestMismatch,
            transition.path.clone(),
        ));
    }
    Ok(())
}

fn error(
    kind: UpgradeAuthenticationDiagnosticKind,
    subject: impl Into<String>,
) -> UpgradeAuthenticationError {
    UpgradeAuthenticationError {
        diagnostic: UpgradeAuthenticationDiagnostic {
            schema: UPGRADE_AUTHENTICATION_SCHEMA,
            kind,
            subject: subject.into(),
        },
    }
}

fn diagnostic_name(kind: UpgradeAuthenticationDiagnosticKind) -> &'static str {
    match kind {
        UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot => "unsafe-application-root",
        UpgradeAuthenticationDiagnosticKind::InvalidManifest => "invalid-manifest",
        UpgradeAuthenticationDiagnosticKind::UnsupportedRelease => "unsupported-release",
        UpgradeAuthenticationDiagnosticKind::UnsupportedComposition => "unsupported-composition",
        UpgradeAuthenticationDiagnosticKind::OwnershipMismatch => "ownership-mismatch",
        UpgradeAuthenticationDiagnosticKind::MissingManagedSource => "missing-managed-source",
        UpgradeAuthenticationDiagnosticKind::UnexpectedManagedSource => "unexpected-managed-source",
        UpgradeAuthenticationDiagnosticKind::UnsupportedSourceType => "unsupported-source-type",
        UpgradeAuthenticationDiagnosticKind::SourceDigestMismatch => "source-digest-mismatch",
        UpgradeAuthenticationDiagnosticKind::SourceLimitExceeded => "source-limit-exceeded",
        UpgradeAuthenticationDiagnosticKind::PackageContract => "package-contract",
    }
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
struct ApplicationSource {
    root_path: PathBuf,
    root: rustix::fd::OwnedFd,
    identity: (u64, u64),
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
impl ApplicationSource {
    fn open(root: &Path) -> Result<Self, UpgradeAuthenticationError> {
        use rustix::fs::{self, Mode, OFlags};
        const DIRECTORY: OFlags = OFlags::RDONLY
            .union(OFlags::DIRECTORY)
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC);
        let root_path = absolute_lexical(root)?;
        let mut directory = fs::open("/", DIRECTORY, Mode::empty()).map_err(|_| {
            error(
                UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                "application-root",
            )
        })?;
        for component in root_path.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            directory = fs::openat(&directory, name, DIRECTORY, Mode::empty()).map_err(|_| {
                error(
                    UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                    "application-root",
                )
            })?;
        }
        let stat = fs::fstat(&directory).map_err(|_| {
            error(
                UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                "application-root",
            )
        })?;
        Ok(Self {
            root_path,
            root: directory,
            identity: (stat.st_dev as u64, stat.st_ino as u64),
        })
    }

    fn read_required(&self, path: &Path) -> Result<Vec<u8>, UpgradeAuthenticationError> {
        use rustix::{
            fs::{self, AtFlags, FileType, Mode, OFlags},
            io::Errno,
        };
        use std::io::Read as _;
        const FILE: OFlags = OFlags::RDONLY
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC)
            .union(OFlags::NONBLOCK);
        let (parent, name) = self.parent(path)?;
        let observed = match fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(observed) => observed,
            Err(Errno::NOENT) => {
                return Err(error(
                    UpgradeAuthenticationDiagnosticKind::MissingManagedSource,
                    relative_subject(path),
                ));
            }
            Err(_) => {
                return Err(error(
                    UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                    relative_subject(path),
                ));
            }
        };
        if !FileType::from_raw_mode(observed.st_mode).is_file() {
            return Err(error(
                UpgradeAuthenticationDiagnosticKind::UnsupportedSourceType,
                relative_subject(path),
            ));
        }
        let descriptor = fs::openat(&parent, name, FILE, Mode::empty()).map_err(|_| {
            error(
                UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                relative_subject(path),
            )
        })?;
        let stat = fs::fstat(&descriptor).map_err(|_| {
            error(
                UpgradeAuthenticationDiagnosticKind::UnsupportedSourceType,
                relative_subject(path),
            )
        })?;
        if !rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file() {
            return Err(error(
                UpgradeAuthenticationDiagnosticKind::UnsupportedSourceType,
                relative_subject(path),
            ));
        }
        if stat.st_size < 0 || stat.st_size as u64 > MAX_MANAGED_SOURCE_BYTES {
            return Err(error(
                UpgradeAuthenticationDiagnosticKind::SourceLimitExceeded,
                relative_subject(path),
            ));
        }
        let mut bytes = Vec::with_capacity(stat.st_size as usize);
        std::fs::File::from(descriptor)
            .take(MAX_MANAGED_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                error(
                    UpgradeAuthenticationDiagnosticKind::UnsupportedSourceType,
                    relative_subject(path),
                )
            })?;
        if bytes.len() as u64 > MAX_MANAGED_SOURCE_BYTES {
            return Err(error(
                UpgradeAuthenticationDiagnosticKind::SourceLimitExceeded,
                relative_subject(path),
            ));
        }
        Ok(bytes)
    }

    fn ensure_absent(&self, path: &Path) -> Result<(), UpgradeAuthenticationError> {
        use rustix::{
            fs::{self, AtFlags},
            io::Errno,
        };
        let (parent, name) = self.parent(path)?;
        match fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => Ok(()),
            Ok(_) => Err(error(
                UpgradeAuthenticationDiagnosticKind::UnexpectedManagedSource,
                relative_subject(path),
            )),
            Err(_) => Err(error(
                UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                relative_subject(path),
            )),
        }
    }

    fn parent<'a>(
        &self,
        path: &'a Path,
    ) -> Result<(rustix::fd::OwnedFd, &'a std::ffi::OsStr), UpgradeAuthenticationError> {
        use rustix::fs::{self, Mode, OFlags};
        const DIRECTORY: OFlags = OFlags::RDONLY
            .union(OFlags::DIRECTORY)
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC);
        validate_relative(path)?;
        let mut directory =
            fs::openat(&self.root, ".", DIRECTORY, Mode::empty()).map_err(|_| {
                error(
                    UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                    relative_subject(path),
                )
            })?;
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        for component in parent.components() {
            let Component::Normal(name) = component else {
                unreachable!("validated path")
            };
            directory = fs::openat(&directory, name, DIRECTORY, Mode::empty()).map_err(|_| {
                error(
                    UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                    relative_subject(path),
                )
            })?;
        }
        Ok((
            directory,
            path.file_name().expect("validated path names a file"),
        ))
    }

    fn verify_root(&self) -> Result<(), UpgradeAuthenticationError> {
        let current = Self::open(&self.root_path)?;
        if current.identity != self.identity {
            return Err(error(
                UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                "application-root",
            ));
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
struct ApplicationSource;

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
impl ApplicationSource {
    fn open(_: &Path) -> Result<Self, UpgradeAuthenticationError> {
        Err(error(
            UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
            "unsupported-platform",
        ))
    }

    fn read_required(&self, _: &Path) -> Result<Vec<u8>, UpgradeAuthenticationError> {
        Err(error(
            UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
            "unsupported-platform",
        ))
    }

    fn ensure_absent(&self, _: &Path) -> Result<(), UpgradeAuthenticationError> {
        Err(error(
            UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
            "unsupported-platform",
        ))
    }

    fn verify_root(&self) -> Result<(), UpgradeAuthenticationError> {
        Err(error(
            UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
            "unsupported-platform",
        ))
    }
}

fn absolute_lexical(path: &Path) -> Result<PathBuf, UpgradeAuthenticationError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| {
                error(
                    UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                    "application-root",
                )
            })?
            .join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::RootDir => resolved.push(Path::new("/")),
            Component::CurDir => {}
            Component::Normal(part) => resolved.push(part),
            Component::ParentDir | Component::Prefix(_) => {
                return Err(error(
                    UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot,
                    "application-root",
                ));
            }
        }
    }
    Ok(resolved)
}

fn validate_relative(path: &Path) -> Result<(), UpgradeAuthenticationError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(error(
            UpgradeAuthenticationDiagnosticKind::PackageContract,
            "managed-integration.path",
        ));
    }
    Ok(())
}

fn relative_subject(path: &Path) -> String {
    path.to_str().unwrap_or("managed-source").to_owned()
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "android", target_vendor = "apple")
))]
mod tests {
    use std::{
        fs,
        os::unix::{fs::symlink, net::UnixListener},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "hegira-upgrade-auth-{}-{sequence}-{name}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn root_replacement_is_detected_without_following_the_new_namespace() {
        let fixture = Fixture::new("root-replacement");
        let root = fixture.0.join("application");
        let moved = fixture.0.join("moved");
        let outside = fixture.0.join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(root.join("managed.rs"), b"expected managed bytes").unwrap();
        fs::write(outside.join("managed.rs"), b"credential-shaped-secret").unwrap();
        let source = ApplicationSource::open(&root).unwrap();

        fs::rename(&root, &moved).unwrap();
        symlink(&outside, &root).unwrap();

        assert_eq!(
            source.read_required(Path::new("managed.rs")).unwrap(),
            b"expected managed bytes"
        );
        let error = source.verify_root().unwrap_err();
        assert_eq!(
            error.diagnostic().kind,
            UpgradeAuthenticationDiagnosticKind::UnsafeApplicationRoot
        );
        assert!(!error.to_string().contains("credential-shaped-secret"));
    }

    #[test]
    fn special_oversized_and_traversal_sources_fail_closed() {
        let fixture = Fixture::new("unsafe-types");
        let socket_path = fixture.0.join("managed.sock");
        let _socket = UnixListener::bind(&socket_path).unwrap();
        let oversized_path = fixture.0.join("oversized.rs");
        let oversized = fs::File::create(&oversized_path).unwrap();
        oversized.set_len(MAX_MANAGED_SOURCE_BYTES + 1).unwrap();
        let source = ApplicationSource::open(&fixture.0).unwrap();

        let special = source.read_required(Path::new("managed.sock")).unwrap_err();
        assert_eq!(
            special.diagnostic().kind,
            UpgradeAuthenticationDiagnosticKind::UnsupportedSourceType
        );
        let oversized = source.read_required(Path::new("oversized.rs")).unwrap_err();
        assert_eq!(
            oversized.diagnostic().kind,
            UpgradeAuthenticationDiagnosticKind::SourceLimitExceeded
        );
        let traversal = source.read_required(Path::new("../outside")).unwrap_err();
        assert_eq!(
            traversal.diagnostic().kind,
            UpgradeAuthenticationDiagnosticKind::PackageContract
        );
    }
}
