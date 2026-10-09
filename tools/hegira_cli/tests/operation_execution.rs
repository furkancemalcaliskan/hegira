#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use hegira_cli::{
    ApplicationContextRequest,
    operations::{
        OperationIntent, OperationPlan, OperationRequest, RuntimeProfile,
        execution::{
            ChildOutput, DatabaseExecutionApproval, ExecutionConsent, ExecutionControl,
            ExecutionOutcome, TrustedToolchain, execute_application_operation,
            execute_database_operation,
        },
        plan_application_operation,
    },
};
use rustix::process::{Pid, Signal, WaitOptions, kill_process, kill_process_group, waitpid};
use template_renderer::{RenderRequest, render};

#[path = "support/operation_contract.rs"]
mod operation_contract;

static NEXT: AtomicU64 = AtomicU64::new(0);
static HELPER: Mutex<Weak<Helper>> = Mutex::new(Weak::new());

struct Helper(PathBuf);
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn helper() -> Arc<Helper> {
    let mut weak = HELPER.lock().unwrap();
    if let Some(helper) = weak.upgrade() {
        return helper;
    }
    let helper = Arc::new(Helper(temporary("helper")));
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/operation_child.rs");
    let result = Command::new("rustc")
        .args(["--edition=2024", "-C", "debuginfo=0"])
        .arg(source)
        .arg("-o")
        .arg(helper.0.join("cargo"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    *weak = Arc::downgrade(&helper);
    helper
}

fn temporary(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "hegira-operation-execution-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}

struct Fixture {
    parent: PathBuf,
    root: PathBuf,
    tools: PathBuf,
    _helper: Arc<Helper>,
}

impl Fixture {
    fn new() -> Self {
        let parent = temporary("application");
        // Shell metacharacters are valid path bytes, never command syntax.
        let destination_parent = parent.join("workspace ; $(unintended-command)");
        fs::create_dir(&destination_parent).unwrap();
        let root = destination_parent.join("application");
        render(&RenderRequest {
            repository_root: repository(),
            template: "layered".to_owned(),
            output: root.clone(),
            components: None,
            variables: [("application_name".to_owned(), "execution-app".to_owned())].into(),
        })
        .unwrap();
        let tools = parent.join("tools");
        fs::create_dir(&tools).unwrap();
        let helper = helper();
        fs::copy(helper.0.join("cargo"), tools.join("cargo")).unwrap();
        fs::write(root.join("child-mode"), "success").unwrap();
        Self {
            parent,
            root,
            tools,
            _helper: helper,
        }
    }

    fn plan(&self, intent: OperationIntent) -> OperationPlan {
        plan_application_operation(
            &repository(),
            &OperationRequest {
                application: ApplicationContextRequest::discover_from(&self.root),
                intent,
            },
        )
        .unwrap()
    }

    fn toolchain(&self) -> TrustedToolchain {
        TrustedToolchain::resolve(&self.tools.join("cargo"), std::slice::from_ref(&self.tools))
            .unwrap()
    }

    fn execute(
        &self,
        plan: &OperationPlan,
        control: &ExecutionControl,
    ) -> Result<
        hegira_cli::operations::execution::ExecutionReport,
        hegira_cli::operations::OperationError,
    > {
        execute_application_operation(
            &repository(),
            plan,
            ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
            &self.toolchain(),
            control,
            ChildOutput::Discard,
        )
    }

    fn mode(&self, mode: &str) {
        fs::write(self.root.join("child-mode"), mode).unwrap();
    }

    fn public_command(&self, operation: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hegira"));
        command
            .arg(operation)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "")
            .stdin(Stdio::null());
        if operation == "build" {
            command.arg("--release");
        }
        command
    }

    fn public_execute(&self, operation: &str, json: bool) -> Command {
        let mut command = self.public_command(operation);
        command
            .args(["--execute", "--trust-application", "--cargo"])
            .arg(self.tools.join("cargo"))
            .arg("--tool-directory")
            .arg(&self.tools);
        if operation == "dev" || operation == "build" {
            command
                .arg("--wasm-bindgen")
                .arg(self.tools.join("wasm-bindgen"));
        }
        if operation == "build" {
            command.arg("--wasm-opt").arg(self.tools.join("wasm-opt"));
        }
        if json {
            command.arg("--json");
        }
        command
    }

    fn database_command(
        &self,
        operation: &str,
        profile: &str,
        execute: bool,
        json: bool,
    ) -> Command {
        let mut command = self.public_command("db");
        command.args([operation, "--profile", profile]);
        if execute {
            command
                .args(["--execute", "--trust-application", "--cargo"])
                .arg(self.tools.join("cargo"))
                .arg("--tool-directory")
                .arg(&self.tools);
        } else {
            command.arg("--dry-run");
        }
        if json {
            command.arg("--json");
        }
        command
    }

    fn development_tools(&self, root: &Path) {
        for name in ["cargo-leptos", "rustc", "node", "wasm-bindgen", "wasm-opt"] {
            fs::copy(self.tools.join("cargo"), self.tools.join(name)).unwrap();
        }
        let target = self.tools.join("wasm-target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("libcore-fixture.rlib"), "controlled target").unwrap();
        fs::write(root.join("probe-target"), target.to_str().unwrap()).unwrap();
        let rust: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("rust-toolchain.toml")).unwrap()).unwrap();
        fs::write(
            root.join("probe-rust-version"),
            rust["toolchain"]["channel"].as_str().unwrap(),
        )
        .unwrap();
        let lock: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("Cargo.lock")).unwrap()).unwrap();
        let wasm = lock["package"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"].as_str() == Some("wasm-bindgen"))
            .unwrap();
        fs::write(
            root.join("probe-wasm-version"),
            wasm["version"].as_str().unwrap(),
        )
        .unwrap();
        let lock: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("apps/web/src/package-lock.json")).unwrap())
                .unwrap();
        let version = lock["packages"]["node_modules/@tailwindcss/cli"]["version"]
            .as_str()
            .unwrap();
        fs::write(root.join("probe-tailwind-version"), version).unwrap();
        let modules = root.join("apps/web/src/node_modules");
        let cli = modules.join("@tailwindcss/cli");
        fs::create_dir_all(cli.join("dist")).unwrap();
        fs::write(
            modules.join(".package-lock.json"),
            serde_json::to_vec(&lock).unwrap(),
        )
        .unwrap();
        fs::write(cli.join("package.json"), serde_json::to_vec(&serde_json::json!({"name":"@tailwindcss/cli", "version":version, "bin":{"tailwindcss":"./dist/index.mjs"}})).unwrap()).unwrap();
        let script = cli.join("dist/index.mjs");
        fs::write(
            &script,
            "#!/usr/bin/env node\n// Controlled fixture, never a real compiler.\n",
        )
        .unwrap();
        fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
        // A general npm tool directory must never enter the trusted PATH.
        fs::create_dir(modules.join(".bin")).unwrap();
        fs::write(modules.join(".bin/cargo"), "untrusted fixture").unwrap();
    }

    fn doctor_tools(&self, root: &Path) {
        self.development_tools(root);
        for (source, target) in [
            ("probe-rust-version", "doctor-rust-version"),
            ("probe-wasm-version", "doctor-wasm-version"),
            ("probe-target", "doctor-target"),
        ] {
            fs::copy(root.join(source), self.tools.join(target)).unwrap();
        }
    }

    fn doctor(&self, root: &Path, operation: &str, probe: bool, database: &str) -> Command {
        let mut command = self.public_command("doctor");
        command
            .current_dir(root)
            .args(["--operation", operation, "--json"]);
        if operation.starts_with("database-") {
            command.args([
                "--profile",
                if database == "sqlite" {
                    "sqlite"
                } else {
                    "development"
                },
            ]);
        }
        if probe {
            command
                .arg("--probe-tools")
                .arg("--cargo")
                .arg(self.tools.join("cargo"))
                .arg("--tool-directory")
                .arg(&self.tools)
                .arg("--wasm-bindgen")
                .arg(self.tools.join("wasm-bindgen"))
                .arg("--wasm-opt")
                .arg(self.tools.join("wasm-opt"));
        }
        command
            .env(
                "APP__DATABASE__URL",
                "doctor-runtime-secret-must-not-appear",
            )
            .env(
                "APP__SECURITY__JWT_SECRET",
                "doctor-runtime-secret-must-not-appear",
            )
            .env("NODE_OPTIONS", "--require=untrusted-application-preload")
            .env("RUSTC_WRAPPER", "untrusted-application-hook");
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}

fn public_json(output: &Output, code: i32) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(code), "{:?}", output);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["output_schema"], 1);
    assert_eq!(value.as_object().unwrap().len(), 5);
    value
}

// Signal tests must not strand a controlled command even if an assertion panics.
// Reap the CLI; it owns normal group cleanup. Kill its observed group only as a
// bounded last resort while the CLI is still alive (never reuse a stale PID).
struct ControlledCommand {
    child: Option<Child>,
    leader: Option<Pid>,
}

impl ControlledCommand {
    fn spawn(mut command: Command) -> Self {
        Self {
            child: Some(
                command
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap(),
            ),
            leader: None,
        }
    }

    fn pid(&self) -> Pid {
        Pid::from_raw(self.child.as_ref().unwrap().id().try_into().unwrap()).unwrap()
    }

    fn output(mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.child.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "controlled CLI did not finish");
            thread::sleep(Duration::from_millis(10));
        }
        self.child.take().unwrap().wait_with_output().unwrap()
    }
}

impl Drop for ControlledCommand {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        if let Some(pid) = Pid::from_raw(child.id().try_into().unwrap()) {
            let _ = kill_process(pid, Signal::TERM);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if let Some(leader) = self.leader {
            let _ = kill_process_group(leader, Signal::KILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn doctor_json(output: &Output, code: i32) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["output_schema"], 1);
    assert_eq!(value.as_object().unwrap().len(), 4);
    for forbidden in [
        "doctor-runtime-secret-must-not-appear",
        "private-tool-output-must-not-appear",
        "untrusted-application",
    ] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(forbidden));
    }
    value
}

fn doctor_status(report: &serde_json::Value, code: &str) -> String {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["code"] == code)
        .unwrap_or_else(|| panic!("missing {code}: {report}"))
        .get("status")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn operation_doctor_covers_six_compositions_and_every_intent_without_application_writes() {
    let fixture = Fixture::new();
    for composition in ["default", "minimal", "identity-added"] {
        for database in ["sqlite", "postgres"] {
            let root = fixture
                .parent
                .join(format!("doctor-{composition}-{database}"));
            let mut create = Command::new(env!("CARGO_BIN_EXE_hegira"));
            create
                .args(["new", "doctor-operation-app", "--destination"])
                .arg(&root)
                .args(["--database", database]);
            if composition != "default" {
                create.args(["--composition", "minimal"]);
            }
            assert!(create.output().unwrap().status.success());
            if composition == "identity-added" {
                assert!(
                    Command::new(env!("CARGO_BIN_EXE_hegira"))
                        .args(["component", "add", "identity"])
                        .current_dir(&root)
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
            fixture.doctor_tools(&root);
            fs::write(
                root.join("config/production.yaml"),
                "doctor-runtime-secret-must-not-appear",
            )
            .unwrap();
            let before = application_source_tree(&root);
            for operation in [
                "dev",
                "check",
                "test",
                "build",
                "database-status",
                "database-migrate",
            ] {
                let exit = 0;
                let first = fixture
                    .doctor(&root, operation, true, database)
                    .output()
                    .unwrap();
                let second = fixture
                    .doctor(&root, operation, true, database)
                    .output()
                    .unwrap();
                assert_eq!(first.stdout, second.stdout);
                let report = doctor_json(&first, exit);
                assert_eq!(doctor_status(&report, "operation-rust"), "pass");
                assert_eq!(doctor_status(&report, "operation-cargo"), "pass");
                assert!(!String::from_utf8_lossy(&first.stdout).contains(root.to_str().unwrap()));
                if operation == "dev" || operation == "build" {
                    for code in [
                        "operation-node",
                        "operation-cargo-leptos",
                        "operation-wasm-bindgen",
                        "operation-wasm-target",
                        "operation-frontend-state",
                        "operation-public-assets",
                    ] {
                        assert_eq!(doctor_status(&report, code), "pass");
                    }
                    assert_eq!(
                        doctor_status(&report, "operation-frontend-execution"),
                        "warning"
                    );
                }
                if operation == "build" {
                    assert_eq!(doctor_status(&report, "operation-wasm-opt"), "pass");
                    assert_eq!(doctor_status(&report, "operation-release-output"), "pass");
                }
                if operation.starts_with("database-") {
                    assert_eq!(
                        doctor_status(&report, "operation-database-entry-point"),
                        "pass"
                    );
                }
                assert!(!root.join("target").exists());
                assert!(!root.join("child.pid").exists());
                assert!(!root.join("proxy.log").exists());
                assert_eq!(application_source_tree(&root), before);
            }
        }
    }
    let probes = fs::read_to_string(fixture.tools.join("doctor-probes.log")).unwrap();
    assert!(probes.contains("cargo: [\"--version\"]"));
    assert!(!probes.contains("metadata"));
    assert!(!probes.contains("build"));
    assert!(!probes.contains("--help"));
}

#[test]
fn operation_doctor_without_probe_consent_is_read_only_and_tool_free() {
    let fixture = Fixture::new();
    fixture.doctor_tools(&fixture.root);
    let before = application_source_tree(&fixture.root);
    for operation in [
        "dev",
        "check",
        "test",
        "build",
        "database-status",
        "database-migrate",
    ] {
        let output = fixture
            .doctor(&fixture.root, operation, false, "sqlite")
            .output()
            .unwrap();
        let report = doctor_json(&output, 0);
        assert_eq!(doctor_status(&report, "operation-cargo"), "warning");
    }
    assert!(!fixture.tools.join("doctor-probes.log").exists());
    assert!(!fixture.root.join("target").exists());
    assert_eq!(application_source_tree(&fixture.root), before);
    // The fixture helper always adds JSON; construct the human variant directly.
    let mut human = fixture.public_command("doctor");
    let output = human.args(["--operation", "build"]).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("[WARN] operation-cargo"));
}

#[test]
fn operation_doctor_reports_tool_mismatches_missing_assets_and_frontend_receipts() {
    let fixture = Fixture::new();
    fixture.doctor_tools(&fixture.root);
    for (mode, code) in [
        ("cargo", "operation-cargo"),
        ("rustc", "operation-rust"),
        ("node", "operation-node"),
        ("leptos", "operation-cargo-leptos"),
        ("wasm", "operation-wasm-bindgen"),
        ("optimizer", "operation-wasm-opt"),
        ("oversize", "operation-rust"),
    ] {
        fs::write(fixture.tools.join("doctor-mode"), mode).unwrap();
        let report = doctor_json(
            &fixture
                .doctor(&fixture.root, "build", true, "sqlite")
                .output()
                .unwrap(),
            if mode == "oversize" { 3 } else { 0 },
        );
        assert_eq!(doctor_status(&report, code), "warning");
    }
    fs::remove_file(fixture.tools.join("doctor-mode")).unwrap();
    for (name, code) in [
        ("cargo", "operation-cargo"),
        ("node", "operation-node"),
        ("cargo-leptos", "operation-cargo-leptos"),
        ("wasm-bindgen", "operation-wasm-bindgen"),
        ("wasm-opt", "operation-wasm-opt"),
    ] {
        let file = fixture.tools.join(name);
        let original = fs::read(&file).unwrap();
        fs::remove_file(&file).unwrap();
        let report = doctor_json(
            &fixture
                .doctor(&fixture.root, "build", true, "sqlite")
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(doctor_status(&report, code), "warning");
        fs::write(&file, original).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::remove_file(fixture.tools.join("wasm-target/libcore-fixture.rlib")).unwrap();
    let report = doctor_json(
        &fixture
            .doctor(&fixture.root, "check", true, "sqlite")
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(doctor_status(&report, "operation-wasm-target"), "warning");
    for name in [
        "apps/web/src/style/main.css",
        "apps/web/src/node_modules/.package-lock.json",
    ] {
        let original = fs::read(fixture.root.join(name)).unwrap();
        fs::remove_file(fixture.root.join(name)).unwrap();
        let report = doctor_json(
            &fixture
                .doctor(&fixture.root, "build", false, "sqlite")
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(
            doctor_status(&report, "operation-frontend-state"),
            "warning"
        );
        fs::write(fixture.root.join(name), original).unwrap();
    }
    let receipt = fixture
        .root
        .join("apps/web/src/node_modules/.package-lock.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    value["packages"]["node_modules/@tailwindcss/cli"]["version"] = "0.0.0".into();
    fs::write(receipt, serde_json::to_vec(&value).unwrap()).unwrap();
    let report = doctor_json(
        &fixture
            .doctor(&fixture.root, "build", false, "sqlite")
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(
        doctor_status(&report, "operation-frontend-state"),
        "warning"
    );
    assert!(!fixture.root.join("target").exists());
    assert!(!fixture.root.join("child.pid").exists());
}

#[test]
fn operation_doctor_invalid_composition_profile_and_recovery_never_probe_tools() {
    let _helper = helper();
    for mode in [
        "manifest",
        "provider",
        "client",
        "profile",
        "file",
        "directory",
        "symlink",
    ] {
        let fixture = Fixture::new();
        fixture.doctor_tools(&fixture.root);
        let marker = fixture.root.join(application_mutator::MUTATION_MARKER);
        match mode {
            "manifest" => fs::write(
                fixture.root.join("hegira.toml"),
                "invalid = doctor-private-source",
            )
            .unwrap(),
            "provider" => {
                let path = fixture.root.join("hegira.toml");
                let source = fs::read_to_string(&path).unwrap();
                fs::write(path, source.replace("sqlite", "unsupported-provider")).unwrap();
            }
            "client" => {
                let path = fixture.root.join("hegira.toml");
                let source = fs::read_to_string(&path).unwrap();
                fs::write(path, source.replace("leptos", "unsupported-client")).unwrap();
            }
            "profile" => {}
            "file" => fs::write(&marker, "doctor-private-source").unwrap(),
            "directory" => fs::create_dir(&marker).unwrap(),
            _ => symlink(fixture.parent.join("private-missing-target"), &marker).unwrap(),
        }
        let mut command = fixture.doctor(
            &fixture.root,
            if mode == "profile" {
                "database-status"
            } else {
                "check"
            },
            true,
            if mode == "profile" {
                "postgres"
            } else {
                "sqlite"
            },
        );
        let output = command.output().unwrap();
        let report = doctor_json(&output, 3);
        assert_eq!(report["status"], "failure");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("doctor-private-source"));
        assert!(!fixture.tools.join("doctor-probes.log").exists());
        if matches!(mode, "file" | "directory" | "symlink") {
            assert!(fs::symlink_metadata(&marker).is_ok());
        }
    }
}

#[test]
fn operation_doctor_output_ownership_and_unsafe_tool_selection_fail_without_writes() {
    let _helper = helper();
    for mode in [
        "output",
        "symlink",
        "relative",
        "script",
        "application-tool",
    ] {
        let fixture = Fixture::new();
        fixture.doctor_tools(&fixture.root);
        let mut command = fixture.public_command("doctor");
        command.args(["--operation", "build", "--json"]);
        let expected = if mode == "output" || mode == "symlink" {
            "operation-release-output"
        } else {
            "operation-tool-selection"
        };
        match mode {
            "output" => {
                fs::create_dir_all(fixture.root.join("target/hegira/release-build")).unwrap();
                fs::write(
                    fixture.root.join("target/hegira/release-build/user-data"),
                    "preserve",
                )
                .unwrap();
            }
            "symlink" => symlink(&fixture.tools, fixture.root.join("target")).unwrap(),
            _ => {
                let cargo = match mode {
                    "relative" => PathBuf::from("cargo"),
                    "script" => {
                        let path = fixture.tools.join("script-cargo");
                        fs::write(&path, "#!/bin/sh\ntouch unintended-doctor-hook\n").unwrap();
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
                        path
                    }
                    _ => {
                        let path = fixture.root.join("untrusted-cargo");
                        fs::copy(fixture.tools.join("cargo"), &path).unwrap();
                        path
                    }
                };
                command
                    .arg("--probe-tools")
                    .arg("--cargo")
                    .arg(cargo)
                    .arg("--tool-directory")
                    .arg(&fixture.tools);
            }
        }
        let report = doctor_json(&command.output().unwrap(), 3);
        assert_eq!(doctor_status(&report, expected), "failure");
        assert!(!fixture.tools.join("doctor-probes.log").exists());
        assert!(!fixture.root.join("unintended-doctor-hook").exists());
        if mode == "output" {
            assert_eq!(
                fs::read_to_string(fixture.root.join("target/hegira/release-build/user-data"))
                    .unwrap(),
                "preserve"
            );
        }
    }
}

#[test]
fn operation_doctor_interrupted_probes_are_bounded_and_reaped() {
    let fixture = Fixture::new();
    fixture.doctor_tools(&fixture.root);
    fs::write(fixture.tools.join("doctor-mode"), "hang").unwrap();
    let child = fixture
        .doctor(&fixture.root, "check", true, "sqlite")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let probe = pid(&fixture.tools, "doctor-probe.pid");
    kill_process(
        Pid::from_raw(child.id().try_into().unwrap()).unwrap(),
        Signal::INT,
    )
    .unwrap();
    let report = doctor_json(&child.wait_with_output().unwrap(), 3);
    assert_eq!(
        doctor_status(&report, "operation-probe-interrupted"),
        "failure"
    );
    assert_stopped(probe);
    assert!(!fixture.root.join("target").exists());
}

#[test]
fn operation_doctor_times_out_once_and_never_runs_followup_probes() {
    let fixture = Fixture::new();
    fixture.doctor_tools(&fixture.root);
    fs::write(fixture.tools.join("doctor-mode"), "hang").unwrap();
    let start = Instant::now();
    let output = fixture
        .doctor(&fixture.root, "build", true, "sqlite")
        .output()
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(15));
    let report = doctor_json(&output, 3);
    assert_eq!(
        doctor_status(&report, "operation-probe-interrupted"),
        "failure"
    );
    assert_eq!(
        fs::read_to_string(fixture.tools.join("doctor-probes.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_stopped(pid(&fixture.tools, "doctor-probe.pid"));
    assert!(!fixture.root.join("target").exists());
}

#[test]
fn operation_doctor_does_not_compete_with_an_active_executor() {
    let fixture = Fixture::new();
    fixture.doctor_tools(&fixture.root);
    fixture.mode("wait");
    let control = ExecutionControl::default();
    let plan = fixture.plan(OperationIntent::Check);
    thread::scope(|scope| {
        let worker = scope.spawn(|| fixture.execute(&plan, &control));
        wait_file(&fixture.root, "child.pid");
        let output = fixture
            .doctor(&fixture.root, "check", true, "sqlite")
            .output()
            .unwrap();
        control.cancel();
        worker.join().unwrap().unwrap();
        let report = doctor_json(&output, 3);
        assert_eq!(doctor_status(&report, "operation-active"), "failure");
        assert!(!fixture.tools.join("doctor-probes.log").exists());
        assert!(!fixture.root.join(".hegira-mutation.lock").exists());
    });
}

#[test]
fn operation_doctor_accepts_owned_release_output_without_mutating_it() {
    let fixture = Fixture::new();
    fixture.doctor_tools(&fixture.root);
    public_json(&fixture.public_execute("build", true).output().unwrap(), 0);
    let files = [
        ".hegira-release-build.json",
        "release/app_server",
        "site/pkg/app_bg.wasm",
        "site/pkg/app.js",
        "site/pkg/app.css",
    ];
    let root = fixture.root.join("target/hegira/release-build");
    let before: Vec<_> = files
        .iter()
        .map(|file| fs::read(root.join(file)).unwrap())
        .collect();
    let report = doctor_json(
        &fixture
            .doctor(&fixture.root, "build", true, "sqlite")
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(doctor_status(&report, "operation-release-output"), "pass");
    let after: Vec<_> = files
        .iter()
        .map(|file| fs::read(root.join(file)).unwrap())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn operation_doctor_parser_limits_intents_profiles_and_probe_authority() {
    let fixture = Fixture::new();
    for args in [
        vec!["--operation", "shell"],
        vec!["--operation", "database-status"],
        vec!["--operation", "check", "--profile", "production"],
        vec!["--probe-tools"],
        vec!["--operation", "check", "--cargo", "/absolute/cargo"],
    ] {
        let output = fixture
            .public_command("doctor")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    let output = fixture
        .public_command("doctor")
        .args(["--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--probe-tools"));
    assert!(!fixture.root.join("target").exists());
}

#[test]
fn operation_doctor_never_reads_redirected_sources_or_blocks_on_special_files() {
    let _helper = helper();
    for mode in ["fifo", "css", "public-assets"] {
        let fixture = Fixture::new();
        fixture.doctor_tools(&fixture.root);
        let secret = fixture.tools.join("private-doctor-source");
        fs::write(&secret, "doctor-private-source-must-not-appear").unwrap();
        match mode {
            "fifo" => {
                let path = fixture.root.join("apps/server/src/server.rs");
                fs::remove_file(&path).unwrap();
                rustix::fs::mkfifoat(
                    rustix::fs::CWD,
                    path,
                    rustix::fs::Mode::from_raw_mode(0o600),
                )
                .unwrap();
            }
            "css" => {
                let path = fixture.root.join("apps/web/src/style/main.css");
                fs::remove_file(&path).unwrap();
                symlink(&secret, path).unwrap();
            }
            _ => {
                let path = fixture.root.join("apps/web/src/public");
                fs::rename(&path, fixture.root.join("original-public-assets")).unwrap();
                symlink(&fixture.tools, path).unwrap();
            }
        }
        let start = Instant::now();
        let output = fixture
            .doctor(&fixture.root, "build", false, "sqlite")
            .output()
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        let report = doctor_json(&output, if mode == "fifo" { 3 } else { 0 });
        assert_eq!(
            doctor_status(
                &report,
                match mode {
                    "fifo" => "managed-integrations",
                    "css" => "operation-frontend-state",
                    _ => "operation-public-assets",
                }
            ),
            if mode == "fifo" { "failure" } else { "warning" }
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout)
                .contains("doctor-private-source-must-not-appear")
        );
        assert_eq!(
            fs::read_to_string(secret).unwrap(),
            "doctor-private-source-must-not-appear"
        );
        assert!(!fixture.tools.join("doctor-probes.log").exists());
    }
}

fn application_source_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, directory: &Path, result: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.strip_prefix(root).unwrap() == Path::new("target/hegira/release-build") {
                continue; // Only this separately claimed build root belongs to Hegira.
            }
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                // Only these controlled child observations are test-owned output.
                if [
                    "child.log",
                    "child.pid",
                    "child.cwd",
                    "child.path",
                    "child.auto-install",
                    "child.override",
                    "child.profile",
                    "child.backend",
                    "child.bind",
                    "child.leptos-bind",
                    "proxy.log",
                ]
                .iter()
                .any(|name| relative == Path::new(name))
                {
                    continue;
                }
                result.push((relative.to_owned(), fs::read(path).unwrap()));
            }
        }
    }
    let mut result = Vec::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn public_help_and_parser_require_explicit_mode_consent_and_tool_selection() {
    let fixture = Fixture::new();
    for operation in ["check", "test", "dev", "build"] {
        let result = fixture
            .public_command(operation)
            .arg("--help")
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let help = String::from_utf8(result.stdout).unwrap();
        for option in [
            "--dry-run",
            "--execute",
            "--trust-application",
            "--cargo",
            "--tool-directory",
            "--application-root",
            "--json",
        ] {
            assert!(help.contains(option));
        }
        for args in [
            vec![],
            vec!["--execute"],
            vec!["--execute", "--trust-application"],
            vec!["--dry-run", "--execute"],
            vec!["--dry-run", "--trust-application"],
            vec!["--dry-run", "--cargo", "relative-cargo"],
            vec!["--dry-run", "--tool-directory", "."],
            vec!["--dry-run", "--ignored"],
            vec!["--dry-run", "--", "--ignored"],
            vec!["--dry-run", "--features", "db-postgres"],
        ] {
            let result = fixture
                .public_command(operation)
                .args(args)
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(2));
            assert!(result.stdout.is_empty());
        }
        if operation == "dev" {
            // Check the additional dev-only parser requirement separately.
            let mut command = fixture.public_command("dev");
            command
                .args(["--execute", "--trust-application", "--cargo"])
                .arg(fixture.tools.join("cargo"))
                .arg("--tool-directory")
                .arg(&fixture.tools);
            assert_eq!(command.output().unwrap().status.code(), Some(2));
        }
        assert!(!fixture.root.join("child.log").exists());
        assert!(!fixture.root.join("target").exists());
    }
}

#[test]
fn public_preview_and_execution_share_plans_across_all_six_compositions() {
    let fixture = Fixture::new();
    for composition in ["default", "minimal", "identity-added"] {
        for database in ["sqlite", "postgres"] {
            let root = fixture
                .parent
                .join(format!("public-{composition}-{database}"));
            let mut create = Command::new(env!("CARGO_BIN_EXE_hegira"));
            create
                .args(["new", "public-operation-app", "--destination"])
                .arg(&root)
                .args(["--database", database]);
            if composition != "default" {
                create.args(["--composition", "minimal"]);
            }
            assert!(create.output().unwrap().status.success());
            if composition == "identity-added" {
                assert!(
                    Command::new(env!("CARGO_BIN_EXE_hegira"))
                        .args(["component", "add", "identity"])
                        .current_dir(&root)
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
            fs::write(root.join("child-mode"), "success").unwrap();
            fixture.development_tools(&root);
            let before = application_source_tree(&root);
            for operation in ["check", "test", "dev", "build"] {
                let preview = fixture
                    .public_command(operation)
                    .current_dir(root.join("apps/web/src"))
                    .args(["--dry-run", "--json"])
                    .output()
                    .unwrap();
                let preview = public_json(&preview, 0);
                let reviewed_plan = operation_contract::plan(
                    "public-operation-app",
                    composition,
                    database,
                    operation,
                );
                assert_eq!(
                    preview,
                    operation_contract::preview(reviewed_plan),
                    "review the full {composition}/{database}/{operation} plan before changing its snapshot"
                );
                assert_eq!(preview["mode"], "preview");
                assert!(preview["execution"].is_null());
                assert_eq!(preview["diagnostics"], serde_json::json!([]));
                let repeated = fixture
                    .public_command(operation)
                    .current_dir(&fixture.parent)
                    .arg("--application-root")
                    .arg(&root)
                    .args(["--dry-run", "--json"])
                    .output()
                    .unwrap();
                assert_eq!(public_json(&repeated, 0), preview);
                let human = fixture
                    .public_command(operation)
                    .current_dir(&root)
                    .arg("--dry-run")
                    .output()
                    .unwrap();
                assert!(human.status.success());
                assert!(human.stderr.is_empty());
                let human = String::from_utf8(human.stdout).unwrap();
                assert_eq!(
                    human,
                    operation_contract::human_preview("public-operation-app", database, operation)
                );
                assert!(human.contains("Planning only"));
                assert!(
                    human.contains(if operation == "dev" || operation == "build" {
                        "leptos"
                    } else {
                        "wasm32-unknown-unknown"
                    })
                );
                assert!(!human.contains(root.to_str().unwrap()));
                assert!(!root.join("target").exists());
                assert!(!root.join("child.log").exists());
                assert_eq!(application_source_tree(&root), before);
                let executed = fixture
                    .public_execute(operation, true)
                    .current_dir(&root)
                    .env("APP_ENV", "production")
                    .env("APP__DATABASE__BACKEND", "incorrect-inherited-provider")
                    .env("APP__SERVER__ADDR", "0.0.0.0:9000")
                    .env("LEPTOS_SITE_ADDR", "0.0.0.0:9000")
                    .env("LEPTOS_STYLE_FILE", "inherited.scss")
                    .env("LEPTOS_BIN_CARGO_COMMAND", "untrusted-inherited-command")
                    .output()
                    .unwrap();
                let executed = public_json(&executed, 0);
                let expected_plan = operation_contract::plan(
                    "public-operation-app",
                    composition,
                    database,
                    operation,
                );
                let expected_outcome =
                    operation_contract::outcome(operation, "success", &expected_plan);
                assert_eq!(
                    executed,
                    operation_contract::execution(expected_plan, &expected_outcome)
                );
                assert_eq!(executed["mode"], "execute");
                assert_eq!(executed["plan"], preview["plan"]);
                assert_eq!(executed["execution"]["outcome"]["status"], "succeeded");
                assert_eq!(
                    executed["execution"]["completed_steps"],
                    if operation == "dev" || operation == "build" {
                        1
                    } else {
                        2
                    }
                );
                assert_eq!(executed["diagnostics"], serde_json::json!([]));
                let expected: Vec<_> = executed["plan"]["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|step| {
                        serde_json::from_value::<Vec<String>>(step["arguments"].clone()).unwrap()
                    })
                    .collect();
                let log = fs::read_to_string(root.join("child.log")).unwrap();
                assert_eq!(
                    log.lines().collect::<Vec<_>>(),
                    expected
                        .iter()
                        .map(|args| format!("{args:?}"))
                        .collect::<Vec<_>>()
                );
                if operation == "build" {
                    assert_eq!(expected[0][..2], ["leptos", "build"]);
                    assert!(expected[0].contains(&"--release".to_owned()));
                    assert!(expected[0].contains(&format!("ssr,db-{database}")));
                    assert_eq!(
                        executed["execution"]["artifacts"],
                        preview["plan"]["artifacts"]
                    );
                    assert_eq!(
                        executed["execution"]["artifacts"]["owner"],
                        "hegira-release-build"
                    );
                    assert!(
                        root.join("target/hegira/release-build/site/pkg/app_bg.wasm")
                            .is_file()
                    );
                } else if operation == "dev" {
                    assert_eq!(expected[0][..2], ["leptos", "watch"]);
                    assert!(expected[0].contains(&format!("ssr,db-{database}")));
                    assert_eq!(
                        fs::read_to_string(root.join("child.profile")).unwrap(),
                        if database == "sqlite" {
                            "sqlite"
                        } else {
                            "development"
                        }
                    );
                    assert_eq!(
                        fs::read_to_string(root.join("child.backend")).unwrap(),
                        database
                    );
                    for path in ["child.bind", "child.leptos-bind"] {
                        assert_eq!(
                            fs::read_to_string(root.join(path)).unwrap(),
                            "127.0.0.1:3000"
                        );
                    }
                    let path = fs::read_to_string(root.join("child.path")).unwrap();
                    assert!(!path.contains("node_modules/.bin"));
                    let shim = std::env::split_paths(&path).next().unwrap();
                    assert!(
                        !shim.exists(),
                        "private development tool selection should be cleaned"
                    );
                    let proxy = fs::read_to_string(root.join("proxy.log")).unwrap();
                    assert_eq!(proxy.lines().count(), 2);
                    assert!(proxy.lines().all(|line| line.contains("--locked")));
                } else {
                    assert_eq!(expected[0][0], operation);
                    assert!(
                        expected[0].contains(&format!("app_server/ssr,app_server/db-{database}"))
                    );
                    assert_eq!(expected[1][0], "check");
                    assert!(expected[1].contains(&"wasm32-unknown-unknown".to_owned()));
                    assert!(
                        expected
                            .iter()
                            .all(|args| args.contains(&"--locked".to_owned())
                                && !args.contains(&"--ignored".to_owned()))
                    );
                }
                fs::remove_file(root.join("child.log")).unwrap();
                assert_eq!(application_source_tree(&root), before);
                assert!(!root.join(".hegira-mutation.lock").exists());
                assert_eq!(root.join("target").exists(), operation == "build");
            }
        }
    }
}

#[test]
fn public_previews_never_invoke_tools_or_load_runtime_configuration() {
    let fixture = Fixture::new();
    for name in [
        "rustc",
        "npm",
        "node",
        "cargo-leptos",
        "wasm-bindgen",
        "wasm-opt",
    ] {
        fs::copy(fixture.tools.join("cargo"), fixture.tools.join(name)).unwrap();
    }
    fs::write(fixture.tools.join("preview-trap-marker"), "controlled trap").unwrap();
    fs::write(
        fixture.root.join("config/production.yaml"),
        "invalid runtime fixture; never load",
    )
    .unwrap();
    let before = application_source_tree(&fixture.root);
    let tools_before = application_source_tree(&fixture.tools);
    for operation in ["check", "test", "dev", "build"] {
        for json in [false, true] {
            let mut command = fixture.public_command(operation);
            command
                .arg("--dry-run")
                .env("PATH", &fixture.tools)
                .env("APP_ENV", "production");
            if json {
                command.arg("--json");
            }
            let output = ControlledCommand::spawn(command).output();
            if json {
                assert_eq!(
                    public_json(&output, 0),
                    operation_contract::preview(operation_contract::plan(
                        "execution-app",
                        "default",
                        "sqlite",
                        operation
                    ))
                );
            } else {
                assert!(output.status.success(), "{output:?}");
                assert!(output.stderr.is_empty());
                assert_eq!(
                    String::from_utf8(output.stdout).unwrap(),
                    operation_contract::human_preview("execution-app", "sqlite", operation)
                );
            }
            assert_eq!(application_source_tree(&fixture.root), before);
            assert_eq!(application_source_tree(&fixture.tools), tools_before);
            assert!(!fixture.root.join("target").exists());
            assert!(!fixture.root.join("child.pid").exists());
            assert!(!fixture.tools.join("preview-trap-observed").exists());
        }
    }
}

#[test]
fn public_process_outcomes_match_reviewed_json_and_human_snapshots() {
    for operation in ["check", "test", "dev", "build"] {
        for case in [
            "success",
            "failure",
            "failure-hydration",
            "child-signalled",
            "cancelled",
            "terminated",
            "spawn",
            "missing-lock",
            "recovery",
        ] {
            if matches!(operation, "dev" | "build") && matches!(case, "spawn" | "failure-hydration")
            {
                continue; // Cargo spawn and the second hydration step belong to native check/test.
            }
            for json in [true, false] {
                let fixture = Fixture::new();
                if matches!(operation, "dev" | "build") {
                    fixture.development_tools(&fixture.root);
                }
                let signalled = matches!(case, "child-signalled" | "cancelled" | "terminated");
                match case {
                    "spawn" => {
                        // Pass native selection but fail at the OS spawn boundary, not parsing.
                        fs::write(
                            fixture.tools.join("cargo"),
                            b"\x7fELFcontrolled invalid executable",
                        )
                        .unwrap();
                        fs::set_permissions(
                            fixture.tools.join("cargo"),
                            fs::Permissions::from_mode(0o755),
                        )
                        .unwrap();
                    }
                    "missing-lock" => fs::remove_file(fixture.root.join("Cargo.lock")).unwrap(),
                    "recovery" => fs::write(
                        fixture.root.join(".hegira-mutation.lock"),
                        "pending fixture recovery; preserve",
                    )
                    .unwrap(),
                    _ => fixture.mode(if signalled { "wait" } else { case }),
                }
                let before = application_source_tree(&fixture.root);
                let mut command = ControlledCommand::spawn(fixture.public_execute(operation, json));
                let leader = if signalled {
                    let leader = pid(&fixture.root, "child.pid");
                    command.leader = Some(leader);
                    kill_process(
                        if case == "child-signalled" {
                            leader
                        } else {
                            command.pid()
                        },
                        if case == "cancelled" {
                            Signal::INT
                        } else {
                            Signal::TERM
                        },
                    )
                    .unwrap();
                    Some(leader)
                } else {
                    None
                };
                let output = command.output();
                let expected_plan =
                    operation_contract::plan("execution-app", "default", "sqlite", operation);
                let expected = operation_contract::outcome(operation, case, &expected_plan);
                let exit = expected["exit"].as_i64().unwrap().try_into().unwrap();
                if json {
                    assert_eq!(
                        public_json(&output, exit),
                        operation_contract::execution(expected_plan, &expected),
                        "{operation}/{case}"
                    );
                } else {
                    assert_eq!(
                        output.status.code(),
                        Some(exit),
                        "{operation}/{case}: {output:?}"
                    );
                    assert_eq!(
                        String::from_utf8(output.stdout).unwrap(),
                        expected["stdout"].as_str().unwrap(),
                        "{operation}/{case}"
                    );
                    assert_eq!(
                        String::from_utf8(output.stderr).unwrap(),
                        expected["stderr"].as_str().unwrap(),
                        "{operation}/{case}"
                    );
                }
                if let Some(leader) = leader {
                    assert_stopped(leader);
                }
                if matches!(case, "spawn" | "missing-lock" | "recovery") {
                    assert!(!fixture.root.join("child.pid").exists());
                    assert!(!fixture.root.join("target").exists());
                } else {
                    assert_stopped(pid(&fixture.root, "child.pid"));
                }
                if matches!(operation, "dev" | "build") && fixture.root.join("child.path").exists()
                {
                    let path = fs::read_to_string(fixture.root.join("child.path")).unwrap();
                    assert!(!std::env::split_paths(&path).next().unwrap().exists());
                }
                assert_eq!(application_source_tree(&fixture.root), before);
                if case != "recovery" {
                    assert!(!fixture.root.join(".hegira-mutation.lock").exists());
                }
                if signalled {
                    // A new operation must succeed: no stale coordination state or lock.
                    fixture.mode("success");
                    public_json(
                        &fixture.public_execute(operation, true).output().unwrap(),
                        0,
                    );
                }
            }
        }
    }
}

#[test]
fn public_json_discards_child_output_while_human_execution_inherits_it() {
    let fixture = Fixture::new();
    fixture.mode("noisy");
    let json = fixture.public_execute("check", true).output().unwrap();
    public_json(&json, 0);
    assert!(!String::from_utf8_lossy(&json.stdout).contains("fixture-child"));
    assert!(!String::from_utf8_lossy(&json.stdout).contains(fixture.parent.to_str().unwrap()));
    let human = fixture.public_execute("check", false).output().unwrap();
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("fixture-child-output-only"));
    assert!(String::from_utf8_lossy(&human.stderr).contains("fixture-child-diagnostic-only"));
    assert!(String::from_utf8_lossy(&human.stdout).contains("Completed steps: 2/2"));
}

#[test]
fn release_build_requires_release_and_explicit_optimizer_selection() {
    let fixture = Fixture::new();
    let missing_release = Command::new(env!("CARGO_BIN_EXE_hegira"))
        .args(["build", "--dry-run"])
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert_eq!(missing_release.status.code(), Some(2));
    let missing_optimizer = fixture
        .public_command("build")
        .args(["--execute", "--trust-application", "--cargo"])
        .arg(fixture.tools.join("cargo"))
        .arg("--tool-directory")
        .arg(&fixture.tools)
        .arg("--wasm-bindgen")
        .arg(fixture.tools.join("wasm-bindgen"))
        .output()
        .unwrap();
    assert_eq!(missing_optimizer.status.code(), Some(2));
    assert!(!fixture.root.join("target").exists());
    assert!(!fixture.root.join("child.pid").exists());
}

#[test]
fn release_failures_and_modified_locks_never_issue_an_artifact_receipt() {
    for (mode, exit, diagnostic) in [
        ("failure", 1, None),
        ("incomplete", 3, Some("release-artifacts-incomplete")),
        ("rewrite-lock", 4, Some("execution-precondition")),
        ("optimizer", 3, Some("release-wasm-opt-version")),
    ] {
        let fixture = Fixture::new();
        fixture.development_tools(&fixture.root);
        if mode == "optimizer" {
            fs::write(fixture.root.join("probe-mode"), mode).unwrap();
        } else {
            fixture.mode(mode);
        }
        let lock = fs::read(fixture.root.join("Cargo.lock")).unwrap();
        let output = fixture.public_execute("build", true).output().unwrap();
        let report = public_json(&output, exit);
        assert!(report["execution"]["artifacts"].is_null());
        if let Some(code) = diagnostic {
            assert_eq!(report["diagnostics"][0]["code"], code);
            assert!(report["execution"].is_null());
        } else {
            assert_eq!(report["execution"]["outcome"]["status"], "child-failed");
        }
        if mode == "rewrite-lock" {
            // Fail closed without silently restoring or overwriting a trusted builder's changes.
            assert_ne!(fs::read(fixture.root.join("Cargo.lock")).unwrap(), lock);
            assert!(!String::from_utf8_lossy(&output.stdout).contains("unreviewed fixture lock"));
        } else {
            assert_eq!(fs::read(fixture.root.join("Cargo.lock")).unwrap(), lock);
        }
        if mode == "optimizer" {
            assert!(!fixture.root.join("child.pid").exists());
            assert!(!fixture.root.join("target").exists());
        }
    }
}

#[test]
fn release_output_never_adopts_unclaimed_or_redirected_data() {
    for mode in [
        "unclaimed",
        "marker",
        "target",
        "hegira",
        "root",
        "nested-symlink",
        "external-hard-link",
    ] {
        let fixture = Fixture::new();
        fixture.development_tools(&fixture.root);
        let external = fixture.parent.join("external-output");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("sentinel"), "unrelated data; preserve").unwrap();
        let root = fixture.root.join("target/hegira/release-build");
        match mode {
            "unclaimed" | "marker" => {
                fs::create_dir_all(&root).unwrap();
                fs::write(root.join("sentinel"), "unclaimed data; preserve").unwrap();
                if mode == "marker" {
                    fs::write(
                        root.join(".hegira-release-build.json"),
                        "not an ownership claim",
                    )
                    .unwrap();
                }
            }
            "target" => symlink(&external, fixture.root.join("target")).unwrap(),
            "hegira" => {
                fs::create_dir(fixture.root.join("target")).unwrap();
                symlink(&external, fixture.root.join("target/hegira")).unwrap();
            }
            "root" => {
                fs::create_dir_all(fixture.root.join("target/hegira")).unwrap();
                symlink(&external, &root).unwrap();
            }
            _ => {
                public_json(&fixture.public_execute("build", true).output().unwrap(), 0);
                fs::remove_file(fixture.root.join("child.pid")).unwrap();
                if mode == "nested-symlink" {
                    symlink(&external, root.join("site/redirect")).unwrap();
                } else {
                    fs::hard_link(external.join("sentinel"), root.join("aliased-file")).unwrap();
                }
            }
        }
        let report = public_json(&fixture.public_execute("build", true).output().unwrap(), 4);
        assert_eq!(report["diagnostics"][0]["code"], "release-output-ownership");
        assert!(report["execution"].is_null());
        assert!(!fixture.root.join("child.pid").exists());
        assert_eq!(
            fs::read_to_string(external.join("sentinel")).unwrap(),
            "unrelated data; preserve"
        );
        assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
        if mode == "unclaimed" || mode == "marker" {
            assert_eq!(
                fs::read_to_string(root.join("sentinel")).unwrap(),
                "unclaimed data; preserve"
            );
        }
    }
}

#[test]
fn release_reuses_only_owned_output_preserves_developer_cache_and_handles_termination() {
    let fixture = Fixture::new();
    fixture.development_tools(&fixture.root);
    fs::create_dir_all(fixture.root.join("target/release")).unwrap();
    fs::write(
        fixture.root.join("target/release/developer-output"),
        "developer-owned; preserve",
    )
    .unwrap();
    fs::write(
        fixture.root.join("config/production.yaml"),
        "never read fixture runtime credentials",
    )
    .unwrap();
    let before = application_source_tree(&fixture.root);
    for _ in 0..2 {
        let mut command = fixture.public_execute("build", true);
        command
            .env("CARGO_TARGET_DIR", &fixture.tools)
            .env("LEPTOS_SITE_ROOT", &fixture.tools)
            .env("LEPTOS_ASSETS_DIR", &fixture.tools)
            .env("LEPTOS_BIN_CARGO_COMMAND", "unreviewed-command")
            .env("LEPTOS_HASH_FILES", "true");
        let report = public_json(&command.output().unwrap(), 0);
        assert_eq!(report["execution"]["completed_steps"], 1);
        assert_eq!(application_source_tree(&fixture.root), before);
    }
    let human = fixture.public_execute("build", false).output().unwrap();
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("Verified artifacts"));
    fixture.mode("wait");
    fs::remove_file(fixture.root.join("child.pid")).unwrap();
    let child = fixture
        .public_execute("build", true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let leader = pid(&fixture.root, "child.pid");
    kill_process(
        Pid::from_raw(child.id().try_into().unwrap()).unwrap(),
        Signal::TERM,
    )
    .unwrap();
    let report = public_json(&child.wait_with_output().unwrap(), 1);
    assert_eq!(report["execution"]["outcome"]["status"], "terminated");
    assert!(report["execution"]["artifacts"].is_null());
    assert_stopped(leader);
    assert_eq!(
        fs::read_to_string(fixture.root.join("target/release/developer-output")).unwrap(),
        "developer-owned; preserve"
    );
}

#[test]
fn development_preflight_is_explicit_actionable_and_never_starts_a_server_on_failure() {
    for (mode, diagnostic) in [
        ("leptos", "development-leptos-version"),
        ("wasm", "development-wasm-version"),
        ("node", "development-node-version"),
        ("oversize", "development-probe"),
        ("target", "development-wasm-target"),
        ("receipt", "development-frontend-lock"),
        ("metadata", "development-metadata"),
        ("frontend", "development-frontend-lock"),
    ] {
        let fixture = Fixture::new();
        fixture.development_tools(&fixture.root);
        fs::write(fixture.root.join("probe-mode"), mode).unwrap();
        match mode {
            "target" => {
                fs::remove_file(fixture.tools.join("wasm-target/libcore-fixture.rlib")).unwrap()
            }
            "receipt" => fs::write(
                fixture
                    .root
                    .join("apps/web/src/node_modules/.package-lock.json"),
                r#"{"lockfileVersion":3,"packages":{}}"#,
            )
            .unwrap(),
            "metadata" => {
                let path = fixture.root.join("apps/server/Cargo.toml");
                let text = fs::read_to_string(&path).unwrap().replace(
                    "../web/src/style/tailwind.css",
                    "../web/src/style/custom.scss",
                );
                fs::write(path, text).unwrap();
            }
            "frontend" => fs::remove_file(
                fixture
                    .root
                    .join("apps/web/src/node_modules/.package-lock.json"),
            )
            .unwrap(),
            _ => {}
        }
        let before = application_source_tree(&fixture.root);
        // Preview does not inspect installed frontend/tools or run probes.
        public_json(
            &fixture
                .public_command("dev")
                .args(["--dry-run", "--json"])
                .output()
                .unwrap(),
            0,
        );
        let output = fixture.public_execute("dev", true).output().unwrap();
        let result = public_json(&output, 3);
        assert_eq!(result["diagnostics"][0]["code"], diagnostic);
        assert!(result["execution"].is_null());
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains(fixture.parent.to_str().unwrap())
        );
        assert!(!fixture.root.join("child.pid").exists());
        assert!(!fixture.root.join("proxy.log").exists());
        assert_eq!(application_source_tree(&fixture.root), before);
    }
}

#[test]
fn development_preflight_cancellation_reaps_the_probe() {
    let fixture = Fixture::new();
    fixture.development_tools(&fixture.root);
    fs::write(fixture.root.join("probe-mode"), "hang").unwrap();
    let child = fixture
        .public_execute("dev", true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let probe = pid(&fixture.root, "probe.pid");
    kill_process(
        Pid::from_raw(child.id().try_into().unwrap()).unwrap(),
        Signal::INT,
    )
    .unwrap();
    let report = public_json(&child.wait_with_output().unwrap(), 1);
    assert_eq!(report["execution"]["outcome"]["status"], "cancelled");
    assert_stopped(probe);
    assert!(!fixture.root.join("child.pid").exists());
}

#[test]
fn development_runtime_failure_and_signals_clean_foreground_groups() {
    for mode in ["failure", "int", "term"] {
        let fixture = Fixture::new();
        fixture.development_tools(&fixture.root);
        fixture.mode(if mode == "failure" {
            "failure"
        } else {
            "descendant"
        });
        let child = fixture
            .public_execute("dev", true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let leader = pid(&fixture.root, "child.pid");
        let descendant = if mode != "failure" {
            let descendant = pid(&fixture.root, "descendant.pid");
            kill_process(
                Pid::from_raw(child.id().try_into().unwrap()).unwrap(),
                if mode == "int" {
                    Signal::INT
                } else {
                    Signal::TERM
                },
            )
            .unwrap();
            Some(descendant)
        } else {
            None
        };
        let report = public_json(&child.wait_with_output().unwrap(), 1);
        assert_eq!(report["execution"]["completed_steps"], 0);
        assert_eq!(
            report["execution"]["outcome"]["status"],
            match mode {
                "failure" => "child-failed",
                "int" => "cancelled",
                _ => "terminated",
            }
        );
        assert_stopped(leader);
        if let Some(descendant) = descendant {
            assert_stopped(descendant);
        }
        let path = fs::read_to_string(fixture.root.join("child.path")).unwrap();
        assert!(!std::env::split_paths(&path).next().unwrap().exists());
    }
}

#[test]
fn public_child_failures_stop_the_plan_and_report_exact_completed_steps() {
    for (mode, completed, code) in [("failure", 0, 23), ("failure-hydration", 1, 24)] {
        let fixture = Fixture::new();
        fixture.mode(mode);
        let result = fixture.public_execute("test", true).output().unwrap();
        let report = public_json(&result, 1);
        assert_eq!(report["execution"]["completed_steps"], completed);
        assert_eq!(
            report["execution"]["outcome"],
            serde_json::json!({"status":"child-failed", "exit_code":code})
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("child.log"))
                .unwrap()
                .lines()
                .count(),
            completed + 1
        );
        assert_stopped(pid(&fixture.root, "child.pid"));
    }
}

#[test]
fn public_missing_prerequisites_recovery_and_relative_tools_fail_before_spawn() {
    for (mode, code, diagnostic) in [
        ("lockfile", 3, "execution-path"),
        ("tool", 3, "execution-path"),
        ("recovery", 4, "application-recovery"),
    ] {
        let fixture = Fixture::new();
        if mode == "lockfile" {
            fs::remove_file(fixture.root.join("Cargo.lock")).unwrap();
        } else if mode == "recovery" {
            fs::write(
                fixture.root.join(".hegira-mutation.lock"),
                "fixture recovery; preserve",
            )
            .unwrap();
        }
        let mut command = fixture.public_execute("check", true);
        if mode == "tool" {
            command = fixture.public_command("check");
            command.args([
                "--execute",
                "--trust-application",
                "--cargo",
                "relative-cargo",
                "--tool-directory",
                ".",
                "--json",
            ]);
        }
        let result = public_json(&command.output().unwrap(), code);
        assert_eq!(result["diagnostics"][0]["code"], diagnostic);
        assert!(result["execution"].is_null());
        assert!(!fixture.root.join("child.log").exists());
        if mode == "recovery" {
            assert_eq!(
                fs::read_to_string(fixture.root.join(".hegira-mutation.lock")).unwrap(),
                "fixture recovery; preserve"
            );
        }
    }
}

#[test]
fn public_sigint_and_sigterm_clean_owned_groups_and_return_non_success() {
    for (signal, status) in [(Signal::INT, "cancelled"), (Signal::TERM, "terminated")] {
        let fixture = Fixture::new();
        fixture.mode("descendant");
        let child = fixture
            .public_execute("test", true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let leader = pid(&fixture.root, "child.pid");
        let descendant = pid(&fixture.root, "descendant.pid");
        kill_process(
            Pid::from_raw(child.id().try_into().unwrap()).unwrap(),
            signal,
        )
        .unwrap();
        let result = child.wait_with_output().unwrap();
        let report = public_json(&result, 1);
        assert_eq!(report["execution"]["outcome"]["status"], status);
        assert_eq!(report["execution"]["completed_steps"], 0);
        assert_stopped(leader);
        assert_stopped(descendant);
        fixture.mode("success");
        public_json(&fixture.public_execute("check", true).output().unwrap(), 0);
    }
}

#[test]
fn public_planning_errors_use_versioned_json_or_static_human_diagnostics() {
    let fixture = Fixture::new();
    for operation in ["check", "test"] {
        let failed = fixture
            .public_command(operation)
            .current_dir(&fixture.tools)
            .args(["--dry-run", "--json"])
            .output()
            .unwrap();
        let report = public_json(&failed, 3);
        assert!(report["plan"].is_null());
        assert!(report["execution"].is_null());
        assert_eq!(report["diagnostics"][0]["code"], "application-context");
        let failed = fixture
            .public_command(operation)
            .current_dir(&fixture.tools)
            .arg("--dry-run")
            .output()
            .unwrap();
        assert_eq!(failed.status.code(), Some(3));
        assert!(failed.stdout.is_empty());
        assert!(String::from_utf8_lossy(&failed.stderr).contains("application-context"));
        assert!(
            !String::from_utf8_lossy(&failed.stderr).contains(fixture.parent.to_str().unwrap())
        );
    }
}

fn wait_file(root: &Path, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !root.join(name).exists() {
        assert!(
            Instant::now() < deadline,
            "controlled child did not publish {name}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn pid(root: &Path, name: &str) -> Pid {
    wait_file(root, name);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(pid) = fs::read_to_string(root.join(name))
            .ok()
            .and_then(|text| text.parse::<i32>().ok())
            .and_then(Pid::from_raw)
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "controlled child did not publish its PID"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn assert_reaped(pid: Pid) {
    assert_eq!(
        waitpid(Some(pid), WaitOptions::NOHANG).unwrap_err(),
        rustix::io::Errno::CHILD
    );
}

fn assert_stopped(pid: Pid) {
    let path = format!("/proc/{}/stat", pid.as_raw_pid());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        // Orphaned descendants are reaped by the host's init/subreaper, not us.
        match fs::read_to_string(&path) {
            Err(_) => return,
            Ok(stat) if stat.rsplit_once(") ").unwrap().1.starts_with('Z') => return,
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "owned process-group descendant is still running"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn closed_arguments_and_anchored_working_directory_execute_without_a_shell() {
    let fixture = Fixture::new();
    let plan = fixture.plan(OperationIntent::Check);
    assert!(!fixture.root.join("child.log").exists());
    let report = fixture
        .execute(&plan, &ExecutionControl::default())
        .unwrap();
    assert!(report.succeeded());
    assert_eq!(report.exit(), hegira_cli::CliExit::Success);
    assert_eq!(report.completed_steps, 2);
    let log = fs::read_to_string(fixture.root.join("child.log")).unwrap();
    assert!(log.contains("\"check\", \"--locked\", \"--workspace\""));
    assert!(log.contains("\"hydrate\", \"--target\", \"wasm32-unknown-unknown\""));
    assert_eq!(
        fs::read_to_string(fixture.root.join("child.cwd")).unwrap(),
        fixture.root.to_str().unwrap()
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("child.path")).unwrap(),
        fixture.tools.to_str().unwrap()
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("child.auto-install")).unwrap(),
        "0"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("child.override")).unwrap(),
        ""
    );
    assert!(!fixture.root.join(".hegira-operation.lock").exists());
    assert!(!fixture.root.join("target").exists());
    assert_reaped(pid(&fixture.root, "child.pid"));
    let json = serde_json::to_string(&report).unwrap();
    assert_eq!(
        json,
        "{\"output_schema\":1,\"intent\":{\"operation\":\"check\"},\"completed_steps\":2,\"outcome\":{\"status\":\"succeeded\"}}"
    );
    assert!(!json.contains(fixture.parent.to_str().unwrap()));
}

#[test]
fn every_supported_tool_intent_uses_the_reviewed_ordered_steps() {
    for intent in [OperationIntent::Check, OperationIntent::Test] {
        let fixture = Fixture::new();
        let plan = fixture.plan(intent);
        let report = fixture
            .execute(&plan, &ExecutionControl::default())
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.completed_steps, plan.summary().steps.len());
        let expected: Vec<_> = plan
            .summary()
            .steps
            .iter()
            .map(|step| {
                let hegira_cli::operations::OperationStep::Tool { arguments, .. } = step else {
                    unreachable!();
                };
                format!("{arguments:?}")
            })
            .collect();
        let log = fs::read_to_string(fixture.root.join("child.log")).unwrap();
        assert_eq!(log.lines().collect::<Vec<_>>(), expected);
    }
}

#[test]
fn failure_stops_the_plan_and_never_reports_success() {
    let fixture = Fixture::new();
    fixture.mode("failure");
    let report = fixture
        .execute(
            &fixture.plan(OperationIntent::Test),
            &ExecutionControl::default(),
        )
        .unwrap();
    assert!(!report.succeeded());
    assert_ne!(report.exit(), hegira_cli::CliExit::Success);
    assert_eq!(
        report.outcome,
        ExecutionOutcome::ChildFailed { exit_code: 23 }
    );
    assert_eq!(report.completed_steps, 0);
    assert_eq!(
        fs::read_to_string(fixture.root.join("child.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_reaped(pid(&fixture.root, "child.pid"));
}

#[test]
fn cancellation_before_execution_never_spawns_a_child() {
    let fixture = Fixture::new();
    let control = ExecutionControl::default();
    control.cancel();
    let report = fixture
        .execute(&fixture.plan(OperationIntent::Check), &control)
        .unwrap();
    assert_eq!(report.outcome, ExecutionOutcome::Cancelled);
    assert!(!fixture.root.join("child.log").exists());
}

#[test]
fn cancelled_and_terminated_children_are_reaped_and_coordination_is_released() {
    for terminate in [false, true] {
        let fixture = Fixture::new();
        fixture.mode("wait");
        let control = ExecutionControl::default();
        let plan = fixture.plan(OperationIntent::Check);
        thread::scope(|scope| {
            let worker = scope.spawn(|| fixture.execute(&plan, &control));
            let child = pid(&fixture.root, "child.pid");
            if terminate {
                control.terminate();
            } else {
                control.cancel();
            }
            let report = worker.join().unwrap().unwrap();
            assert_eq!(
                report.outcome,
                if terminate {
                    ExecutionOutcome::Terminated
                } else {
                    ExecutionOutcome::Cancelled
                }
            );
            assert_reaped(child);
        });
        fixture.mode("success");
        assert!(
            fixture
                .execute(
                    &fixture.plan(OperationIntent::Check),
                    &ExecutionControl::default()
                )
                .unwrap()
                .succeeded()
        );
    }
}

#[test]
fn stopped_child_escalates_to_kill_and_is_reaped() {
    let fixture = Fixture::new();
    fixture.mode("wait");
    let control = ExecutionControl::default();
    let plan = fixture.plan(OperationIntent::Check);
    thread::scope(|scope| {
        let worker = scope.spawn(|| fixture.execute(&plan, &control));
        let child = pid(&fixture.root, "child.pid");
        kill_process(child, Signal::STOP).unwrap();
        let started = Instant::now();
        control.cancel();
        assert_eq!(
            worker.join().unwrap().unwrap().outcome,
            ExecutionOutcome::Cancelled
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_reaped(child);
    });
}

#[test]
fn externally_signalled_child_is_not_success() {
    let fixture = Fixture::new();
    fixture.mode("wait");
    let plan = fixture.plan(OperationIntent::Check);
    let control = ExecutionControl::default();
    thread::scope(|scope| {
        let worker = scope.spawn(|| fixture.execute(&plan, &control));
        let child = pid(&fixture.root, "child.pid");
        kill_process(child, Signal::TERM).unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap().outcome,
            ExecutionOutcome::ChildSignalled { signal: 15 }
        );
        assert_reaped(child);
    });
}

#[test]
fn owned_process_group_descendants_are_stopped_on_cancellation_and_leader_exit() {
    for mode in ["descendant", "leftover"] {
        let fixture = Fixture::new();
        fixture.mode(mode);
        let control = ExecutionControl::default();
        let plan = fixture.plan(OperationIntent::Check);
        thread::scope(|scope| {
            let worker = scope.spawn(|| fixture.execute(&plan, &control));
            let descendant = pid(&fixture.root, "descendant.pid");
            if mode == "descendant" {
                control.cancel();
            }
            let report = worker.join().unwrap().unwrap();
            assert_eq!(
                report.outcome,
                if mode == "descendant" {
                    ExecutionOutcome::Cancelled
                } else {
                    ExecutionOutcome::Succeeded
                }
            );
            assert_stopped(descendant);
            assert_reaped(pid(&fixture.root, "child.pid"));
        });
    }
}

#[test]
fn concurrent_execution_fails_closed_without_creating_a_lock_file() {
    let fixture = Fixture::new();
    fixture.mode("wait");
    let control = ExecutionControl::default();
    let plan = fixture.plan(OperationIntent::Check);
    thread::scope(|scope| {
        let worker = scope.spawn(|| fixture.execute(&plan, &control));
        wait_file(&fixture.root, "child.pid");
        let error = fixture
            .execute(&plan, &ExecutionControl::default())
            .unwrap_err();
        control.cancel();
        worker.join().unwrap().unwrap();
        assert_eq!(error.code, "execution-active");
    });
}

#[test]
fn recovery_markers_of_every_type_block_without_being_read_or_deleted() {
    for mode in ["file", "directory", "symlink"] {
        let fixture = Fixture::new();
        let plan = fixture.plan(OperationIntent::Check);
        let marker = fixture.root.join(application_mutator::MUTATION_MARKER);
        match mode {
            "file" => fs::write(&marker, "private recovery content").unwrap(),
            "directory" => fs::create_dir(&marker).unwrap(),
            _ => symlink(fixture.parent.join("missing-target"), &marker).unwrap(),
        }
        let error = fixture
            .execute(&plan, &ExecutionControl::default())
            .unwrap_err();
        assert_eq!(error.code, "application-recovery");
        assert!(fs::symlink_metadata(marker).is_ok());
        assert!(!fixture.root.join("child.log").exists());
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("private recovery content")
        );
    }
}

#[test]
fn substituted_application_root_and_manifest_fail_before_spawn() {
    for mode in ["root", "symlink", "manifest"] {
        let fixture = Fixture::new();
        let plan = fixture.plan(OperationIntent::Check);
        match mode {
            "manifest" => {
                let manifest = fixture.root.join("hegira.toml");
                let mut content = fs::read_to_string(&manifest).unwrap();
                content.push_str("\n# changed after review\n");
                fs::write(manifest, content).unwrap();
            }
            _ => {
                let retained = fixture.parent.join("retained");
                fs::rename(&fixture.root, &retained).unwrap();
                if mode == "symlink" {
                    symlink(&retained, &fixture.root).unwrap();
                } else {
                    fs::create_dir(&fixture.root).unwrap();
                }
            }
        }
        assert!(
            fixture
                .execute(&plan, &ExecutionControl::default())
                .is_err()
        );
        assert!(!fixture.root.join("child.log").exists());
    }
}

#[test]
fn unsafe_prerequisites_do_not_execute() {
    for mode in ["missing-lock", "symlink-lock", "directory-toolchain"] {
        let fixture = Fixture::new();
        let plan = fixture.plan(OperationIntent::Check);
        match mode {
            "missing-lock" => fs::remove_file(fixture.root.join("Cargo.lock")).unwrap(),
            "symlink-lock" => {
                fs::rename(fixture.root.join("Cargo.lock"), fixture.parent.join("lock")).unwrap();
                symlink(fixture.parent.join("lock"), fixture.root.join("Cargo.lock")).unwrap();
            }
            "directory-toolchain" => {
                fs::remove_file(fixture.root.join("rust-toolchain.toml")).unwrap();
                fs::create_dir(fixture.root.join("rust-toolchain.toml")).unwrap();
            }
            _ => {}
        }
        assert_eq!(
            fixture
                .execute(&plan, &ExecutionControl::default())
                .unwrap_err()
                .code,
            "execution-path"
        );
        assert!(!fixture.root.join("child.log").exists());
    }
}

#[test]
fn tool_substitution_relative_paths_scripts_and_application_local_tools_are_rejected() {
    let fixture = Fixture::new();
    assert!(
        TrustedToolchain::resolve(Path::new("cargo"), std::slice::from_ref(&fixture.tools))
            .is_err()
    );
    assert!(
        TrustedToolchain::resolve(&fixture.tools.join("cargo"), &[PathBuf::from(".")]).is_err()
    );
    let script = fixture.tools.join("script");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        TrustedToolchain::resolve(&script, std::slice::from_ref(&fixture.tools))
            .unwrap_err()
            .code,
        "execution-tool"
    );
    let plan = fixture.plan(OperationIntent::Check);
    let tools = fixture.toolchain();
    fs::rename(
        fixture.tools.join("cargo"),
        fixture.tools.join("retained-cargo"),
    )
    .unwrap();
    fs::copy(
        fixture.tools.join("retained-cargo"),
        fixture.tools.join("cargo"),
    )
    .unwrap();
    let error = execute_application_operation(
        &repository(),
        &plan,
        ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
        &tools,
        &ExecutionControl::default(),
        ChildOutput::Discard,
    )
    .unwrap_err();
    assert_eq!(error.code, "execution-precondition");
    let local = fixture.root.join("local-cargo");
    fs::copy(fixture.tools.join("cargo"), &local).unwrap();
    let tools = TrustedToolchain::resolve(&local, std::slice::from_ref(&fixture.tools)).unwrap();
    assert_eq!(
        execute_application_operation(
            &repository(),
            &plan,
            ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
            &tools,
            &ExecutionControl::default(),
            ChildOutput::Discard
        )
        .unwrap_err()
        .code,
        "application-toolchain"
    );
    assert!(!fixture.root.join("child.log").exists());
    assert!(!format!("{tools:?}").contains(fixture.parent.to_str().unwrap()));
}

#[test]
fn cargo_proxy_symlinks_resolve_explicitly_and_spawn_errors_remain_redacted() {
    let fixture = Fixture::new();
    let alias = fixture.tools.join("cargo-proxy");
    symlink(fixture.tools.join("cargo"), &alias).unwrap();
    let tools = TrustedToolchain::resolve(&alias, std::slice::from_ref(&fixture.tools)).unwrap();
    let plan = fixture.plan(OperationIntent::Check);
    assert!(
        execute_application_operation(
            &repository(),
            &plan,
            ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
            &tools,
            &ExecutionControl::default(),
            ChildOutput::Discard
        )
        .unwrap()
        .succeeded()
    );
    fs::set_permissions(
        fixture.tools.join("cargo"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let error = execute_application_operation(
        &repository(),
        &plan,
        ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
        &tools,
        &ExecutionControl::default(),
        ChildOutput::Discard,
    )
    .unwrap_err();
    assert_eq!(error.code, "execution-spawn");
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains(fixture.parent.to_str().unwrap())
    );
}

#[test]
fn general_executor_cannot_acquire_database_execution_authority() {
    let fixture = Fixture::new();
    for intent in [
        OperationIntent::DatabaseStatus {
            profile: RuntimeProfile::Sqlite,
        },
        OperationIntent::DatabaseMigrate {
            profile: RuntimeProfile::Sqlite,
        },
    ] {
        let error = fixture
            .execute(&fixture.plan(intent), &ExecutionControl::default())
            .unwrap_err();
        assert_eq!(error.code, "execution-entry-point");
        assert!(!fixture.root.join("child.log").exists());
    }
}

#[test]
fn public_database_parser_requires_profile_mode_and_independent_approval() {
    let fixture = Fixture::new();
    for args in [
        vec![],
        vec!["reset"],
        vec!["status"],
        vec!["status", "--profile", "sqlite"],
        vec!["status", "--profile", "invalid", "--dry-run"],
        vec!["status", "--profile", "sqlite", "--execute"],
        vec![
            "migrate",
            "--profile",
            "production",
            "--dry-run",
            "--approve-production-migration",
        ],
        vec![
            "status",
            "--profile",
            "production",
            "--dry-run",
            "--approve-production-migration",
        ],
        vec![
            "migrate",
            "--profile",
            "sqlite",
            "--dry-run",
            "--url",
            "private-value",
        ],
    ] {
        let output = fixture.public_command("db").args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    for operation in ["status", "migrate"] {
        let output = fixture
            .public_command("db")
            .args([operation, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("--profile"));
    }
    assert!(!fixture.root.join("child.log").exists());
}

#[test]
fn database_preview_is_deterministic_tool_free_and_read_only_for_six_compositions() {
    let mut fixture = Fixture::new();
    fs::write(fixture.tools.join("preview-trap-marker"), "active").unwrap();
    for composition in ["default", "minimal", "identity-added"] {
        for provider in ["sqlite", "postgres"] {
            let root = fixture.parent.join(format!("db-{composition}-{provider}"));
            let mut create = fixture.public_command("new");
            create
                .args(["database-app", "--destination"])
                .arg(&root)
                .args(["--database", provider]);
            if composition != "default" {
                create.args(["--composition", "minimal"]);
            }
            assert!(create.output().unwrap().status.success());
            fixture.root = root;
            if composition == "identity-added" {
                assert!(
                    fixture
                        .public_command("component")
                        .args(["add", "identity"])
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
            let before = application_source_tree(&fixture.root);
            let profile = if provider == "sqlite" {
                "sqlite"
            } else {
                "development"
            };
            for operation in ["status", "migrate"] {
                let preview = fixture
                    .database_command(operation, profile, false, true)
                    .env("APP_ENV", "private-invalid-profile")
                    .env("APP__DATABASE__URL", "private-target")
                    .output()
                    .unwrap();
                let repeated = fixture
                    .database_command(operation, profile, false, true)
                    .output()
                    .unwrap();
                assert_eq!(preview.stdout, repeated.stdout);
                let json = public_json(&preview, 0);
                assert_eq!(json["mode"], "preview");
                assert!(json["execution"].is_null());
                assert_eq!(json["plan"]["steps"][0]["database"], provider);
                assert_eq!(json["plan"]["steps"][0]["profile"], profile);
                assert_eq!(
                    json["plan"]["effect"],
                    if operation == "status" {
                        "database-read"
                    } else {
                        "database-write"
                    }
                );
                assert!(!String::from_utf8_lossy(&preview.stdout).contains("private"));
                assert_eq!(before, application_source_tree(&fixture.root));
            }
        }
    }
    assert!(!fixture.tools.join("preview-trap-observed").exists());
}

#[test]
fn public_database_execution_uses_closed_arguments_profile_provider_and_safe_results() {
    let fixture = Fixture::new();
    for operation in ["status", "migrate"] {
        let output = fixture
            .database_command(operation, "sqlite", true, true)
            .env("APP_ENV", "production")
            .env("APP__DATABASE__BACKEND", "postgres")
            .env("APP__DATABASE__URL", "private-selected-target")
            .output()
            .unwrap();
        let json = public_json(&output, 0);
        assert_eq!(json["execution"]["outcome"]["status"], "succeeded");
        assert_eq!(json["execution"]["completed_steps"], 1);
        assert_eq!(json["execution"]["database"]["operation"], operation);
        assert_eq!(json["execution"]["database"]["provider"], "sqlite");
        assert_eq!(
            json["execution"]["database"]["status"]["migrations"][0]["version"],
            1
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("child.profile")).unwrap(),
            "sqlite"
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("child.provider")).unwrap(),
            "sqlite"
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("child.database-url")).unwrap(),
            "private-selected-target"
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private"));
        assert!(
            !fs::read_to_string(fixture.root.join("child.log"))
                .unwrap()
                .contains("private")
        );
    }
    let human = fixture
        .database_command("status", "sqlite", true, false)
        .output()
        .unwrap();
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("1 application Applied"));
    assert!(!String::from_utf8_lossy(&human.stdout).contains("private"));
    assert!(human.stderr.is_empty());
}

#[test]
fn production_migration_approval_is_required_in_public_and_library_boundaries() {
    let mut fixture = Fixture::new();
    let root = fixture.parent.join("production");
    assert!(
        fixture
            .public_command("new")
            .args(["production-app", "--destination"])
            .arg(&root)
            .args(["--database", "postgres"])
            .output()
            .unwrap()
            .status
            .success()
    );
    fixture.root = root;
    fixture.mode("success");
    let preview = public_json(
        &fixture
            .database_command("migrate", "production", false, true)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(
        preview["plan"]["policy"]["production_migration_approval_required"],
        true
    );
    let denied = public_json(
        &fixture
            .database_command("migrate", "production", true, true)
            .output()
            .unwrap(),
        3,
    );
    assert_eq!(
        denied["diagnostics"][0]["code"],
        "production-migration-approval"
    );
    let plan = fixture.plan(OperationIntent::DatabaseMigrate {
        profile: RuntimeProfile::Production,
    });
    let error = execute_database_operation(
        &repository(),
        &plan,
        ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
        DatabaseExecutionApproval::Ordinary,
        &fixture.toolchain(),
        &ExecutionControl::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, "production-migration-approval");
    assert!(!fixture.root.join("child.log").exists());
    let approved = public_json(
        &fixture
            .database_command("migrate", "production", true, true)
            .arg("--approve-production-migration")
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(approved["execution"]["database"]["operation"], "migrate");
    assert_eq!(
        fs::read_to_string(fixture.root.join("child.profile")).unwrap(),
        "production"
    );
    let status = public_json(
        &fixture
            .database_command("status", "production", true, true)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(status["execution"]["database"]["operation"], "status");
    let denied = public_json(
        &fixture
            .database_command("migrate", "development", true, true)
            .arg("--approve-production-migration")
            .output()
            .unwrap(),
        3,
    );
    assert_eq!(
        denied["diagnostics"][0]["code"],
        "production-migration-approval"
    );
}

#[test]
fn database_execution_rejects_profiles_missing_unsafe_entry_points_and_recovery_before_spawn() {
    for mode in [
        "profile",
        "binary",
        "registration",
        "feature",
        "infrastructure",
        "symlink",
        "runtime-profile",
        "recovery",
    ] {
        let fixture = Fixture::new();
        let profile = if mode == "profile" {
            "development"
        } else {
            "sqlite"
        };
        match mode {
            "binary" => {
                fs::remove_file(fixture.root.join("apps/server/src/bin/app_database.rs")).unwrap()
            }
            "infrastructure" => fs::remove_file(
                fixture
                    .root
                    .join("crates/infrastructure/src/database_operations.rs"),
            )
            .unwrap(),
            "registration" | "feature" => {
                let file = fixture.root.join("apps/server/Cargo.toml");
                let source = fs::read_to_string(&file).unwrap();
                fs::write(
                    file,
                    if mode == "registration" {
                        source.replace("src/bin/app_database.rs", "src/main.rs")
                    } else {
                        source.replace("database-operations =", "removed-feature =")
                    },
                )
                .unwrap();
            }
            "symlink" => {
                let file = fixture.root.join("apps/server/src/bin/app_database.rs");
                fs::remove_file(&file).unwrap();
                symlink("/private-missing-file", file).unwrap();
            }
            "runtime-profile" => fs::remove_file(fixture.root.join("config/sqlite.yaml")).unwrap(),
            "recovery" => {
                fs::write(fixture.root.join(".hegira-mutation.lock"), "private-state").unwrap()
            }
            _ => (),
        }
        let expected = if mode == "recovery" { 4 } else { 3 };
        let json = public_json(
            &fixture
                .database_command("status", profile, true, true)
                .output()
                .unwrap(),
            expected,
        );
        assert!(!json["diagnostics"].as_array().unwrap().is_empty());
        assert!(!fixture.root.join("child.log").exists());
    }
}

#[test]
fn database_failures_invalid_and_oversized_output_cannot_report_success_or_leak_output() {
    let fixture = Fixture::new();
    for mode in ["db-failure", "db-malformed", "db-oversize"] {
        fixture.mode(mode);
        for operation in ["status", "migrate"] {
            let output = fixture
                .database_command(operation, "sqlite", true, true)
                .output()
                .unwrap();
            let json = public_json(&output, 1);
            assert!(!String::from_utf8_lossy(&output.stdout).contains("private"));
            if mode == "db-failure" {
                assert_eq!(json["execution"]["outcome"]["status"], "child-failed");
                assert_eq!(json["execution"]["outcome"]["exit_code"], 4);
                assert_eq!(json["execution"]["completed_steps"], 0);
                assert!(json["execution"].get("database").is_none());
            } else {
                assert_eq!(json["diagnostics"][0]["code"], "database-protocol");
                assert!(json["execution"].is_null());
            }
        }
    }
}

#[test]
fn database_protocol_rejects_extra_fields_mismatched_identity_and_incomplete_migrations() {
    let fixture = Fixture::new();
    fixture.mode("db-report");
    for mutation in [
        "schema",
        "provider",
        "operation",
        "extra",
        "module",
        "order",
        "state",
        "history",
    ] {
        let mut result = serde_json::json!({"output_schema":1,"outcome":"success","operation":"migrate","provider":"sqlite",
            "status":{"output_schema":1,"history":"present","migrations":[{"module_id":"application","version":1,"state":"applied"}]}});
        match mutation {
            "schema" => result["status"]["output_schema"] = 99.into(),
            "provider" => result["provider"] = "postgres".into(),
            "operation" => result["operation"] = "status".into(),
            "extra" => result["private-extra"] = "private-raw-content".into(),
            "module" => {
                result["status"]["migrations"][0]["module_id"] = "private-runtime-identity".into()
            }
            "order" => {
                let duplicate = result["status"]["migrations"][0].clone();
                result["status"]["migrations"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            "state" => result["status"]["migrations"][0]["state"] = "pending".into(),
            "history" => result["status"]["history"] = "database-missing".into(),
            _ => unreachable!(),
        }
        fs::write(
            fixture.root.join("database-result.json"),
            result.to_string(),
        )
        .unwrap();
        let output = fixture
            .database_command("migrate", "sqlite", true, true)
            .output()
            .unwrap();
        let json = public_json(&output, 1);
        assert_eq!(json["diagnostics"][0]["code"], "database-protocol");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("private"));
    }
}

#[test]
fn database_capture_obeys_public_signal_cancellation_and_releases_coordination() {
    let fixture = Fixture::new();
    fixture.mode("db-wait");
    let mut command =
        ControlledCommand::spawn(fixture.database_command("migrate", "sqlite", true, true));
    wait_file(&fixture.root, "child.pid");
    let leader = pid(&fixture.root, "child.pid");
    command.leader = Some(leader);
    kill_process(command.pid(), Signal::TERM).unwrap();
    let output = command.output();
    let json = public_json(&output, 1);
    assert_eq!(json["execution"]["outcome"]["status"], "terminated");
    assert!(json["execution"].get("database").is_none());
    assert_stopped(leader);
    assert_reaped(leader);
    fixture.mode("success");
    assert!(
        fixture
            .database_command("status", "sqlite", true, true)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn leptos_plans_cannot_bootstrap_unverified_frontend_tools() {
    let fixture = Fixture::new();
    // Directory presence alone cannot certify tool versions or disable a
    // third-party build tool's automatic installation behavior.
    fs::create_dir(fixture.root.join("apps/web/src/node_modules")).unwrap();
    for intent in [OperationIntent::Develop, OperationIntent::ReleaseBuild] {
        let plan = fixture.plan(intent);
        let error = fixture
            .execute(&plan, &ExecutionControl::default())
            .unwrap_err();
        assert_eq!(error.code, "execution-readiness");
        assert!(!fixture.root.join("child.log").exists());
    }
}

#[test]
fn changed_preconditions_between_steps_stop_execution_and_release_coordination() {
    for mode in ["manifest", "executable", "auxiliary", "recovery"] {
        let fixture = Fixture::new();
        fixture.mode("release");
        let plan = fixture.plan(OperationIntent::Check);
        let auxiliary = fixture.parent.join("auxiliary");
        fs::create_dir(&auxiliary).unwrap();
        let tools = TrustedToolchain::resolve(
            &fixture.tools.join("cargo"),
            &[fixture.tools.clone(), auxiliary.clone()],
        )
        .unwrap();
        let control = ExecutionControl::default();
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                execute_application_operation(
                    &repository(),
                    &plan,
                    ExecutionConsent::ExecuteTrustedApplicationAndToolchain,
                    &tools,
                    &control,
                    ChildOutput::Discard,
                )
            });
            let child = pid(&fixture.root, "child.pid");
            match mode {
                "manifest" => {
                    let path = fixture.root.join("hegira.toml");
                    let mut bytes = fs::read(&path).unwrap();
                    bytes.extend_from_slice(b"\n# changed while the first step ran\n");
                    fs::write(path, bytes).unwrap();
                }
                "executable" => {
                    fs::rename(fixture.tools.join("cargo"), fixture.tools.join("retained"))
                        .unwrap();
                    fs::copy(fixture.tools.join("retained"), fixture.tools.join("cargo")).unwrap();
                }
                "auxiliary" => {
                    fs::rename(&auxiliary, fixture.parent.join("old-auxiliary")).unwrap();
                    fs::create_dir(&auxiliary).unwrap();
                }
                _ => fs::write(
                    fixture.root.join(application_mutator::MUTATION_MARKER),
                    "recovery pending",
                )
                .unwrap(),
            }
            fs::write(fixture.root.join("child-release"), "release").unwrap();
            let error = worker.join().unwrap().unwrap_err();
            assert_eq!(
                error.code,
                if mode == "recovery" {
                    "application-recovery"
                } else {
                    "execution-precondition"
                }
            );
            assert_reaped(child);
            assert_eq!(
                fs::read_to_string(fixture.root.join("child.log"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
        });
        if mode != "recovery" {
            fixture.mode("success");
            assert!(
                fixture
                    .execute(
                        &fixture.plan(OperationIntent::Check),
                        &ExecutionControl::default()
                    )
                    .unwrap()
                    .succeeded()
            );
        } else {
            assert!(
                fixture
                    .root
                    .join(application_mutator::MUTATION_MARKER)
                    .is_file()
            );
        }
    }
}
