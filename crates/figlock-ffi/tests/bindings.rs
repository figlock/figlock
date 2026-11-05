//! The generated bindings, built from this commit: the generator must produce Swift and Kotlin for the
//! compiled library, and (where `swiftc` is installed) a Swift program must compile against the generated
//! Swift and run the whole flow through the real library (`tests/swift/main.swift`).

use std::path::{Path, PathBuf};
use std::process::Command;

fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Build the cdylib for the profile this test runs under and return its path.
fn built_library() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe.parent().unwrap().parent().unwrap().to_path_buf(); // target/<profile>
    let release = profile_dir.file_name().is_some_and(|n| n == "release");
    let mut build = Command::new(cargo());
    build.args([
        "build",
        "-p",
        "figlock-ffi",
        "--features",
        "bindgen",
        "--lib",
    ]);
    if release {
        build.arg("--release");
    }
    let status = build.status().expect("run cargo build");
    assert!(status.success(), "cargo build of the library failed");
    let name = if cfg!(target_os = "macos") {
        "libfiglock_ffi.dylib"
    } else if cfg!(target_os = "windows") {
        "figlock_ffi.dll"
    } else {
        "libfiglock_ffi.so"
    };
    let lib = profile_dir.join(name);
    assert!(lib.exists(), "library not found at {}", lib.display());
    lib
}

fn generate(lib: &Path, language: &str, out: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_uniffi-bindgen"))
        .env("CARGO", cargo())
        .args(["generate", "--library"])
        .arg(lib)
        .args(["--language", language, "--no-format", "--out-dir"])
        .arg(out)
        .status()
        .expect("run uniffi-bindgen");
    assert!(status.success(), "uniffi-bindgen failed for {language}");
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn swift_and_kotlin_bindings_generate_from_the_built_library() {
    let lib = built_library();
    let tmp = tempfile::tempdir().unwrap();
    for (language, ext) in [("swift", "swift"), ("kotlin", "kt")] {
        let out = tmp.path().join(language);
        generate(&lib, language, &out);
        let files = files_under(&out);
        let source = files
            .iter()
            .find(|p| p.extension().is_some_and(|e| e == ext))
            .unwrap_or_else(|| panic!("no .{ext} file generated"));
        assert!(
            std::fs::metadata(source).unwrap().len() > 1000,
            "{language} bindings are empty"
        );
    }
}

#[test]
fn swift_program_runs_the_flow_through_the_generated_bindings() {
    if Command::new("swiftc").arg("--version").output().is_err() {
        eprintln!("swiftc not installed: skipping the Swift run (generation is still tested)");
        return;
    }
    let lib = built_library();
    let lib_dir = lib.parent().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("swift");
    generate(&lib, "swift", &gen_dir);

    let main = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/swift/main.swift");
    let exe = tmp.path().join("smoke");
    let compile = Command::new("swiftc")
        .current_dir(&gen_dir)
        .args([
            "-I",
            ".",
            "-Xcc",
            "-fmodule-map-file=figlock_ffiFFI.modulemap",
            "-L",
        ])
        .arg(lib_dir)
        .arg("-lfiglock_ffi")
        .arg("figlock_ffi.swift")
        .arg(&main)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run swiftc");
    assert!(
        compile.status.success(),
        "swiftc failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&exe)
        .env("DYLD_LIBRARY_PATH", lib_dir)
        .env("LD_LIBRARY_PATH", lib_dir)
        .output()
        .expect("run swift program");
    assert!(
        run.status.success(),
        "swift program failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "OK");
}
