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

/// A folder as the window sends it.
pub(super) fn folder(path: &str) -> Storage {
    Storage {
        path: Some(path.to_owned()),
        drive: false,
    }
}

/// Drive with the default local folder, as the window sends it for "open from Google Drive".
fn drive_only() -> Storage {
    Storage {
        path: None,
        drive: true,
    }
}

pub(super) fn vault_dir() -> String {
    std::env::var("FIGLOCK_TEST_VAULT").unwrap()
}

/// The Secret Key of a vault being created, as the person would type it.
pub(super) fn created_key(d: &Desk, password: &str) -> String {
    d.create(&folder(&vault_dir()), password)
        .unwrap()
        .secret_key
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
        code_of(d.create(&folder(&vault_dir()), &pw[..4]).unwrap_err()),
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
    assert!(d.create(&folder(&vault_dir()), &password()).is_err());
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
    let dir = vault_dir();
    std::fs::create_dir_all(format!("{dir}/../empty")).unwrap();
    assert_eq!(
        code_of(
            d.open_existing(&folder(&dir), &wrong, &key, false)
                .unwrap_err()
        ),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(
            d.open_existing(&folder(&dir), &pw, &other, false)
                .unwrap_err()
        ),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(
            d.open_existing(&folder(&dir), &pw, "garbage", false)
                .unwrap_err()
        ),
        "bad_secret_key"
    );
    assert_eq!(
        code_of(
            d.open_existing(&folder(&format!("{dir}/../empty")), &pw, &key, false)
                .unwrap_err()
        ),
        "no_vault"
    );
    // The failures set nothing up.
    assert_eq!(d.state().unwrap().phase, "none");

    assert_eq!(
        d.open_existing(&folder(&dir), &pw, &key, false)
            .unwrap()
            .phase,
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

#[test]
fn choosing_google_drive_needs_a_sign_in_first() {
    scenario("drive_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn drive_inner() {
    let d = desk();
    assert!(!drive::status().unwrap().signed_in);

    // Not signed in: every way of picking Drive says so, and nothing is set up.
    assert_eq!(
        code_of(storage_place(&drive_only(), true).unwrap_err()),
        "drive_sign_in"
    );
    assert_eq!(
        code_of(d.create(&drive_only(), &password()).unwrap_err()),
        "drive_sign_in"
    );
    assert_eq!(
        code_of(
            d.open_existing(&drive_only(), &password(), "garbage", false)
                .unwrap_err()
        ),
        "bad_secret_key",
        "a bad Secret Key is reported before Drive is touched"
    );
    assert_eq!(d.state().unwrap().phase, "none");

    // Signed in (a made-up token: nothing here talks to Google).
    config::save_drive_token(&password()).unwrap();
    assert!(drive::status().unwrap().signed_in);
    assert_eq!(
        storage_place(&drive_only(), true).unwrap().remote,
        Some(Remote::Drive)
    );

    // A damaged token file is removed on sign-out, with nothing to revoke at Google.
    let token = config::home().unwrap().join("drive-token.json");
    std::fs::write(&token, b"{not json").unwrap();
    assert!(!drive::sign_out().unwrap().revoked);
    assert!(!token.exists());
    assert!(!drive::status().unwrap().signed_in);

    // Signing out when signed out is harmless.
    assert!(!drive::sign_out().unwrap().revoked);
}

#[test]
fn the_window_sends_a_folder_and_whether_to_sync_it() {
    let f: Storage = serde_json::from_str(r#"{"path":"/tmp/v"}"#).unwrap();
    assert_eq!(f, folder("/tmp/v"));
    let d: Storage = serde_json::from_str(r#"{"drive":true}"#).unwrap();
    assert_eq!(d, drive_only());
    let both: Storage = serde_json::from_str(r#"{"path":"/tmp/v","drive":true}"#).unwrap();
    assert_eq!((both.path.as_deref(), both.drive), (Some("/tmp/v"), true));
    assert_eq!(there(&d), "this Google Drive");
    assert_eq!(there(&f), "that folder");
}

fn there(s: &Storage) -> &'static str {
    s.there()
}

#[test]
fn picking_neither_a_folder_nor_drive_is_refused() {
    let none = Storage {
        path: None,
        drive: false,
    };
    assert_eq!(code_of(storage_place(&none, true).unwrap_err()), "folder");
    let blank = Storage {
        path: Some("  ".into()),
        drive: false,
    };
    assert_eq!(code_of(storage_place(&blank, true).unwrap_err()), "folder");
}

#[test]
fn sync_is_off_until_turned_on_and_turning_it_on_needs_a_sign_in() {
    scenario("sync_off_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn sync_off_inner() {
    let d = desk();
    let pw = password();
    let key = created_key(&d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    d.finish_create(false).unwrap();

    let v = d.sync_status().unwrap();
    assert!(!v.enabled);
    assert_eq!(v.state, "off");
    assert_eq!(v.message, None);
    assert_eq!(
        v.local_folder.as_deref(),
        Some(canonical(&vault_dir()).as_str())
    );
    assert!(!d.sync_tick(), "no sync runs while it is off");
    assert_eq!(d.sync_now(false).unwrap().state, "off");

    // Not signed in: nothing is changed.
    assert_eq!(code_of(d.sync_enable().unwrap_err()), "drive_sign_in");
    assert!(config::load().unwrap().place.remote.is_none());

    // Signed in with a made-up token but a build with no Google client (like a source build): the first sync
    // fails, so sync is not left on.
    config::save_drive_token(&password()).unwrap();
    assert_eq!(code_of(d.sync_enable().unwrap_err()), "sync_failed");
    assert_eq!(config::load().unwrap().place.remote, None);
    assert_eq!(d.sync_status().unwrap().state, "off");

    // Turning it off when it is off is harmless.
    assert_eq!(d.sync_disable().unwrap().state, "off");
}

fn canonical(p: &str) -> String {
    std::fs::canonicalize(p).unwrap().display().to_string()
}

#[test]
fn a_failing_remote_shows_as_a_state_and_backs_off_without_touching_the_vault() {
    scenario("sync_failing_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn sync_failing_inner() {
    let d = desk();
    let pw = password();
    let key = created_key(&d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    d.check_kit_key(&groups[groups.len() - 2..].join("-"))
        .unwrap();
    d.finish_create(false).unwrap();
    // A device with sync on and no working sign-in, as after the sign-in was revoked.
    config::set_remote(Some(Remote::Drive)).unwrap();

    let v = d.sync_status().unwrap();
    assert!(v.enabled);
    assert_eq!(v.state, "idle", "nothing has failed yet");

    assert_eq!(d.sync_now(false).unwrap().state, "failed");
    let v = d.sync_status().unwrap();
    assert!(v.message.is_some());
    assert!(!d.sync_tick(), "backing off: not due again yet");
    // The vault itself is untouched and still opens.
    assert_eq!(d.state().unwrap().phase, "unlocked");
    assert!(d.list_items("").unwrap().is_empty());
}

#[test]
fn what_another_device_synced_shows_up_in_the_open_vault() {
    scenario("sync_pull_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn sync_pull_inner() {
    use figlock_store::{BlobStore, LocalFolder};
    let d = desk();
    let pw = password();
    let key = created_key(&d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    d.check_kit_key(&groups[groups.len() - 2..].join("-"))
        .unwrap();
    d.finish_create(false).unwrap();
    assert!(d.list_items("").unwrap().is_empty());

    // A second device of the same vault writes into the folder, as a sync would leave it.
    let store = LocalFolder::open(std::path::Path::new(&vault_dir())).unwrap();
    let vault_json = store.get("vault.json").unwrap();
    let records: Vec<Vec<u8>> = store
        .list("devices/")
        .unwrap()
        .iter()
        .map(|e| store.get(&e.path).unwrap())
        .collect();
    let key = figlock_core::SecretKey::parse(&key).unwrap();
    let auk = Vault::unlock_key(&vault_json, &pw, &key).unwrap();
    let (mut other, record) = Vault::unlock_with_key(
        &vault_json,
        &auk,
        figlock_core::DeviceKeys::generate().unwrap(),
        "Other Mac",
        &records,
        now_ms(),
    )
    .unwrap();
    store
        .put_new(
            &format!("devices/{}.json", hex::encode(other.device_id())),
            &record.unwrap(),
        )
        .unwrap();
    other
        .add_item(
            figlock_core::ItemType::Login,
            "Personal",
            &[("title", "From the other Mac")],
            now_ms(),
        )
        .unwrap();
    let file = other.commit().unwrap().unwrap();
    store.put_new(&file.path, &file.bytes).unwrap();

    // What the sync thread does after a pull.
    assert!(!d.absorb_pull_for_test(), "nothing to flag: a clean pull");
    let titles: Vec<String> = d
        .list_items("")
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, vec!["From the other Mac".to_owned()]);
}
