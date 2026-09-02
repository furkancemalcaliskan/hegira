use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use application_manifest::{ApplicationManifest, ClientAdapter, DatabaseAdapter};
use template_renderer::{RenderRequest, plan_snapshot, render};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[test]
fn layered_template_snapshot_is_deterministic() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("snapshot");
    let request = canonical_request(&repository, output_parent.path().join("application"));

    let first = plan_snapshot(&request).expect("first render plan should succeed");
    let second = plan_snapshot(&request).expect("second render plan should succeed");

    assert_eq!(first, second);
    assert_eq!(
        first,
        include_str!("snapshots/layered.txt"),
        "canonical template snapshot changed"
    );
}

#[test]
fn layered_template_renders_release_dependencies_and_binary_assets() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("canonical");
    let output = output_parent.path().join("application");
    let result =
        render(&canonical_request(&repository, output.clone())).expect("render should succeed");

    assert_eq!(
        result.components,
        ["layered-base", "layered-leptos-identity"]
    );
    let manifest = fs::read_to_string(output.join("Cargo.toml")).expect("manifest should exist");
    assert!(manifest.contains(
        r#"identity_application = { git = "https://github.com/furkancemalcaliskan/hegira.git", tag = "v0.3.0", default-features = false }"#
    ));
    for compatibility_dependency in [
        "application",
        "application_contracts",
        "domain",
        "domain_shared",
        "infrastructure",
        "presentation",
        "web",
    ] {
        assert!(
            !manifest
                .lines()
                .any(|line| line.starts_with(&format!("{compatibility_dependency} = "))),
            "canonical manifest must not depend on compatibility package `{compatibility_dependency}`"
        );
    }
    for official_dependency in [
        "identity_application",
        "identity_application_contracts",
        "identity_domain",
        "identity_domain_shared",
        "identity_http",
        "identity_leptos",
        "identity_sqlx",
    ] {
        assert!(
            manifest
                .lines()
                .any(|line| line.starts_with(&format!("{official_dependency} = "))),
            "canonical manifest must select official package `{official_dependency}`"
        );
    }
    assert!(!manifest.contains("{{"));
    assert!(!manifest.contains(&repository.to_string_lossy().into_owned()));

    let application_manifest = ApplicationManifest::read(output.join("hegira.toml"))
        .expect("generated application manifest should be valid");
    assert_eq!(application_manifest.application, "application");
    assert_eq!(application_manifest.framework.version, "v0.3.0");
    assert_eq!(
        application_manifest.selection.databases,
        [DatabaseAdapter::Postgres, DatabaseAdapter::Sqlite]
            .into_iter()
            .collect()
    );
    assert_eq!(
        application_manifest.selection.clients,
        [ClientAdapter::Leptos].into_iter().collect()
    );

    let source_logo = repository
        .join("templates/applications/layered/apps/web/src/public/assets/branding/hegira-logo.png");
    let rendered_logo = output.join("apps/web/src/public/assets/branding/hegira-logo.png");
    assert_eq!(
        fs::read(source_logo).expect("source logo should exist"),
        fs::read(rendered_logo).expect("rendered logo should exist")
    );
}

#[test]
fn application_identity_override_is_validated_before_output() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("application-identity");
    let output = output_parent.path().join("application");
    let mut request = canonical_request(&repository, output.clone());
    request.variables.insert(
        "application_name".to_string(),
        "../unsafe-application".to_string(),
    );

    let error = render(&request).expect_err("unsafe application identity should fail");

    assert!(
        error
            .to_string()
            .contains("invalid application manifest name")
    );
    assert!(!output.exists());
}

#[test]
fn repository_validation_can_patch_framework_dependencies_locally() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("framework-patch");
    let output = output_parent.path().join("application");
    let mut request = canonical_request(&repository, output.clone());
    request.framework_root = Some(repository.clone());

    render(&request).expect("locally patched render should succeed");

    let manifest = fs::read_to_string(output.join("Cargo.toml")).expect("manifest should exist");
    let application_path = repository.join("modules/identity/application");
    assert!(manifest.contains(&format!(
        "identity_application = {{ path = {:?}, default-features = false }}",
        application_path.to_string_lossy()
    )));
    assert!(!manifest.contains(" git = "));
}

#[test]
fn repository_validation_can_use_a_safe_relative_framework_path() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("relative-framework-patch");
    let output = output_parent.path().join("application");
    let mut request = canonical_request(&repository, output.clone());
    request.framework_root = Some(repository);
    request.framework_path = Some(PathBuf::from(".hegira-validation/framework"));

    render(&request).expect("relative framework patch should succeed");

    let manifest = fs::read_to_string(output.join("Cargo.toml")).expect("manifest should exist");
    assert!(manifest.contains(
        r#"identity_application = { path = ".hegira-validation/framework/modules/identity/application", default-features = false }"#
    ));
}

#[test]
fn unsafe_relative_framework_paths_are_rejected_before_output() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("unsafe-framework-patch");
    let output = output_parent.path().join("application");
    let mut request = canonical_request(&repository, output.clone());
    request.framework_root = Some(repository);
    request.framework_path = Some(PathBuf::from("../framework"));

    let error = render(&request).expect_err("unsafe framework path should fail");

    assert!(error.to_string().contains("unsafe component"));
    assert!(!output.exists());
}

#[test]
fn missing_component_fails_before_creating_output() {
    let fixture = Fixture::new("missing-component");
    fixture.write_template("components = [\"feature\"]\n");
    fixture.write_component(
        "feature",
        "requires = [\"missing\"]\n",
        &[("feature.txt", "feature")],
    );

    let output = fixture.root.path().join("output");
    let error = render(&fixture.request(output.clone())).expect_err("render should fail");

    assert!(error.to_string().contains("does not exist"));
    assert!(!output.exists());
}

#[test]
fn cyclic_component_requirements_fail_before_creating_output() {
    let fixture = Fixture::new("cycle");
    fixture.write_template("components = [\"alpha\"]\n");
    fixture.write_component(
        "alpha",
        "requires = [\"beta\"]\n",
        &[("alpha.txt", "alpha")],
    );
    fixture.write_component("beta", "requires = [\"alpha\"]\n", &[("beta.txt", "beta")]);

    let output = fixture.root.path().join("output");
    let error = render(&fixture.request(output.clone())).expect_err("render should fail");

    assert!(error.to_string().contains("cycle"));
    assert!(!output.exists());
}

#[test]
fn executable_component_fields_are_rejected() {
    let fixture = Fixture::new("component-script");
    fixture.write_template("components = [\"feature\"]\n");
    fixture.write_component(
        "feature",
        "script = \"run-me.sh\"\n",
        &[("feature.txt", "feature")],
    );

    let output = fixture.root.path().join("output");
    let error = render(&fixture.request(output.clone())).expect_err("render should fail");

    assert!(error.to_string().contains("unknown field"));
    assert!(!output.exists());
}

#[test]
fn conflicting_components_fail_before_creating_output() {
    let fixture = Fixture::new("conflict");
    fixture.write_template("components = [\"alpha\", \"beta\"]\n");
    fixture.write_component(
        "alpha",
        "conflicts = [\"beta\"]\n",
        &[("alpha.txt", "alpha")],
    );
    fixture.write_component("beta", "", &[("beta.txt", "beta")]);

    let output = fixture.root.path().join("output");
    let error = render(&fixture.request(output.clone())).expect_err("render should fail");

    assert!(error.to_string().contains("conflicts"));
    assert!(!output.exists());
}

#[test]
fn output_collisions_fail_before_creating_output() {
    let fixture = Fixture::new("collision");
    fixture.write_template("components = [\"alpha\", \"beta\"]\n");
    fixture.write_component("alpha", "", &[("shared.txt", "alpha")]);
    fixture.write_component("beta", "", &[("shared.txt", "beta")]);

    let output = fixture.root.path().join("output");
    let error = render(&fixture.request(output.clone())).expect_err("render should fail");

    assert!(error.to_string().contains("output collision"));
    assert!(!output.exists());
}

#[test]
fn missing_variables_leave_no_partial_output() {
    let fixture = Fixture::new("missing-variable");
    fixture.write_template("components = [\"feature\"]\n");
    fixture.write_component("feature", "", &[("feature.txt", "value={{missing_value}}")]);

    let output = fixture.root.path().join("output");
    let error = render(&fixture.request(output.clone())).expect_err("render should fail");

    assert!(error.to_string().contains("missing template variable"));
    assert!(!output.exists());
    assert!(
        fs::read_dir(fixture.root.path())
            .expect("fixture root should remain readable")
            .all(|entry| !entry
                .expect("fixture entry should be readable")
                .file_name()
                .to_string_lossy()
                .contains("hegira-render"))
    );
}

#[test]
fn an_existing_output_is_never_overwritten() {
    let fixture = Fixture::new("existing-output");
    fixture.write_template("components = [\"feature\"]\n");
    fixture.write_component("feature", "", &[("feature.txt", "new")]);
    let output = fixture.root.path().join("output");
    fs::create_dir(&output).expect("existing output should be created");
    fs::write(output.join("preserved.txt"), "preserved").expect("sentinel should be written");

    let error = render(&fixture.request(output.clone())).expect_err("render should fail");

    assert!(error.to_string().contains("already exists"));
    assert_eq!(
        fs::read_to_string(output.join("preserved.txt")).expect("sentinel should remain"),
        "preserved"
    );
}

fn canonical_request(repository: &Path, output: PathBuf) -> RenderRequest {
    RenderRequest {
        repository_root: repository.to_path_buf(),
        template: "layered".to_string(),
        output,
        variables: BTreeMap::new(),
        framework_root: None,
        framework_path: None,
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tool should live under the repository tools directory")
        .to_path_buf()
}

struct Fixture {
    root: TestDirectory,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = TestDirectory::new(name);
        fs::create_dir_all(root.path().join("templates/applications/test/source"))
            .expect("template source should be created");
        fs::create_dir_all(root.path().join("templates/components"))
            .expect("component directory should be created");
        Self { root }
    }

    fn write_template(&self, body: &str) {
        let manifest = format!("schema = 1\nid = \"test\"\n{body}");
        fs::write(
            self.root
                .path()
                .join("templates/applications/test/template.toml"),
            manifest,
        )
        .expect("template manifest should be written");
    }

    fn write_component(&self, id: &str, body: &str, files: &[(&str, &str)]) {
        let source = self.root.path().join("templates/applications/test/source");
        let includes = files
            .iter()
            .map(|(path, _)| format!("{path:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            "schema = 1\nid = {id:?}\nsource = \"applications/test/source\"\ninclude = [{includes}]\n{body}"
        );
        fs::write(
            self.root
                .path()
                .join("templates/components")
                .join(format!("{id}.toml")),
            manifest,
        )
        .expect("component manifest should be written");
        for (path, content) in files {
            let target = source.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).expect("fixture parent should be created");
            }
            fs::write(target, content).expect("component source should be written");
        }
    }

    fn request(&self, output: PathBuf) -> RenderRequest {
        RenderRequest {
            repository_root: self.root.path().to_path_buf(),
            template: "test".to_string(),
            output,
            variables: BTreeMap::new(),
            framework_root: None,
            framework_path: None,
        }
    }
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "hegira-template-renderer-{name}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir(&path).expect("test directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
