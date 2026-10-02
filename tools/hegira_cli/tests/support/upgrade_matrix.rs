//! Shared public-process assertions for immutable-baseline tests and compile fixtures.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use application_manifest::ApplicationManifest;
use serde_json::Value;

pub fn verify(binary: &Path, root: &Path) {
    let source = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
    let before = files(root);
    let ready = read_only(binary, root, &["upgrade", "status", "--json"], 0);
    assert_eq!(ready["status"], "ready");
    let planned = read_only(binary, root, &["upgrade", "--dry-run", "--json"], 0);
    assert_eq!(planned["outcome"], "planned");
    assert_eq!(planned["assessment"], ready);
    let applied = json(run(binary, root, &["upgrade", "--json"]), 0);
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["plan"], planned["plan"]);
    assert_eq!(applied["receipt"]["changed_files"], 3);
    assert_eq!(applied["receipt"]["target"], applied["plan"]["target"]);

    let target = ApplicationManifest::read(root.join("hegira.toml")).unwrap();
    assert_eq!(target.schema, 3);
    assert_eq!(target.framework.version, "v0.7.0");
    assert_eq!(target.framework.repository, source.framework.repository);
    assert_eq!(target.application, source.application);
    assert_eq!(target.selection, source.selection);
    assert_eq!(
        target.installed_component_ids(),
        source.installed_component_ids()
    );
    let old = source.composition.unwrap();
    let composition = target.composition.as_ref().unwrap();
    assert_eq!(composition.package.id, old.package.id);
    assert_eq!(composition.package.version, "v0.7.0");
    assert_eq!(composition.capabilities, old.capabilities);
    assert_eq!(
        composition
            .modules
            .iter()
            .map(|module| &module.id)
            .collect::<Vec<_>>(),
        old.modules
            .iter()
            .map(|module| &module.id)
            .collect::<Vec<_>>()
    );
    assert!(
        composition
            .components
            .iter()
            .all(|item| item.version == "v0.7.0")
    );
    assert!(
        composition
            .modules
            .iter()
            .all(|item| item.version == "v0.7.0")
    );
    let identity = composition
        .modules
        .iter()
        .any(|module| module.id == "identity");
    assert_eq!(composition.capabilities.len(), if identity { 2 } else { 0 });
    let after = files(root);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        before
            .iter()
            .filter(|(path, bytes)| after.get(*path) != Some(*bytes))
            .map(|(path, _)| path.to_str().unwrap())
            .collect::<Vec<_>>(),
        ["Cargo.lock", "Cargo.toml", "hegira.toml"]
    );
    // Public output remains release-pinned; local compile staging happens later.
    let cargo = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let framework_lines: Vec<_> = cargo
        .lines()
        .filter(|line| line.contains(&format!("git = \"{}\"", target.framework.repository)))
        .collect();
    assert!(!framework_lines.is_empty());
    for line in framework_lines {
        assert!(line.contains("tag = \"v0.7.0\""));
        assert!(!line.contains("path ="));
    }
    let lock = fs::read_to_string(root.join("Cargo.lock")).unwrap();
    assert!(lock.contains(&format!("git+{}?tag=v0.7.0#", target.framework.repository)));
    assert!(!lock.contains(&format!("git+{}?tag=v0.6.0#", target.framework.repository)));

    let inspection = read_only(binary, root, &["inspect", "--json"], 0);
    assert_eq!(inspection["output_schema"], 2);
    assert_eq!(
        inspection["manifest"],
        serde_json::to_value(&target).unwrap()
    );
    assert_eq!(inspection["composition"]["status"], "compatible");
    assert_eq!(inspection["mutation_compatibility"]["status"], "compatible");
    assert_eq!(
        inspection["composition"]["diagnostics"],
        serde_json::json!([])
    );
    let doctor = read_only(binary, root, &["doctor", "--json"], 0);
    assert_eq!(doctor["output_schema"], 1);
    for code in [
        "manifest",
        "composition",
        "recovery-marker",
        "managed-integrations",
    ] {
        let check = doctor["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["code"] == code)
            .unwrap();
        assert_eq!(check["status"], "pass", "{code}: {doctor}");
    }
    // Empty PATH isolates tool prerequisites. They warn, never mask a failed integration.
    assert_eq!(doctor["status"], "warning");
    let resource = run(
        binary,
        root,
        &[
            "generate",
            "resource",
            "MatrixProbe",
            "--field",
            "name:string",
            "--dry-run",
            "--json",
        ],
    );
    if identity {
        assert_eq!(json(resource, 0)["outcome"], "planned");
    } else {
        assert_eq!(resource.status.code(), Some(3));
        assert!(resource.stdout.is_empty());
        let diagnostic: Value = serde_json::from_slice(&resource.stderr).unwrap();
        assert_eq!(diagnostic["code"], "missing-capabilities");
        assert_eq!(
            diagnostic["missing"],
            serde_json::json!(["authentication", "authorization"])
        );
    }
    assert_eq!(after, files(root));
    assert_eq!(
        read_only(binary, root, &["upgrade", "status", "--json"], 0)["status"],
        "no-upgrade"
    );
    assert_eq!(
        read_only(binary, root, &["upgrade", "--dry-run", "--json"], 0)["outcome"],
        "no-upgrade"
    );
    let repeated = read_only(binary, root, &["upgrade", "--json"], 3);
    assert!(repeated["plan"].is_null());
    assert!(repeated.get("receipt").is_none());
    assert_eq!(
        repeated["assessment"]["diagnostics"][0]["code"],
        "direct-edge"
    );
    assert_eq!(after, files(root));
}

fn run(binary: &Path, root: &Path, arguments: &[&str]) -> Output {
    Command::new(binary)
        .args(arguments)
        .current_dir(root)
        .env_clear()
        .env("PATH", "")
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root)
        .output()
        .unwrap()
}

fn json(output: Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn read_only(binary: &Path, root: &Path, arguments: &[&str], exit: i32) -> Value {
    let before = files(root);
    let first = run(binary, root, arguments);
    let second = run(binary, root, arguments);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
    let value = json(first, exit);
    json(second, exit);
    assert_eq!(before, files(root));
    value
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_symlink());
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), output);
            } else {
                output.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}
