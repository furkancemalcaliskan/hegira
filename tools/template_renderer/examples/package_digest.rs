use std::{env, path::PathBuf, process::ExitCode};

use template_renderer::ManifestCatalog;

fn main() -> ExitCode {
    match run() {
        Ok(digest) => {
            println!("{digest}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("package digest: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let mut arguments = env::args().skip(1);
    let mut repository_root = None;
    let mut template = None;
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--repository-root" => repository_root = Some(PathBuf::from(value)),
            "--template" => template = Some(value),
            _ => return Err(format!("unknown argument: {flag}\n{}", usage())),
        }
    }
    ManifestCatalog::calculate_package_digest(
        repository_root.ok_or_else(usage)?,
        &template.ok_or_else(usage)?,
    )
    .map_err(|error| error.to_string())
}

fn usage() -> String {
    "usage: package_digest --repository-root <path> --template <id>".to_string()
}
