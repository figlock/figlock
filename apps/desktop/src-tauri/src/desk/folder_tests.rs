//! A vault folder that is gone from under a set-up device (FIG-118). The window must learn it from the state, before
//! asking for credentials, and each way forward must work without deleting anything unconfirmed. Each scenario runs
//! as a child with its own device config (see `tests.rs`).

use std::path::PathBuf;

use super::tests::{code_of, created_key, desk, password, scenario_in, vault_dir};
use super::*;

fn scenario(name: &str) {
    scenario_in("folder_tests", name);
}

/// A locked vault on this device, plus where its folder is moved to.
fn locked_vault(d: &Desk) -> (String, String, PathBuf) {
    let pw = password();
    let key = created_key(d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    d.recovery_skip(true).unwrap();
    d.finish_create(false).unwrap();
    d.lock();
    let elsewhere = PathBuf::from(std::env::var("FIGLOCK_TEST_DIR").unwrap()).join("moved");
    (pw, key, elsewhere)
}

#[test]
fn a_missing_folder_is_reported_before_any_credentials() {
    scenario("a_missing_folder_is_reported_before_any_credentials_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn a_missing_folder_is_reported_before_any_credentials_inner() {
    let d = desk();
    let (_pw, _key, elsewhere) = locked_vault(&d);
    let s = d.state().unwrap();
    assert_eq!((s.phase, s.folder_missing.is_none()), ("locked", true));
    // The folder as the device stored it (canonical: /var is /private/var on a Mac).
    let stored = std::fs::canonicalize(vault_dir()).unwrap();

    std::fs::rename(vault_dir(), &elsewhere).unwrap();
    let s = d.state().unwrap();
    assert_eq!(s.phase, "locked");
    let missing = s.folder_missing.expect("the folder is missing");
    assert_eq!(missing.path, stored.display().to_string());
    assert!(!missing.can_restore, "sync was never on");
    // Nothing is offered that reads a vault that is not there.
    assert!(!s.secret_key_kept && !s.quick_unlock);
}

#[test]
fn locating_the_moved_folder_brings_the_vault_back() {
    scenario("locating_the_moved_folder_brings_the_vault_back_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn locating_the_moved_folder_brings_the_vault_back_inner() {
    let d = desk();
    let (pw, key, elsewhere) = locked_vault(&d);
    std::fs::rename(vault_dir(), &elsewhere).unwrap();
    assert!(d.state().unwrap().folder_missing.is_some());

    // A folder without a vault is refused and changes nothing.
    let empty = elsewhere.with_file_name("empty");
    std::fs::create_dir(&empty).unwrap();
    let err = folder::locate(empty.to_str().unwrap()).unwrap_err();
    assert_eq!(code_of(err), "no_vault");
    assert!(d.state().unwrap().folder_missing.is_some());

    folder::locate(elsewhere.to_str().unwrap()).unwrap();
    assert!(d.state().unwrap().folder_missing.is_none());
    // The same device, the same vault: the old master password and Secret Key still open it.
    assert_eq!(d.unlock(&pw, Some(&key)).unwrap().phase, "unlocked");
}

#[test]
fn forgetting_the_vault_needs_a_confirmation_and_deletes_no_vault_data() {
    scenario("forgetting_the_vault_needs_a_confirmation_and_deletes_no_vault_data_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn forgetting_the_vault_needs_a_confirmation_and_deletes_no_vault_data_inner() {
    let d = desk();
    let (_pw, _key, elsewhere) = locked_vault(&d);
    std::fs::rename(vault_dir(), &elsewhere).unwrap();

    // Without the person's yes, nothing changes.
    assert_eq!(code_of(folder::forget(false).unwrap_err()), "not_confirmed");
    assert_eq!(d.state().unwrap().phase, "locked");

    folder::forget(true).unwrap();
    assert_eq!(d.state().unwrap().phase, "none");
    // The vault itself is untouched wherever it was kept.
    assert!(elsewhere.join("vault.json").is_file());
}

#[test]
fn recovery_commands_refuse_when_the_folder_is_there() {
    scenario("recovery_commands_refuse_when_the_folder_is_there_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn recovery_commands_refuse_when_the_folder_is_there_inner() {
    let d = desk();
    let (_pw, _key, _elsewhere) = locked_vault(&d);
    // A healthy vault cannot be forgotten, repointed or restored through the recovery path.
    assert_eq!(code_of(folder::forget(true).unwrap_err()), "failed");
    assert_eq!(code_of(folder::locate(&vault_dir()).unwrap_err()), "failed");
    assert_eq!(
        code_of(folder::restore(None, folder::RestoreFrom::Drive).unwrap_err()),
        "failed"
    );
    assert_eq!(d.state().unwrap().phase, "locked");
}

#[test]
fn restoring_without_sync_says_there_is_no_drive_copy() {
    scenario("restoring_without_sync_says_there_is_no_drive_copy_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn restoring_without_sync_says_there_is_no_drive_copy_inner() {
    let d = desk();
    let (_pw, _key, elsewhere) = locked_vault(&d);
    std::fs::rename(vault_dir(), &elsewhere).unwrap();
    assert_eq!(
        code_of(folder::restore(None, folder::RestoreFrom::Drive).unwrap_err()),
        "failed"
    );
    assert!(d.state().unwrap().folder_missing.is_some());
}

#[test]
fn a_failed_restore_leaves_no_folder_and_the_missing_screen() {
    scenario("a_failed_restore_leaves_no_folder_and_the_missing_screen_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn a_failed_restore_leaves_no_folder_and_the_missing_screen_inner() {
    let d = desk();
    let (_pw, _key, elsewhere) = locked_vault(&d);
    // Sync on with a saved sign-in. The build has no Google client, so the download cannot start: a failed restore.
    config::save_drive_token("revoked-refresh-token").unwrap();
    config::add_remote(config::Remote::Drive).unwrap();
    std::fs::rename(vault_dir(), &elsewhere).unwrap();
    assert!(
        d.state()
            .unwrap()
            .folder_missing
            .is_some_and(|m| m.can_restore)
    );
    // Reading the state (the window does it all the time) must not make the folder again.
    assert!(!std::path::Path::new(&vault_dir()).exists());

    let err = folder::restore(None, folder::RestoreFrom::Drive).unwrap_err();
    assert_ne!(
        err.message,
        "The vault folder is available, so there is nothing to recover."
    );
    assert!(
        !std::path::Path::new(&vault_dir()).exists(),
        "no empty folder is left"
    );
    assert!(
        !std::path::Path::new(&vault_dir())
            .with_file_name(".vault.restoring")
            .exists()
    );
    assert!(
        d.state().unwrap().folder_missing.is_some(),
        "still the missing-folder screen"
    );
}

#[test]
fn an_empty_vault_folder_shows_the_missing_screen_not_unlock() {
    scenario("an_empty_vault_folder_shows_the_missing_screen_not_unlock_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn an_empty_vault_folder_shows_the_missing_screen_not_unlock_inner() {
    let d = desk();
    let (_pw, _key, elsewhere) = locked_vault(&d);
    std::fs::rename(vault_dir(), &elsewhere).unwrap();
    std::fs::create_dir(vault_dir()).unwrap();
    let s = d.state().unwrap();
    assert_eq!(s.phase, "locked");
    assert!(
        s.folder_missing.is_some(),
        "a folder with no vault is not a vault folder"
    );
    // Putting the vault back in it returns the normal unlock screen.
    std::fs::remove_dir(vault_dir()).unwrap();
    std::fs::rename(&elsewhere, vault_dir()).unwrap();
    assert!(d.state().unwrap().folder_missing.is_none());
}

#[test]
fn a_refused_sign_in_while_restoring_asks_to_sign_in_again() {
    let e = folder::restore_failure(figlock_session::sync::SyncError::SignIn);
    assert_eq!(e.code, "drive_sign_in");
    assert_eq!(
        e.message,
        "Google has stopped FigLock's access. Sign in again to restore."
    );
}

#[test]
fn a_vault_synced_to_icloud_is_restored_from_icloud_drive() {
    scenario("a_vault_synced_to_icloud_is_restored_from_icloud_drive_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn a_vault_synced_to_icloud_is_restored_from_icloud_drive_inner() {
    let d = desk();
    let (_pw, _key, elsewhere) = locked_vault(&d);
    // iCloud Drive is on, and this vault syncs to it (sync works on ciphertext, so locked is fine).
    std::fs::create_dir_all(std::env::var("FIGLOCK_ICLOUD_DIR").unwrap()).unwrap();
    d.sync_enable_icloud().unwrap();
    std::fs::rename(vault_dir(), &elsewhere).unwrap();
    let missing = d
        .state()
        .unwrap()
        .folder_missing
        .expect("the folder is missing");
    assert!(missing.can_restore_icloud);
    assert!(!missing.can_restore, "Google Drive was never on");
    // Drive is not what was asked for.
    assert_eq!(
        code_of(folder::restore(None, folder::RestoreFrom::Drive).unwrap_err()),
        "failed"
    );

    folder::restore(None, folder::RestoreFrom::Icloud).unwrap();
    assert!(d.state().unwrap().folder_missing.is_none());
    let restored = config::load().unwrap().place.local;
    assert!(restored.join("vault.json").is_file());
    assert_ne!(restored, std::fs::canonicalize(&elsewhere).unwrap());
}

#[test]
fn restoring_from_icloud_while_it_is_off_says_what_to_switch_on_and_changes_nothing() {
    scenario("restoring_from_icloud_while_it_is_off_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn restoring_from_icloud_while_it_is_off_inner() {
    let d = desk();
    let (_pw, _key, elsewhere) = locked_vault(&d);
    let icloud = std::env::var("FIGLOCK_ICLOUD_DIR").unwrap();
    std::fs::create_dir_all(&icloud).unwrap();
    d.sync_enable_icloud().unwrap();
    std::fs::rename(vault_dir(), &elsewhere).unwrap();
    // iCloud Drive signed out: its folder is gone.
    std::fs::remove_dir_all(&icloud).unwrap();
    let err = folder::restore(None, folder::RestoreFrom::Icloud).unwrap_err();
    assert!(err.message.contains("System Settings"), "{}", err.message);
    assert!(d.state().unwrap().folder_missing.is_some());
    assert!(!std::path::Path::new(&vault_dir()).exists());
}
