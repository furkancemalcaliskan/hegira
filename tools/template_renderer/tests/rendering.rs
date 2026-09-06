use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use application_manifest::{ApplicationManifest, ClientAdapter, DatabaseAdapter};
use template_renderer::{
    ManifestCatalog, RenderRequest, RendererErrorKind, plan, plan_snapshot, render,
    repository_validation::{RepositoryValidationRequest, render as render_for_validation},
};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[test]
fn stages_verified_generated_bytes_without_mutating_release_source() {
    let repository = repository_root();
    let parent = TestDirectory::new("verified-stage");
    let source = parent.path().join("source");
    render(&canonical_request(&repository, source.clone())).unwrap();
    let before = output_tree(&source);
    let destination = parent.path().join("validation");
    let request = RepositoryValidationRequest {
        render: canonical_request(&repository, destination.clone()),
        framework_root: repository,
        framework_path: Some(PathBuf::from(".hegira-validation/framework")),
    };
    template_renderer::repository_validation::stage_generated(&request, &source).unwrap();
    assert_eq!(before, output_tree(&source));
    let staged = output_tree(&destination);
    assert!(
        fs::read_to_string(destination.join("Cargo.toml"))
            .unwrap()
            .contains("exclude = [\".hegira-validation/framework\"]")
    );
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        staged.keys().collect::<Vec<_>>()
    );
    for (path, bytes) in &before {
        if path.file_name().unwrap() != "Cargo.toml" {
            assert_eq!(bytes, &staged[path], "{}", path.display());
        }
    }
    assert!(
        fs::read_to_string(destination.join("Cargo.toml"))
            .unwrap()
            .contains("path = \".hegira-validation/framework/")
    );
}

#[test]
fn staging_rejects_modified_missing_and_extra_generated_files_before_writes() {
    for mutation in ["modified", "missing", "extra", "empty-directory"] {
        let repository = repository_root();
        let parent = TestDirectory::new(mutation);
        let source = parent.path().join("source");
        render(&canonical_request(&repository, source.clone())).unwrap();
        match mutation {
            "modified" => fs::write(source.join("hegira.toml"), "modified").unwrap(),
            "missing" => fs::remove_file(source.join("hegira.toml")).unwrap(),
            "empty-directory" => fs::create_dir(source.join("unexpected")).unwrap(),
            _ => fs::write(source.join("unexpected"), "unexpected").unwrap(),
        }
        let before = output_tree(&source);
        let destination = parent.path().join("validation");
        let request = RepositoryValidationRequest {
            render: canonical_request(&repository, destination.clone()),
            framework_root: repository,
            framework_path: None,
        };
        let error = template_renderer::repository_validation::stage_generated(&request, &source)
            .unwrap_err();
        assert_eq!(error.kind(), RendererErrorKind::RepositoryValidation);
        assert!(!destination.exists());
        assert_eq!(before, output_tree(&source));
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn staging_rejects_symlinked_generated_content() {
    let repository = repository_root();
    let parent = TestDirectory::new("staging-symlink");
    let source = parent.path().join("source");
    render(&canonical_request(&repository, source.clone())).unwrap();
    let manifest = source.join("hegira.toml");
    let outside = parent.path().join("original");
    fs::rename(&manifest, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &manifest).unwrap();
    let request = RepositoryValidationRequest {
        render: canonical_request(&repository, parent.path().join("validation")),
        framework_root: repository,
        framework_path: None,
    };
    let error =
        template_renderer::repository_validation::stage_generated(&request, &source).unwrap_err();
    assert_eq!(error.kind(), RendererErrorKind::RepositoryValidation);
    assert!(!request.render.output.exists());
    assert!(
        fs::symlink_metadata(manifest)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(outside.is_file());
}

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
fn identical_generation_inputs_render_byte_equivalent_output_trees() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("byte-determinism");
    let first_output = output_parent.path().join("first");
    let second_output = output_parent.path().join("second");

    render(&canonical_request(&repository, first_output.clone()))
        .expect("first render should succeed");
    render(&canonical_request(&repository, second_output.clone()))
        .expect("second render should succeed");

    assert_eq!(
        output_tree(&first_output),
        output_tree(&second_output),
        "identical requests should produce byte-equivalent output trees"
    );
}

#[test]
fn reusable_plan_exposes_components_and_files_before_publication() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("plan-contract");
    let output = output_parent.path().join("application");

    let plan = plan(&canonical_request(&repository, output.clone())).expect("plan should succeed");

    let package = plan
        .package()
        .expect("canonical render should use a package");
    let current_release = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(package.id, "hegira-canonical");
    assert_eq!(package.version, current_release);
    assert_eq!(package.framework.version, current_release);
    assert_eq!(
        plan.components(),
        ["layered-base", "layered-leptos-identity"]
    );
    assert!(plan.files().any(|path| path == Path::new("hegira.toml")));
    assert!(!output.exists());
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
    assert_eq!(
        result.package.as_ref().map(|package| package.id.as_str()),
        Some("hegira-canonical")
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
        [DatabaseAdapter::Sqlite].into_iter().collect()
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
fn package_digest_rejects_untracked_component_content() {
    let repository = repository_root();
    let fixture = TestDirectory::new("package-tamper");
    copy_directory(
        &repository.join("templates"),
        &fixture.path().join("templates"),
    );
    let injected = fixture
        .path()
        .join("templates/applications/layered/config/untracked.yaml");
    fs::write(injected, "untracked = true\n").expect("untracked package input should be written");
    let output = fixture.path().join("application");

    let calculated = ManifestCatalog::calculate_package_digest(fixture.path(), "layered")
        .expect("maintainer tooling should calculate the changed digest");

    let error = render(&canonical_request(fixture.path(), output.clone()))
        .expect_err("changed package content should fail");

    assert_eq!(error.kind(), RendererErrorKind::Catalog);
    assert!(error.to_string().contains("content digest mismatch"));
    assert!(error.to_string().contains(&calculated));
    assert!(!output.exists());
}

#[test]
fn package_framework_identity_cannot_be_overridden() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("package-framework-override");
    let output = output_parent.path().join("application");
    let mut request = canonical_request(&repository, output.clone());
    request
        .variables
        .insert("framework_version".to_string(), "v9.9.9".to_string());

    let error = render(&request).expect_err("package framework version should be immutable");

    assert_eq!(error.kind(), RendererErrorKind::Variables);
    assert!(error.to_string().contains("not declared"));
    assert!(!output.exists());
}

#[test]
fn package_rejects_framework_sources_with_credentials() {
    let repository = repository_root();
    let fixture = TestDirectory::new("package-framework-credentials");
    copy_directory(
        &repository.join("templates"),
        &fixture.path().join("templates"),
    );
    let package_path = fixture.path().join("templates/package.toml");
    let package = fs::read_to_string(&package_path)
        .expect("component package manifest should be readable")
        .replace(
            "https://github.com/furkancemalcaliskan/hegira.git",
            "https://identity@github.com/furkancemalcaliskan/hegira.git",
        );
    fs::write(package_path, package).expect("component package manifest should be updated");
    let output = fixture.path().join("application");

    let error = render(&canonical_request(fixture.path(), output.clone()))
        .expect_err("credentialed framework source should fail");

    assert_eq!(error.kind(), RendererErrorKind::Catalog);
    assert!(error.to_string().contains("invalid framework repository"));
    assert!(!output.exists());
}

#[test]
fn package_and_framework_versions_must_match() {
    let repository = repository_root();
    let fixture = TestDirectory::new("package-version-mismatch");
    copy_directory(
        &repository.join("templates"),
        &fixture.path().join("templates"),
    );
    let package_path = fixture.path().join("templates/package.toml");
    let current_version = format!("version = \"v{}\"", env!("CARGO_PKG_VERSION"));
    let original =
        fs::read_to_string(&package_path).expect("component package manifest should be readable");
    let package = original.replacen(&current_version, "version = \"v9.9.9\"", 1);
    assert_ne!(
        package, original,
        "fixture package version should be replaced"
    );
    fs::write(package_path, package).expect("component package manifest should be updated");
    let output = fixture.path().join("application");

    let error = render(&canonical_request(fixture.path(), output.clone()))
        .expect_err("incompatible package version should fail");

    assert_eq!(error.kind(), RendererErrorKind::Catalog);
    assert!(
        error
            .to_string()
            .contains("does not match framework version")
    );
    assert!(!output.exists());
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
    assert_eq!(error.kind(), RendererErrorKind::ApplicationManifest);
    assert!(!output.exists());
}

#[test]
fn repository_validation_can_patch_framework_dependencies_locally() {
    let repository = repository_root();
    let output_parent = TestDirectory::new("framework-patch");
    let output = output_parent.path().join("application");
    let request = RepositoryValidationRequest {
        render: canonical_request(&repository, output.clone()),
        framework_root: repository.clone(),
        framework_path: None,
    };

    render_for_validation(&request).expect("locally patched render should succeed");

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
    let request = RepositoryValidationRequest {
        render: canonical_request(&repository, output.clone()),
        framework_root: repository,
        framework_path: Some(PathBuf::from(".hegira-validation/framework")),
    };

    render_for_validation(&request).expect("relative framework patch should succeed");

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
    let request = RepositoryValidationRequest {
        render: canonical_request(&repository, output.clone()),
        framework_root: repository.clone(),
        framework_path: Some(PathBuf::from("../framework")),
    };

    let error = render_for_validation(&request).expect_err("unsafe framework path should fail");

    assert!(error.to_string().contains("unsafe component"));
    assert_eq!(error.kind(), RendererErrorKind::RepositoryValidation);
    assert!(
        !error
            .to_string()
            .contains(&repository.to_string_lossy().into_owned())
    );
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
    assert_eq!(error.kind(), RendererErrorKind::ComponentResolution);
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
    assert_eq!(error.kind(), RendererErrorKind::Catalog);
    assert!(
        !error
            .to_string()
            .contains(&fixture.root.path().to_string_lossy().into_owned())
    );
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
    assert!(error.to_string().contains("shared.txt"));
    assert!(error.to_string().contains("alpha"));
    assert!(error.to_string().contains("beta"));
    assert_eq!(error.kind(), RendererErrorKind::Collision);
    assert!(!output.exists());
}

#[test]
fn traversing_component_paths_fail_before_creating_output() {
    let fixture = Fixture::new("path-traversal");
    fixture.write_template("components = [\"feature\"]\n");
    fixture.write_component("feature", "", &[("feature.txt", "feature")]);
    fixture.replace_component_manifest(
        "feature",
        "source = \"applications/test/source\"",
        "source = \"../outside\"",
    );
    let output = fixture.root.path().join("output");

    let error = render(&fixture.request(output.clone())).expect_err("traversal should fail");

    assert_eq!(error.kind(), RendererErrorKind::Catalog);
    assert!(error.to_string().contains("non-traversing relative path"));
    assert!(!output.exists());
}

#[test]
fn absolute_component_paths_fail_before_creating_output() {
    let fixture = Fixture::new("absolute-path");
    fixture.write_template("components = [\"feature\"]\n");
    fixture.write_component("feature", "", &[("feature.txt", "feature")]);
    let absolute =
        toml::Value::String(fixture.root.path().to_string_lossy().into_owned()).to_string();
    fixture.replace_component_manifest(
        "feature",
        "source = \"applications/test/source\"",
        &format!("source = {absolute}"),
    );
    let output = fixture.root.path().join("output");

    let error = render(&fixture.request(output.clone())).expect_err("absolute path should fail");

    assert_eq!(error.kind(), RendererErrorKind::Catalog);
    assert!(error.to_string().contains("non-traversing relative path"));
    assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn symbolic_links_in_component_content_fail_before_creating_output() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("source-symlink");
    fixture.write_template("components = [\"feature\"]\n");
    fixture.write_component("feature", "", &[("safe.txt", "safe")]);
    fixture.replace_component_manifest("feature", "include = [\"safe.txt\"]", "include = [\".\"]");
    let external = fixture.root.path().join("external.txt");
    fs::write(&external, "external").expect("external source should be written");
    symlink(
        &external,
        fixture
            .root
            .path()
            .join("templates/applications/test/source/link.txt"),
    )
    .expect("source symlink should be created");
    let output = fixture.root.path().join("output");

    let error = render(&fixture.request(output.clone())).expect_err("source symlink should fail");

    assert_eq!(error.kind(), RendererErrorKind::Rendering);
    assert!(error.to_string().contains("may not be a symbolic link"));
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
    assert_eq!(error.kind(), RendererErrorKind::Variables);
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
    assert_eq!(error.kind(), RendererErrorKind::Conflict);
    assert_eq!(
        fs::read_to_string(output.join("preserved.txt")).expect("sentinel should remain"),
        "preserved"
    );
}

#[test]
fn normal_renderer_cli_does_not_expose_repository_rewrites() {
    let normal_cli = include_str!("../src/main.rs");
    let validation_adapter = include_str!("../examples/repository_validation_renderer.rs");

    assert!(!normal_cli.contains("--framework-root"));
    assert!(!normal_cli.contains("--framework-path"));
    assert!(validation_adapter.contains("--framework-root"));
}

fn canonical_request(repository: &Path, output: PathBuf) -> RenderRequest {
    RenderRequest {
        repository_root: repository.to_path_buf(),
        template: "layered".to_string(),
        output,
        variables: BTreeMap::new(),
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

    fn replace_component_manifest(&self, id: &str, from: &str, to: &str) {
        let path = self
            .root
            .path()
            .join("templates/components")
            .join(format!("{id}.toml"));
        let manifest = fs::read_to_string(&path).expect("component manifest should be readable");
        assert!(
            manifest.contains(from),
            "fixture manifest should contain replacement source"
        );
        fs::write(path, manifest.replacen(from, to, 1))
            .expect("component manifest should be updated");
    }

    fn request(&self, output: PathBuf) -> RenderRequest {
        RenderRequest {
            repository_root: self.root.path().to_path_buf(),
            template: "test".to_string(),
            output,
            variables: BTreeMap::new(),
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

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("copy destination should be created");
    let mut entries = fs::read_dir(source)
        .expect("copy source should be readable")
        .map(|entry| entry.expect("copy source entry should be readable").path())
        .collect::<Vec<_>>();
    entries.sort();
    for source_entry in entries {
        let destination_entry = destination.join(
            source_entry
                .file_name()
                .expect("copy source entry should have a name"),
        );
        let metadata =
            fs::symlink_metadata(&source_entry).expect("copy source metadata should be readable");
        assert!(
            !metadata.file_type().is_symlink(),
            "test source is a symlink"
        );
        if metadata.is_dir() {
            copy_directory(&source_entry, &destination_entry);
        } else {
            fs::copy(&source_entry, &destination_entry).expect("test source should be copied");
        }
    }
}

fn output_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(directory)
            .expect("rendered directory should be readable")
            .map(|entry| entry.expect("rendered entry should be readable").path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            let metadata = fs::symlink_metadata(&entry).expect("rendered metadata should exist");
            assert!(
                !metadata.file_type().is_symlink(),
                "rendered output must not contain symlinks"
            );
            if metadata.is_dir() {
                collect(root, &entry, files);
            } else {
                let relative = entry
                    .strip_prefix(root)
                    .expect("rendered file should remain inside output")
                    .to_path_buf();
                files.insert(
                    relative,
                    fs::read(entry).expect("rendered file should be readable"),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    collect(root, root, &mut files);
    files
}
