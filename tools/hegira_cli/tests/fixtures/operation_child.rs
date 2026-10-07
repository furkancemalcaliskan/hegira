//! Controlled native Cargo substitute used only by execution-boundary tests.
use std::{fs, io::Write, process::Command, thread, time::Duration};

fn wait_forever() -> ! {
    loop {
        thread::sleep(Duration::from_millis(20));
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let name = std::env::args().next().unwrap();
    let name = std::path::Path::new(&name)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    if ["cargo-leptos", "wasm-bindgen", "wasm-opt", "rustc", "node"].contains(&name) {
        let mode = fs::read_to_string("probe-mode").unwrap_or_default();
        if mode.trim() == "hang" {
            fs::write("probe.pid", std::process::id().to_string()).unwrap();
            wait_forever();
        }
        if mode.trim() == "oversize" {
            println!("{}", "x".repeat(20_000));
            return;
        }
        match name {
            "wasm-opt" => println!(
                "wasm-opt version {}",
                if mode.trim() == "optimizer" { 122 } else { 123 }
            ),
            "cargo-leptos" => println!(
                "cargo-leptos {}",
                if mode.trim() == "leptos" {
                    "0.3.6"
                } else {
                    "0.3.7"
                }
            ),
            "wasm-bindgen" => println!(
                "wasm-bindgen {}",
                if mode.trim() == "wasm" {
                    "0.0.0".to_owned()
                } else {
                    fs::read_to_string("probe-wasm-version").unwrap()
                }
            ),
            "rustc" if args.first().map(String::as_str) == Some("--print") => {
                println!("{}", fs::read_to_string("probe-target").unwrap())
            }
            "rustc" => println!(
                "rustc {}",
                fs::read_to_string("probe-rust-version").unwrap()
            ),
            "node" if args.first().map(String::as_str) == Some("--version") => {
                println!("v{}.0.0", if mode.trim() == "node" { 20 } else { 22 })
            }
            "node" => println!(
                "tailwindcss v{}",
                fs::read_to_string("probe-tailwind-version").unwrap()
            ),
            _ => unreachable!(),
        }
        return;
    }
    if matches!(args.first().map(String::as_str), Some("metadata" | "build")) {
        assert_eq!(args.iter().filter(|arg| *arg == "--locked").count(), 1);
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("proxy.log")
            .unwrap();
        writeln!(log, "{args:?}").unwrap();
        return;
    }
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
    if args.first().map(String::as_str) == Some("leptos") {
        assert_eq!(std::env::var("LEPTOS_BIN_CARGO_COMMAND").unwrap(), "cargo");
        if args.get(1).map(String::as_str) == Some("build") {
            assert!(args.iter().any(|arg| arg == "--release"));
            let target = std::env::var("CARGO_TARGET_DIR").unwrap();
            assert_eq!(target, "target/hegira/release-build");
            assert_eq!(std::env::var("LEPTOS_BIN_TARGET_DIR").unwrap(), target);
            assert_eq!(
                std::env::var("LEPTOS_SITE_ROOT").unwrap(),
                "CARGO_TARGET_DIR/site"
            );
            assert_eq!(
                std::env::var("LEPTOS_ASSETS_DIR").unwrap(),
                "../web/src/public"
            );
            for args in [
                vec!["metadata", "--format-version", "1"],
                vec!["build", "--locked"],
            ] {
                assert!(
                    Command::new(std::env::var_os("CARGO").unwrap())
                        .args(args)
                        .status()
                        .unwrap()
                        .success()
                );
            }
            if mode.trim() == "failure" {
                std::process::exit(23);
            }
            if mode.trim() == "wait" {
                wait_forever();
            }
            if mode.trim() == "rewrite-lock" {
                fs::write("Cargo.lock", "unreviewed fixture lock").unwrap();
                return;
            }
            if mode.trim() == "incomplete" {
                return;
            }
            fs::create_dir_all(format!("{target}/release")).unwrap();
            fs::create_dir_all(format!("{target}/site/pkg")).unwrap();
            fs::copy(
                std::env::current_exe().unwrap(),
                format!("{target}/release/app_server"),
            )
            .unwrap();
            fs::write(format!("{target}/site/pkg/app_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
            fs::write(format!("{target}/site/pkg/app.js"), "controlled JavaScript").unwrap();
            fs::write(
                format!("{target}/site/pkg/app.css"),
                "controlled stylesheet",
            )
            .unwrap();
            return;
        }
        fs::write("child.profile", std::env::var("APP_ENV").unwrap()).unwrap();
        fs::write(
            "child.backend",
            std::env::var("APP__DATABASE__BACKEND").unwrap(),
        )
        .unwrap();
        fs::write("child.bind", std::env::var("APP__SERVER__ADDR").unwrap()).unwrap();
        fs::write(
            "child.leptos-bind",
            std::env::var("LEPTOS_SITE_ADDR").unwrap(),
        )
        .unwrap();
        for args in [
            vec!["metadata", "--format-version", "1"],
            vec!["build", "--locked"],
        ] {
            assert!(
                Command::new(std::env::var_os("CARGO").unwrap())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
    }
    match mode.trim() {
        "success" => {}
        "noisy" => {
            println!("fixture-child-output-only");
            eprintln!("fixture-child-diagnostic-only");
        }
        "failure" => std::process::exit(23),
        "failure-hydration" if args.iter().any(|arg| arg == "wasm32-unknown-unknown") => {
            std::process::exit(24);
        }
        "failure-hydration" => {}
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
