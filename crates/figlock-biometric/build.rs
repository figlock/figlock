//! Builds the Swift shim for Secure Enclave quick unlock (`swift/enclave.swift`, ADR-0025) on macOS only.
//!
//! Without `swiftc`, or when it fails, the crate still builds and `enclave::available()` is false, so the CLI
//! keeps the keychain item. Other targets never reach this code: nothing here runs for Linux or Windows.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(figlock_enclave)");
    println!("cargo:rerun-if-changed=swift/enclave.swift");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        _ => return,
    };
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let lib = out.join("libfiglock_enclave.a");
    let built = Command::new("swiftc")
        .args(["-O", "-parse-as-library", "-emit-library", "-static"])
        .args(["-module-name", "FigLockEnclave"])
        .args(["-target", &format!("{arch}-apple-macos12.0")])
        .arg("swift/enclave.swift")
        .arg("-o")
        .arg(&lib)
        .status();
    if !built.is_ok_and(|s| s.success()) {
        println!(
            "cargo:warning=swiftc is missing or failed: Secure Enclave quick unlock is off in this build"
        );
        return;
    }
    println!("cargo:rustc-cfg=figlock_enclave");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=figlock_enclave");
    // The Swift runtime ships with macOS; the shim's autolink entries need to find its stubs at link time and
    // the loader needs the system path at run time.
    for dir in swift_library_dirs() {
        println!("cargo:rustc-link-search=native={dir}");
    }
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn swift_library_dirs() -> Vec<String> {
    let mut dirs = Vec::new();
    if let Some(sdk) = run("xcrun", &["--show-sdk-path"]) {
        dirs.push(format!("{sdk}/usr/lib/swift"));
    }
    if let Some(swiftc) = run("xcrun", &["--find", "swiftc"])
        && let Some(root) = PathBuf::from(swiftc).parent().and_then(|p| p.parent())
    {
        dirs.push(format!("{}/lib/swift/macosx", root.display()));
    }
    dirs
}
