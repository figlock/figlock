//! `figlock drive` as a person meets it without a Google account: nothing here talks to Google.

use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn figlock(home: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_figlock"))
        .args(args)
        .env("FIGLOCK_HOME", home.path())
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

#[cfg(unix)]
#[test]
fn a_token_file_other_users_can_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let home = TempDir::new().unwrap();
    let f = home.path().join("drive-token.json");
    std::fs::write(&f, br#"{"refresh_token":"not-a-real-token"}"#).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    let out = figlock(&home, &["drive", "logout"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("readable by other users"), "{err}");
    assert!(
        !err.contains("not-a-real-token"),
        "the token never appears in output"
    );
    assert!(
        f.exists(),
        "a refused token file is left for the owner to deal with"
    );
}
