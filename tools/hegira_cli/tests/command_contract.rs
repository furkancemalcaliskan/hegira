use std::{
    ffi::OsString,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn hegira(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(arguments)
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("hegira command should run")
}

#[test]
fn top_level_help_is_human_readable_output() {
    let result = hegira(&["--help"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("help should be UTF-8");
    assert!(output.starts_with("Create and maintain Hegira applications"));
    assert!(output.contains("Usage: hegira <COMMAND>"));
    assert!(output.contains("new"));
}

#[test]
fn version_is_human_readable_output() {
    let result = hegira(&["--version"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    assert_eq!(
        String::from_utf8(result.stdout).expect("version should be UTF-8"),
        format!("hegira {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn new_help_succeeds_without_accessing_global_configuration() {
    let result = hegira(&["new", "--help"]);

    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    let output = String::from_utf8(result.stdout).expect("help should be UTF-8");
    assert!(output.contains("Usage: hegira new"));
}

#[test]
fn invalid_usage_is_reported_only_on_standard_error() {
    let result = hegira(&["unknown"]);

    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    let diagnostic = String::from_utf8(result.stderr).expect("diagnostic should be UTF-8");
    assert!(diagnostic.contains("unrecognized subcommand 'unknown'"));
    assert!(diagnostic.contains("Usage: hegira <COMMAND>"));
    assert!(!diagnostic.contains("stack backtrace"));
}

#[test]
fn new_requires_an_application_name_and_destination() {
    let result = hegira(&["new"]);

    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    let diagnostic = String::from_utf8(result.stderr).expect("diagnostic should be UTF-8");
    assert!(
        diagnostic.contains("non-interactive application creation requires a name and destination")
    );
    assert!(diagnostic.contains("provide `hegira new <NAME> --destination <PATH>`"));
}

#[test]
fn interactive_defaults_match_explicit_default_generation() {
    let output = TestDirectory::new("interactive-defaults");
    let guided_destination = output.path().join("guided");
    let explicit_destination = output.path().join("explicit");
    let input = format!("guided-app\n{}\n\n\n\n\n", guided_destination.display());

    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], &input);
    assert_eq!(exit, 0, "{diagnostics}");
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Application name: "));
    assert!(stdout.contains("Database [sqlite] (sqlite/postgres): "));
    assert!(stdout.contains("Application summary:"));
    assert!(stdout.contains("Database: sqlite"));

    let result = hegira(&[
        "new",
        "guided-app",
        "--destination",
        path_argument(&explicit_destination),
    ]);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(
        output_tree(&guided_destination),
        output_tree(&explicit_destination)
    );
}

#[test]
fn interactive_selection_reprompts_and_maps_to_supported_values() {
    let output = TestDirectory::new("interactive-postgres");
    let destination = output.path().join("ApplicationOutput");
    let input = format!(
        "guided-postgres\n{}\nmysql\nPostgres\nunknown\nLeptos\nother\nIdentity\nyes\n",
        destination.display()
    );

    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], &input);

    assert_eq!(exit, 0, "{diagnostics}");
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Please choose `sqlite` or `postgres`."));
    assert!(stdout.contains("The currently supported client is `leptos`."));
    assert!(stdout.contains("The currently supported component is `identity`."));
    assert!(stdout.contains("Destination: "));
    assert!(stdout.contains("Database: postgres"));
    let manifest = fs::read_to_string(destination.join("hegira.toml"))
        .expect("application manifest should exist");
    assert!(manifest.contains("databases = [\"postgres\"]"));
}

#[test]
fn interactive_cancellation_leaves_no_output() {
    let output = TestDirectory::new("interactive-cancel");
    let destination = output.path().join("cancelled");
    let input = format!("cancelled-app\n{}\n\n\n\nn\n", destination.display());

    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], &input);

    assert_eq!(exit, 0);
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Application summary:"));
    assert!(stdout.contains("Cancelled; no files were written."));
    assert!(!destination.exists());
}

#[test]
fn interactive_end_of_input_cancels_safely() {
    let (exit, stdout, diagnostics) = interactive_hegira(&["new"], "");

    assert_eq!(exit, 0);
    assert!(diagnostics.is_empty());
    assert!(stdout.contains("Application name: "));
    assert!(stdout.contains("Cancelled; no files were written."));
}

#[test]
fn new_generates_the_default_layered_application_without_prompts() {
    let output = TestDirectory::new("default");
    let destination = output.path().join("application");
    let result = hegira(&[
        "new",
        "my-application",
        "--destination",
        path_argument(&destination),
    ]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("output should be UTF-8");
    assert!(stdout.contains("Created my-application"));
    assert!(stdout.contains("APP_ENV=sqlite cargo leptos watch"));

    let manifest = fs::read_to_string(destination.join("hegira.toml"))
        .expect("application manifest should exist");
    assert!(manifest.contains("application = \"my-application\""));
    assert!(manifest.contains("databases = [\"sqlite\"]"));
    assert!(manifest.contains("clients = [\"leptos\"]"));
    assert!(manifest.contains("\"layered-leptos-identity\""));

    let server_manifest = fs::read_to_string(destination.join("apps/server/Cargo.toml"))
        .expect("server manifest should exist");
    let workspace_manifest = fs::read_to_string(destination.join("Cargo.toml"))
        .expect("workspace manifest should exist");
    assert!(server_manifest.contains("default = [\"db-sqlite\"]"));
    assert!(workspace_manifest.contains("git = \"https://github.com/"));
    assert!(workspace_manifest.contains("tag = \"v0.3.0\""));
    assert!(!workspace_manifest.contains(repository_root().to_string_lossy().as_ref()));
}

#[test]
fn new_represents_an_explicit_postgres_selection_consistently() {
    let output = TestDirectory::new("postgres");
    let destination = output.path().join("application");
    let result = hegira(&[
        "new",
        "postgres-app",
        "--destination",
        path_argument(&destination),
        "--database",
        "postgres",
        "--client",
        "leptos",
        "--component",
        "identity",
    ]);

    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty());
    let stdout = String::from_utf8(result.stdout).expect("output should be UTF-8");
    assert!(stdout.contains("APP_ENV=development cargo leptos watch"));
    assert!(stdout.contains("--bin-features ssr,db-postgres"));

    let manifest = fs::read_to_string(destination.join("hegira.toml"))
        .expect("application manifest should exist");
    assert!(manifest.contains("databases = [\"postgres\"]"));
    let server_manifest = fs::read_to_string(destination.join("apps/server/Cargo.toml"))
        .expect("server manifest should exist");
    let web_manifest = fs::read_to_string(destination.join("apps/web/Cargo.toml"))
        .expect("web manifest should exist");
    assert!(server_manifest.contains("default = [\"db-postgres\"]"));
    assert!(web_manifest.contains("default = [\"db-postgres\"]"));
}

#[test]
fn identical_requests_create_byte_equivalent_output_trees() {
    let output = TestDirectory::new("deterministic");
    let first = output.path().join("first");
    let second = output.path().join("second");

    for destination in [&first, &second] {
        let result = hegira(&[
            "new",
            "deterministic-app",
            "--destination",
            path_argument(destination),
        ]);
        assert!(result.status.success(), "{:?}", result.stderr);
    }

    assert_eq!(output_tree(&first), output_tree(&second));
}

#[test]
fn existing_destination_is_a_conflict_and_is_not_modified() {
    let output = TestDirectory::new("conflict");
    let destination = output.path().join("application");
    fs::create_dir(&destination).expect("destination fixture should exist");
    fs::write(destination.join("preserved.txt"), "preserved").expect("sentinel should be written");

    let result = hegira(&[
        "new",
        "my-application",
        "--destination",
        path_argument(&destination),
    ]);

    assert_eq!(result.status.code(), Some(4));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("destination already exists"));
    assert_eq!(
        fs::read_to_string(destination.join("preserved.txt")).expect("sentinel should remain"),
        "preserved"
    );
}

fn path_argument(path: &Path) -> &str {
    path.to_str().expect("test path should be UTF-8")
}

#[test]
fn adversarial_identities_fail_before_creating_parent_or_output() {
    let root = TestDirectory::new("invalid-identities");
    let destination = root.path().join("missing/application");
    for name in [
        "../escape",
        "/absolute",
        "BadName",
        "naïve",
        "app\nname",
        "app\u{1b}name",
        "con",
        "aux",
        "com1",
        "type",
        "self",
        "gen",
        "target",
        "a--b",
    ] {
        let result = hegira(&["new", name, "--destination", path_argument(&destination)]);
        assert_eq!(
            result.status.code(),
            Some(3),
            "{name:?}: {:?}",
            result.stderr
        );
        assert!(result.stdout.is_empty());
        assert!(!root.path().join("missing").exists());
    }
}

#[test]
fn adversarial_destinations_leave_user_data_unchanged() {
    let root = TestDirectory::new("invalid-destinations");
    fs::write(root.path().join("sentinel"), "preserved").unwrap();
    for path in [
        "child/../escape",
        "missing/application",
        "CON",
        "aux",
        "com1",
        "foo.",
        "foo/.",
        "naïve",
        "app\nname",
        "app\\name",
    ] {
        let destination = root.path().join(path);
        let result = hegira(&[
            "new",
            "safe-app",
            "--destination",
            path_argument(&destination),
        ]);
        assert_eq!(
            result.status.code(),
            Some(3),
            "{path:?}: {:?}",
            result.stderr
        );
        assert!(result.stdout.is_empty());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    assert_eq!(
        fs::read_to_string(root.path().join("sentinel")).unwrap(),
        "preserved"
    );
}

#[test]
fn interactive_identity_is_validated_without_silent_normalization() {
    let root = TestDirectory::new("interactive-invalid");
    let destination = root.path().join("application");
    let input = format!(" bad-name \n{}\n\n\n\ny\n", destination.display());
    let (exit, _, _) = interactive_hegira(&["new"], &input);
    assert_eq!(exit, 3);
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn non_utf8_destination_is_rejected_before_writing() {
    use std::os::unix::ffi::OsStringExt;
    let root = TestDirectory::new("non-utf8");
    let destination = root.path().join(OsString::from_vec(vec![b'a', 0xff]));
    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(["new", "safe-app", "--destination"])
        .arg(destination)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(3));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn explicit_sibling_destination_still_works() {
    let root = TestDirectory::new("sibling");
    let cwd = root.path().join("caller");
    fs::create_dir(&cwd).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .current_dir(cwd)
        .args(["new", "sibling-app", "--destination", "../application"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(root.path().join("application/hegira.toml").is_file());
}

fn interactive_hegira(arguments: &[&str], input: &str) -> (u8, String, String) {
    let arguments = std::iter::once(OsString::from("hegira"))
        .chain(arguments.iter().map(OsString::from))
        .collect::<Vec<_>>();
    let mut input = Cursor::new(input.as_bytes());
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    let exit =
        hegira_cli::run_interactive_from(arguments, &mut input, &mut output, &mut diagnostics);
    (
        exit.code(),
        String::from_utf8(output).expect("interactive output should be UTF-8"),
        String::from_utf8(diagnostics).expect("interactive diagnostics should be UTF-8"),
    )
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI should live under tools")
        .to_path_buf()
}

fn output_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(root: &Path, directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries = fs::read_dir(directory)
            .expect("output directory should be readable")
            .map(|entry| entry.expect("output entry should be readable").path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                collect(root, &entry, files);
            } else {
                files.push((
                    entry
                        .strip_prefix(root)
                        .expect("output should remain under root")
                        .to_path_buf(),
                    fs::read(&entry).expect("output file should be readable"),
                ));
            }
        }
    }

    let mut files = Vec::new();
    collect(root, root, &mut files);
    files
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "hegira-cli-{name}-{}-{sequence}",
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
