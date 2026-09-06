use std::{
    env,
    io::{self, IsTerminal},
    process::ExitCode,
};

fn main() -> ExitCode {
    let stdout = io::stdout();
    let stderr = io::stderr();
    let stdin = io::stdin();
    let exit = if stdin.is_terminal() && stdout.is_terminal() {
        hegira_cli::run_interactive_from(
            env::args_os(),
            &mut stdin.lock(),
            &mut stdout.lock(),
            &mut stderr.lock(),
        )
    } else {
        hegira_cli::run_from(env::args_os(), &mut stdout.lock(), &mut stderr.lock())
    };
    ExitCode::from(exit.code())
}
