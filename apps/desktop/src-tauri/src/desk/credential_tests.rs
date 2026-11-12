//! Changing the master password and rotating the Secret Key, against a real folder (see `tests.rs`).

use super::device_tests::unlocked;
use super::tests::{code_of, desk, password, scenario_in};
use super::*;

fn scenario(name: &str) {
    scenario_in("credential_tests", name);
}

fn end_of(key: &str) -> String {
    let groups: Vec<&str> = key.split('-').collect();
    groups[groups.len() - 2..].join("-")
}

#[test]
fn a_locked_vault_cannot_change_credentials() {
    let d = desk();
    let key = SecretKey::from_bytes([7; 16]).format().to_string();
    assert_eq!(
        code_of(
            d.change_credentials("x", Some(&key), Some("longenough"), false)
                .unwrap_err()
        ),
        "locked"
    );
}

#[test]
fn a_new_master_password_replaces_the_old_one_and_the_key_stays() {
    scenario("change_password_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn change_password_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    let new_pw = password();

    // Nothing changes on a refusal.
    assert_eq!(
        code_of(
            d.change_credentials(&pw, Some(&key), None, false)
                .unwrap_err()
        ),
        "failed"
    );
    assert_eq!(
        code_of(
            d.change_credentials(&pw, Some(&key), Some("short"), false)
                .unwrap_err()
        ),
        "weak_password"
    );
    assert_eq!(
        code_of(
            d.change_credentials(&pw, Some("FL1-AAAA"), Some(&new_pw), false)
                .unwrap_err()
        ),
        "bad_secret_key"
    );
    assert_eq!(
        code_of(
            d.change_credentials(&password(), Some(&key), Some(&new_pw), false)
                .unwrap_err()
        ),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(
            d.change_credentials(&pw, None, Some(&new_pw), false)
                .unwrap_err()
        ),
        "needs_secret_key"
    );

    let changed = d
        .change_credentials(&pw, Some(&key), Some(&new_pw), false)
        .unwrap();
    assert!(
        changed.secret_key.is_none(),
        "a password change makes no new key"
    );
    // No kit step follows a password change.
    assert_eq!(code_of(d.emergency_kit().unwrap_err()), "failed");
    d.end_rotation().unwrap();

    d.lock();
    assert_eq!(
        code_of(d.unlock(&pw, Some(&key)).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(d.unlock(&new_pw, Some(&key)).unwrap().phase, "unlocked");
}

#[test]
fn rotating_the_secret_key_needs_the_new_kit_to_be_confirmed() {
    scenario("rotate_key_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn rotate_key_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);

    let changed = d.change_credentials(&pw, Some(&key), None, true).unwrap();
    let new_key = changed.secret_key.expect("a rotation returns the new key");
    assert_ne!(new_key, key);

    // The kit is for the new key, and the step cannot be closed until its end is typed back.
    let pdf = d.emergency_kit().unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    assert_eq!(code_of(d.end_rotation().unwrap_err()), "key_not_confirmed");
    assert!(
        !d.check_kit_key(&end_of(&key)).unwrap(),
        "the old key does not confirm"
    );
    assert_eq!(code_of(d.end_rotation().unwrap_err()), "key_not_confirmed");
    assert!(d.check_kit_key(&end_of(&new_key)).unwrap());
    d.end_rotation().unwrap();
    assert_eq!(code_of(d.emergency_kit().unwrap_err()), "failed");

    // The old key no longer opens the vault, the new one does, and the password is the same.
    d.lock();
    assert_eq!(
        code_of(d.unlock(&pw, Some(&key)).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(d.unlock(&pw, Some(&new_key)).unwrap().phase, "unlocked");
}

#[test]
fn locking_keeps_an_unconfirmed_new_key() {
    scenario("lock_keeps_new_key_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn lock_keeps_new_key_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    let new_key = d
        .change_credentials(&pw, Some(&key), None, true)
        .unwrap()
        .secret_key
        .unwrap();
    d.lock();
    assert_eq!(d.state().unwrap().pending_kit, None);
    let state = d.unlock(&pw, Some(&new_key)).unwrap();
    assert_eq!(state.phase, "unlocked");
    assert_eq!(state.pending_kit.as_deref(), Some(new_key.as_str()));
    assert!(d.emergency_kit().is_ok());
    assert_eq!(code_of(d.end_rotation().unwrap_err()), "key_not_confirmed");
    assert!(d.check_kit_key(&end_of(&new_key)).unwrap());
    d.end_rotation().unwrap();
    assert_eq!(d.state().unwrap().pending_kit, None);
}
