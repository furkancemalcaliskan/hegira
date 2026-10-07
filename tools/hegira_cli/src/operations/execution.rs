//! Explicit, trusted operation execution; not an application sandbox.
//!
//! Public dev/check/test/build commands use this library. Callers must acknowledge
//! trusted application, toolchain, Cargo configuration, and inherited environment.
//! Child output is inherited or discarded, never captured in framework summaries.

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use serde::Serialize;

use super::{OperationError, OperationErrorKind, OperationIntent, OperationPlan};

pub const OPERATION_EXECUTION_SCHEMA: u32 = 1;

/// A caller's explicit trust decision, not a verification or sandbox certificate.
/// No default or deserialization path can turn a preview into consent.
#[derive(Debug, Clone, Copy)]
pub enum ExecutionConsent {
    ExecuteTrustedApplicationAndToolchain,
}

#[derive(Debug, Clone, Copy)]
pub enum ChildOutput {
    /// Arbitrary application/tool output may contain secrets; it is NOT redacted.
    Inherit,
    Discard,
}

/// The caller connects cancellation/termination (including OS signals) here.
/// The library does not install process-global signal handlers.
#[derive(Debug, Clone, Default)]
pub struct ExecutionControl(Arc<AtomicUsize>);

impl ExecutionControl {
    #[cfg(target_os = "linux")]
    pub(crate) fn signal_state(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.0)
    }

    pub fn cancel(&self) {
        self.0.store(1, Ordering::Release);
    }

    pub fn terminate(&self) {
        self.0.store(2, Ordering::Release);
    }

    fn outcome(&self) -> Option<ExecutionOutcome> {
        match self.0.load(Ordering::Acquire) {
            1 => Some(ExecutionOutcome::Cancelled),
            2 => Some(ExecutionOutcome::Terminated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ExecutionOutcome {
    Succeeded,
    ChildFailed { exit_code: i32 },
    ChildSignalled { signal: i32 },
    Cancelled,
    Terminated,
}

/// Does not include paths, environment values, source, or child output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionReport {
    pub output_schema: u32,
    pub intent: OperationIntent,
    pub completed_steps: usize,
    pub outcome: ExecutionOutcome,
    /// Verified output locations only after a successful release build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<super::ReleaseBuildArtifacts>,
}

impl ExecutionReport {
    pub fn succeeded(&self) -> bool {
        self.outcome == ExecutionOutcome::Succeeded
    }

    /// A child returning zero is not sufficient: cancelled or incomplete
    /// operations and lifecycle errors must remain nonzero at the CLI boundary.
    pub fn exit(&self) -> crate::CliExit {
        if self.succeeded() {
            crate::CliExit::Success
        } else {
            crate::CliExit::Internal
        }
    }
}

/// Re-authenticates the plan and executes explicitly trusted application steps.
/// Leptos operations require installed frontend tools and an explicit selection;
/// database entry points remain unavailable. Plans never authorize installation.
pub fn execute_application_operation(
    repository_root: &Path,
    plan: &OperationPlan,
    consent: ExecutionConsent,
    toolchain: &TrustedToolchain,
    control: &ExecutionControl,
    output: ChildOutput,
) -> Result<ExecutionReport, OperationError> {
    let ExecutionConsent::ExecuteTrustedApplicationAndToolchain = consent;
    platform::execute(repository_root, plan, toolchain, control, output)
}

fn failure(kind: OperationErrorKind, code: &'static str, message: &'static str) -> OperationError {
    OperationError::new(kind, code, message)
}

fn unsupported() -> OperationError {
    failure(
        OperationErrorKind::Validation,
        "execution-platform",
        "Anchored process execution requires Linux and an accessible /proc/self/fd.",
    )
}

pub(super) use anchors::RootAnchor;
pub use platform::TrustedToolchain;

pub(crate) fn diagnose_operation(
    plan: &OperationPlan,
    tools: Option<&super::readiness::ReadinessTools>,
) -> Vec<super::readiness::ReadinessCheck> {
    platform::diagnose_operation(plan, tools)
}

/// Internal binary entry point for the private development Cargo proxy.
#[doc(hidden)]
pub fn development_cargo_proxy() -> Option<u8> {
    #[cfg(target_os = "linux")]
    {
        platform::development_cargo_proxy()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
mod anchors {
    use super::*;
    use application_manifest::ApplicationManifest;
    use rustix::{
        fd::OwnedFd,
        fs::{self, Mode, OFlags},
    };
    use std::{
        io::Read,
        path::{Component, PathBuf},
    };

    pub(super) const DIRECTORY: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const FILE: OFlags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::NONBLOCK)
        .union(OFlags::CLOEXEC);

    #[derive(Clone)]
    pub(crate) struct RootAnchor {
        pub(super) directory: Directory,
        manifest: Vec<u8>,
    }

    impl PartialEq for RootAnchor {
        fn eq(&self, other: &Self) -> bool {
            self.directory == other.directory && self.manifest == other.manifest
        }
    }
    impl Eq for RootAnchor {}

    impl RootAnchor {
        pub(crate) fn capture(root: &Path) -> Result<Self, OperationError> {
            let directory = Directory::open(root)?;
            let manifest = read_file(&directory.fd, Path::new("hegira.toml"), 1024 * 1024)?;
            directory.verify()?;
            Ok(Self {
                directory,
                manifest,
            })
        }

        pub(crate) fn matches_manifest(
            &self,
            manifest: &ApplicationManifest,
        ) -> Result<(), OperationError> {
            let captured = std::str::from_utf8(&self.manifest)
                .ok()
                .and_then(|source| ApplicationManifest::from_toml(source).ok());
            if captured.as_ref() != Some(manifest) {
                return Err(changed());
            }
            Ok(())
        }

        pub(super) fn verify(&self) -> Result<(), OperationError> {
            self.directory.verify()?;
            if read_file(&self.directory.fd, Path::new("hegira.toml"), 1024 * 1024)?
                != self.manifest
            {
                return Err(changed());
            }
            Ok(())
        }
    }

    #[derive(Clone)]
    pub(super) struct Directory {
        pub(super) path: PathBuf,
        pub(super) fd: Arc<OwnedFd>,
        identity: (u64, u64),
    }

    impl PartialEq for Directory {
        fn eq(&self, other: &Self) -> bool {
            self.path == other.path && self.identity == other.identity
        }
    }
    impl Eq for Directory {}

    impl Directory {
        pub(super) fn open(path: &Path) -> Result<Self, OperationError> {
            if !path.is_absolute() {
                return Err(unsafe_path());
            }
            let mut fd = fs::open("/", DIRECTORY, Mode::empty()).map_err(|_| unsafe_path())?;
            for component in path.components() {
                match component {
                    Component::Normal(name) => {
                        fd = fs::openat(&fd, name, DIRECTORY, Mode::empty())
                            .map_err(|_| unsafe_path())?;
                    }
                    Component::RootDir => {}
                    _ => return Err(unsafe_path()),
                }
            }
            let identity = identity(&fd)?;
            Ok(Self {
                path: path.to_owned(),
                fd: Arc::new(fd),
                identity,
            })
        }

        pub(super) fn verify(&self) -> Result<(), OperationError> {
            if Self::open(&self.path)?.identity != self.identity {
                return Err(changed());
            }
            Ok(())
        }
    }

    pub(super) fn open_file(root: &OwnedFd, path: &Path) -> Result<OwnedFd, OperationError> {
        let mut parent =
            fs::openat(root, ".", DIRECTORY, Mode::empty()).map_err(|_| unsafe_path())?;
        let parts: Vec<_> = path.components().collect();
        for (index, part) in parts.iter().enumerate() {
            let Component::Normal(name) = part else {
                return Err(unsafe_path());
            };
            if index + 1 == parts.len() {
                let fd =
                    fs::openat(&parent, *name, FILE, Mode::empty()).map_err(|_| unsafe_path())?;
                let stat = fs::fstat(&fd).map_err(|_| unsafe_path())?;
                if !fs::FileType::from_raw_mode(stat.st_mode).is_file() {
                    return Err(unsafe_path());
                }
                return Ok(fd);
            }
            parent =
                fs::openat(&parent, *name, DIRECTORY, Mode::empty()).map_err(|_| unsafe_path())?;
        }
        Err(unsafe_path())
    }

    pub(super) fn read_file(
        root: &OwnedFd,
        path: &Path,
        limit: u64,
    ) -> Result<Vec<u8>, OperationError> {
        let fd = open_file(root, path)?;
        let stat = fs::fstat(&fd).map_err(|_| unsafe_path())?;
        if stat.st_size < 0 || stat.st_size as u64 > limit {
            return Err(unsafe_path());
        }
        let mut bytes = Vec::new();
        std::fs::File::from(fd)
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unsafe_path())?;
        if bytes.len() as u64 > limit {
            return Err(unsafe_path());
        }
        Ok(bytes)
    }

    pub(super) fn identity(fd: &OwnedFd) -> Result<(u64, u64), OperationError> {
        let stat = fs::fstat(fd).map_err(|_| unsafe_path())?;
        Ok((stat.st_dev as u64, stat.st_ino as u64))
    }

    pub(super) fn unsafe_path() -> OperationError {
        failure(
            OperationErrorKind::Validation,
            "execution-path",
            "Execution requires accessible real directories and regular prerequisite files.",
        )
    }

    pub(super) fn changed() -> OperationError {
        failure(
            OperationErrorKind::Conflict,
            "execution-precondition",
            "The approved application or toolchain identity changed; inspect and approve a new plan.",
        )
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
mod anchors {
    use super::*;
    #[derive(Clone, PartialEq, Eq)]
    pub(crate) struct RootAnchor;
    impl RootAnchor {
        pub(crate) fn capture(_: &Path) -> Result<Self, OperationError> {
            Err(unsupported())
        }
        pub(crate) fn matches_manifest(
            &self,
            _: &application_manifest::ApplicationManifest,
        ) -> Result<(), OperationError> {
            Err(unsupported())
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    mod artifacts;
    mod leptos;
    mod readiness;
    use super::{anchors::*, *};
    use crate::{ApplicationContextRequest, operations::*};
    use application_mutator::MUTATION_MARKER;
    pub(super) use leptos::cargo_proxy as development_cargo_proxy;
    pub(super) use readiness::diagnose_operation;
    use rustix::{
        fd::{AsRawFd, OwnedFd},
        fs::{self, AtFlags, FlockOperation, Mode},
        process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid},
    };
    use std::{
        fmt,
        os::unix::process::{CommandExt, ExitStatusExt},
        path::PathBuf,
        process::{Child, Command, ExitStatus, Stdio},
        thread,
        time::{Duration, Instant},
    };

    const POLL: Duration = Duration::from_millis(20);
    const GRACE: Duration = Duration::from_millis(250);

    /// Explicit absolute Cargo selection and absolute auxiliary tool directories.
    /// Resolves ordinary rustup proxy symlinks once, then pins the native ELF
    /// executable and directory identities. Never searches ambient/project PATH.
    pub struct TrustedToolchain {
        requested_cargo: PathBuf,
        cargo_path: PathBuf,
        cargo: OwnedFd,
        search_directories: Vec<Directory>,
        search_path: std::ffi::OsString,
        development_tools: Option<(PathBuf, PathBuf)>,
        release_optimizer: Option<PathBuf>,
    }

    impl fmt::Debug for TrustedToolchain {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("TrustedToolchain (machine-local paths omitted)")
        }
    }

    impl TrustedToolchain {
        /// This only anchors local tools; it spawns no process and installs nothing.
        /// The caller must trust these tools, their directories, and the environment.
        pub fn resolve(
            cargo: &Path,
            search_directories: &[PathBuf],
        ) -> Result<Self, OperationError> {
            if !cargo.is_absolute()
                || search_directories.is_empty()
                || search_directories.iter().any(|path| !path.is_absolute())
            {
                return Err(unsafe_path());
            }
            let cargo_path = std::fs::canonicalize(cargo).map_err(|_| unsafe_path())?;
            let parent = Directory::open(cargo_path.parent().ok_or_else(unsafe_path)?)?;
            let fd = open_file(
                &parent.fd,
                Path::new(cargo_path.file_name().ok_or_else(unsafe_path)?),
            )?;
            let stat = fs::fstat(&fd).map_err(|_| unsafe_path())?;
            let mut magic = [0_u8; 4];
            if rustix::io::pread(&fd, &mut magic, 0).map_err(|_| unsafe_path())? != 4 {
                return Err(unsafe_path());
            }
            if magic != *b"\x7fELF" || stat.st_mode & 0o111 == 0 || stat.st_mode & 0o6000 != 0 {
                return Err(failure(
                    OperationErrorKind::Validation,
                    "execution-tool",
                    "Cargo must resolve to a trusted native ELF executable without set-id permissions.",
                ));
            }
            let directories = search_directories
                .iter()
                .map(|path| {
                    let path = std::fs::canonicalize(path).map_err(|_| unsafe_path())?;
                    Directory::open(&path)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let search_path = std::env::join_paths(directories.iter().map(|dir| &dir.path))
                .map_err(|_| unsafe_path())?;
            Ok(Self {
                requested_cargo: cargo.to_owned(),
                cargo_path,
                cargo: fd,
                search_directories: directories,
                search_path,
                development_tools: None,
                release_optimizer: None,
            })
        }

        /// Explicitly trust the lock-matched wasm CLI and source-built Hegira
        /// native Cargo proxy. Resolution/probes happen only during execution.
        pub fn with_development_tools(
            mut self,
            wasm_bindgen: &Path,
            cargo_proxy: &Path,
        ) -> Result<Self, OperationError> {
            if !wasm_bindgen.is_absolute() || !cargo_proxy.is_absolute() {
                return Err(unsafe_path());
            }
            self.development_tools = Some((wasm_bindgen.to_owned(), cargo_proxy.to_owned()));
            Ok(self)
        }

        /// Select an already installed, explicitly trusted native Binaryen optimizer.
        pub fn with_release_optimizer(mut self, wasm_opt: &Path) -> Result<Self, OperationError> {
            if !wasm_opt.is_absolute() {
                return Err(unsafe_path());
            }
            self.release_optimizer = Some(wasm_opt.to_owned());
            Ok(self)
        }

        fn verify(&self, plan: &OperationPlan) -> Result<(), OperationError> {
            if self.cargo_path.starts_with(&plan.root)
                || self
                    .search_directories
                    .iter()
                    .any(|dir| dir.path.starts_with(&plan.root))
            {
                return Err(failure(
                    OperationErrorKind::Validation,
                    "application-toolchain",
                    "Executable and auxiliary tool directories must be outside the application root.",
                ));
            }
            if std::fs::canonicalize(&self.requested_cargo).map_err(|_| changed())?
                != self.cargo_path
            {
                return Err(changed());
            }
            let parent = Directory::open(self.cargo_path.parent().ok_or_else(unsafe_path)?)?;
            let current = open_file(
                &parent.fd,
                Path::new(self.cargo_path.file_name().ok_or_else(unsafe_path)?),
            )?;
            if identity(&current)? != identity(&self.cargo)? {
                return Err(changed());
            }
            let stat = fs::fstat(&self.cargo).map_err(|_| unsafe_path())?;
            let mut magic = [0_u8; 4];
            if stat.st_mode & 0o6000 != 0
                || rustix::io::pread(&self.cargo, &mut magic, 0).map_err(|_| unsafe_path())? != 4
                || magic != *b"\x7fELF"
            {
                return Err(changed());
            }
            for directory in &self.search_directories {
                directory.verify()?;
            }
            Ok(())
        }
    }

    pub(super) fn execute(
        repository_root: &Path,
        plan: &OperationPlan,
        toolchain: &TrustedToolchain,
        control: &ExecutionControl,
        output: ChildOutput,
    ) -> Result<ExecutionReport, OperationError> {
        // Unsupported steps are rejected for the entire operation, not halfway.
        if plan.summary.steps.iter().any(|step| {
            !matches!(
                step,
                OperationStep::Tool {
                    program: OperationProgram::Cargo,
                    ..
                }
            )
        }) {
            return Err(failure(
                OperationErrorKind::Validation,
                "execution-entry-point",
                "This operation requires an unavailable application-owned database entry point.",
            ));
        }
        if !matches!(
            plan.summary.intent,
            OperationIntent::Check
                | OperationIntent::Test
                | OperationIntent::Develop
                | OperationIntent::ReleaseBuild
        ) {
            return Err(failure(
                OperationErrorKind::Validation,
                "execution-readiness",
                "This operation is unavailable; the executor supports check, test, and explicitly prepared Leptos development/release builds only.",
            ));
        }
        if let Some(outcome) = control.outcome() {
            return Ok(report(plan, 0, outcome));
        }
        plan.anchor.verify()?;
        let lock = fs::openat(&plan.anchor.directory.fd, ".", DIRECTORY, Mode::empty())
            .map_err(|_| unsafe_path())?;
        fs::flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
            if error == rustix::io::Errno::WOULDBLOCK {
                failure(
                    OperationErrorKind::Conflict,
                    "execution-active",
                    "Another application operation is active; retry after it finishes.",
                )
            } else {
                failure(
                    OperationErrorKind::Validation,
                    "execution-lock",
                    "The filesystem does not support safe application-operation coordination.",
                )
            }
        })?;
        // The independent open description owns this advisory directory lock.
        // No persistent lock file is created or unlinked; close releases it.
        ensure_no_recovery(plan)?;
        let current = plan_application_operation(
            repository_root,
            &OperationRequest {
                application: ApplicationContextRequest::explicit(&plan.root, &plan.root),
                intent: plan.summary.intent,
            },
        )?;
        if current != *plan {
            return Err(changed());
        }
        validate_prerequisite_paths(plan)?;
        toolchain.verify(plan)?;
        let cargo_path = descriptor_path(&toolchain.cargo);
        let root_path = descriptor_path(&plan.anchor.directory.fd);
        verify_descriptor(&cargo_path, &toolchain.cargo)?;
        verify_descriptor(&root_path, &plan.anchor.directory.fd)?;
        let development = if matches!(
            plan.summary.intent,
            OperationIntent::Develop | OperationIntent::ReleaseBuild
        ) {
            match leptos::Session::prepare(plan, toolchain, control) {
                Ok(session) => Some(session),
                Err(error) => {
                    if let Some(outcome) = control.outcome() {
                        return Ok(report(plan, 0, outcome));
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        let build_outputs = if plan.summary.intent == OperationIntent::ReleaseBuild {
            Some(artifacts::BuildOutputs::prepare(plan)?)
        } else {
            None
        };

        for (index, step) in plan.summary.steps.iter().enumerate() {
            if let Some(outcome) = control.outcome() {
                return Ok(report(plan, index, outcome));
            }
            plan.anchor.verify()?;
            ensure_no_recovery(plan)?;
            validate_prerequisite_paths(plan)?;
            toolchain.verify(plan)?;
            if let Some(session) = &development {
                session.verify(plan, toolchain)?;
            }
            if let Some(outputs) = &build_outputs {
                outputs.verify(plan)?;
            }
            let OperationStep::Tool {
                arguments,
                environment,
                ..
            } = step
            else {
                unreachable!("all steps validated before spawning");
            };
            let mut command = Command::new(&cargo_path);
            command
                .arg0("cargo")
                .args(arguments)
                .current_dir(&root_path)
                .env("PATH", &toolchain.search_path)
                .env("RUSTUP_AUTO_INSTALL", "0")
                .env_remove("RUSTUP_TOOLCHAIN")
                .envs(environment)
                .stdin(Stdio::null())
                .process_group(0);
            if let Some(session) = &development {
                session.configure(&mut command)?;
            }
            match output {
                ChildOutput::Inherit => {
                    command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
                }
                ChildOutput::Discard => {
                    command.stdout(Stdio::null()).stderr(Stdio::null());
                }
            }
            let child = command.spawn().map_err(|_| failure(
                OperationErrorKind::Internal, "execution-spawn", "Cannot start the approved application tool; no child output or OS details are included."
            ))?;
            let outcome = OwnedChild::new(child)?.run(control)?;
            if outcome != ExecutionOutcome::Succeeded {
                return Ok(report(plan, index, outcome));
            }
        }
        if let Some(outputs) = &build_outputs {
            // Trusted builders must not silently rewrite reviewed dependency locks.
            development
                .as_ref()
                .expect("release frontend preflight")
                .verify(plan, toolchain)?;
            outputs.complete(plan)?;
        }
        // A cancellation arriving during the final cleanup still is not success.
        Ok(report(
            plan,
            plan.summary.steps.len(),
            control.outcome().unwrap_or(ExecutionOutcome::Succeeded),
        ))
    }

    fn report(
        plan: &OperationPlan,
        completed_steps: usize,
        outcome: ExecutionOutcome,
    ) -> ExecutionReport {
        let artifacts = (plan.summary.intent == OperationIntent::ReleaseBuild
            && outcome == ExecutionOutcome::Succeeded)
            .then(ReleaseBuildArtifacts::default);
        ExecutionReport {
            output_schema: OPERATION_EXECUTION_SCHEMA,
            intent: plan.summary.intent,
            completed_steps,
            outcome,
            artifacts,
        }
    }

    fn ensure_no_recovery(plan: &OperationPlan) -> Result<(), OperationError> {
        match fs::statat(
            &plan.anchor.directory.fd,
            MUTATION_MARKER,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Err(rustix::io::Errno::NOENT) => Ok(()),
            Ok(_) => Err(failure(
                OperationErrorKind::Conflict,
                "application-recovery",
                "Application mutation recovery is pending; follow the documented recovery workflow before executing code.",
            )),
            Err(_) => Err(unsafe_path()),
        }
    }

    fn validate_prerequisite_paths(plan: &OperationPlan) -> Result<(), OperationError> {
        for path in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
            open_file(&plan.anchor.directory.fd, Path::new(path))?;
        }
        Ok(())
    }

    fn descriptor_path(fd: &OwnedFd) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd()))
    }

    fn verify_descriptor(path: &Path, expected: &OwnedFd) -> Result<(), OperationError> {
        let actual = fs::open(
            path,
            fs::OFlags::RDONLY.union(fs::OFlags::CLOEXEC),
            Mode::empty(),
        )
        .map_err(|_| unsupported())?;
        if identity(&actual)? != identity(expected)? {
            return Err(unsupported());
        }
        Ok(())
    }

    struct OwnedChild {
        child: Child,
        group: Pid,
        reaped: bool,
    }

    impl OwnedChild {
        fn new(mut child: Child) -> Result<Self, OperationError> {
            let Some(group) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) else {
                let _ = child.kill();
                let _ = child.wait();
                return Err(cleanup_failure());
            };
            Ok(Self {
                child,
                group,
                reaped: false,
            })
        }

        fn ended(&self) -> Result<bool, OperationError> {
            // WNOWAIT retains the leader's PID until group cleanup finishes.
            // try_wait would reap it early and allow a recycled PGID to be killed.
            match waitid(
                WaitId::Pid(self.group),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            ) {
                Ok(status) => Ok(status.is_some()),
                Err(rustix::io::Errno::INTR) => Ok(false),
                Err(_) => Err(cleanup_failure()),
            }
        }

        fn signal(&self, signal: Signal) -> Result<(), OperationError> {
            match kill_process_group(self.group, signal) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
                Err(_) => Err(cleanup_failure()),
            }
        }

        fn reap(&mut self) -> Result<ExitStatus, OperationError> {
            self.signal(Signal::KILL)?;
            let status = self.child.wait().map_err(|_| cleanup_failure())?;
            self.reaped = true;
            Ok(status)
        }

        fn run(mut self, control: &ExecutionControl) -> Result<ExecutionOutcome, OperationError> {
            loop {
                if let Some(outcome) = control.outcome() {
                    self.signal(Signal::TERM)?;
                    let deadline = Instant::now() + GRACE;
                    while !self.ended()? && Instant::now() < deadline {
                        thread::sleep(POLL);
                    }
                    self.reap()?;
                    return Ok(outcome);
                }
                if self.ended()? {
                    let status = self.reap()?;
                    if let Some(outcome) = control.outcome() {
                        return Ok(outcome);
                    }
                    return if status.success() {
                        Ok(ExecutionOutcome::Succeeded)
                    } else if let Some(signal) = status.signal() {
                        Ok(ExecutionOutcome::ChildSignalled { signal })
                    } else {
                        Ok(ExecutionOutcome::ChildFailed {
                            exit_code: status.code().unwrap_or(1),
                        })
                    };
                }
                thread::sleep(POLL);
            }
        }
    }

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if !self.reaped {
                // Covers unwinding and every error after spawn. Never leave an
                // owned direct child unreaped just because reporting failed.
                let _ = self.signal(Signal::KILL);
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    fn cleanup_failure() -> OperationError {
        failure(
            OperationErrorKind::Internal,
            "execution-cleanup",
            "The owned child lifecycle could not be completed safely; execution is not successful.",
        )
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;
    pub(super) fn diagnose_operation(
        _: &OperationPlan,
        _: Option<&crate::operations::readiness::ReadinessTools>,
    ) -> Vec<crate::operations::readiness::ReadinessCheck> {
        use crate::operations::readiness::{ReadinessCheck, ReadinessStatus};
        vec![ReadinessCheck::new(
            "operation-platform",
            ReadinessStatus::Failure,
            "Operation readiness diagnostics require Linux with accessible proc descriptors.",
            Some("Use the supported Linux host; no tool was probed."),
        )]
    }
    use std::path::PathBuf;

    #[derive(Debug)]
    pub struct TrustedToolchain;
    impl TrustedToolchain {
        pub fn resolve(_: &Path, _: &[PathBuf]) -> Result<Self, OperationError> {
            Err(unsupported())
        }
        pub fn with_development_tools(self, _: &Path, _: &Path) -> Result<Self, OperationError> {
            Err(unsupported())
        }
        pub fn with_release_optimizer(self, _: &Path) -> Result<Self, OperationError> {
            Err(unsupported())
        }
    }
    pub(super) fn execute(
        _: &Path,
        _: &OperationPlan,
        _: &TrustedToolchain,
        _: &ExecutionControl,
        _: ChildOutput,
    ) -> Result<ExecutionReport, OperationError> {
        Err(unsupported())
    }
}
