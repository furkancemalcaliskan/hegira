//! Transport-only one-shot database entry point; no HTTP/application startup.
use app_infrastructure::database_operations::{
    DatabaseOperation, DatabaseOperationConfig, DatabaseOperationError, DatabaseOperationReport,
};
use std::{
    io::{self, Write},
    process::ExitCode,
};

const USAGE: &str = "usage: app_database <status|migrate> [--json]\nAPP_ENV selects development, sqlite, test, or production.\nDatabase access is explicit; migrate changes the selected database.\nCredentials must remain in runtime configuration/environment.";

fn parse(arguments: &[std::ffi::OsString]) -> Result<Option<(DatabaseOperation, bool)>, ()> {
    if arguments.len() == 1 && arguments[0] == "--help" {
        return Ok(None);
    }
    let operation = match arguments.first().and_then(|value| value.to_str()) {
        Some("status") => DatabaseOperation::Status,
        Some("migrate") => DatabaseOperation::Migrate,
        _ => return Err(()),
    };
    match &arguments[1..] {
        [] => Ok(Some((operation, false))),
        [flag] if flag == "--json" => Ok(Some((operation, true))),
        _ => Err(()),
    }
}

fn render(result: &Result<DatabaseOperationReport, DatabaseOperationError>, json: bool) -> String {
    if json {
        match result {
            Ok(report) => serde_json::to_string(report).expect("typed database report serializes"),
            Err(error) => serde_json::json!({
                "output_schema": 1,
                "outcome": "failure",
                "error": error.code(),
                "message": error.to_string(),
            })
            .to_string(),
        }
    } else {
        match result {
            Ok(report) => {
                let mut lines = vec![format!(
                    "Database {} {:?}: {:?}",
                    report.provider, report.operation, report.status.history,
                )];
                for entry in &report.status.migrations {
                    lines.push(format!(
                        "{} {} {:?}",
                        entry.version, entry.module_id, entry.state
                    ));
                }
                lines.join("\n")
            }
            Err(error) => error.to_string(),
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let (operation, json) = match parse(&arguments) {
        Ok(Some(request)) => request,
        Ok(None) => {
            return match writeln!(io::stdout().lock(), "{USAGE}") {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => ExitCode::FAILURE,
            };
        }
        Err(()) => {
            // Never echo unknown input, which may contain credentials.
            let _ = writeln!(io::stderr().lock(), "{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match DatabaseOperationConfig::load() {
        Ok(config) => app_infrastructure::database_operations::execute(operation, &config).await,
        Err(error) => Err(error),
    };
    let exit = result
        .as_ref()
        .map_or_else(|error| error.exit_code(), |_| 0);
    let output = render(&result, json);
    let written = if json || result.is_ok() {
        writeln!(io::stdout().lock(), "{output}")
    } else {
        writeln!(io::stderr().lock(), "{output}")
    };
    if written.is_err() {
        ExitCode::FAILURE
    } else {
        ExitCode::from(exit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_accepts_only_explicit_closed_operations() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            parse(&args(&["status", "--json"])),
            Ok(Some((DatabaseOperation::Status, true)))
        );
        assert_eq!(
            parse(&args(&["migrate"])),
            Ok(Some((DatabaseOperation::Migrate, false)))
        );
        assert_eq!(parse(&args(&["--help"])), Ok(None));
        for values in [
            vec![],
            vec!["reset"],
            vec!["rollback"],
            vec!["migrate", "--seed"],
            vec!["status", "--json", "--json"],
            vec!["status", "--url", "private"],
        ] {
            assert_eq!(parse(&args(&values)), Err(()));
        }
        let json = render(&Err(DatabaseOperationError::Configuration), true);
        let report: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(report["output_schema"], 1);
        assert_eq!(report["outcome"], "failure");
        assert!(!json.contains("private"));
    }
}
