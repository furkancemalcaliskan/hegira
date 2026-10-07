//! Release output ownership, never adoption or cleanup of ordinary Cargo output.

use super::*;
use std::collections::BTreeMap;

const MARKER: &str = ".hegira-release-build.json";
const ENTRY_LIMIT: usize = 200_000;

fn ownership() -> OperationError {
    failure(
        OperationErrorKind::Conflict,
        "release-output-ownership",
        "Release output must be an explicitly Hegira-owned, no-symlink directory; unclaimed output is never adopted or cleared. Inspect target/hegira/release-build manually.",
    )
}

pub(super) struct BuildOutputs {
    root: Directory,
    marker: Vec<u8>,
}

impl BuildOutputs {
    pub(super) fn prepare(plan: &OperationPlan) -> Result<Self, OperationError> {
        let marker = serde_json::to_vec(&serde_json::json!({
            "output_schema": 1,
            "owner": "hegira-release-build",
            "application": plan.summary.application,
        }))
        .map_err(|_| ownership())?;
        let mut parent = plan.anchor.directory.clone();
        for name in ["target", "hegira"] {
            ensure_directory(&parent, name)?;
            parent = Directory::open(&parent.path.join(name)).map_err(|_| ownership())?;
        }
        let fresh = match fs::mkdirat(&parent.fd, "release-build", Mode::from_raw_mode(0o700)) {
            Ok(()) => true,
            Err(rustix::io::Errno::EXIST) => false,
            Err(_) => return Err(ownership()),
        };
        let root = Directory::open(&parent.path.join("release-build")).map_err(|_| ownership())?;
        if fresh {
            // O_EXCL publication only into the directory just exclusively created.
            let file = fs::openat(
                &root.fd,
                MARKER,
                fs::OFlags::WRONLY
                    | fs::OFlags::CREATE
                    | fs::OFlags::EXCL
                    | fs::OFlags::NOFOLLOW
                    | fs::OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(|_| ownership())?;
            use std::io::Write;
            let mut file = std::fs::File::from(file);
            file.write_all(&marker).map_err(|_| ownership())?;
            file.sync_all().map_err(|_| ownership())?;
        }
        let outputs = Self { root, marker };
        outputs.verify(plan)?;
        Ok(outputs)
    }

    pub(super) fn verify(&self, plan: &OperationPlan) -> Result<(), OperationError> {
        plan.anchor.verify()?;
        self.root.verify().map_err(|_| ownership())?;
        if read_file(&self.root.fd, Path::new(MARKER), 4096).map_err(|_| ownership())?
            != self.marker
        {
            return Err(ownership());
        }
        // Cargo can hard-link its own executables inside the cache. Permit those,
        // but reject links whose other names are outside this owned directory.
        let mut links = BTreeMap::new();
        let mut remaining = ENTRY_LIMIT;
        walk(&self.root, &mut remaining, &mut links, 0)?;
        if links.values().any(|&(observed, total)| observed != total) {
            return Err(ownership());
        }
        Ok(())
    }

    pub(super) fn complete(&self, plan: &OperationPlan) -> Result<(), OperationError> {
        self.verify(plan)?;
        let server =
            open_file(&self.root.fd, Path::new("release/app_server")).map_err(|_| incomplete())?;
        let stat = fs::fstat(&server).map_err(|_| incomplete())?;
        let mut magic = [0; 4];
        if stat.st_mode & 0o111 == 0
            || stat.st_mode & 0o6000 != 0
            || rustix::io::pread(&server, &mut magic, 0).map_err(|_| incomplete())? != 4
            || magic != *b"\x7fELF"
        {
            return Err(incomplete());
        }
        let wasm = open_file(&self.root.fd, Path::new("site/pkg/app_bg.wasm"))
            .map_err(|_| incomplete())?;
        if rustix::io::pread(&wasm, &mut magic, 0).map_err(|_| incomplete())? != 4
            || magic != *b"\0asm"
        {
            return Err(incomplete());
        }
        for path in ["site/pkg/app.js", "site/pkg/app.css"] {
            let file = open_file(&self.root.fd, Path::new(path)).map_err(|_| incomplete())?;
            if fs::fstat(&file).map_err(|_| incomplete())?.st_size <= 0 {
                return Err(incomplete());
            }
        }
        Ok(())
    }
}

fn incomplete() -> OperationError {
    failure(
        OperationErrorKind::Validation,
        "release-artifacts-incomplete",
        "The release builder did not produce the expected native executable, browser WASM, JavaScript, and stylesheet; no successful artifact receipt was issued.",
    )
}

fn ensure_directory(parent: &Directory, name: &str) -> Result<(), OperationError> {
    parent.verify().map_err(|_| ownership())?;
    match fs::mkdirat(&parent.fd, name, Mode::from_raw_mode(0o700)) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(_) => return Err(ownership()),
    }
    Directory::open(&parent.path.join(name)).map_err(|_| ownership())?;
    Ok(())
}

fn walk(
    directory: &Directory,
    remaining: &mut usize,
    links: &mut BTreeMap<(u64, u64), (u64, u64)>,
    depth: usize,
) -> Result<(), OperationError> {
    if depth > 64 {
        return Err(ownership());
    }
    // Read through the opened directory, not a replaceable application pathname.
    let entries = std::fs::read_dir(descriptor_path(&directory.fd)).map_err(|_| ownership())?;
    for entry in entries {
        *remaining = remaining.checked_sub(1).ok_or_else(ownership)?;
        let name = entry.map_err(|_| ownership())?.file_name();
        let stat =
            fs::statat(&directory.fd, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| ownership())?;
        match fs::FileType::from_raw_mode(stat.st_mode) {
            fs::FileType::Directory => {
                let child =
                    Directory::open(&directory.path.join(&name)).map_err(|_| ownership())?;
                walk(&child, remaining, links, depth + 1)?;
            }
            fs::FileType::RegularFile => {
                let record = links
                    .entry((stat.st_dev as u64, stat.st_ino as u64))
                    .or_insert((0, stat.st_nlink as u64));
                record.0 += 1;
            }
            _ => return Err(ownership()),
        }
    }
    directory.verify().map_err(|_| ownership())
}
