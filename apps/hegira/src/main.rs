#[cfg(feature = "ssr")]
fn main() -> std::process::ExitCode {
    hegira::server::run()
}

#[cfg(not(feature = "ssr"))]
pub fn main() {}
