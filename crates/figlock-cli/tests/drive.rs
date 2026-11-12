//! `figlock drive` as a person meets it without a Google account: nothing here talks to Google.

use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn figlock(home: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_figlock"))
        .args(args)
        .env("FIGLOCK_HOME", home.path())
        .env("FIGLOCK_DEVICE_KEYS", "file")
        .output()
        .unwrap()
}

#[test]
fn logout_without_a_sign_in_is_a_calm_no_op() {
    let home = TempDir::new().unwrap();
    let out = figlock(&home, &["drive", "logout"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "Not signed in to Google Drive.\n"
    );

    let json = figlock(&home, &["--json", "drive", "logout"]);
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["status"], "not-signed-in");
}

fn token_file(home: &TempDir, body: &[u8], mode: u32) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let f = home.path().join("drive-token.json");
    std::fs::write(&f, body).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode)).unwrap();
    f
}

fn logout_offline(home: &TempDir) -> Output {
    Command::new(env!("CARGO_BIN_EXE_figlock"))
        .args(["drive", "logout"])
        .env("FIGLOCK_HOME", home.path())
        .env("FIGLOCK_DEVICE_KEYS", "file")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("https_proxy", "http://127.0.0.1:9")
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn logout_deletes_a_token_file_other_users_can_read() {
    let home = TempDir::new().unwrap();
    let f = token_file(&home, br#"{"refresh_token":"not-a-real-token"}"#, 0o644);
    let out = logout_offline(&home);
    assert!(out.status.success());
    assert!(!f.exists());
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains("not-a-real-token"), "{all}");
}

#[cfg(unix)]
#[test]
fn logout_deletes_a_damaged_token_file_and_says_so() {
    let home = TempDir::new().unwrap();
    let f = token_file(&home, b"{}", 0o600);
    let out = logout_offline(&home);
    assert!(out.status.success());
    assert!(!f.exists());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("damaged"), "{stdout}");
    assert!(!stdout.contains("could not be reached"), "{stdout}");
}

#[test]
fn init_with_drive_and_a_folder_needs_the_sign_in_and_creates_nothing() {
    let home = TempDir::new().unwrap();
    let folder = home.path().join("vault-here");
    let out = figlock(
        &home,
        &[
            "init",
            "--drive",
            "--store",
            folder.to_str().unwrap(),
            "--kdf",
            "floor",
        ],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("figlock drive login"));
    assert!(!folder.exists());
    assert!(!home.path().join("config.json").exists());
}

#[test]
fn init_on_drive_without_a_sign_in_changes_nothing() {
    let home = TempDir::new().unwrap();
    let out = figlock(&home, &["init", "--drive", "--kdf", "floor"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("figlock drive login"), "{err}");
    let left: Vec<_> = std::fs::read_dir(home.path()).unwrap().collect();
    assert!(
        left.is_empty(),
        "no config or device keys were written: {left:?}"
    );
}
