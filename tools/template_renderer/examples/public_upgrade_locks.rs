//! Resolve unchanged released-upgrade locks before any local-source staging.
#[path = "../../hegira_cli/tests/support/upgrade_matrix.rs"]
mod upgrade_matrix;

use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
};
use upgrade_test_support::{BaselineCatalog, BaselineVersion};

fn main() {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    assert!(
        matches!(arguments.len(), 3 | 4),
        "usage: public_upgrade_locks <repository-root> <new-output> <hegira-binary> [all|default|minimal|identity-added]"
    );
    let composition = arguments
        .get(3)
        .map_or("all", |value| value.to_str().unwrap());
    assert!(
        matches!(
            composition,
            "all" | "default" | "minimal" | "identity-added"
        ),
        "unsupported validation composition"
    );
    let repository = fs::canonicalize(&arguments[0]).unwrap();
    let output = PathBuf::from(&arguments[1]);
    let binary = fs::canonicalize(&arguments[2]).unwrap();
    fs::create_dir(&output).expect("validation output must be new");
    let baselines =
        BaselineCatalog::from_repository_release(&repository, BaselineVersion::V0_7_0).unwrap();
    for request in baselines.requests() {
        if composition != "all" && request.composition.as_str() != composition {
            continue;
        }
        let application = output.join(request.id());
        baselines
            .snapshot(request)
            .unwrap()
            .materialize(&application)
            .unwrap();
        upgrade_matrix::verify(&binary, &application);
        let before = upgrade_matrix::files(&application);
        let result = Command::new("cargo")
            .args([
                "metadata",
                "--locked",
                "--all-features",
                "--format-version",
                "1",
            ])
            .current_dir(&application)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(
            result.success(),
            "unchanged public lock must resolve before staging: {}",
            request.id()
        );
        assert_eq!(
            before,
            upgrade_matrix::files(&application),
            "public lock validation must not rewrite source"
        );
        println!("verified unchanged public upgrade lock: {}", request.id());
    }
}
