mod server;

fn main() -> std::process::ExitCode {
    runtime::run(server::serve)
}
