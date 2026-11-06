//! Data packing only. Use the maintainer shell wrapper to authenticate and render
//! the signed published source before invoking this example.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};
use upgrade_test_support::{
    BaselineCatalog, BaselineComposition, BaselineDatabase, BaselineRequest, BaselineVersion,
    V0_7_0_PROVENANCE,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    if let Err(error) = run() {
        eprintln!("baseline import: {error}");
        std::process::exit(1);
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn collect(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, (Vec<u8>, bool)>,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-UTF-8 path")?
            .to_owned();
        if !relative
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b))
            || relative
                .split('/')
                .any(|part| matches!(part, "." | ".." | ".git" | "target" | "node_modules"))
            || path.file_name().and_then(|s| s.to_str()) == Some(".env")
            || matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("db" | "sqlite" | "pem" | "key")
            )
        {
            return Err("forbidden source path".into());
        }
        if kind.is_dir() {
            collect(root, &path, files)?;
        } else if kind.is_file() {
            if entry.metadata()?.len() > 4 * 1024 * 1024 {
                return Err("source object is too large".into());
            }
            let bytes = fs::read(&path)?;
            for marker in [
                b"-----BEGIN PRIVATE KEY-----".as_slice(),
                b"-----BEGIN RSA PRIVATE KEY-----",
                b"github_pat_",
                b"ghp_",
            ] {
                if bytes.windows(marker.len()).any(|window| window == marker) {
                    return Err("credential-shaped source content".into());
                }
            }
            #[cfg(unix)]
            let executable = {
                use std::os::unix::fs::PermissionsExt;
                entry.metadata()?.permissions().mode() & 0o111 != 0
            };
            #[cfg(not(unix))]
            let executable = false;
            files.insert(relative, (bytes, executable));
        } else {
            return Err("source contains a symlink or special file".into());
        }
    }
    Ok(())
}

fn validate(files: &BTreeMap<String, (Vec<u8>, bool)>, request: BaselineRequest) -> Result<()> {
    let release = BaselineVersion::V0_7_0.expected_release();
    let manifest: toml::Value =
        toml::from_slice(&files.get("hegira.toml").ok_or("missing manifest")?.0)?;
    let modules: Vec<_> = manifest["composition"]
        .get("modules")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|module| module["id"].as_str())
        .collect();
    let expected_modules = if request.composition == BaselineComposition::Minimal {
        vec![]
    } else {
        vec!["identity"]
    };
    if manifest["schema"].as_integer() != Some(3)
        || manifest["application"].as_str() != Some(&release.application)
        || manifest["framework"]["repository"].as_str() != Some(&release.framework_repository)
        || manifest["framework"]["version"].as_str() != Some(&release.tag)
        || manifest["composition"]["package"]["id"].as_str() != Some(&release.package_id)
        || manifest["composition"]["package"]["version"].as_str() != Some(&release.tag)
        || manifest["selection"]["databases"]
            .as_array()
            .map(|v| v.iter().filter_map(toml::Value::as_str).collect::<Vec<_>>())
            != Some(vec![request.database.as_str()])
        || modules != expected_modules
    {
        return Err("source manifest does not match the released composition".into());
    }
    let lock: toml::Value = toml::from_slice(&files.get("Cargo.lock").ok_or("missing lock")?.0)?;
    let expected = format!(
        "git+{}?tag={}#{}",
        release.framework_repository, release.tag, release.lock_revision
    );
    let sources: Vec<_> = lock["package"]
        .as_array()
        .ok_or("missing packages")?
        .iter()
        .filter_map(|package| package.get("source").and_then(toml::Value::as_str))
        .filter(|source| source.starts_with(&format!("git+{}", release.framework_repository)))
        .collect();
    if sources.is_empty() || sources.iter().any(|source| *source != expected) {
        return Err("source lock has an unauthenticated framework revision".into());
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: import_v0_7_0 <authenticated-render-root> <absent-destination>".into());
    }
    let source = PathBuf::from(&args[0]);
    let destination = PathBuf::from(&args[1]);
    import(&source, &destination)
}

fn import(source: &Path, destination: &Path) -> Result<()> {
    if fs::symlink_metadata(source)?.file_type().is_symlink()
        || fs::symlink_metadata(destination).is_ok()
        || !destination.is_absolute()
        || destination
            .parent()
            .ok_or("missing destination parent")?
            .canonicalize()?
            != destination.parent().unwrap()
    {
        return Err(
            "select a real source and an absent destination under a canonical parent".into(),
        );
    }
    let mut objects = BTreeMap::<String, Vec<u8>>::new();
    let mut trees = BTreeMap::new();
    let mut declarations = String::new();
    let requests: Vec<_> = BaselineComposition::ALL
        .into_iter()
        .flat_map(|composition| {
            BaselineDatabase::ALL
                .into_iter()
                .map(move |database| BaselineRequest::new(composition, database))
        })
        .collect();
    let mut actual = fs::read_dir(source)?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    actual.sort();
    let mut expected: Vec<_> = requests
        .iter()
        .map(|r| std::ffi::OsString::from(r.id()))
        .collect();
    expected.sort();
    if actual != expected {
        return Err("source must contain exactly six baseline directories".into());
    }
    for request in requests {
        let directory = source.join(request.id());
        if !fs::symlink_metadata(&directory)?.is_dir() {
            return Err("invalid baseline directory".into());
        }
        let mut files = BTreeMap::new();
        collect(&directory, &directory, &mut files)?;
        if files.is_empty() || files.len() > 256 {
            return Err("unsupported baseline file count".into());
        }
        validate(&files, request)?;
        let id = request.id();
        let mut tree = format!(
            "schema = 1\nid = \"{id}\"\ncomposition = \"{}\"\ndatabase = \"{}\"\nclient = \"leptos\"\n",
            request.composition.as_str(),
            request.database.as_str()
        );
        for (path, (bytes, executable)) in files {
            let hash = digest(&bytes);
            tree.push_str(&format!(
                "\n[[files]]\npath = \"{path}\"\ndigest = \"{hash}\"\nexecutable = {executable}\n"
            ));
            objects.entry(hash[7..].to_owned()).or_insert(bytes);
        }
        declarations.push_str(&format!("\n[[baselines]]\nid = \"{id}\"\ncomposition = \"{}\"\ndatabase = \"{}\"\nclient = \"leptos\"\ntree = \"trees/{id}.toml\"\ntree_digest = \"{}\"\n", request.composition.as_str(), request.database.as_str(), digest(tree.as_bytes())));
        trees.insert(id, tree);
    }
    if objects.values().map(Vec::len).sum::<usize>() > 16 * 1024 * 1024 {
        return Err("baseline objects exceed the size limit".into());
    }
    let release = BaselineVersion::V0_7_0.expected_release();
    let manifest = format!(
        "schema = 1\ntag = \"{}\"\ntag_object = \"{}\"\ncommit = \"{}\"\nsource_tree = \"{}\"\nframework_repository = \"{}\"\npackage_id = \"{}\"\npackage_version = \"{}\"\npackage_digest = \"{}\"\nlock_revision = \"{}\"\napplication = \"{}\"\nprovenance_digest = \"{}\"\n{}",
        release.tag,
        release.tag_object,
        release.commit,
        release.source_tree,
        release.framework_repository,
        release.package_id,
        release.package_version,
        release.package_digest,
        release.lock_revision,
        release.application,
        digest(V0_7_0_PROVENANCE.as_bytes()),
        declarations
    );
    fs::create_dir(destination)?;
    let publication = (|| -> Result<()> {
        fs::create_dir(destination.join("objects"))?;
        fs::create_dir(destination.join("trees"))?;
        for (hash, bytes) in objects {
            fs::write(destination.join("objects").join(hash), bytes)?;
        }
        for (id, tree) in trees {
            fs::write(destination.join("trees").join(format!("{id}.toml")), tree)?;
        }
        fs::write(destination.join("release.toml"), manifest)?;
        fs::write(destination.join("provenance.toml"), V0_7_0_PROVENANCE)?;
        // A replay may not replace reviewed history by rehashing changed input.
        BaselineCatalog::load_for_release(destination, BaselineVersion::V0_7_0)?;
        Ok(())
    })();
    if publication.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    publication
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = env::temp_dir().join(format!(
                "hegira-baseline-import-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn outputs(&self) -> PathBuf {
            let source = self.0.join("outputs");
            fs::create_dir(&source).unwrap();
            let catalog = catalog();
            for request in catalog.requests() {
                catalog
                    .snapshot(request)
                    .unwrap()
                    .materialize(source.join(request.id()))
                    .unwrap();
            }
            source
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn catalog() -> BaselineCatalog {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        BaselineCatalog::from_repository_release(repository, BaselineVersion::V0_7_0).unwrap()
    }

    #[test]
    fn offline_replay_reproduces_the_authenticated_inventory_and_refuses_overwrite() {
        let fixture = Fixture::new();
        let source = fixture.outputs();
        for repeat in ["first", "second"] {
            let destination = fixture.0.join(repeat);
            import(&source, &destination).unwrap();
            let actual =
                BaselineCatalog::load_for_release(&destination, BaselineVersion::V0_7_0).unwrap();
            let expected = catalog();
            assert_eq!(actual.release(), expected.release());
            for request in expected.requests() {
                assert_eq!(
                    actual.snapshot(request).unwrap(),
                    expected.snapshot(request).unwrap()
                );
            }
            let before = fs::read(destination.join("release.toml")).unwrap();
            assert!(import(&source, &destination).is_err());
            assert_eq!(before, fs::read(destination.join("release.toml")).unwrap());
        }
    }

    #[test]
    fn modified_source_cannot_be_published_as_rehashed_history() {
        let fixture = Fixture::new();
        let source = fixture.outputs();
        let path = source.join("default-sqlite/Cargo.toml");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"\n# owner customization\n");
        fs::write(&path, &bytes).unwrap();
        let destination = fixture.0.join("modified");
        assert!(import(&source, &destination).is_err());
        assert!(!destination.exists());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn runtime_state_and_undeclared_source_entries_fail_before_destination_creation() {
        for relative in [
            "extra",
            "default-sqlite/.env",
            "default-sqlite/local.sqlite",
        ] {
            let fixture = Fixture::new();
            let source = fixture.outputs();
            fs::write(source.join(relative), b"private runtime state").unwrap();
            let destination = fixture.0.join("rejected");
            assert!(import(&source, &destination).is_err());
            assert!(!destination.exists());
        }
        #[cfg(unix)]
        {
            let fixture = Fixture::new();
            let source = fixture.outputs();
            std::os::unix::fs::symlink(
                source.join("default-sqlite/Cargo.toml"),
                source.join("default-sqlite/link"),
            )
            .unwrap();
            assert!(import(&source, &fixture.0.join("symlink")).is_err());
        }
    }
}
