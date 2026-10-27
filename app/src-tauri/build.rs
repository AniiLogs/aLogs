fn main() {
    // build.json carries this build's RC number (tauri-build.mjs writes it; the updater and About read it). It isn't
    // in the repository, so a plain `cargo build` of a fresh checkout gets RC 0.
    if !std::path::Path::new("build.json").exists() {
        std::fs::write("build.json", "{\"version\":\"0.0.0\",\"rc\":0}\n").expect("write build.json");
    }
    tauri_build::build()
}
