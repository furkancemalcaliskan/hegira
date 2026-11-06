//! One descriptor-relative metadata walk. No output claims, subprocesses or cleanup.

use std::{collections::BTreeMap, path::Path};

use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, Dir, FileType, FlockOperation, Mode, Stat},
};

use super::{OperationError, OperationErrorKind, OperationPlan, anchors::*};
use crate::artifacts::{ArtifactRegion, ArtifactReport};

const RELEASE_ROOT: &str = "target/hegira/release-build";
const CACHE_ROOT: &str = "target/hegira/validation";
const MARKER: &str = ".hegira-release-build.json";
const ENTRY_LIMIT: usize = 200_000;
const DEPTH_LIMIT: usize = 64;

struct FileRecord {
    names: u64,
    links: u64,
    size: u64,
    allocated: u64,
}

struct Scan {
    regions: Vec<ArtifactRegion>,
    files: [BTreeMap<(u64, u64), FileRecord>; 3],
    diagnostics: BTreeMap<&'static str, OperationError>,
    remaining: usize,
    device: u64,
    marker: Vec<u8>,
}

impl Scan {
    fn issue(&mut self, region: usize, code: &'static str, message: &'static str) {
        self.regions[region].status = "incomplete";
        self.diagnostics
            .entry(code)
            .or_insert_with(|| OperationError::new(OperationErrorKind::Validation, code, message));
    }

    fn unsafe_entry(&mut self, region: usize) {
        if region == 1 && self.regions[1].owner == "none" {
            self.regions[1].owner = "unclaimed";
        }
        self.regions[region].skipped_entries += 1;
        self.issue(region, "artifact-unsafe-entry", "Symlinks, special files, mount boundaries or inaccessible/substituted entries were skipped; sizes are partial. Inspect output manually.");
    }

    fn record(&mut self, region: usize, stat: &Stat) {
        let Some(size) = u64::try_from(stat.st_size).ok() else {
            self.unsafe_entry(region);
            return;
        };
        let Some(allocated) = u64::try_from(stat.st_blocks)
            .ok()
            .and_then(|blocks| blocks.checked_mul(512))
        else {
            self.unsafe_entry(region);
            return;
        };
        let key = (stat.st_dev, stat.st_ino);
        if let Some(record) = self.files[region].get_mut(&key) {
            record.names += 1;
            if record.size != size || record.allocated != allocated || record.links != stat.st_nlink
            {
                self.issue(region, "artifact-changed", "Output changed during measurement; sizes are partial. Retry after active builds finish.");
            }
            return;
        }
        let entry = &mut self.regions[region];
        let totals = entry
            .logical_bytes
            .checked_add(size)
            .zip(entry.allocated_bytes.checked_add(allocated));
        let Some((logical, allocated_total)) = totals else {
            self.issue(
                region,
                "artifact-size-overflow",
                "Artifact sizes exceed supported counters; measurement is incomplete.",
            );
            return;
        };
        entry.logical_bytes = logical;
        entry.allocated_bytes = allocated_total;
        entry.unique_files += 1;
        self.files[region].insert(
            key,
            FileRecord {
                names: 1,
                links: stat.st_nlink,
                size,
                allocated,
            },
        );
    }

    fn walk(&mut self, directory: &OwnedFd, relative: &Path, region: usize, depth: usize) {
        if depth > DEPTH_LIMIT {
            self.issue(region, "artifact-scan-limit", "Artifact inspection exceeds 200000 entries or 64 directory levels; sizes are partial. Inspect oversized output manually.");
            return;
        }
        let Ok(before) = fs::fstat(directory) else {
            self.unsafe_entry(region);
            return;
        };
        let Ok(entries) = Dir::read_from(directory) else {
            self.unsafe_entry(region);
            return;
        };
        let mut names = Vec::new();
        for entry in entries {
            let Ok(entry) = entry else {
                self.unsafe_entry(region);
                return;
            };
            let name = entry.file_name();
            if matches!(name.to_bytes(), b"." | b"..") {
                continue;
            }
            let Some(remaining) = self.remaining.checked_sub(1) else {
                self.issue(region, "artifact-scan-limit", "Artifact inspection exceeds 200000 entries or 64 directory levels; sizes are partial. Inspect oversized output manually.");
                return;
            };
            self.remaining = remaining;
            names.push(name.to_owned());
        }
        names.sort();
        for name in names {
            use std::os::unix::ffi::OsStrExt;
            let path = relative.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
            // The dedicated ownership walker measures this subtree once below.
            if path == Path::new("hegira/validation") {
                continue;
            }
            let child_region = if path.starts_with("hegira/release-build") {
                1
            } else {
                region
            };
            let Ok(stat) = fs::statat(directory, &name, AtFlags::SYMLINK_NOFOLLOW) else {
                self.unsafe_entry(child_region);
                continue;
            };
            if stat.st_dev != self.device {
                self.unsafe_entry(child_region);
                continue;
            }
            match FileType::from_raw_mode(stat.st_mode) {
                FileType::Directory => {
                    let Ok(child) = fs::openat(directory, &name, DIRECTORY, Mode::empty()) else {
                        self.unsafe_entry(child_region);
                        continue;
                    };
                    if !same_entry(&child, &stat) {
                        self.unsafe_entry(child_region);
                        continue;
                    }
                    if path == Path::new("hegira/release-build") {
                        self.regions[1].status = "measured";
                        self.regions[1].owner = "unclaimed";
                        // Exact bytes match the existing executor's ownership contract.
                        if read_file(&child, Path::new(MARKER), 4096).ok().as_deref()
                            == Some(self.marker.as_slice())
                        {
                            self.regions[1].owner = "hegira-release-build";
                        } else {
                            self.issue(1, "artifact-release-unclaimed", "Release output has no matching ownership claim. Never forge a marker to adopt it; inspect the existing output manually.");
                        }
                    }
                    self.walk(&child, &path, child_region, depth + 1);
                    if path == Path::new("hegira/release-build")
                        && self.regions[1].owner == "hegira-release-build"
                        && read_file(&child, Path::new(MARKER), 4096).ok().as_deref()
                            != Some(self.marker.as_slice())
                    {
                        self.issue(1, "artifact-changed", "Output changed during measurement; sizes are partial. Retry after active builds finish.");
                    }
                    if fs::statat(directory, &name, AtFlags::SYMLINK_NOFOLLOW)
                        .ok()
                        .is_none_or(|current| {
                            current.st_dev != stat.st_dev || current.st_ino != stat.st_ino
                        })
                    {
                        self.unsafe_entry(child_region);
                    }
                }
                FileType::RegularFile => {
                    if path == Path::new("hegira/release-build") {
                        self.unsafe_entry(1);
                    } else {
                        self.record(child_region, &stat);
                    }
                }
                _ => self.unsafe_entry(child_region),
            }
        }
        if fs::fstat(directory).ok().is_none_or(|after| {
            before.st_mtime != after.st_mtime
                || before.st_mtime_nsec != after.st_mtime_nsec
                || before.st_ctime != after.st_ctime
                || before.st_ctime_nsec != after.st_ctime_nsec
        }) {
            self.issue(region, "artifact-changed", "Output changed during measurement; sizes are partial. Retry after active builds finish.");
            if relative.as_os_str().is_empty() || relative == Path::new("hegira") {
                self.issue(1, "artifact-changed", "Output changed during measurement; sizes are partial. Retry after active builds finish.");
            }
        }
    }
}

fn same_entry(fd: &OwnedFd, expected: &Stat) -> bool {
    fs::fstat(fd).is_ok_and(|stat| stat.st_dev == expected.st_dev && stat.st_ino == expected.st_ino)
}

pub(crate) fn inspect_artifact_usage(
    plan: &OperationPlan,
) -> Result<ArtifactReport, OperationError> {
    plan.anchor.verify()?;
    // A shared nonblocking lock observes the executor's exclusive root lock.
    // No persistent lock file is created. An active operation is never waited on.
    let root = fs::openat(&plan.anchor.directory.fd, ".", DIRECTORY, Mode::empty())
        .map_err(|_| unsafe_path())?;
    let activity = match fs::flock(&root, FlockOperation::NonBlockingLockShared) {
        Ok(()) => "idle",
        Err(rustix::io::Errno::WOULDBLOCK) => "active",
        Err(_) => "unknown",
    };
    let mut developer = ArtifactRegion::new("target", "developer-or-unclaimed");
    developer.excluded_paths.push(RELEASE_ROOT);
    developer.excluded_paths.push(CACHE_ROOT);
    let mut scan = Scan {
        regions: vec![
            developer,
            ArtifactRegion::new(RELEASE_ROOT, "none"),
            ArtifactRegion::new(CACHE_ROOT, "none"),
        ],
        files: Default::default(),
        diagnostics: BTreeMap::new(),
        remaining: ENTRY_LIMIT,
        device: fs::fstat(&root).map_err(|_| unsafe_path())?.st_dev,
        marker: crate::operations::release_output_marker(&plan.summary.application)
            .map_err(|_| unsafe_path())?,
    };
    if activity == "unknown" {
        scan.issue(0, "artifact-activity-unknown", "The filesystem cannot report application-operation locking. Activity is unknown; no cleanup is eligible.");
    }
    let target_name = c"target";
    match fs::statat(&root, target_name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(rustix::io::Errno::NOENT) => {}
        Ok(stat)
            if FileType::from_raw_mode(stat.st_mode).is_dir() && stat.st_dev == scan.device =>
        {
            let target = fs::openat(&root, target_name, DIRECTORY, Mode::empty())
                .map_err(|_| unsafe_path())?;
            if !same_entry(&target, &stat) {
                return Err(changed());
            }
            scan.regions[0].status = "measured";
            scan.walk(&target, Path::new(""), 0, 0);
            if fs::statat(&root, target_name, AtFlags::SYMLINK_NOFOLLOW)
                .ok()
                .is_none_or(|current| {
                    current.st_dev != stat.st_dev || current.st_ino != stat.st_ino
                })
            {
                return Err(changed());
            }
        }
        _ => {
            scan.unsafe_entry(0);
            scan.regions[1].status = "unavailable";
        }
    }
    if scan.files[1]
        .values()
        .any(|record| record.names != record.links)
    {
        scan.issue(1, "artifact-external-hard-link", "Release output has hard links outside its claimed root; it cannot be treated as CLI-owned. Inspect output manually.");
    }
    if scan.regions[0].status == "incomplete" && scan.regions[1].status == "missing" {
        scan.regions[1].status = "unavailable";
    }
    if scan.regions[1].status != "measured" && scan.regions[1].owner == "hegira-release-build" {
        scan.regions[1].owner = "unclaimed";
    }
    let validation_cache = match super::maintain_validation_cache(
        plan,
        super::super::ValidationCache::from_mib(super::super::ValidationCache::DEFAULT_BUDGET_MIB)?,
        crate::artifacts::CacheAction::Status,
    ) {
        Ok(report) => {
            if report.allocated_bytes > 0 {
                let region = &mut scan.regions[2];
                region.status = "measured";
                region.owner = "hegira-validation-cache";
                region.logical_bytes = report.logical_bytes;
                region.unique_files = 1 + report
                    .caches
                    .iter()
                    .map(|cache| cache.unique_files)
                    .sum::<u64>();
                region.allocated_bytes = report.allocated_bytes;
                region.cleanup_eligible = report.caches.iter().any(|cache| cache.cleanup_eligible);
                region.cleanup_reason = "Explicit artifacts prune revalidates ownership and activity; preview first. Allocated cache bytes include directory and ownership metadata.";
                Some(report)
            } else {
                None
            }
        }
        Err(error) => {
            scan.regions[2].owner = "unclaimed";
            scan.regions[2].status = "incomplete";
            scan.diagnostics.insert("artifact-validation-cache", error);
            None
        }
    };
    plan.anchor.verify()?;
    Ok(ArtifactReport {
        output_schema: 1,
        application: plan.summary.application.clone(),
        operation_activity: activity,
        budget_bytes: None,
        budget_status: "not-enforced",
        regions: scan.regions,
        exclusions: vec![
            "Global Cargo registry/Git/toolchain caches",
            "Docker images, containers and volumes",
            "External CARGO_TARGET_DIR and configured build/site output outside target",
        ],
        guidance: vec![
            "Sizes use logical bytes and allocated 512-byte blocks; hard links count once per region. Developer/release sizes exclude directory metadata; validation budgets include directory/claim metadata. Cross-region links may overlap.",
            "Only the fixed local target tree is measured, once, without following symlinks. Incomplete measurements are partial; active builds can change sizes immediately.",
            "Provider/features, native/WASM targets, debug/release profiles and incremental compilation grow separate artifacts; watch/development output has no size cap.",
            "Explicit --validation-cache check/test/build output has a separate pre/post budget and LRU lifecycle. artifacts cache-status accepts the selected --cache-budget-mib; ordinary developer and release output remain uncapped.",
            "Review developer output manually with the application owner. Never forge ownership markers or infer deletion authority from an idle observation.",
        ],
        diagnostics: scan.diagnostics.into_values().collect(),
        validation_cache,
    })
}
