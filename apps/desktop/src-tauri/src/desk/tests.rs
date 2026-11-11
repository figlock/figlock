//! The vault lifecycle against a real folder. The device config lives under `FIGLOCK_HOME`, and the workspace
//! forbids the `unsafe` that setting an environment variable needs, so each scenario re-runs this test
//! binary as a child with its own environment: the `#[test]` runs the child, the `#[ignore]`d function is
//! the scenario.

use std::process::Command;

use super::*;

pub(super) fn scenario_in(module: &str, name: &str) {
    let dir = tempfile::tempdir().unwrap();
    // A clipboard that is a file, so a scenario can see what a copy left and what a clear did.
    let clip = dir.path().join("clipboard");
    std::fs::write(&clip, "").unwrap();
    for (script, body) in [
        ("copy.sh", "#!/bin/sh\ncat > \"$FIGLOCK_TEST_CLIP\"\n"),
        ("paste.sh", "#!/bin/sh\ncat \"$FIGLOCK_TEST_CLIP\"\n"),
    ] {
        let path = dir.path().join(script);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("desk::{module}::{name}"),
            "--ignored",
            "--nocapture",
        ])
        .env("FIGLOCK_HOME", dir.path().join("home"))
        .env("FIGLOCK_DEVICE_KEYS", "file")
        .env("FIGLOCK_TEST_VAULT", dir.path().join("vault"))
        .env("FIGLOCK_TEST_CLIP", &clip)
        .env("FIGLOCK_CLIPBOARD_COPY", dir.path().join("copy.sh"))
        .env("FIGLOCK_CLIPBOARD_PASTE", dir.path().join("paste.sh"))
        .env("FIGLOCK_TEST_DIR", dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn scenario(name: &str) {
    scenario_in("tests", name);
}

/// A fresh master password for one scenario: generated, never a literal.
pub(super) fn password() -> String {
    SecretKey::random().unwrap().format().to_string()
}

pub(super) fn desk() -> Desk {
    Desk::with(
        KdfParams::FLOOR,
        Duration::from_secs(300),
        Duration::from_secs(30),
    )
}

pub(super) fn vault_dir() -> String {
    std::env::var("FIGLOCK_TEST_VAULT").unwrap()
}

/// The Secret Key of a vault being created, as the person would type it.
pub(super) fn created_key(d: &Desk, password: &str) -> String {
    d.create(&vault_dir(), password).unwrap().secret_key
}

pub(super) fn code_of(e: CmdError) -> &'static str {
    e.code
}

#[test]
fn create_lock_and_unlock_with_wrong_and_right_credentials() {
    scenario("create_lock_and_unlock_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn create_lock_and_unlock_inner() {
    let d = desk();
    let (pw, wrong) = (password(), password());
    assert_eq!(d.state().unwrap().phase, "none");
    assert_eq!(
        code_of(d.create(&vault_dir(), &pw[..4]).unwrap_err()),
        "weak_password"
    );
    let key = created_key(&d, &pw);
    // Nothing is on disk until the Secret Key has been confirmed.
    assert!(
        !std::path::Path::new(&vault_dir())
            .join("vault.json")
            .exists()
    );
    assert_eq!(d.state().unwrap().phase, "none");

    // Storage is never written before the Secret Key check has passed, whatever the window does.
    assert_eq!(
        code_of(d.finish_create(false).unwrap_err()),
        "key_not_confirmed"
    );
    assert!(
        !std::path::Path::new(&vault_dir())
            .join("vault.json")
            .exists()
    );
    assert_eq!(d.state().unwrap().phase, "none");

    // The forced check: the last two groups, typed back.
    let groups: Vec<&str> = key.split('-').collect();
    assert!(!d.check_kit_key("AAAA-AAAA").unwrap());
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    assert!(!d.emergency_kit().unwrap().is_empty());

    assert_eq!(d.finish_create(false).unwrap().phase, "unlocked");
    assert!(
        std::path::Path::new(&vault_dir())
            .join("vault.json")
            .exists()
    );

    d.lock();
    assert_eq!(d.state().unwrap().phase, "locked");

    let other = SecretKey::random().unwrap().format().to_string();
    assert_eq!(
        code_of(d.unlock(&wrong, &key).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.unlock(&pw, &other).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.unlock(&pw, "FL1-nope").unwrap_err()),
        "bad_secret_key"
    );
    assert_eq!(
        code_of(d.unlock(&pw[..0], &key).unwrap_err()),
        "empty_password"
    );
    assert_eq!(d.state().unwrap().phase, "locked");

    assert_eq!(d.unlock(&pw, &key).unwrap().phase, "unlocked");
    // Touch ID was never turned on, so it cannot unlock, and says to type the credentials.
    d.lock();
    assert_eq!(code_of(d.quick_unlock().unwrap_err()), "quick_unavailable");
    assert_eq!(d.state().unwrap().phase, "locked");
    // A second vault cannot be made over the first.
    assert!(d.create(&vault_dir(), &password()).is_err());
}

#[test]
fn an_abandoned_create_leaves_nothing_behind() {
    scenario("abandoned_create_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn abandoned_create_inner() {
    let d = desk();
    created_key(&d, &password());
    d.cancel_create();
    assert_eq!(code_of(d.finish_create(false).unwrap_err()), "failed");
    assert_eq!(d.state().unwrap().phase, "none");
    assert!(
        !std::path::Path::new(&vault_dir())
            .join("vault.json")
            .exists()
    );
}

#[test]
fn an_existing_vault_opens_on_a_new_device_only_with_both_credentials() {
    scenario("open_existing_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn open_existing_inner() {
    let (pw, wrong) = (password(), password());
    // Make a vault, then forget this device so the same folder is "someone else's" vault.
    let d = desk();
    let key = created_key(&d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    d.finish_create(false).unwrap();
    d.lock();
    config::forget();
    assert_eq!(d.state().unwrap().phase, "none");

    let other = SecretKey::random().unwrap().format().to_string();
    let folder = vault_dir();
    std::fs::create_dir_all(format!("{folder}/../empty")).unwrap();
    assert_eq!(
        code_of(d.open_existing(&folder, &wrong, &key, false).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.open_existing(&folder, &pw, &other, false).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.open_existing(&folder, &pw, "garbage", false).unwrap_err()),
        "bad_secret_key"
    );
    assert_eq!(
        code_of(
            d.open_existing(&format!("{folder}/../empty"), &pw, &key, false)
                .unwrap_err()
        ),
        "no_vault"
    );
    // The failures set nothing up.
    assert_eq!(d.state().unwrap().phase, "none");

    assert_eq!(
        d.open_existing(&folder, &pw, &key, false).unwrap().phase,
        "unlocked"
    );
}

#[test]
fn the_lock_timer_locks_on_idle_and_after_sleep() {
    let s = Duration::from_secs;
    assert!(!should_lock(s(10), s(1), s(300)));
    assert!(should_lock(s(300), s(1), s(300)));
    // The Mac slept: one tick arrives a long time after the last.
    assert!(should_lock(s(10), SLEEP_GAP, s(300)));
}

#[test]
fn the_date_on_the_kit_is_the_calendar_date() {
    assert_eq!(civil_date(0), "1970-01-01");
    assert_eq!(civil_date(1_700_000_000_000), "2023-11-14");
    assert_eq!(civil_date(951_782_400_000), "2000-02-29");
}
