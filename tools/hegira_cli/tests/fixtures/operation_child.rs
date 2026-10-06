//! Controlled native Cargo substitute used only by execution-boundary tests.
use std::{fs, io::Write, process::Command, thread, time::Duration};

fn wait_forever() -> ! {
    loop {
        thread::sleep(Duration::from_millis(20));
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("descendant") {
        fs::write("descendant.pid", std::process::id().to_string()).unwrap();
        wait_forever();
    }
    let mode = fs::read_to_string("child-mode").unwrap();
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("child.log")
        .unwrap();
    writeln!(log, "{args:?}").unwrap();
    fs::write("child.pid", std::process::id().to_string()).unwrap();
    fs::write(
        "child.cwd",
        std::env::current_dir().unwrap().to_str().unwrap(),
    )
    .unwrap();
    fs::write("child.path", std::env::var("PATH").unwrap()).unwrap();
    fs::write(
        "child.auto-install",
        std::env::var("RUSTUP_AUTO_INSTALL").unwrap(),
    )
    .unwrap();
    fs::write(
        "child.override",
        std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_default(),
    )
    .unwrap();
    match mode.trim() {
        "success" => {}
        "failure" => std::process::exit(23),
        "wait" => wait_forever(),
        "release" => {
            while !std::path::Path::new("child-release").exists() {
                thread::sleep(Duration::from_millis(5));
            }
        }
        "descendant" | "leftover" => {
            let _ = fs::remove_file("descendant.pid");
            // Children remain in the executor-created process group.
            let child = Command::new(std::env::current_exe().unwrap())
                .arg("descendant")
                .spawn()
                .unwrap();
            while !std::path::Path::new("descendant.pid").exists() {
                thread::sleep(Duration::from_millis(5));
            }
            if mode.trim() == "leftover" {
                // Deliberately exercise cleanup after the leader exits.
                drop(child);
            } else {
                wait_forever();
            }
        }
        _ => std::process::exit(24),
    }
}
