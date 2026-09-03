use std::{collections::BTreeMap, env, path::PathBuf, process::ExitCode};

use template_renderer::{
    RenderRequest,
    repository_validation::{RepositoryValidationRequest, render},
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("repository validation renderer: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    if arguments.next().as_deref() != Some("render") {
        return Err(usage());
    }

    let mut repository_root = None;
    let mut template = None;
    let mut output = None;
    let mut framework_root = None;
    let mut framework_path = None;
    let mut variables = BTreeMap::new();

    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--repository-root" => repository_root = Some(PathBuf::from(value)),
            "--template" => template = Some(value),
            "--output" => output = Some(PathBuf::from(value)),
            "--framework-root" => framework_root = Some(PathBuf::from(value)),
            "--framework-path" => framework_path = Some(PathBuf::from(value)),
            "--set" => {
                let (name, value) = value
                    .split_once('=')
                    .ok_or_else(|| "--set requires NAME=VALUE".to_string())?;
                if variables
                    .insert(name.to_string(), value.to_string())
                    .is_some()
                {
                    return Err(format!("duplicate template variable override: {name}"));
                }
            }
            _ => return Err(format!("unknown argument: {flag}\n{}", usage())),
        }
    }

    let request = RepositoryValidationRequest {
        render: RenderRequest {
            repository_root: repository_root.ok_or_else(usage)?,
            template: template.ok_or_else(usage)?,
            output: output.ok_or_else(usage)?,
            variables,
        },
        framework_root: framework_root.ok_or_else(usage)?,
        framework_path,
    };
    let result = render(&request).map_err(|error| error.to_string())?;
    println!(
        "rendered repository validation template {} with {} components and {} files to {}",
        request.render.template,
        result.components.len(),
        result.files.len(),
        result.output.display()
    );
    Ok(())
}

fn usage() -> String {
    "usage: repository_validation_renderer render --repository-root <path> --template <id> --output <path> --framework-root <path> [--framework-path <path>] [--set NAME=VALUE]".to_string()
}
