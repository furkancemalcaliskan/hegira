fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "openapi")]
    embed_swagger_ui();
}

#[cfg(feature = "openapi")]
fn embed_swagger_ui() {
    use std::{env, fs, io::Cursor, path::PathBuf};

    // The official data crate is authenticated by Cargo.lock. No build-time
    // network download, arbitrary extraction path, or executable is needed.
    let mut archive =
        zip::ZipArchive::new(Cursor::new(utoipa_swagger_ui_vendored::SWAGGER_UI_VENDORED))
            .expect("official Swagger UI archive must be valid");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo supplies OUT_DIR"));
    for (asset, directory) in [
        ("swagger-ui.css", "dist/"),
        ("swagger-ui-bundle.js", "dist/"),
        ("swagger-ui-standalone-preset.js", "dist/"),
        ("favicon-16x16.png", "dist/"),
        ("favicon-32x32.png", "dist/"),
        ("oauth2-redirect.html", "dist/"),
        ("LICENSE", ""),
        ("NOTICE", ""),
    ] {
        let suffix = format!("/{directory}{asset}");
        let depth = if directory.is_empty() { 2 } else { 3 };
        let entries = archive
            .file_names()
            .map(|name| {
                name.expect("official asset name must be valid")
                    .into_owned()
            })
            .filter(|name| name.ends_with(&suffix) && name.split('/').count() == depth)
            .collect::<Vec<_>>();
        assert_eq!(
            entries.len(),
            1,
            "official Swagger UI asset must be unique: {asset}"
        );
        let mut entry = archive.by_name(&entries[0]).expect("asset must exist");
        let mut file = fs::File::create(output.join(asset)).expect("asset output must be writable");
        std::io::copy(&mut entry, &mut file).expect("asset must be readable");
    }
}
