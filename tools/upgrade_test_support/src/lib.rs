use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const RELEASE_TAG: &str = "v0.6.0";
pub const RELEASE_TAG_OBJECT: &str = "9cb1f4aa01a16cc762653a8b170d0c4fcdb412b8";
pub const RELEASE_COMMIT: &str = "91c522b0dcfa71502ef5b4f0beed2ac6fb676dd3";
pub const RELEASE_SOURCE_TREE: &str = "65cdcaf0bd0dbebfd81c657ea8424b044d2e81e2";
pub const FRAMEWORK_REPOSITORY: &str = "https://github.com/furkancemalcaliskan/hegira.git";
pub const PACKAGE_ID: &str = "hegira-canonical";
pub const PACKAGE_DIGEST: &str =
    "sha256:cff6969dadc6e0155af7b944877dba5c61b6e0c4c3ea041d9a8a2665949c285e";
pub const LOCK_REVISION: &str = "67d0708e135a233335969752890c23d5bb1fb3c6";
pub const APPLICATION_NAME: &str = "baseline-application";

const RELEASE_SCHEMA: u32 = 1;
const TREE_SCHEMA: u32 = 1;
const MAX_RELEASE_BYTES: u64 = 64 * 1024;
const MAX_TREE_BYTES: u64 = 256 * 1024;
const MAX_OBJECT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_OBJECT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FILES_PER_TREE: usize = 256;
const FIXTURE_PATH: &str = "test-fixtures/application-baselines/v0.6.0";

/// Only reviewed, published releases can be selected; legacy callers retain v0.6.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineVersion {
    V0_6_0,
    V0_7_0,
}

pub const V0_7_0_PROVENANCE: &str = r#"schema = 1
authentication = "git-ssh-tag"
signer_fingerprint = "SHA256:Nq2p1V/hlkhUrXEv1UiPMlLKnZ4w5D7ux7/ki5XDRDA"
published_release = "https://github.com/furkancemalcaliskan/hegira/releases/tag/v0.7.0"
published_at = "2026-10-04T15:13:18Z"
producer = "released-source-hegira-cli"
identity_added_lock = "matching-released-default-composition"
"#;

impl BaselineVersion {
    pub const ALL: [Self; 2] = [Self::V0_6_0, Self::V0_7_0];

    pub const fn tag(self) -> &'static str {
        match self {
            Self::V0_6_0 => RELEASE_TAG,
            Self::V0_7_0 => "v0.7.0",
        }
    }

    pub fn expected_release(self) -> BaselineRelease {
        let (tag_object, commit, source_tree, package_digest, lock_revision, provenance_digest) =
            match self {
                Self::V0_6_0 => (
                    RELEASE_TAG_OBJECT,
                    RELEASE_COMMIT,
                    RELEASE_SOURCE_TREE,
                    PACKAGE_DIGEST,
                    LOCK_REVISION,
                    None,
                ),
                Self::V0_7_0 => (
                    "f83979ef149eb1ed0017edee143cd502c070ce07",
                    "635c163d50375f7d250311426c67413da382ed23",
                    "2547cb85aae856024771cbe4432a2a88e4703574",
                    "sha256:34ddf6356d441791c93f306f5ea6aeb5927cab1062d2e6d7f8b8928ee198b053",
                    "7a9ed5bce734f5459fee62d3a2069ce1cfc7d212",
                    Some(V0_7_0_PROVENANCE_DIGEST),
                ),
            };
        BaselineRelease {
            tag: self.tag().to_owned(),
            tag_object: tag_object.to_owned(),
            commit: commit.to_owned(),
            source_tree: source_tree.to_owned(),
            framework_repository: FRAMEWORK_REPOSITORY.to_owned(),
            package_id: PACKAGE_ID.to_owned(),
            package_version: self.tag().to_owned(),
            package_digest: package_digest.to_owned(),
            lock_revision: lock_revision.to_owned(),
            application: APPLICATION_NAME.to_owned(),
            provenance_digest: provenance_digest.map(str::to_owned),
        }
    }

    fn manifest_digest(self) -> &'static str {
        match self {
            Self::V0_6_0 => {
                "sha256:14f576d2378f4ce1fcbd82aa10a25e1159c9db0cc30b2519fd4671a95d81c71a"
            }
            Self::V0_7_0 => V0_7_0_MANIFEST_DIGEST,
        }
    }
}

// Calculated from the authenticated import, never from evolving templates.
const V0_7_0_PROVENANCE_DIGEST: &str =
    "sha256:017f5f6f1b26f3a82437ec24699a528887d2bc2a8fc841b275d35576c4210606";
const V0_7_0_MANIFEST_DIGEST: &str =
    "sha256:8307ab374455467b007545764faaa1c0e2d56f9d95b324be10946d5992cd85e1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BaselineComposition {
    Default,
    IdentityAdded,
    Minimal,
}

impl BaselineComposition {
    pub const ALL: [Self; 3] = [Self::Default, Self::IdentityAdded, Self::Minimal];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::IdentityAdded => "identity-added",
            Self::Minimal => "minimal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BaselineDatabase {
    Postgres,
    Sqlite,
}

impl BaselineDatabase {
    pub const ALL: [Self; 2] = [Self::Postgres, Self::Sqlite];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaselineRequest {
    pub composition: BaselineComposition,
    pub database: BaselineDatabase,
}

impl BaselineRequest {
    pub const fn new(composition: BaselineComposition, database: BaselineDatabase) -> Self {
        Self {
            composition,
            database,
        }
    }

    pub fn id(self) -> String {
        format!("{}-{}", self.composition.as_str(), self.database.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineRelease {
    pub tag: String,
    pub tag_object: String,
    pub commit: String,
    pub source_tree: String,
    pub framework_repository: String,
    pub package_id: String,
    pub package_version: String,
    pub package_digest: String,
    pub lock_revision: String,
    pub application: String,
    pub provenance_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineFile {
    bytes: Arc<[u8]>,
    executable: bool,
}

impl BaselineFile {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn executable(&self) -> bool {
        self.executable
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineSnapshot {
    request: BaselineRequest,
    tree_digest: String,
    files: BTreeMap<PathBuf, BaselineFile>,
}

impl BaselineSnapshot {
    pub const fn request(&self) -> BaselineRequest {
        self.request
    }

    pub fn tree_digest(&self) -> &str {
        &self.tree_digest
    }

    pub fn files(&self) -> &BTreeMap<PathBuf, BaselineFile> {
        &self.files
    }

    pub fn file(&self, path: impl AsRef<Path>) -> Option<&BaselineFile> {
        self.files.get(path.as_ref())
    }

    pub fn materialize(&self, destination: impl AsRef<Path>) -> Result<()> {
        let destination = destination.as_ref();
        fs::create_dir(destination).map_err(|_| error(BaselineErrorKind::Destination))?;
        let result = self.write_files(destination);
        if result.is_err() {
            let _ = fs::remove_dir_all(destination);
        }
        result
    }

    fn write_files(&self, destination: &Path) -> Result<()> {
        for (path, file) in &self.files {
            let target = destination.join(path);
            let parent = target
                .parent()
                .ok_or_else(|| error(BaselineErrorKind::InvalidPath))?;
            fs::create_dir_all(parent).map_err(|_| error(BaselineErrorKind::Destination))?;
            fs::write(&target, file.bytes()).map_err(|_| error(BaselineErrorKind::Destination))?;
            set_permissions(&target, file.executable)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct BaselineCatalog {
    release: BaselineRelease,
    snapshots: BTreeMap<BaselineRequest, BaselineSnapshot>,
}

impl BaselineCatalog {
    pub fn from_repository(repository: impl AsRef<Path>) -> Result<Self> {
        Self::load(repository.as_ref().join(FIXTURE_PATH))
    }

    pub fn from_repository_release(
        repository: impl AsRef<Path>,
        version: BaselineVersion,
    ) -> Result<Self> {
        Self::load_for_release(
            repository
                .as_ref()
                .join("test-fixtures/application-baselines")
                .join(version.tag()),
            version,
        )
    }

    pub fn repository_fixture() -> Result<Self> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| error(BaselineErrorKind::Identity))?;
        Self::from_repository(root)
    }

    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        Self::load_for_release(root, BaselineVersion::V0_6_0)
    }

    pub fn load_for_release(root: impl AsRef<Path>, version: BaselineVersion) -> Result<Self> {
        let root = root.as_ref();
        reject_symlink(root)?;
        reject_symlink(&root.join("trees"))?;
        reject_symlink(&root.join("objects"))?;
        let release_bytes = read_bounded(&root.join("release.toml"), MAX_RELEASE_BYTES)?;
        // Pin the inventory as well as the objects. Rehashing a modified object,
        // tree and release manifest together must not authenticate new history.
        verify_digest(&release_bytes, version.manifest_digest())?;
        let manifest: ReleaseManifest = toml::from_slice(&release_bytes)
            .map_err(|_| error(BaselineErrorKind::InvalidManifest))?;
        validate_release(&manifest, version)?;
        if let Some(digest) = &manifest.provenance_digest {
            verify_digest(
                &read_bounded(&root.join("provenance.toml"), MAX_RELEASE_BYTES)?,
                digest,
            )?;
        }
        validate_root(root, manifest.provenance_digest.is_some())?;

        let mut object_cache = BTreeMap::<String, Arc<[u8]>>::new();
        let mut snapshots = BTreeMap::new();
        let mut tree_names = BTreeSet::new();
        let mut total_object_bytes = 0_u64;
        for baseline in &manifest.baselines {
            let request = baseline.request()?;
            if !tree_names.insert(format!("{}.toml", baseline.id))
                || snapshots.contains_key(&request)
            {
                return Err(error(BaselineErrorKind::Duplicate));
            }
            let tree_path = validated_child(root, &baseline.tree)?;
            let tree_bytes = read_bounded(&tree_path, MAX_TREE_BYTES)?;
            verify_digest(&tree_bytes, &baseline.tree_digest)?;
            let tree: TreeManifest = toml::from_slice(&tree_bytes)
                .map_err(|_| error(BaselineErrorKind::InvalidManifest))?;
            let snapshot = load_tree(
                root,
                request,
                baseline,
                tree,
                &mut object_cache,
                &mut total_object_bytes,
            )?;
            snapshots.insert(request, snapshot);
        }
        validate_matrix(&snapshots)?;
        validate_declared_directory(&root.join("trees"), tree_names.iter().map(String::as_str))?;
        validate_declared_directory(
            &root.join("objects"),
            object_cache.keys().map(String::as_str),
        )?;

        Ok(Self {
            release: manifest.into_release(),
            snapshots,
        })
    }

    pub fn release(&self) -> &BaselineRelease {
        &self.release
    }

    pub fn requests(&self) -> impl ExactSizeIterator<Item = BaselineRequest> + '_ {
        self.snapshots.keys().copied()
    }

    pub fn snapshot(&self, request: BaselineRequest) -> Result<&BaselineSnapshot> {
        self.snapshots
            .get(&request)
            .ok_or_else(|| error(BaselineErrorKind::UnsupportedBaseline))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineErrorKind {
    Io,
    InvalidManifest,
    Identity,
    InvalidPath,
    Duplicate,
    LimitExceeded,
    DigestMismatch,
    UndeclaredEntry,
    UnsupportedBaseline,
    Destination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineError {
    kind: BaselineErrorKind,
}

impl BaselineError {
    pub const fn kind(&self) -> BaselineErrorKind {
        self.kind
    }
}

impl Display for BaselineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self.kind {
            BaselineErrorKind::Io => "released application baseline could not be read",
            BaselineErrorKind::InvalidManifest => {
                "released application baseline manifest is invalid"
            }
            BaselineErrorKind::Identity => "released application baseline identity is invalid",
            BaselineErrorKind::InvalidPath => "released application baseline path is invalid",
            BaselineErrorKind::Duplicate => {
                "released application baseline contains duplicate declarations"
            }
            BaselineErrorKind::LimitExceeded => {
                "released application baseline exceeds a bounded limit"
            }
            BaselineErrorKind::DigestMismatch => {
                "released application baseline authentication failed"
            }
            BaselineErrorKind::UndeclaredEntry => {
                "released application baseline contains an undeclared entry"
            }
            BaselineErrorKind::UnsupportedBaseline => {
                "released application baseline is unsupported"
            }
            BaselineErrorKind::Destination => {
                "released application baseline could not be materialized"
            }
        })
    }
}

impl std::error::Error for BaselineError {}

pub type Result<T> = std::result::Result<T, BaselineError>;

fn error(kind: BaselineErrorKind) -> BaselineError {
    BaselineError { kind }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseManifest {
    schema: u32,
    tag: String,
    tag_object: String,
    commit: String,
    source_tree: String,
    framework_repository: String,
    package_id: String,
    package_version: String,
    package_digest: String,
    lock_revision: String,
    application: String,
    #[serde(default)]
    provenance_digest: Option<String>,
    baselines: Vec<BaselineDeclaration>,
}

impl ReleaseManifest {
    fn into_release(self) -> BaselineRelease {
        BaselineRelease {
            tag: self.tag,
            tag_object: self.tag_object,
            commit: self.commit,
            source_tree: self.source_tree,
            framework_repository: self.framework_repository,
            package_id: self.package_id,
            package_version: self.package_version,
            package_digest: self.package_digest,
            lock_revision: self.lock_revision,
            application: self.application,
            provenance_digest: self.provenance_digest,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselineDeclaration {
    id: String,
    composition: String,
    database: String,
    client: String,
    tree: String,
    tree_digest: String,
}

impl BaselineDeclaration {
    fn request(&self) -> Result<BaselineRequest> {
        if self.client != "leptos" {
            return Err(error(BaselineErrorKind::UnsupportedBaseline));
        }
        let composition = match self.composition.as_str() {
            "default" => BaselineComposition::Default,
            "identity-added" => BaselineComposition::IdentityAdded,
            "minimal" => BaselineComposition::Minimal,
            _ => return Err(error(BaselineErrorKind::UnsupportedBaseline)),
        };
        let database = match self.database.as_str() {
            "postgres" => BaselineDatabase::Postgres,
            "sqlite" => BaselineDatabase::Sqlite,
            _ => return Err(error(BaselineErrorKind::UnsupportedBaseline)),
        };
        let request = BaselineRequest::new(composition, database);
        if self.id != request.id() || self.tree != format!("trees/{}.toml", self.id) {
            return Err(error(BaselineErrorKind::Identity));
        }
        Ok(request)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TreeManifest {
    schema: u32,
    id: String,
    composition: String,
    database: String,
    client: String,
    files: Vec<TreeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TreeFile {
    path: String,
    digest: String,
    executable: bool,
}

fn validate_release(manifest: &ReleaseManifest, version: BaselineVersion) -> Result<()> {
    let expected = version.expected_release();
    if manifest.schema != RELEASE_SCHEMA
        || manifest.tag != expected.tag
        || manifest.tag_object != expected.tag_object
        || manifest.commit != expected.commit
        || manifest.source_tree != expected.source_tree
        || manifest.framework_repository != FRAMEWORK_REPOSITORY
        || manifest.package_id != PACKAGE_ID
        || manifest.package_version != expected.package_version
        || manifest.package_digest != expected.package_digest
        || manifest.lock_revision != expected.lock_revision
        || manifest.application != APPLICATION_NAME
        || manifest.provenance_digest != expected.provenance_digest
    {
        return Err(error(BaselineErrorKind::Identity));
    }
    Ok(())
}

fn validate_root(root: &Path, provenance: bool) -> Result<()> {
    let mut expected = BTreeSet::from(["release.toml", "trees", "objects"]);
    if provenance {
        expected.insert("provenance.toml");
    }
    for entry in fs::read_dir(root).map_err(|_| error(BaselineErrorKind::Io))? {
        let entry = entry.map_err(|_| error(BaselineErrorKind::Io))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| error(BaselineErrorKind::InvalidPath))?;
        if !expected.remove(name.as_str()) {
            return Err(error(BaselineErrorKind::UndeclaredEntry));
        }
    }
    if !expected.is_empty() {
        return Err(error(BaselineErrorKind::UndeclaredEntry));
    }
    Ok(())
}

fn load_tree(
    root: &Path,
    request: BaselineRequest,
    declaration: &BaselineDeclaration,
    tree: TreeManifest,
    object_cache: &mut BTreeMap<String, Arc<[u8]>>,
    total_object_bytes: &mut u64,
) -> Result<BaselineSnapshot> {
    if tree.schema != TREE_SCHEMA
        || tree.id != declaration.id
        || tree.composition != declaration.composition
        || tree.database != declaration.database
        || tree.client != declaration.client
    {
        return Err(error(BaselineErrorKind::Identity));
    }
    if tree.files.is_empty() || tree.files.len() > MAX_FILES_PER_TREE {
        return Err(error(BaselineErrorKind::LimitExceeded));
    }

    let mut files = BTreeMap::new();
    let mut previous = None::<&str>;
    for file in &tree.files {
        if previous.is_some_and(|value| value >= file.path.as_str()) {
            return Err(error(BaselineErrorKind::Duplicate));
        }
        previous = Some(&file.path);
        let relative = validate_relative_path(&file.path)?;
        let digest = parse_digest(&file.digest)?;
        let bytes = if let Some(bytes) = object_cache.get(digest) {
            Arc::clone(bytes)
        } else {
            let object = validated_child(root, &format!("objects/{digest}"))?;
            let bytes = read_bounded(&object, MAX_OBJECT_BYTES)?;
            verify_digest(&bytes, &file.digest)?;
            *total_object_bytes = total_object_bytes
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| error(BaselineErrorKind::LimitExceeded))?;
            if *total_object_bytes > MAX_TOTAL_OBJECT_BYTES {
                return Err(error(BaselineErrorKind::LimitExceeded));
            }
            let bytes = Arc::<[u8]>::from(bytes);
            object_cache.insert(digest.to_owned(), Arc::clone(&bytes));
            bytes
        };
        files.insert(
            relative,
            BaselineFile {
                bytes,
                executable: file.executable,
            },
        );
    }

    Ok(BaselineSnapshot {
        request,
        tree_digest: declaration.tree_digest.clone(),
        files,
    })
}

fn validate_matrix(snapshots: &BTreeMap<BaselineRequest, BaselineSnapshot>) -> Result<()> {
    let expected = BaselineComposition::ALL
        .into_iter()
        .flat_map(|composition| {
            BaselineDatabase::ALL
                .into_iter()
                .map(move |database| BaselineRequest::new(composition, database))
        })
        .collect::<BTreeSet<_>>();
    if snapshots.keys().copied().collect::<BTreeSet<_>>() != expected {
        return Err(error(BaselineErrorKind::UnsupportedBaseline));
    }
    Ok(())
}

fn validate_declared_directory<'a>(
    directory: &Path,
    declared: impl Iterator<Item = &'a str>,
) -> Result<()> {
    reject_symlink(directory)?;
    let declared = declared.map(str::to_owned).collect::<BTreeSet<_>>();
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(directory).map_err(|_| error(BaselineErrorKind::Io))? {
        let entry = entry.map_err(|_| error(BaselineErrorKind::Io))?;
        if entry
            .file_type()
            .map_err(|_| error(BaselineErrorKind::Io))?
            .is_symlink()
            || !entry
                .file_type()
                .map_err(|_| error(BaselineErrorKind::Io))?
                .is_file()
        {
            return Err(error(BaselineErrorKind::InvalidPath));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| error(BaselineErrorKind::InvalidPath))?;
        actual.insert(name);
    }
    if actual != declared {
        return Err(error(BaselineErrorKind::UndeclaredEntry));
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.len() > 512 || value.contains(['\n', '\r', '\0', '\\']) {
        return Err(error(BaselineErrorKind::InvalidPath));
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(error(BaselineErrorKind::InvalidPath));
    }
    Ok(path.to_path_buf())
}

fn validated_child(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = validate_relative_path(relative)?;
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component.as_os_str());
        reject_symlink(&path)?;
    }
    Ok(path)
}

fn reject_symlink(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error(BaselineErrorKind::Io))?;
    if metadata.file_type().is_symlink() {
        return Err(error(BaselineErrorKind::InvalidPath));
    }
    Ok(())
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    reject_symlink(path)?;
    let metadata = fs::metadata(path).map_err(|_| error(BaselineErrorKind::Io))?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(error(BaselineErrorKind::LimitExceeded));
    }
    fs::read(path).map_err(|_| error(BaselineErrorKind::Io))
}

fn parse_digest(value: &str) -> Result<&str> {
    let digest = value
        .strip_prefix("sha256:")
        .ok_or_else(|| error(BaselineErrorKind::InvalidManifest))?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(error(BaselineErrorKind::InvalidManifest));
    }
    Ok(digest)
}

fn verify_digest(bytes: &[u8], expected: &str) -> Result<()> {
    let expected = parse_digest(expected)?;
    if format!("{:x}", Sha256::digest(bytes)) != expected {
        return Err(error(BaselineErrorKind::DigestMismatch));
    }
    Ok(())
}

#[cfg(unix)]
fn set_permissions(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|_| error(BaselineErrorKind::Destination))
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn authenticates_the_complete_released_application_matrix() {
        let catalog = BaselineCatalog::repository_fixture().unwrap();
        assert_eq!(catalog.release().tag, RELEASE_TAG);
        assert_eq!(catalog.release().tag_object, RELEASE_TAG_OBJECT);
        assert_eq!(catalog.release().commit, RELEASE_COMMIT);
        assert_eq!(catalog.release().source_tree, RELEASE_SOURCE_TREE);
        assert_eq!(catalog.release().package_digest, PACKAGE_DIGEST);
        assert_eq!(catalog.release().lock_revision, LOCK_REVISION);
        assert_eq!(catalog.requests().len(), 6);

        for composition in BaselineComposition::ALL {
            for database in BaselineDatabase::ALL {
                let request = BaselineRequest::new(composition, database);
                let snapshot = catalog.snapshot(request).unwrap();
                assert_eq!(snapshot.request(), request);
                assert!(snapshot.files().contains_key(Path::new("Cargo.lock")));
                assert!(snapshot.files().contains_key(Path::new("hegira.toml")));
                let manifest = text(snapshot, "hegira.toml");
                assert!(manifest.contains("application = \"baseline-application\""));
                assert!(manifest.contains("version = \"v0.6.0\""));
                assert!(manifest.contains("id = \"hegira-canonical\""));
                assert!(manifest.contains(&format!("databases = [\"{}\"]", database.as_str())));
                let lock = text(snapshot, "Cargo.lock");
                assert!(lock.contains(&format!("?tag=v0.6.0#{LOCK_REVISION}")));
                assert!(snapshot.files().keys().all(|path| !forbidden(path)));
                assert!(
                    snapshot
                        .files()
                        .values()
                        .all(|file| !contains_credential(file.bytes()))
                );
            }
        }
    }

    #[test]
    fn materializes_exact_snapshots_only_into_absent_destinations() {
        let fixture = TestDirectory::new("materialize");
        let catalog = BaselineCatalog::repository_fixture().unwrap();
        for request in catalog.requests() {
            let snapshot = catalog.snapshot(request).unwrap();
            let destination = fixture.path.join(request.id());
            snapshot.materialize(&destination).unwrap();
            for (path, expected) in snapshot.files() {
                assert_eq!(fs::read(destination.join(path)).unwrap(), expected.bytes());
            }
            assert_eq!(
                snapshot.materialize(&destination).unwrap_err().kind(),
                BaselineErrorKind::Destination
            );
        }
    }

    #[test]
    fn any_object_change_or_undeclared_entry_fails_authentication() {
        let fixture = TestDirectory::new("tamper");
        let source = fixture_root();
        let copy = fixture.path.join("copy");
        copy_tree(&source, &copy);
        let object = fs::read_dir(copy.join("objects"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(object, b"changed").unwrap();
        assert_eq!(
            BaselineCatalog::load(&copy).unwrap_err().kind(),
            BaselineErrorKind::DigestMismatch
        );

        let copy = fixture.path.join("tree-change");
        copy_tree(&source, &copy);
        let tree = copy.join("trees/default-sqlite.toml");
        let mut bytes = fs::read(&tree).unwrap();
        bytes.push(b'\n');
        fs::write(tree, bytes).unwrap();
        assert_eq!(
            BaselineCatalog::load(&copy).unwrap_err().kind(),
            BaselineErrorKind::DigestMismatch
        );

        let copy = fixture.path.join("extra");
        copy_tree(&source, &copy);
        fs::write(copy.join("objects/extra"), b"extra").unwrap();
        assert_eq!(
            BaselineCatalog::load(&copy).unwrap_err().kind(),
            BaselineErrorKind::UndeclaredEntry
        );
    }

    #[test]
    fn v0_7_0_materializes_offline_with_exact_composition_ownership_and_locks() {
        let fixture = TestDirectory::new("v0-7-offline");
        let isolated = fixture.path.join("isolated-repository");
        let data = isolated.join("test-fixtures/application-baselines/v0.7.0");
        fs::create_dir_all(data.parent().unwrap()).unwrap();
        copy_tree(&fixture_root().parent().unwrap().join("v0.7.0"), &data);
        let catalog =
            BaselineCatalog::from_repository_release(&isolated, BaselineVersion::V0_7_0).unwrap();
        assert_eq!(
            catalog.release(),
            &BaselineVersion::V0_7_0.expected_release()
        );
        assert_eq!(catalog.requests().len(), 6);
        assert!(!isolated.join(".git").exists());
        assert!(!isolated.join("templates").exists());
        assert_eq!(
            fs::read(data.join("provenance.toml")).unwrap(),
            V0_7_0_PROVENANCE.as_bytes()
        );
        for request in catalog.requests() {
            let snapshot = catalog.snapshot(request).unwrap();
            let manifest: toml::Value = toml::from_str(text(snapshot, "hegira.toml")).unwrap();
            assert_eq!(manifest["schema"].as_integer(), Some(3));
            assert_eq!(manifest["framework"]["version"].as_str(), Some("v0.7.0"));
            assert_eq!(
                manifest["composition"]["package"]["version"].as_str(),
                Some("v0.7.0")
            );
            assert_eq!(
                manifest["upgrade"]["framework"]["version"].as_str(),
                Some("v0.7.0")
            );
            assert_eq!(
                manifest["selection"]["databases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>(),
                [request.database.as_str()]
            );
            assert_eq!(
                manifest["selection"]["clients"].as_array().unwrap()[0].as_str(),
                Some("leptos")
            );
            let modules = manifest["composition"]
                .get("modules")
                .and_then(toml::Value::as_array);
            assert_eq!(
                modules.map_or(0, Vec::len),
                usize::from(request.composition != BaselineComposition::Minimal)
            );
            let components: Vec<_> = manifest["composition"]["components"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect();
            assert_eq!(
                components,
                match request.composition {
                    BaselineComposition::Default => vec!["layered-base", "layered-leptos-identity"],
                    BaselineComposition::Minimal => vec!["layered-base", "layered-leptos-minimal"],
                    BaselineComposition::IdentityAdded =>
                        vec!["identity", "layered-base", "layered-leptos-minimal"],
                }
            );
            let claims = manifest["upgrade"]["ownership"]["claims"]
                .as_array()
                .unwrap();
            for provider in ["sqlite", "postgres"] {
                let prefix = format!("crates/infrastructure/migrations/{provider}");
                assert!(
                    claims
                        .iter()
                        .any(|claim| claim["path"].as_str() == Some(&prefix)
                            && claim["class"].as_str() == Some("immutable-history"))
                );
            }
            for managed in ["Cargo.toml", "Cargo.lock", "hegira.toml"] {
                assert!(
                    claims
                        .iter()
                        .any(|claim| claim["path"].as_str() == Some(managed)
                            && claim["class"].as_str() == Some("managed-integration"))
                );
            }
            assert!(
                text(snapshot, "Cargo.lock")
                    .contains("?tag=v0.7.0#7a9ed5bce734f5459fee62d3a2069ce1cfc7d212")
            );
            assert!(
                !snapshot
                    .files()
                    .contains_key(Path::new(".github/workflows/quality.yml"))
            );
            assert!(snapshot.files().keys().all(|path| !forbidden(path)));
            for repeat in ["first", "second"] {
                let output = fixture.path.join(format!("{}-{repeat}", request.id()));
                snapshot.materialize(&output).unwrap();
                for (path, file) in snapshot.files() {
                    assert_eq!(fs::read(output.join(path)).unwrap(), file.bytes());
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        assert_eq!(
                            fs::metadata(output.join(path))
                                .unwrap()
                                .permissions()
                                .mode()
                                & 0o111
                                != 0,
                            file.executable()
                        );
                    }
                }
            }
        }
        assert_eq!(
            BaselineCatalog::repository_fixture().unwrap().release().tag,
            "v0.6.0"
        );
        assert!(
            BaselineCatalog::load(&data).is_err(),
            "legacy loading never silently selects a new release"
        );
    }

    #[test]
    fn coordinated_object_tree_and_inventory_rehashing_cannot_rewrite_either_release() {
        let fixture = TestDirectory::new("reseal");
        for version in BaselineVersion::ALL {
            let source = fixture_root().parent().unwrap().join(version.tag());
            let copy = fixture.path.join(version.tag());
            copy_tree(&source, &copy);
            let original = fs::read_dir(copy.join("objects"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let old = format!("sha256:{}", original.file_name().unwrap().to_str().unwrap());
            let changed = b"coordinated replacement of reviewed history";
            let new = format!("sha256:{:x}", Sha256::digest(changed));
            fs::write(copy.join("objects").join(&new[7..]), changed).unwrap();
            fs::remove_file(original).unwrap();
            let mut release: toml::Value =
                toml::from_str(&fs::read_to_string(copy.join("release.toml")).unwrap()).unwrap();
            for baseline in release["baselines"].as_array_mut().unwrap() {
                let path = copy.join(baseline["tree"].as_str().unwrap());
                let mut tree: toml::Value =
                    toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
                for file in tree["files"].as_array_mut().unwrap() {
                    if file["digest"].as_str() == Some(&old) {
                        file["digest"] = toml::Value::String(new.clone());
                    }
                }
                let bytes = toml::to_string(&tree).unwrap();
                baseline["tree_digest"] =
                    toml::Value::String(format!("sha256:{:x}", Sha256::digest(bytes.as_bytes())));
                fs::write(path, bytes).unwrap();
            }
            fs::write(
                copy.join("release.toml"),
                toml::to_string(&release).unwrap(),
            )
            .unwrap();
            assert_eq!(
                BaselineCatalog::load_for_release(&copy, version)
                    .unwrap_err()
                    .kind(),
                BaselineErrorKind::DigestMismatch
            );
        }
    }

    #[test]
    fn provenance_unknown_entries_wrong_releases_and_symlinks_fail_closed() {
        let fixture = TestDirectory::new("v0-7-tamper");
        let source = fixture_root().parent().unwrap().join("v0.7.0");
        for (name, relative, content, expected) in [
            (
                "provenance",
                "provenance.toml",
                "changed provenance",
                BaselineErrorKind::DigestMismatch,
            ),
            (
                "unknown-object",
                "objects/extra",
                "extra",
                BaselineErrorKind::UndeclaredEntry,
            ),
            (
                "unknown-root",
                "extra",
                "extra",
                BaselineErrorKind::UndeclaredEntry,
            ),
            (
                "unknown-tree",
                "trees/extra.toml",
                "extra",
                BaselineErrorKind::UndeclaredEntry,
            ),
            (
                "wrong-release",
                "release.toml",
                "schema = 1\ntag = \"v0.8.0\"\n",
                BaselineErrorKind::DigestMismatch,
            ),
        ] {
            let copy = fixture.path.join(name);
            copy_tree(&source, &copy);
            fs::write(copy.join(relative), content).unwrap();
            assert_eq!(
                BaselineCatalog::load_for_release(&copy, BaselineVersion::V0_7_0)
                    .unwrap_err()
                    .kind(),
                expected
            );
        }
        #[cfg(unix)]
        {
            let copy = fixture.path.join("symlink");
            copy_tree(&source, &copy);
            fs::remove_file(copy.join("provenance.toml")).unwrap();
            std::os::unix::fs::symlink(
                source.join("provenance.toml"),
                copy.join("provenance.toml"),
            )
            .unwrap();
            assert_eq!(
                BaselineCatalog::load_for_release(&copy, BaselineVersion::V0_7_0)
                    .unwrap_err()
                    .kind(),
                BaselineErrorKind::InvalidPath
            );
        }
    }

    fn text<'a>(snapshot: &'a BaselineSnapshot, path: &str) -> &'a str {
        std::str::from_utf8(snapshot.file(path).unwrap().bytes()).unwrap()
    }

    fn forbidden(path: &Path) -> bool {
        path.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some(".git" | "target" | "node_modules")
            )
        }) || matches!(
            path.file_name().and_then(|value| value.to_str()),
            Some(".env")
        ) || matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("db" | "sqlite" | "pem" | "key")
        )
    }

    fn contains_credential(bytes: &[u8]) -> bool {
        let Ok(value) = std::str::from_utf8(bytes) else {
            return false;
        };
        [
            "-----BEGIN PRIVATE KEY-----",
            "-----BEGIN RSA PRIVATE KEY-----",
            "github_pat_",
            "ghp_",
            "AKIA",
        ]
        .iter()
        .any(|pattern| value.contains(pattern))
    }

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join(FIXTURE_PATH)
    }

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "hegira-upgrade-baseline-{}-{name}-{nonce}-{sequence}",
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
