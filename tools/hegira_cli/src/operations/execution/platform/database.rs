//! Fixed application-owned database invocation through the existing owned child lifecycle.
use super::super::database_protocol::{OUTPUT_LIMIT, protocol_error};
use super::*;
use application_manifest::DatabaseAdapter;
use std::io::{ErrorKind, Read};

pub(super) fn provider_name(provider: DatabaseAdapter) -> &'static str {
    match provider {
        DatabaseAdapter::Sqlite => "sqlite",
        DatabaseAdapter::Postgres => "postgres",
    }
}

pub(super) fn arguments(operation: DatabaseOperation, provider: DatabaseAdapter) -> Vec<String> {
    let features = format!("database-operations,db-{}", provider_name(provider));
    [
        "run",
        "--locked",
        "--quiet",
        "-p",
        "app_server",
        "--bin",
        "app_database",
        "--no-default-features",
        "--features",
        &features,
        "--",
        match operation {
            DatabaseOperation::Status => "status",
            DatabaseOperation::Migrate => "migrate",
        },
        "--json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn validate_entry_point(plan: &OperationPlan) -> Result<(), OperationError> {
    let invalid = || {
        failure(
            OperationErrorKind::Validation,
            "database-entry-point",
            "The application requires a real app_database binary, database-operations feature, Infrastructure operation source, and selected profile; no database command was started.",
        )
    };
    let bytes = read_file(
        &plan.anchor.directory.fd,
        Path::new("apps/server/Cargo.toml"),
        1024 * 1024,
    )
    .map_err(|_| invalid())?;
    let manifest: toml::Value = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|source| toml::from_str(source).ok())
        .ok_or_else(invalid)?;
    let bins = manifest
        .get("bin")
        .and_then(toml::Value::as_array)
        .ok_or_else(invalid)?;
    let matching: Vec<_> = bins
        .iter()
        .filter(|bin| bin.get("name").and_then(toml::Value::as_str) == Some("app_database"))
        .collect();
    let registered = matching.len() == 1
        && matching[0].get("path").and_then(toml::Value::as_str) == Some("src/bin/app_database.rs")
        && matching[0]
            .get("required-features")
            .and_then(toml::Value::as_array)
            .is_some_and(|features| {
                features
                    .iter()
                    .any(|feature| feature.as_str() == Some("database-operations"))
            });
    if manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        != Some("app_server")
        || !registered
        || manifest
            .get("features")
            .and_then(|f| f.get("database-operations"))
            .and_then(toml::Value::as_array)
            .is_none()
    {
        return Err(invalid());
    }
    for path in [
        "apps/server/src/bin/app_database.rs",
        "crates/infrastructure/src/database_operations.rs",
    ] {
        open_file(&plan.anchor.directory.fd, Path::new(path)).map_err(|_| invalid())?;
    }
    let profile = match plan.summary.intent {
        OperationIntent::DatabaseStatus { profile }
        | OperationIntent::DatabaseMigrate { profile } => profile,
        _ => return Err(invalid()),
    };
    open_file(
        &plan.anchor.directory.fd,
        Path::new(&format!("config/{}.yaml", profile.name())),
    )
    .map_err(|_| invalid())?;
    Ok(())
}

pub(super) fn run(
    mut command: Command,
    plan: &OperationPlan,
    control: &ExecutionControl,
) -> Result<(ExecutionOutcome, Option<DatabaseResult>), OperationError> {
    // Both modes discard compiler/driver output. Only the closed JSON protocol
    // can enter public output; even a nominally successful child is not trusted
    // to emit arbitrary text or a different operation/provider result.
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let child = command.spawn().map_err(|_| failure(OperationErrorKind::Internal,
        "execution-spawn", "Cannot start the approved application database command; OS and child details are redacted."))?;
    let mut child = OwnedChild::new(child)?;
    let mut stdout = child.child.stdout.take().ok_or_else(protocol_error)?;
    let flags = fs::fcntl_getfl(&stdout).map_err(|_| protocol_error())?;
    fs::fcntl_setfl(&stdout, flags | fs::OFlags::NONBLOCK).map_err(|_| protocol_error())?;
    let mut bytes = Vec::new();
    loop {
        if control.outcome().is_some() {
            return Ok((child.run(control)?, None));
        }
        read_output(&mut stdout, &mut bytes, control)?;
        if child.ended()? {
            let status = child.reap()?;
            if let Some(outcome) = control.outcome() {
                return Ok((outcome, None));
            }
            if !status.success() {
                return Ok((
                    if let Some(signal) = status.signal() {
                        ExecutionOutcome::ChildSignalled { signal }
                    } else {
                        ExecutionOutcome::ChildFailed {
                            exit_code: status.code().unwrap_or(1),
                        }
                    },
                    None,
                ));
            }
            read_output(&mut stdout, &mut bytes, control)?;
            let result = DatabaseResult::parse(&bytes, plan)?;
            if let Some(outcome) = control.outcome() {
                return Ok((outcome, None));
            }
            return Ok((ExecutionOutcome::Succeeded, Some(result)));
        }
        thread::sleep(POLL);
    }
}

fn read_output(
    stdout: &mut impl Read,
    bytes: &mut Vec<u8>,
    control: &ExecutionControl,
) -> Result<(), OperationError> {
    let mut buffer = [0_u8; 4096];
    while control.outcome().is_none() {
        match stdout.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) if bytes.len() + count <= OUTPUT_LIMIT => {
                bytes.extend_from_slice(&buffer[..count])
            }
            Ok(_) => return Err(protocol_error()),
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(protocol_error()),
        }
    }
    Ok(())
}
