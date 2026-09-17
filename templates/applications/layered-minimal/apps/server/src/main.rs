#[cfg(feature = "ssr")]
fn main() -> std::process::ExitCode {
    app_server::server::run()
}

#[cfg(not(feature = "ssr"))]
fn main() {}
