use std::{fs, path::Path};

fn main() {
    // `generate_context!` needs the front-end build output to exist. Lint and test runs that have not built
    // the UI get a placeholder; `pnpm build` replaces it.
    let dist = Path::new("../dist");
    if !dist.join("index.html").exists() {
        fs::create_dir_all(dist).expect("create dist");
        fs::write(
            dist.join("index.html"),
            "<!doctype html><title>FigLock</title>",
        )
        .expect("write placeholder index.html");
    }
    tauri_build::build();
}
