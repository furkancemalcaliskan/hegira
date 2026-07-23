#[cfg(feature = "ssr")]
fn main() -> std::process::ExitCode {
    hegira::runtime::run()
}

#[cfg(not(feature = "ssr"))]
pub fn main() {}
