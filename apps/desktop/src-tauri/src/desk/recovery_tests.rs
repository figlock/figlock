//! The Recovery Key in the desktop: the required choice at setup, the forced re-entry, writing it into
//! `vault.json`, and Settings (set up, replace, turn off). Each scenario runs as a child with its own device
//! config (see `tests.rs`); the vault is a real folder, and recovery is then proved with the core against what
//! was actually stored.

use figlock_core::{DeviceKeys, Recover, RecoveryKey, Vault};
use figlock_store::{BlobStore, LocalFolder};

use super::tests::{code_of, created_key, desk, password, scenario_in, vault_dir};
use super::*;

fn scenario(name: &str) {
    scenario_in("recovery_tests", name);
}

/// Create a vault up to the point where the Secret Key is confirmed, which is where the recovery choice starts.
fn at_recovery_step(d: &Desk) -> (String, String) {
    let pw = password();
    let key = created_key(d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    (pw, key)
}

fn stored_vault_json() -> Vec<u8> {
    LocalFolder::open(std::path::Path::new(&vault_dir()))
        .unwrap()
        .get("vault.json")
        .unwrap()
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

/// Recover the stored vault with `key` and a new password, as a fresh install would.
fn recover_with(key: &RecoveryKey, new_password: &str) -> figlock_core::Result<()> {
    let records: Vec<Vec<u8>> = {
        let store = LocalFolder::open(std::path::Path::new(&vault_dir())).unwrap();
        store
            .list("devices/")
            .unwrap()
            .iter()
            .map(|e| store.get(&e.path).unwrap())
            .collect()
    };
    Vault::recover(
        Recover {
            vault_json: &stored_vault_json(),
            recovery_key: key,
            new_password,
            new_recovery_key: None,
        },
        DeviceKeys::generate()?,
        "Reinstalled",
        &records,
        now_ms(),
    )
    .map(|_| ())
}

/// What a person would type for `material`: the last two groups of the code, as read off the page.
fn answer(m: &recovery::RecoveryMaterial) -> String {
    let groups: Vec<&str> = m.code.split('-').collect();
    groups[groups.len() - 2..].join("-").to_lowercase()
}

#[test]
fn setup_requires_a_choice_and_a_confirmed_code_is_written_with_the_vault() {
    scenario("code_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn code_inner() {
    let d = desk();
    // Nothing to show without a vault being made or unlocked.
    assert_eq!(code_of(d.recovery_prepare().unwrap_err()), "locked");
    let (pw, _key) = at_recovery_step(&d);

    // No choice yet: the vault is not written, and the setup is still there to finish.
    assert_eq!(
        code_of(d.finish_create(false).unwrap_err()),
        "recovery_choice_needed"
    );
    assert!(
        LocalFolder::open(std::path::Path::new(&vault_dir()))
            .unwrap()
            .get("vault.json")
            .is_err()
    );

    let m = d.recovery_prepare().unwrap();
    let code = m.code.clone();
    assert!(code.starts_with("FLR1-"));
    // Cannot use it before the re-entry check passed.
    assert_eq!(
        code_of(d.recovery_use_draft().unwrap_err()),
        "recovery_not_confirmed"
    );
    assert!(!d.recovery_check("AAAA-AAAA").unwrap());
    assert_eq!(
        code_of(d.recovery_use_draft().unwrap_err()),
        "recovery_not_confirmed"
    );
    // The printable page is made and is a PDF.
    assert!(d.recovery_kit().unwrap().starts_with(b"%PDF-1.4"));
    assert!(d.recovery_check(&answer(&m)).unwrap());
    d.recovery_use_draft().unwrap();

    let state = d.finish_create(false).unwrap();
    assert_eq!(state.phase, "unlocked");
    assert!(state.recovery_enabled);

    // What was stored carries the members, and the code recovers the vault after the password is lost.
    let stored = json(&stored_vault_json());
    assert_eq!(stored["min_format_version"], 2);
    let typed = RecoveryKey::parse(&code.to_lowercase()).unwrap();
    assert_eq!(
        stored["rk_pub"],
        typed
            .public()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    recover_with(&typed, &pw).unwrap();
    assert!(recover_with(&RecoveryKey::random().unwrap(), &pw).is_err());
}

#[test]
fn setup_can_decline_only_with_the_acknowledgement() {
    scenario("skip_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn skip_inner() {
    let d = desk();
    assert_eq!(code_of(d.recovery_skip(true).unwrap_err()), "failed");
    at_recovery_step(&d);
    assert_eq!(
        code_of(d.recovery_skip(false).unwrap_err()),
        "not_acknowledged"
    );
    assert_eq!(
        code_of(d.finish_create(false).unwrap_err()),
        "recovery_choice_needed"
    );
    d.recovery_skip(true).unwrap();
    let state = d.finish_create(false).unwrap();
    assert_eq!(state.phase, "unlocked");
    assert!(!state.recovery_enabled);
    let stored = json(&stored_vault_json());
    assert!(stored.get("rk_pub").is_none());
    assert_eq!(stored["min_format_version"], 1);
}

#[test]
fn backing_out_drops_the_key() {
    scenario("cancel_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn cancel_inner() {
    let d = desk();
    at_recovery_step(&d);
    let m = d.recovery_prepare().unwrap();
    assert!(d.recovery_check(&answer(&m)).unwrap());
    d.recovery_cancel();
    assert_eq!(code_of(d.recovery_use_draft().unwrap_err()), "failed");
    assert_eq!(code_of(d.recovery_kit().unwrap_err()), "failed");
    // A confirmed key cannot be used after cancel, even if the choice said so earlier.
    d.recovery_skip(true).unwrap();
    d.cancel_create();
    assert_eq!(code_of(d.recovery_check("").unwrap_err()), "failed");
}

#[test]
fn settings_set_up_replace_and_turn_off_the_key() {
    scenario("settings_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn settings_inner() {
    let d = desk();
    let (pw, key) = at_recovery_step(&d);
    d.recovery_skip(true).unwrap();
    assert!(!d.finish_create(false).unwrap().recovery_enabled);

    // A draft must be confirmed first; the credentials are the password and the Secret Key.
    let first = d.recovery_prepare().unwrap();
    assert_eq!(
        code_of(d.recovery_enable(&pw, Some(&key)).unwrap_err()),
        "recovery_not_confirmed"
    );
    assert!(d.recovery_check(&answer(&first)).unwrap());
    // This device keeps no Secret Key in tests and, with the held one dropped (ADR-0030), it is asked for;
    // wrong ones change nothing.
    d.inner().held_key = None;
    assert_eq!(
        code_of(d.recovery_enable(&pw, None).unwrap_err()),
        "needs_secret_key"
    );
    assert_eq!(
        code_of(d.recovery_enable(&password(), Some(&key)).unwrap_err()),
        "wrong_credentials"
    );
    assert!(!d.state().unwrap().recovery_enabled);
    d.recovery_enable(&pw, Some(&key)).unwrap();
    assert!(d.state().unwrap().recovery_enabled);
    let old = RecoveryKey::parse(&first.code).unwrap();
    recover_with(&old, &pw).unwrap();

    // Replacing: the old key stops working for the new vault.json, the new one works.
    let second = d.recovery_prepare().unwrap();
    assert!(d.recovery_check(&answer(&second)).unwrap());
    d.recovery_enable(&pw, Some(&key)).unwrap();
    assert!(recover_with(&old, &pw).is_err());
    recover_with(&RecoveryKey::parse(&second.code).unwrap(), &pw).unwrap();

    // Turning it off needs the credentials too, and removes the members.
    assert_eq!(
        code_of(d.recovery_disable(&password(), Some(&key)).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.recovery_disable(&pw, None).unwrap_err()),
        "needs_secret_key"
    );
    assert!(d.state().unwrap().recovery_enabled);
    d.recovery_disable(&pw, Some(&key)).unwrap();
    assert!(!d.state().unwrap().recovery_enabled);
    let stored = json(&stored_vault_json());
    assert!(stored.get("rk_pub").is_none());
    assert_eq!(stored["min_format_version"], 1);
    assert!(recover_with(&RecoveryKey::parse(&second.code).unwrap(), &pw).is_err());
    assert_eq!(
        code_of(d.recovery_disable(&pw, Some(&key)).unwrap_err()),
        "failed"
    );

    // The vault still opens with its own credentials, after a lock.
    d.lock();
    assert_eq!(d.unlock(&pw, Some(&key)).unwrap().phase, "unlocked");
}
