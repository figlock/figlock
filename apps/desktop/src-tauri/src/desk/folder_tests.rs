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
    assert_eq!(code_of(folder::restore(None).unwrap_err()), "failed");
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
    assert_eq!(code_of(folder::restore(None).unwrap_err()), "failed");
    assert!(d.state().unwrap().folder_missing.is_some());
}

#[test]
fn a_failed_restore_removes_only_a_folder_it_created() {
    let base = std::env::temp_dir().join(format!("figlock-restore-{}", std::process::id()));
    let created = base.join("created");
    let existing = base.join("existing");
    std::fs::create_dir_all(&created).unwrap();
    std::fs::create_dir_all(&existing).unwrap();
    std::fs::write(existing.join("keep.txt"), b"x").unwrap();

    folder::remove_created(&created, false);
    folder::remove_created(&existing, true);

    assert!(!created.exists());
    assert!(existing.join("keep.txt").is_file());
    std::fs::remove_dir_all(&base).unwrap();
}
