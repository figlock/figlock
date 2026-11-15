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

/// A device with sync on and a stored sign-in; `refused` is what the last sync learned from Google.
fn synced_device(home: &TempDir, refused: bool) {
    std::fs::write(
        home.path().join("config.json"),
        serde_json::json!({
            "store": home.path().join("vault"),
            "remote": "drive",
            "device_name": "Test",
        })
        .to_string(),
    )
    .unwrap();
    token_file(home, br#"{"refresh_token":"generated-test-token"}"#, 0o600);
    let state = if refused {
        serde_json::json!({ "synced": true, "last_sync_ms": 1, "sign_in_refused": true })
    } else {
        serde_json::json!({ "synced": true, "last_sync_ms": 1 })
    };
    std::fs::write(home.path().join("sync-state.json"), state.to_string()).unwrap();
}

#[cfg(unix)]
#[test]
fn sync_status_says_signed_in_until_google_refuses_the_sign_in() {
    let home = TempDir::new().unwrap();
    synced_device(&home, false);
    let out = figlock(&home, &["sync", "status"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("Signed in to Google: yes"));

    synced_device(&home, true);
    let out = figlock(&home, &["sync", "status"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Signed in to Google: no"), "{text}");
    assert!(
        text.contains("Google has stopped FigLock's access"),
        "{text}"
    );
    assert!(text.contains("figlock drive login"), "{text}");

    let json = figlock(&home, &["--json", "sync", "status"]);
    let v: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        (v["signed_in"].clone(), v["sign_in_refused"].clone()),
        (false.into(), true.into())
    );
}

/// FIG-122: deleting the Drive copy asks for the typed phrase first, and nothing reaches Google (or the local
/// folder, or the sync settings) without it.
#[cfg(unix)]
#[test]
fn deleting_the_drive_copy_needs_the_phrase_and_a_sign_in_and_changes_nothing_without_them() {
    let run = |home: &TempDir, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_figlock"))
            .args(args)
            .env("FIGLOCK_HOME", home.path())
            .env("FIGLOCK_DEVICE_KEYS", "file")
            // No route to Google: a request that was sent would fail loudly instead of passing quietly.
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("https_proxy", "http://127.0.0.1:9")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    };
    let text = |o: &Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )
    };

    // Not signed in.
    let home = TempDir::new().unwrap();
    std::fs::write(
        home.path().join("config.json"),
        serde_json::json!({ "store": home.path().join("vault"), "device_name": "Test" })
            .to_string(),
    )
    .unwrap();
    let out = run(
        &home,
        &[
            "sync",
            "delete-remote",
            "--confirm",
            "delete from Google Drive",
        ],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("not signed in"), "{}", text(&out));

    // Signed in, with sync on: no phrase, a wrong phrase and a script without --confirm all stop first.
    let home = TempDir::new().unwrap();
    synced_device(&home, false);
    let settings = std::fs::read(home.path().join("config.json")).unwrap();
    for args in [
        vec!["sync", "delete-remote"],
        vec!["sync", "delete-remote", "--confirm", "yes"],
        vec!["sync", "delete-remote", "--confirm", "Personal"],
    ] {
        let out = run(&home, &args);
        assert!(!out.status.success(), "{args:?}");
        let said = text(&out);
        assert!(
            said.contains("nothing was deleted") && said.contains("delete from Google Drive"),
            "{said}"
        );
    }
    assert_eq!(
        std::fs::read(home.path().join("config.json")).unwrap(),
        settings
    );
    assert!(home.path().join("sync-state.json").exists());
    assert!(home.path().join("drive-token.json").exists());
}
