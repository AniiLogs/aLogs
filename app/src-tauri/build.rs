fn main() {
    // build.json carries this build's RC number (tauri-build.mjs writes it; the updater and About read it). It isn't
    // in the repository, so a plain `cargo build` of a fresh checkout gets RC 0.
    if !std::path::Path::new("build.json").exists() {
        std::fs::write("build.json", "{\"version\":\"0.0.0\",\"rc\":0}\n").expect("write build.json");
    }
    // A map of the program's functions beside a release build (target/release/alogs-app.map): what the addresses in
    // app.log's "window: not answering" line (watchdog.rs) are looked up in. It stays on the build PC; tauri-build.mjs
    // keeps one per test build.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        let map = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("target").join("release").join("alogs-app.map");
        println!("cargo:rustc-link-arg-bins=/MAP:{}", map.display());
    }
    tauri_build::build()
}
