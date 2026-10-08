//! Operation doctor: no hooks, shims, installations, output claims, or services.

use super::*;
use crate::operations::{
    command::CommandSignals,
    readiness::{ReadinessCheck, ReadinessStatus as Status, ReadinessTools},
};
use leptos::{NativeTool, probe, selected, toml_source};

const LIMIT: u64 = 4 * 1024 * 1024;

fn check(
    code: &'static str,
    valid: bool,
    message: &'static str,
    action: &'static str,
) -> ReadinessCheck {
    ReadinessCheck::new(
        code,
        if valid { Status::Pass } else { Status::Warning },
        message,
        (!valid).then_some(action),
    )
}

fn blocked(code: &'static str, message: &'static str, action: &'static str) -> ReadinessCheck {
    ReadinessCheck::new(code, Status::Failure, message, Some(action))
}

pub(crate) fn diagnose_operation(
    plan: &OperationPlan,
    selection: Option<&ReadinessTools>,
) -> Vec<ReadinessCheck> {
    let lock = match fs::openat(&plan.anchor.directory.fd, ".", DIRECTORY, Mode::empty()) {
        Ok(lock) if fs::flock(&lock, FlockOperation::NonBlockingLockExclusive).is_ok() => lock,
        _ => {
            return vec![blocked(
                "operation-active",
                "Read-only operation diagnostics are blocked by concurrent execution or unsupported coordination.",
                "Wait for active Hegira operations to finish; do not remove recovery state.",
            )];
        }
    };
    // This separate open description releases its advisory lock without a file write.
    let _lock = lock;
    if plan.anchor.verify().is_err() || ensure_no_recovery(plan).is_err() {
        return vec![blocked(
            "operation-state",
            "Application identity changed or pending recovery blocks operation diagnostics.",
            "Inspect application identity and the documented manual recovery workflow.",
        )];
    }
    let channel = toml_source(plan, "rust-toolchain.toml")
        .ok()
        .and_then(|(_, rust)| {
            rust.get("toolchain")?
                .get("channel")?
                .as_str()
                .filter(|channel| {
                    let parts: Vec<_> = channel.split('.').collect();
                    parts.len() == 3
                        && parts.iter().all(|part| {
                            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                        })
                })
                .map(str::to_owned)
        });
    let lock = toml_source(plan, "Cargo.lock").ok().map(|(_, lock)| lock);
    let wasm_version = lock
        .as_ref()
        .and_then(|lock| lock.get("package")?.as_array())
        .and_then(|packages| {
            let versions: Vec<_> = packages
                .iter()
                .filter(|package| {
                    package.get("name").and_then(toml::Value::as_str) == Some("wasm-bindgen")
                })
                .filter_map(|package| package.get("version").and_then(toml::Value::as_str))
                .collect();
            (versions.len() == 1).then(|| versions[0].to_owned())
        });
    let control = ExecutionControl::default();
    let signals = selection.map(|_| CommandSignals::register(&control));
    if signals.as_ref().is_some_and(Result::is_err) {
        return vec![blocked(
            "operation-probe-control",
            "Safe tool-probe signal handling is unavailable.",
            "Retry on the supported Linux host; no tool was started.",
        )];
    }
    let mut checks = Vec::new();
    let toolchain = match selection {
        Some(selection) => {
            match TrustedToolchain::resolve(&selection.cargo, &selection.directories).and_then(
                |tools| {
                    tools.verify(plan)?;
                    Ok(tools)
                },
            ) {
                Ok(tools) => Some(tools),
                Err(_)
                    if selection.cargo.is_absolute()
                        && std::fs::symlink_metadata(&selection.cargo)
                            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
                {
                    checks.push(ReadinessCheck::new("operation-tool-selection", Status::Warning,
                        "The explicitly selected Cargo executable is missing; no tool was probed.",
                        Some("Install the application-pinned Cargo explicitly and select its real absolute path.")));
                    None
                }
                Err(_) => {
                    return vec![blocked(
                        "operation-tool-selection",
                        "Tool probes require absolute trusted native Cargo and external auxiliary directories.",
                        "Select real external native tools with --cargo and repeated --tool-directory; scripts and application PATH entries are not accepted.",
                    )];
                }
            }
        }
        None => None,
    };
    checks.push(check(
        "operation-workspace-manifest",
        toml_source(plan, "Cargo.toml").is_ok(),
        "The operation requires a real readable Cargo workspace manifest.",
        "Restore and review the application-owned Cargo.toml; doctor never invokes Cargo metadata.",
    ));
    for prerequisite in &plan.summary.prerequisites {
        let result = match prerequisite {
            OperationPrerequisite::ApplicationRustToolchain { .. } => {
                checks.push(check("operation-toolchain-file", channel.is_some(), "The application toolchain must pin a readable stable Rust release.", "Restore a real rust-toolchain.toml with the application-pinned stable Rust version; doctor installs nothing."));
                tool_check(
                    plan,
                    toolchain.as_ref(),
                    selection,
                    channel.as_deref(),
                    wasm_version.as_deref(),
                    ToolProbe::Rust,
                    &control,
                )
            }
            OperationPrerequisite::Cargo => tool_check(
                plan,
                toolchain.as_ref(),
                selection,
                channel.as_deref(),
                wasm_version.as_deref(),
                ToolProbe::Cargo,
                &control,
            ),
            OperationPrerequisite::CargoLockfile { .. } => check(
                "operation-cargo-lock",
                lock.as_ref()
                    .and_then(|lock| lock.get("package"))
                    .and_then(toml::Value::as_array)
                    .is_some_and(|packages| !packages.is_empty()),
                "Cargo.lock must be a readable, valid locked package graph.",
                "Review and restore the committed Cargo.lock manually; doctor does not resolve dependencies.",
            ),
            OperationPrerequisite::WasmTarget { .. } => tool_check(
                plan,
                toolchain.as_ref(),
                selection,
                channel.as_deref(),
                wasm_version.as_deref(),
                ToolProbe::WasmTarget,
                &control,
            ),
            OperationPrerequisite::CargoLeptos { .. } => {
                checks.push(tool_check(
                    plan,
                    toolchain.as_ref(),
                    selection,
                    channel.as_deref(),
                    wasm_version.as_deref(),
                    ToolProbe::Node,
                    &control,
                ));
                tool_check(
                    plan,
                    toolchain.as_ref(),
                    selection,
                    channel.as_deref(),
                    wasm_version.as_deref(),
                    ToolProbe::Leptos,
                    &control,
                )
            }
            OperationPrerequisite::FrontendDependencies { lockfile } => {
                let valid = read_file(&plan.anchor.directory.fd, Path::new(lockfile), LIMIT)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .is_some_and(|lock| lock["lockfileVersion"] == 3);
                check(
                    "operation-frontend-lock",
                    valid,
                    "The committed frontend lock must use the supported npm lockfile format.",
                    "Restore apps/web/src/package-lock.json and explicitly run npm ci --prefix apps/web/src after reviewing it.",
                )
            }
            OperationPrerequisite::LockfileWasmBindgen { .. } => {
                checks.push(check("operation-wasm-lock", wasm_version.is_some(), "Cargo.lock must select exactly one wasm-bindgen CLI version.", "Review the application lockfile; prepare its exact wasm-bindgen CLI explicitly with the documented script."));
                tool_check(
                    plan,
                    toolchain.as_ref(),
                    selection,
                    channel.as_deref(),
                    wasm_version.as_deref(),
                    ToolProbe::Bindgen,
                    &control,
                )
            }
            OperationPrerequisite::LockfileTailwindCli { .. } => {
                let frontend = leptos::frontend_sources(plan);
                match frontend {
                    Ok(_) => check(
                        "operation-frontend-state",
                        true,
                        "Canonical Leptos metadata, frontend receipts, Tailwind entry point, and required CSS files match the execution preflight; no frontend code was executed.",
                        "",
                    ),
                    Err(error) => ReadinessCheck::new(
                        "operation-frontend-state",
                        Status::Warning,
                        error.message,
                        Some(
                            "Review canonical Leptos metadata and source assets; explicitly prepare locked frontend dependencies. Use an owner-reviewed manual workflow for customized composition.",
                        ),
                    ),
                }
            }
            OperationPrerequisite::WasmOpt { .. } => tool_check(
                plan,
                toolchain.as_ref(),
                selection,
                channel.as_deref(),
                wasm_version.as_deref(),
                ToolProbe::Optimizer,
                &control,
            ),
            OperationPrerequisite::RuntimeConfiguration { profile } => {
                let path = format!("config/{}.yaml", profile.name());
                check(
                    "operation-runtime-profile",
                    open_file(&plan.anchor.directory.fd, Path::new(&path)).is_ok(),
                    "The selected runtime profile must be a real readable file; values and credentials were not loaded or validated.",
                    "Review the selected application-owned config profile before any explicitly authorized runtime or database operation.",
                )
            }
            OperationPrerequisite::PostgresService => ReadinessCheck::new(
                "operation-postgres-service",
                Status::Warning,
                "PostgreSQL connectivity and credentials were deliberately not probed.",
                Some(
                    "Review the intended database and runtime settings yourself before authorized startup; doctor never connects.",
                ),
            ),
            OperationPrerequisite::ApplicationDatabaseEntryPoint => blocked(
                "operation-database-entry-point",
                "CLI database execution is unavailable; a library plan does not invoke the separate application-owned entry point.",
                "Use an owner-reviewed application database workflow; public Hegira database execution is not supported.",
            ),
        };
        checks.push(result);
    }
    if matches!(
        plan.summary.intent,
        OperationIntent::Develop | OperationIntent::ReleaseBuild
    ) {
        checks.push(check(
            "operation-public-assets",
            Directory::open(&plan.root.join("apps/web/src/public")).is_ok(),
            "The canonical public asset directory must be real and readable.",
            "Restore apps/web/src/public without symlink redirection; doctor does not create it.",
        ));
        checks.push(ReadinessCheck::new("operation-frontend-execution", Status::Warning, "Frontend code and application hooks were not executed; runnable Tailwind and full startup/build preflight remain separate.", Some("Review these diagnostics, then use the explicit trusted dev/build execution preflight; this report is not execution consent.")));
    }
    if plan.summary.intent == OperationIntent::ReleaseBuild {
        checks.push(if artifacts::BuildOutputs::inspect(plan).is_ok() {
            check("operation-release-output", true, "The release root is absent or has a matching safe ownership claim; no directory or marker was created.", "")
        } else {
            blocked("operation-release-output", "Existing release output is unclaimed, unsafe, or inconsistent with this application.", "Inspect target/hegira/release-build manually; never forge its marker or delete unrelated output to bypass this diagnostic.")
        });
    }
    if plan.summary.policy.production_migration_approval_required {
        checks.push(ReadinessCheck::new("operation-production-approval", Status::Warning,
            "Production migration requires separate explicit owner approval; doctor cannot grant it.",
            Some("Review the production database, backups, migration plan, and approval outside this read-only diagnostic.")));
    }
    if plan.anchor.verify().is_err() || ensure_no_recovery(plan).is_err() {
        checks.push(blocked(
            "operation-state",
            "Application identity changed or recovery appeared during diagnostics.",
            "Inspect the application and rerun diagnostics; no execution authority was granted.",
        ));
    }
    if control.outcome().is_some() {
        checks.push(blocked("operation-probe-interrupted", "Tool diagnostics were stopped after a probe failure, interruption, or safe probe limit; no readiness or execution authority was established.",
            "Review the tool failure, interruption, or bounded probe limit and rerun doctor."));
    }
    checks
}

#[derive(Clone, Copy)]
enum ToolProbe {
    Cargo,
    Rust,
    WasmTarget,
    Leptos,
    Node,
    Bindgen,
    Optimizer,
}

impl ToolProbe {
    fn details(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Cargo => (
                "operation-cargo",
                "The selected Cargo version must match the application toolchain.",
                "Explicitly select trusted Cargo from the application-pinned stable Rust toolchain.",
            ),
            Self::Rust => (
                "operation-rust",
                "The selected Rust version must match rust-toolchain.toml.",
                "Explicitly install and select the application-pinned Rust compiler.",
            ),
            Self::WasmTarget => (
                "operation-wasm-target",
                "The selected Rust toolchain must contain the real WASM core target library.",
                "Explicitly run rustup target add wasm32-unknown-unknown for the pinned toolchain.",
            ),
            Self::Leptos => (
                "operation-cargo-leptos",
                "The selected Cargo Leptos must be version 0.3.7.",
                "Explicitly install cargo-leptos --locked --version 0.3.7.",
            ),
            Self::Node => (
                "operation-node",
                "The selected native Node.js must be version 22 or newer.",
                "Explicitly install and select a trusted native Node.js 22+ executable.",
            ),
            Self::Bindgen => (
                "operation-wasm-bindgen",
                "The selected wasm-bindgen CLI must exactly match Cargo.lock.",
                "Explicitly prepare the lock-matched wasm-bindgen CLI and select it with --wasm-bindgen.",
            ),
            Self::Optimizer => (
                "operation-wasm-opt",
                "The selected native Binaryen optimizer must be wasm-opt 123.",
                "Explicitly install reviewed Binaryen 123 and select its external native --wasm-opt executable.",
            ),
        }
    }
}

// Exactly seven inputs; none is an arbitrary command or application hook.
fn tool_check(
    plan: &OperationPlan,
    tools: Option<&TrustedToolchain>,
    selection: Option<&ReadinessTools>,
    channel: Option<&str>,
    wasm: Option<&str>,
    kind: ToolProbe,
    control: &ExecutionControl,
) -> ReadinessCheck {
    let (code, message, action) = kind.details();
    let Some((tools, selection, channel)) = tools
        .zip(selection)
        .zip(channel)
        .map(|((a, b), c)| (a, b, c))
    else {
        return ReadinessCheck::new(
            code,
            Status::Warning,
            "A required operation tool was not probed; readiness has not been established.",
            Some(
                "Use --probe-tools with explicit trusted --cargo and --tool-directory selections after restoring valid toolchain metadata.",
            ),
        );
    };
    let result = probe_tool(plan, tools, selection, channel, wasm, kind, control);
    check(code, result.unwrap_or(false), message, action)
}

fn probe_tool(
    plan: &OperationPlan,
    tools: &TrustedToolchain,
    selection: &ReadinessTools,
    channel: &str,
    wasm: Option<&str>,
    kind: ToolProbe,
    control: &ExecutionControl,
) -> Result<bool, OperationError> {
    plan.anchor.verify()?;
    tools.verify(plan)?;
    let (name, arguments): (&str, &[&str]) = match kind {
        ToolProbe::Cargo => ("cargo", &["--version"]),
        ToolProbe::Rust => ("rustc", &["--version"]),
        ToolProbe::WasmTarget => (
            "rustc",
            &[
                "--print",
                "target-libdir",
                "--target",
                "wasm32-unknown-unknown",
            ],
        ),
        ToolProbe::Leptos => ("cargo-leptos", &["leptos", "--version"]),
        ToolProbe::Node => ("node", &["--version"]),
        ToolProbe::Bindgen => ("wasm-bindgen", &["--version"]),
        ToolProbe::Optimizer => ("wasm-opt", &["--version"]),
    };
    let tool = match kind {
        ToolProbe::Cargo => NativeTool::open(&selection.cargo)?,
        ToolProbe::Bindgen => {
            NativeTool::open(selection.wasm_bindgen.as_deref().ok_or_else(unsafe_path)?)?
        }
        ToolProbe::Optimizer => {
            NativeTool::open(selection.wasm_opt.as_deref().ok_or_else(unsafe_path)?)?
        }
        _ => selected(name, tools, plan)?,
    };
    if tool.path.starts_with(&plan.root) {
        // Only the explicitly selected lock-matched CLI may use the documented
        // prepared-tool location. It is native tool code, never a project hook.
        if !matches!(kind, ToolProbe::Bindgen)
            || tool.path
                != plan
                    .root
                    .join("target/hegira-tools/wasm-bindgen/bin/wasm-bindgen")
        {
            return Err(unsafe_path());
        }
    }
    tool.verify()?;
    let mut command = Command::new(descriptor_path(&tool.file));
    command
        .arg0(name)
        .args(arguments)
        .current_dir("/")
        .env_clear()
        .env("PATH", &tools.search_path)
        .env("RUSTUP_TOOLCHAIN", channel)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::null())
        .process_group(0);
    // Rustup proxy discovery only. No runtime, Node preload, or Cargo hook env.
    for key in ["HOME", "USERPROFILE", "RUSTUP_HOME", "CARGO_HOME"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let output = match probe(command, control) {
        Ok(output) => output,
        Err(error) => {
            // A failed bounded probe must not turn into multiple hanging probes.
            control.cancel();
            return Err(error);
        }
    };
    tool.verify()?;
    tools.verify(plan)?;
    Ok(match kind {
        ToolProbe::Cargo | ToolProbe::Rust => {
            output.split_whitespace().nth(1) == Some(channel)
                && output.split_whitespace().next() == Some(name)
        }
        ToolProbe::Leptos => output.trim() == "cargo-leptos 0.3.7",
        ToolProbe::Node => output
            .trim()
            .strip_prefix('v')
            .and_then(|version| version.split('.').next())
            .and_then(|major| major.parse::<u32>().ok())
            .is_some_and(|major| major >= 22),
        ToolProbe::Bindgen => {
            wasm.is_some_and(|version| output.trim() == format!("wasm-bindgen {version}"))
        }
        ToolProbe::Optimizer => {
            ["wasm-opt version 123", "wasm-opt version 123 (version_123)"].contains(&output.trim())
        }
        ToolProbe::WasmTarget => {
            let target = Directory::open(Path::new(output.trim()))?;
            if target.path.starts_with(&plan.root) {
                return Err(unsafe_path());
            }
            let mut found = false;
            for entry in std::fs::read_dir(&target.path)
                .map_err(|_| unsafe_path())?
                .take(2048)
            {
                let entry = entry.map_err(|_| unsafe_path())?;
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("libcore-") && name.ends_with(".rlib"))
                {
                    open_file(&target.fd, Path::new(&entry.file_name()))?;
                    found = true;
                    break;
                }
            }
            target.verify()?;
            found
        }
    })
}
