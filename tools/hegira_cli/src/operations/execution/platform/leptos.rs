//! Lock-matched development tool preflight and a private, allowlisted tool PATH.

use super::*;
use std::{
    collections::BTreeMap,
    io::{ErrorKind, Read},
    os::unix::{fs::symlink, process::CommandExt},
    sync::atomic::{AtomicU64, Ordering},
};

const LIMIT: u64 = 4 * 1024 * 1024;
const PROBE_LIMIT: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
static NEXT: AtomicU64 = AtomicU64::new(0);

fn prerequisite(code: &'static str, message: &'static str) -> OperationError {
    failure(OperationErrorKind::Validation, code, message)
}

struct NativeTool {
    path: PathBuf,
    file: OwnedFd,
}

impl NativeTool {
    fn open(path: &Path) -> Result<Self, OperationError> {
        if !path.is_absolute() {
            return Err(unsafe_path());
        }
        let path = std::fs::canonicalize(path).map_err(|_| unsafe_path())?;
        let parent = Directory::open(path.parent().ok_or_else(unsafe_path)?)?;
        let file = open_file(
            &parent.fd,
            Path::new(path.file_name().ok_or_else(unsafe_path)?),
        )?;
        let stat = fs::fstat(&file).map_err(|_| unsafe_path())?;
        let mut magic = [0_u8; 4];
        if stat.st_mode & 0o111 == 0
            || stat.st_mode & 0o6000 != 0
            || rustix::io::pread(&file, &mut magic, 0).map_err(|_| unsafe_path())? != 4
            || magic != *b"\x7fELF"
        {
            return Err(prerequisite(
                "development-tool",
                "Development tools must be explicitly trusted native ELF executables; select their real binaries, not scripts.",
            ));
        }
        Ok(Self { path, file })
    }

    fn verify(&self) -> Result<(), OperationError> {
        if identity(&Self::open(&self.path)?.file)? != identity(&self.file)? {
            return Err(changed());
        }
        Ok(())
    }

    fn command(
        &self,
        name: &str,
        plan: &OperationPlan,
        toolchain: &TrustedToolchain,
    ) -> Result<Command, OperationError> {
        self.verify()?;
        let mut command = Command::new(descriptor_path(&self.file));
        command
            .arg0(name)
            .current_dir(descriptor_path(&plan.anchor.directory.fd))
            .env("PATH", &toolchain.search_path)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env_remove("RUSTUP_TOOLCHAIN")
            .stdin(Stdio::null())
            .process_group(0);
        Ok(command)
    }
}

fn selected(
    name: &str,
    toolchain: &TrustedToolchain,
    plan: &OperationPlan,
) -> Result<NativeTool, OperationError> {
    for directory in &toolchain.search_directories {
        let path = directory.path.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                let tool = NativeTool::open(&path)?;
                if tool.path.starts_with(&plan.root) {
                    return Err(unsafe_path());
                }
                return Ok(tool);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(unsafe_path()),
        }
    }
    Err(prerequisite(
        "development-tools-missing",
        "Install Rust with the wasm32-unknown-unknown target, cargo-leptos 0.3.7, and Node.js 22+ explicitly, then select their trusted --tool-directory entries; no tools were installed.",
    ))
}

/// Bounded, cancellable probes use the same owned process-group lifecycle.
/// Raw stdout is consumed privately for matching only; stderr is discarded.
fn probe(mut command: Command, control: &ExecutionControl) -> Result<String, OperationError> {
    if control.outcome().is_some() {
        return Err(probe_failure());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let child = command.spawn().map_err(|_| probe_failure())?;
    let mut child = OwnedChild::new(child)?;
    let mut stdout = child.child.stdout.take().ok_or_else(probe_failure)?;
    let flags = fs::fcntl_getfl(&stdout).map_err(|_| probe_failure())?;
    fs::fcntl_setfl(&stdout, flags | fs::OFlags::NONBLOCK).map_err(|_| probe_failure())?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut bytes = Vec::new();
    loop {
        if control.outcome().is_some() || Instant::now() >= deadline {
            return Err(probe_failure());
        }
        read_probe(&mut stdout, &mut bytes)?;
        if child.ended()? {
            let status = child.reap()?;
            read_probe(&mut stdout, &mut bytes)?;
            if !status.success() {
                return Err(probe_failure());
            }
            return String::from_utf8(bytes).map_err(|_| probe_failure());
        }
        thread::sleep(POLL);
    }
}

fn read_probe(stdout: &mut impl Read, bytes: &mut Vec<u8>) -> Result<(), OperationError> {
    let mut buffer = [0_u8; 4096];
    loop {
        match stdout.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) if bytes.len() + count <= PROBE_LIMIT => {
                bytes.extend_from_slice(&buffer[..count])
            }
            Ok(_) => return Err(probe_failure()),
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(probe_failure()),
        }
    }
    Ok(())
}

fn probe_failure() -> OperationError {
    prerequisite(
        "development-probe",
        "A trusted tool probe failed, exceeded its bounded output/time limit, or was interrupted; verify installed tool versions before starting development.",
    )
}

fn json(plan: &OperationPlan, path: &str) -> Result<(Vec<u8>, serde_json::Value), OperationError> {
    let bytes = read_file(&plan.anchor.directory.fd, Path::new(path), LIMIT).map_err(|_| prerequisite("development-frontend-lock", "Frontend prerequisites are missing or unsafe; run npm ci --prefix apps/web/src explicitly with the committed lockfile, then review installed files."))?;
    let value = serde_json::from_slice(&bytes).map_err(|_| prerequisite("development-frontend-lock", "Run npm ci --prefix apps/web/src explicitly with the committed lockfile before development; frontend metadata must be valid and lock-matched."))?;
    Ok((bytes, value))
}

fn toml_source(plan: &OperationPlan, path: &str) -> Result<(Vec<u8>, toml::Value), OperationError> {
    let bytes = read_file(&plan.anchor.directory.fd, Path::new(path), LIMIT)?;
    let source = std::str::from_utf8(&bytes).map_err(|_| unsafe_path())?;
    let value = toml::from_str(source).map_err(|_| prerequisite("development-metadata", "Development requires valid pinned Rust/Cargo metadata; inspect application files without changing their lockfiles."))?;
    Ok((bytes, value))
}

pub(super) struct Session {
    sources: BTreeMap<PathBuf, Vec<u8>>,
    tools: BTreeMap<&'static str, NativeTool>,
    shim: Shim,
    cargo_identity: (u64, u64),
}

impl Session {
    pub(super) fn prepare(
        plan: &OperationPlan,
        toolchain: &TrustedToolchain,
        control: &ExecutionControl,
    ) -> Result<Self, OperationError> {
        let (wasm, proxy) = toolchain.development_tools.as_ref().ok_or_else(|| prerequisite(
            "execution-readiness", "Leptos development requires explicit wasm-bindgen and native Hegira Cargo proxy selections plus lock-matched frontend preflight; no application was started."))?;
        let mut sources = BTreeMap::new();
        let (bytes, rust) = toml_source(plan, "rust-toolchain.toml")?;
        sources.insert("rust-toolchain.toml".into(), bytes);
        let channel = rust
            .get("toolchain")
            .and_then(|value| value.get("channel"))
            .and_then(toml::Value::as_str)
            .ok_or_else(unsafe_path)?;
        let (bytes, lock) = toml_source(plan, "Cargo.lock")?;
        sources.insert("Cargo.lock".into(), bytes);
        let packages = lock
            .get("package")
            .and_then(toml::Value::as_array)
            .ok_or_else(unsafe_path)?;
        let versions: Vec<_> = packages
            .iter()
            .filter(|package| {
                package.get("name").and_then(toml::Value::as_str) == Some("wasm-bindgen")
            })
            .filter_map(|package| package.get("version").and_then(toml::Value::as_str))
            .collect();
        if versions.len() != 1 {
            return Err(prerequisite(
                "development-wasm-lock",
                "Cargo.lock must select exactly one wasm-bindgen version; review the application lockfile manually.",
            ));
        }
        let (bytes, manifest) = toml_source(plan, "apps/server/Cargo.toml")?;
        sources.insert("apps/server/Cargo.toml".into(), bytes);
        let leptos = manifest
            .get("package")
            .and_then(|value| value.get("metadata"))
            .and_then(|value| value.get("leptos"))
            .ok_or_else(unsafe_path)?;
        if leptos
            .get("tailwind-input-file")
            .and_then(toml::Value::as_str)
            != Some("../web/src/style/tailwind.css")
            || leptos
                .get("bin-default-features")
                .and_then(toml::Value::as_bool)
                != Some(false)
            || leptos
                .get("lib-default-features")
                .and_then(toml::Value::as_bool)
                != Some(false)
            || leptos.get("bin-cargo-command").is_some()
        {
            return Err(prerequisite(
                "development-metadata",
                "The current development workflow requires canonical Tailwind and disabled native/hydration default-feature metadata; use a manually reviewed workflow for customized tool composition.",
            ));
        }
        for path in [
            "Cargo.toml",
            "apps/web/src/style/main.css",
            "apps/web/src/style/tailwind.css",
        ] {
            sources.insert(
                path.into(),
                read_file(&plan.anchor.directory.fd, Path::new(path), LIMIT)?,
            );
        }
        let profile = match plan.summary.database {
            application_manifest::DatabaseAdapter::Sqlite => "config/sqlite.yaml",
            application_manifest::DatabaseAdapter::Postgres => "config/development.yaml",
        };
        // Presence/type only: runtime secrets are not loaded or validated here.
        open_file(&plan.anchor.directory.fd, Path::new(profile))?;
        let (bytes, frontend_lock) = json(plan, "apps/web/src/package-lock.json")?;
        sources.insert("apps/web/src/package-lock.json".into(), bytes);
        let (bytes, installed) = json(plan, "apps/web/src/node_modules/.package-lock.json")?;
        sources.insert("apps/web/src/node_modules/.package-lock.json".into(), bytes);
        if frontend_lock["lockfileVersion"] != 3 || installed["lockfileVersion"] != 3 {
            return Err(prerequisite(
                "development-frontend-lock",
                "Run npm ci --prefix apps/web/src explicitly; development requires npm lockfileVersion 3.",
            ));
        }
        let (bytes, package) = json(
            plan,
            "apps/web/src/node_modules/@tailwindcss/cli/package.json",
        )?;
        sources.insert(
            "apps/web/src/node_modules/@tailwindcss/cli/package.json".into(),
            bytes,
        );
        let expected = frontend_lock["packages"]["node_modules/@tailwindcss/cli"]["version"]
            .as_str()
            .ok_or_else(unsafe_path)?;
        if package["version"] != expected
            || package["name"] != "@tailwindcss/cli"
            || package["bin"]["tailwindcss"] != "./dist/index.mjs"
        {
            return Err(prerequisite(
                "development-tailwind-version",
                "Installed @tailwindcss/cli must match the committed package lock; run npm ci explicitly, never an automatic dependency update.",
            ));
        }
        for name in [
            "node_modules/@tailwindcss/cli",
            "node_modules/tailwindcss",
            "node_modules/tw-animate-css",
        ] {
            for key in ["version", "resolved", "integrity"] {
                if frontend_lock["packages"][name][key].is_null()
                    || frontend_lock["packages"][name][key] != installed["packages"][name][key]
                {
                    return Err(prerequisite(
                        "development-frontend-lock",
                        "Installed frontend package receipts do not match the committed lock; run npm ci --prefix apps/web/src explicitly.",
                    ));
                }
            }
        }
        let tailwind = "apps/web/src/node_modules/@tailwindcss/cli/dist/index.mjs";
        sources.insert(
            tailwind.into(),
            read_file(&plan.anchor.directory.fd, Path::new(tailwind), LIMIT)?,
        );
        let tailwind_fd = open_file(&plan.anchor.directory.fd, Path::new(tailwind))?;
        let stat = fs::fstat(&tailwind_fd).map_err(|_| unsafe_path())?;
        if stat.st_mode & 0o111 == 0 || stat.st_mode & 0o6000 != 0 {
            return Err(unsafe_path());
        }
        let mut tools = BTreeMap::new();
        for name in ["cargo-leptos", "rustc", "node"] {
            tools.insert(name, selected(name, toolchain, plan)?);
        }
        tools.insert("wasm-bindgen", NativeTool::open(wasm)?);
        tools.insert("cargo", NativeTool::open(proxy)?);
        if tools["cargo"].path.starts_with(&plan.root) {
            return Err(unsafe_path());
        }
        let mut version = tools["cargo-leptos"].command("cargo-leptos", plan, toolchain)?;
        version.args(["leptos", "--version"]);
        if probe(version, control)?.trim() != "cargo-leptos 0.3.7" {
            return Err(prerequisite(
                "development-leptos-version",
                "Install cargo-leptos --locked --version 0.3.7 explicitly; development does not install or upgrade tools.",
            ));
        }
        let mut version = tools["wasm-bindgen"].command("wasm-bindgen", plan, toolchain)?;
        version.arg("--version");
        if probe(version, control)?.trim() != format!("wasm-bindgen {}", versions[0]) {
            return Err(prerequisite(
                "development-wasm-version",
                "The selected --wasm-bindgen must exactly match Cargo.lock; use the documented authenticated preparation script explicitly.",
            ));
        }
        let mut version = tools["rustc"].command("rustc", plan, toolchain)?;
        version.arg("--version");
        if probe(version, control)?.split_whitespace().nth(1) != Some(channel) {
            return Err(prerequisite(
                "development-rust-version",
                "Select the Rust version pinned in rust-toolchain.toml and explicitly install its wasm32-unknown-unknown target.",
            ));
        }
        let mut target = tools["rustc"].command("rustc", plan, toolchain)?;
        target.args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ]);
        let target = probe(target, control)?;
        let target = Directory::open(Path::new(target.trim())).map_err(|_| prerequisite("development-wasm-target", "Install wasm32-unknown-unknown for the application's pinned Rust toolchain explicitly; the target is unavailable."))?;
        let mut found = false;
        for entry in std::fs::read_dir(&target.path)
            .map_err(|_| unsafe_path())?
            .take(2048)
        {
            let entry = entry.map_err(|_| unsafe_path())?;
            let name = entry.file_name();
            if name
                .to_str()
                .is_some_and(|name| name.starts_with("libcore-") && name.ends_with(".rlib"))
            {
                open_file(&target.fd, Path::new(&name))?;
                found = true;
                break;
            }
        }
        if !found {
            return Err(prerequisite(
                "development-wasm-target",
                "Install wasm32-unknown-unknown for the pinned Rust toolchain explicitly; its core target library is missing.",
            ));
        }
        let mut version = tools["node"].command("node", plan, toolchain)?;
        version.arg("--version");
        let version = probe(version, control)?;
        if version
            .trim()
            .strip_prefix('v')
            .and_then(|version| version.split('.').next())
            .and_then(|major| major.parse::<u32>().ok())
            .is_none_or(|major| major < 22)
        {
            return Err(prerequisite(
                "development-node-version",
                "Select an explicitly installed Node.js 22+ native executable in the trusted tool directories.",
            ));
        }
        let mut help = tools["node"].command("node", plan, toolchain)?;
        help.arg(plan.root.join(tailwind)).arg("--help");
        if !probe(help, control)?.contains(&format!("tailwindcss v{expected}")) {
            return Err(prerequisite(
                "development-tailwind-version",
                "The lock-selected Tailwind CLI is not runnable with the selected Node; run npm ci explicitly and review the installed frontend.",
            ));
        }
        let mut shim = Shim::create(plan, &tools, &plan.root.join(tailwind))?;
        shim.cargo = toolchain.cargo_path.clone();
        let session = Self {
            sources,
            tools,
            shim,
            cargo_identity: identity(&toolchain.cargo)?,
        };
        session.verify(plan, toolchain)?;
        Ok(session)
    }

    pub(super) fn verify(
        &self,
        plan: &OperationPlan,
        toolchain: &TrustedToolchain,
    ) -> Result<(), OperationError> {
        plan.anchor.verify()?;
        toolchain.verify(plan)?;
        for (path, observed) in &self.sources {
            if &read_file(&plan.anchor.directory.fd, path, LIMIT)? != observed {
                return Err(changed());
            }
        }
        for tool in self.tools.values() {
            tool.verify()?;
        }
        self.shim.directory.verify()?;
        Ok(())
    }

    pub(super) fn configure(&self, command: &mut Command) -> Result<(), OperationError> {
        // The first tool PATH contains only reviewed names. It never includes
        // the general application node_modules/.bin directory.
        let auxiliary = command
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .and_then(|(_, value)| value)
            .ok_or_else(unsafe_path)?;
        let combined = std::env::join_paths(
            std::iter::once(self.shim.directory.path.clone())
                .chain(std::env::split_paths(auxiliary)),
        )
        .map_err(|_| unsafe_path())?;
        command
            .env("PATH", combined)
            .env("CARGO", self.shim.directory.path.join("cargo"))
            .env("HEGIRA_OPERATION_CARGO", &self.shim.cargo)
            .env(
                "HEGIRA_OPERATION_CARGO_ID",
                format!("{}:{}", self.cargo_identity.0, self.cargo_identity.1),
            )
            .env("HEGIRA_OPERATION_PROXY", "1");
        Ok(())
    }
}

struct Shim {
    parent: Directory,
    directory: Directory,
    name: std::ffi::OsString,
    names: Vec<&'static str>,
    cargo: PathBuf,
}

impl Shim {
    fn create(
        plan: &OperationPlan,
        tools: &BTreeMap<&'static str, NativeTool>,
        tailwind: &Path,
    ) -> Result<Self, OperationError> {
        let parent = Directory::open(
            &std::fs::canonicalize(std::env::temp_dir()).map_err(|_| unsafe_path())?,
        )?;
        if parent.path.starts_with(&plan.root) {
            return Err(unsafe_path());
        }
        let name = std::ffi::OsString::from(format!(
            "hegira-development-tools-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::mkdirat(&parent.fd, &name, Mode::from_raw_mode(0o700)).map_err(|_| unsafe_path())?;
        let directory = match Directory::open(&parent.path.join(&name)) {
            Ok(directory) => directory,
            Err(error) => {
                let _ = fs::unlinkat(&parent.fd, &name, AtFlags::REMOVEDIR);
                return Err(error);
            }
        };
        let mut shim = Self {
            parent,
            directory,
            name,
            names: Vec::new(),
            cargo: PathBuf::new(),
        };
        for (name, tool) in tools {
            symlink(&tool.path, shim.directory.path.join(name)).map_err(|_| unsafe_path())?;
            shim.names.push(name);
        }
        symlink(tailwind, shim.directory.path.join("tailwindcss")).map_err(|_| unsafe_path())?;
        shim.names.push("tailwindcss");
        Ok(shim)
    }
}

impl Drop for Shim {
    fn drop(&mut self) {
        // Cleanup only our opened private directory, never recursive user paths.
        for name in &self.names {
            let _ = fs::unlinkat(&self.directory.fd, *name, AtFlags::empty());
        }
        if self.directory.verify().is_ok() {
            let _ = fs::unlinkat(&self.parent.fd, &self.name, AtFlags::REMOVEDIR);
        }
    }
}

/// Private argv0-selected Cargo proxy used only by a running development session.
/// Cargo Leptos 0.3.7's initial metadata call otherwise lacks --locked.
pub(crate) fn cargo_proxy() -> Option<u8> {
    let args: Vec<_> = std::env::args_os().collect();
    if Path::new(args.first()?).file_name()? != "cargo"
        || std::env::var_os("HEGIRA_OPERATION_PROXY").as_deref() != Some(std::ffi::OsStr::new("1"))
    {
        return None;
    }
    let result = (|| {
        let selected = std::env::var_os("HEGIRA_OPERATION_CARGO").ok_or_else(unsafe_path)?;
        let tool = NativeTool::open(Path::new(&selected))?;
        let expected = std::env::var("HEGIRA_OPERATION_CARGO_ID").map_err(|_| unsafe_path())?;
        let observed = identity(&tool.file)?;
        if expected != format!("{}:{}", observed.0, observed.1) {
            return Err(changed());
        }
        let verb = args
            .get(1)
            .and_then(|arg| arg.to_str())
            .ok_or_else(unsafe_path)?;
        if !["metadata", "build", "check", "test", "--version", "-V"].contains(&verb) {
            return Err(unsafe_path());
        }
        let mut command = Command::new(descriptor_path(&tool.file));
        command.arg0("cargo").args(&args[1..]);
        if !["--version", "-V"].contains(&verb)
            && !args
                .iter()
                .any(|arg| arg == "--locked" || arg == "--frozen")
        {
            command.arg("--locked");
        }
        let _ = command.exec();
        Err::<(), _>(unsafe_path())
    })();
    let _ = result;
    eprintln!("development-cargo-proxy: cannot run the approved locked Cargo invocation");
    Some(1)
}
