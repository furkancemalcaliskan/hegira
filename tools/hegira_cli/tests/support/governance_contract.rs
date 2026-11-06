//! Compose the separately owned documentation, policy and quality contracts.
use super::{
    APPLICATION_DOCUMENTATION, APPLICATION_POLICY, APPLICATION_QUALITY, TestDirectory, hegira,
    hegira_at, output_tree, path_argument, repository_root,
};
use application_manifest::{ApplicationManifest, DatabaseAdapter, SourceOwnershipClass};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use upgrade_test_support::{
    BaselineCatalog, BaselineComposition, BaselineDatabase, BaselineRequest,
};

fn governance_paths() -> BTreeSet<&'static str> {
    APPLICATION_DOCUMENTATION
        .into_iter()
        .chain(APPLICATION_POLICY)
        .chain(APPLICATION_QUALITY)
        .collect()
}

fn render(parent: &Path, composition: &str, database: &str, name: &str) -> PathBuf {
    let application = parent.join("application");
    let mut arguments = vec![
        "new",
        name,
        "--destination",
        path_argument(&application),
        "--database",
        database,
    ];
    if composition != "default" {
        arguments.extend(["--composition", "minimal"]);
    }
    success(hegira(&arguments));
    if composition == "identity-added" {
        let before = governance_bytes(&application);
        success(hegira_at(&application, &["component", "add", "identity"]));
        assert_eq!(before, governance_bytes(&application));
    }
    application
}

fn governance_bytes(root: &Path) -> BTreeMap<&'static str, Vec<u8>> {
    governance_paths()
        .into_iter()
        .map(|path| (path, fs::read(root.join(path)).unwrap()))
        .collect()
}

fn success(result: Output) -> Output {
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    result
}

fn context(application: &Path) -> Value {
    let manifest = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
    let database = match manifest.selection.databases.iter().next().unwrap() {
        DatabaseAdapter::Sqlite => "sqlite",
        DatabaseAdapter::Postgres => "postgres",
    };
    let profile = if database == "sqlite" {
        "sqlite"
    } else {
        "development"
    };
    let mut plans = BTreeMap::new();
    for operation in ["dev", "check", "test", "build"] {
        let mut args = vec![operation, "--dry-run", "--json"];
        if operation != "dev" {
            args.push("--validation-cache");
        }
        if operation == "build" {
            args.push("--release");
        }
        let output = success(hegira_at(application, &args));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        plans.insert(operation, report["plan"].clone());
    }
    for operation in ["status", "migrate"] {
        let output = success(hegira_at(
            application,
            &["db", operation, "--profile", profile, "--dry-run", "--json"],
        ));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["plan"]["database"], database);
    }
    let server: toml::Value =
        toml::from_str(&fs::read_to_string(application.join("apps/server/Cargo.toml")).unwrap())
            .unwrap();
    json!({ "manifest": manifest, "plans": plans, "server": server })
}

fn composed_check(application: &Path, context: &Value) -> Output {
    let fixture = repository_root().join("tools/hegira_cli/tests/fixtures/governance-contract.mjs");
    let mut child = Command::new("node")
        .arg(fixture)
        .arg(application)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .current_dir(application)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(context).unwrap())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn repository_policy(application: &Path, home: &Path, tool_path: &OsStr) -> Output {
    Command::new("node")
        .args(["scripts/repository-policy.mjs", "repository"])
        .env_clear()
        .env("PATH", tool_path)
        .env("HOME", home)
        .current_dir(application)
        .output()
        .expect("repository policy should run with the selected Node on PATH")
}

#[cfg(unix)]
#[test]
fn repository_policy_uses_selected_node_outside_the_system_path() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let parent = TestDirectory::new("selected-policy-node");
    let application = render(parent.path(), "minimal", "sqlite", "node-selection");
    let before = output_tree(&application);
    let home = TestDirectory::new("selected-node-home");
    let tools = parent.path().join("selected-tools");
    fs::create_dir(&tools).unwrap();
    let installed = success(
        Command::new("node")
            .args(["--print", "process.execPath"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .output()
            .unwrap(),
    );
    let installed = String::from_utf8(installed.stdout).unwrap();
    symlink(installed.trim(), tools.join("actual-node")).unwrap();
    let launcher = tools.join("node");
    fs::write(
        &launcher,
        "#!/bin/sh\nprintf 'selected\\n' > \"$HOME/selected-node\"\nexec \"${0%/*}/actual-node\" \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(launcher, fs::Permissions::from_mode(0o700)).unwrap();

    // No system tool directories are on this PATH. The marker proves that the
    // selected Node runs even on hosts that also have a system Node installed.
    success(repository_policy(
        &application,
        home.path(),
        tools.as_os_str(),
    ));
    assert_eq!(
        fs::read(home.path().join("selected-node")).unwrap(),
        b"selected\n"
    );
    assert_eq!(before, output_tree(&application));
}

#[test]
fn governance_and_quality_form_one_read_only_contract_in_all_six_states() {
    for composition in ["default", "minimal", "identity-added"] {
        for database in ["sqlite", "postgres"] {
            let parent = TestDirectory::new("composed-governance");
            // Generation inside an existing checkout must not touch its metadata.
            success(
                Command::new("git")
                    .args(["-c", "init.defaultBranch=develop", "init", "--quiet"])
                    .arg(parent.path())
                    .env_clear()
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .unwrap(),
            );
            let git_before = output_tree(&parent.path().join(".git"));
            let application = render(parent.path(), composition, database, "route-journal");
            let before = output_tree(&application);
            let manifest = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
            assert_eq!(
                manifest.composition.as_ref().unwrap().modules.is_empty(),
                composition == "minimal"
            );
            let claims = &manifest.upgrade.as_ref().unwrap().ownership.claims;
            for path in governance_paths() {
                let matching: Vec<_> = claims.iter().filter(|claim| claim.path == path).collect();
                assert_eq!(matching.len(), 1, "{composition}/{database}: {path}");
                assert_eq!(matching[0].class, SourceOwnershipClass::GeneratedOnce);
                assert!(matching[0].integration.is_none());
            }
            success(composed_check(&application, &context(&application)));
            // Exercise the delivered entry points together, with an empty home,
            // no inherited credentials, and temporary fixture Git state outside
            // the generated application. These run no real builder/database.
            let environment = TestDirectory::new("governance-environment");
            for args in [
                vec!["sh", "scripts/repository-policy.sh"],
                vec!["node", "--test", "scripts/ci.test.mjs"],
            ] {
                success(
                    Command::new(args[0])
                        .args(&args[1..])
                        .env_clear()
                        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                        .env("HOME", environment.path())
                        .env("TMPDIR", environment.path())
                        .env("GIT_CONFIG_NOSYSTEM", "1")
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .env("GIT_CONFIG_SYSTEM", "/dev/null")
                        .current_dir(&application)
                        .output()
                        .unwrap(),
                );
                assert_eq!(fs::read_dir(environment.path()).unwrap().count(), 0);
            }
            assert_eq!(before, output_tree(&application));
            assert_eq!(git_before, output_tree(&parent.path().join(".git")));
            assert!(!application.join(".git").exists());
            assert!(!application.join("target").exists());
            assert!(!application.join(".hegira-mutation.lock").exists());
        }
    }
}

#[test]
fn composed_contract_rejects_cross_file_drift() {
    let parent = TestDirectory::new("governance-drift");
    let application = render(parent.path(), "minimal", "postgres", "owner-product");
    let context = context(&application);
    success(composed_check(&application, &context));
    for (path, from, to) in [
        (
            "AGENTS.md",
            "release: promote owner-product",
            "release: promote hegira",
        ),
        (
            "docs/repository.md",
            "release: promote owner-product",
            "release: promote hegira",
        ),
        ("docs/development.md", "ssr,db-postgres", "ssr,db-sqlite"),
        (
            "docs/repository.md",
            "Require `repository-policy` and `quality`",
            "Require `repository-policy` and `optional-quality`",
        ),
        ("README.md", "docs/development.md", "docs/missing.md"),
        ("AGENTS.md", "#application-quality-ci", "#missing-heading"),
        ("CLAUDE.md", "@AGENTS.md", "@OTHER.md"),
        (
            "scripts/ci-integration.mjs",
            "provider = \"postgres\"",
            "provider = \"sqlite\"",
        ),
        (
            ".github/repository-policy.json",
            "\"owner-product\"",
            "\"another-product\"",
        ),
        (
            ".github/workflows/quality.yml",
            "test \"$RELEASE_RESULT\" = success",
            "true",
        ),
    ] {
        let original = fs::read_to_string(application.join(path)).unwrap();
        let changed = original.replace(from, to);
        assert_ne!(original, changed, "missing drift fixture: {path}: {from}");
        fs::write(application.join(path), changed).unwrap();
        let result = composed_check(&application, &context);
        fs::write(application.join(path), original).unwrap();
        assert!(!result.status.success(), "accepted drift in {path}: {from}");
    }
}

#[test]
fn public_upgrades_preserve_real_owner_customized_governance_without_adopting_it() {
    let baselines = BaselineCatalog::from_repository_release(
        repository_root(),
        upgrade_test_support::BaselineVersion::V0_7_0,
    )
    .unwrap();
    for composition in BaselineComposition::ALL {
        for database in BaselineDatabase::ALL {
            let parent = TestDirectory::new("governance-upgrade");
            let application = parent.path().join("released");
            baselines
                .snapshot(BaselineRequest::new(composition, database))
                .unwrap()
                .materialize(&application)
                .unwrap();
            let source = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
            let composition_name = match composition {
                BaselineComposition::Default => "default",
                BaselineComposition::Minimal => "minimal",
                BaselineComposition::IdentityAdded => "identity-added",
            };
            let database_name = match database {
                BaselineDatabase::Sqlite => "sqlite",
                BaselineDatabase::Postgres => "postgres",
            };
            let fresh = render(
                parent.path(),
                composition_name,
                database_name,
                &source.application,
            );
            // Adopt only in this disposable application, never in the immutable
            // baseline catalog. Give every artifact a valid owner customization.
            for (path, mut bytes) in governance_bytes(&fresh) {
                if path.ends_with(".json") {
                    let mut policy: Value = serde_json::from_slice(&bytes).unwrap();
                    policy["dependabot"] = json!(true);
                    bytes = serde_json::to_vec_pretty(&policy).unwrap();
                } else {
                    let comment = if path.ends_with(".md") || path.ends_with(".mdc") {
                        "\n<!-- Owner retains this governance source. -->\n"
                    } else if path.ends_with(".mjs") {
                        "\n// Owner retains this governance source.\n"
                    } else {
                        "\n# Owner retains this governance source.\n"
                    };
                    bytes.extend_from_slice(comment.as_bytes());
                }
                fs::create_dir_all(application.join(path).parent().unwrap()).unwrap();
                fs::write(application.join(path), bytes).unwrap();
            }
            // Historical applications predate these generated tool selections.
            // Manual policy adoption includes its required source prerequisites.
            for path in ["rust-toolchain.toml", ".node-version"] {
                fs::copy(fresh.join(path), application.join(path)).unwrap();
            }
            let owned = governance_bytes(&application);
            let before: BTreeMap<_, _> = output_tree(&application).into_iter().collect();
            success(hegira_at(&application, &["upgrade", "status", "--json"]));
            success(hegira_at(&application, &["upgrade", "--dry-run", "--json"]));
            assert_eq!(before, output_tree(&application).into_iter().collect());
            success(hegira_at(&application, &["upgrade", "--json"]));
            assert_eq!(owned, governance_bytes(&application));
            let target = ApplicationManifest::read(application.join("hegira.toml")).unwrap();
            assert!(
                target
                    .upgrade
                    .unwrap()
                    .ownership
                    .claims
                    .iter()
                    .all(|claim| !governance_paths().contains(claim.path.as_str()))
            );
            let after: BTreeMap<_, _> = output_tree(&application).into_iter().collect();
            let changed: Vec<_> = before
                .iter()
                .filter_map(|(path, bytes)| {
                    (after.get(path) != Some(bytes)).then_some(path.to_str().unwrap())
                })
                .collect();
            assert_eq!(before.len() + 1, after.len());
            assert_eq!(
                after
                    .keys()
                    .filter(|path| !before.contains_key(*path))
                    .map(|path| path.to_str().unwrap())
                    .collect::<Vec<_>>(),
                ["crates/infrastructure/src/database_operations.rs"]
            );
            assert_eq!(changed, ["Cargo.lock", "Cargo.toml", "hegira.toml"]);
            // The preserved source remains executable, rather than merely
            // preserving placeholder text in paths with familiar names.
            let environment = TestDirectory::new("upgraded-policy-environment");
            success(repository_policy(
                &application,
                environment.path(),
                &std::env::var_os("PATH").unwrap_or_default(),
            ));
            assert_eq!(after, output_tree(&application).into_iter().collect());
        }
    }
}
