#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
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
            ChildOutput, ExecutionConsent, ExecutionControl, ExecutionOutcome, TrustedToolchain,
            execute_application_operation,
        },
        plan_application_operation,
    },
};
use rustix::process::{Pid, Signal, WaitOptions, kill_process, waitpid};
use template_renderer::{RenderRequest, render};

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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
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
fn database_plans_never_select_or_execute_a_placeholder_binary() {
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
