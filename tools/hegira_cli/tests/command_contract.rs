use std::process::{Command, Output};

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
fn unfinished_new_command_fails_without_claiming_generation() {
    let result = hegira(&["new"]);

    assert_eq!(result.status.code(), Some(2));
    assert!(result.stdout.is_empty());
    assert_eq!(
        String::from_utf8(result.stderr).expect("diagnostic should be UTF-8"),
        concat!(
            "error: application generation arguments are not available in this CLI foundation\n",
            "hint: run `hegira new --help` to inspect the command contract\n"
        )
    );
}
