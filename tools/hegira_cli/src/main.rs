use std::{env, io, process::ExitCode};

fn main() -> ExitCode {
    let stdout = io::stdout();
    let stderr = io::stderr();
    let exit = hegira_cli::run_from(env::args_os(), &mut stdout.lock(), &mut stderr.lock());
    ExitCode::from(exit.code())
}
