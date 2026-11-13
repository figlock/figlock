//! "I forgot my password" on a fresh install, against a real folder (FIG-103). One device makes a vault with a
//! Recovery Key and an item; the device's own setup is then forgotten, as on a fresh install of the app, and the
//! Recovery Key is used to get back in. Each scenario runs as a child with its own device config (see `tests.rs`).

use std::collections::BTreeMap;

use figlock_core::RecoveryKey;
use figlock_store::{BlobStore, LocalFolder};

use super::items::ItemDraft;
use super::tests::{code_of, created_key, desk, folder, password, scenario_in, vault_dir};
use super::*;

fn scenario(name: &str) {
    scenario_in("forgot_tests", name);
}

fn store() -> LocalFolder {
    LocalFolder::open(std::path::Path::new(&vault_dir())).unwrap()
}

/// A vault with one login, locked, with this device's setup forgotten. `with_key` says whether it has a
/// Recovery Key; the code is returned when it has.
struct Fixture {
    password: String,
    secret_key: String,
    code: Option<String>,
    cards: Vec<String>,
    secret_item: String,
}

fn fixture(d: &Desk, how: Option<(bool, u8, u8)>) -> Fixture {
    let pw = password();
    let key = created_key(d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    let (mut code, mut cards) = (None, Vec::new());
    match how {
        None => d.recovery_skip(true).unwrap(),
        Some((shares, m, n)) => {
            let material = d.recovery_prepare(shares, m, n).unwrap();
            let answers: Vec<String> = if shares {
                cards = material.cards.iter().map(|c| c.join(" ")).collect();
                material
                    .challenge
                    .iter()
                    .enumerate()
                    .map(|(i, p)| material.cards[i][p - 1].clone())
                    .collect()
            } else {
                code = material.code.clone();
                let c = material.code.as_ref().unwrap();
                vec![
                    c.split('-')
                        .rev()
                        .take(2)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("-"),
                ]
            };
            assert!(d.recovery_check(&answers).unwrap());
            d.recovery_use_draft().unwrap();
        }
    }
    d.finish_create(false).unwrap();
    let secret_item = password();
    d.add_item(
        "login",
        ItemDraft {
            fields: BTreeMap::from([
                ("title".to_owned(), "Bank".to_owned()),
                ("password".to_owned(), secret_item.clone()),
            ]),
            clear: Vec::new(),
            add_files: Vec::new(),
            remove_files: Vec::new(),
        },
    )
    .unwrap();
    d.lock();
    // A fresh install of the app: the device's own setup is gone, the vault in storage is not.
    figlock_session::config::forget();
    assert_eq!(d.state().unwrap().phase, "none");
    Fixture {
        password: pw,
        secret_key: key,
        code,
        cards,
        secret_item,
    }
}

fn titles(d: &Desk) -> Vec<String> {
    d.list_items("")
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect()
}

fn stored() -> Vec<u8> {
    store().get("vault.json").unwrap()
}

#[test]
fn the_code_gets_back_into_the_vault_with_a_new_password_and_secret_key() {
    scenario("code_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn code_inner() {
    let d = desk();
    let f = fixture(&d, Some((false, 0, 0)));
    let code = f.code.clone().unwrap();
    let folder = folder(&vault_dir());

    // A wrong or malformed key finds nothing and stores nothing.
    let before = stored();
    assert_eq!(
        code_of(
            d.forgot_find(&folder, &["not a code".to_owned()], false)
                .unwrap_err()
        ),
        "bad_recovery_key"
    );
    let other = RecoveryKey::random().unwrap().format().to_string();
    assert_eq!(
        code_of(d.forgot_find(&folder, &[other], false).unwrap_err()),
        "recovery_failed"
    );
    assert_eq!(
        code_of(d.forgot_prepare(&password(), false).unwrap_err()),
        "failed"
    );

    // The right one shows what it found: the item, the one device, a last change.
    let found = d
        .forgot_find(&folder, &[code.to_lowercase()], false)
        .unwrap();
    assert_eq!(found.name, "Personal");
    assert_eq!(found.items, 1);
    assert_eq!(found.devices, 1);
    assert!(found.last_change_ms.is_some());

    assert_eq!(
        code_of(d.forgot_prepare(&password()[..4], false).unwrap_err()),
        "weak_password"
    );
    assert_eq!(
        code_of(d.forgot_prepare(&password(), true).unwrap_err()),
        "failed",
        "a new Recovery Key was asked for and none was made"
    );
    let new_pw = password();
    let new = d.forgot_prepare(&new_pw, false).unwrap();
    assert!(new.secret_key.starts_with("FL1-"));
    assert_ne!(new.secret_key, f.secret_key);
    assert!(!d.emergency_kit().unwrap().is_empty());

    // Nothing was stored yet, and the kit must be confirmed before anything is.
    assert_eq!(stored(), before);
    assert_eq!(
        code_of(d.forgot_commit(false).unwrap_err()),
        "key_not_confirmed"
    );
    assert_eq!(d.state().unwrap().phase, "none");
    let groups: Vec<&str> = new.secret_key.split('-').collect();
    assert!(!d.check_kit_key("AAAA-AAAA").unwrap());
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    let state = d.forgot_commit(false).unwrap();
    assert_eq!(state.phase, "unlocked");
    assert!(state.recovery_enabled);
    assert_eq!(titles(&d), ["Bank"]);
    assert_ne!(stored(), before);

    // The new credentials open it and the old ones do not. The same code still works.
    d.lock();
    assert_eq!(
        code_of(d.unlock(&f.password, Some(&f.secret_key)).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.unlock(&new_pw, Some(&f.secret_key)).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        d.unlock(&new_pw, Some(&new.secret_key)).unwrap().phase,
        "unlocked"
    );
    assert_eq!(
        d.reveal_field(&d.list_items("").unwrap()[0].id, "password")
            .unwrap(),
        f.secret_item
    );
    // And the old code opens the vault again, since it was not replaced.
    d.lock();
    figlock_session::config::forget();
    assert!(d.forgot_find(&folder, &[code], false).is_ok());
}

#[test]
fn a_new_recovery_key_replaces_the_one_just_used() {
    scenario("rotate_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn rotate_inner() {
    let d = desk();
    let f = fixture(&d, Some((false, 0, 0)));
    let old = f.code.clone().unwrap();
    let folder = folder(&vault_dir());
    d.forgot_find(&folder, std::slice::from_ref(&old), false)
        .unwrap();

    // A new key must be shown and confirmed before it can be used.
    let new_pw = password();
    let material = d.recovery_prepare(false, 0, 0).unwrap();
    assert_eq!(
        code_of(d.forgot_prepare(&new_pw, true).unwrap_err()),
        "recovery_not_confirmed"
    );
    let c = material.code.clone().unwrap();
    let tail: Vec<&str> = c.split('-').collect();
    assert!(
        d.recovery_check(&[tail[tail.len() - 2..].join("-")])
            .unwrap()
    );
    let new = d.forgot_prepare(&new_pw, true).unwrap();
    let groups: Vec<&str> = new.secret_key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    assert!(d.forgot_commit(false).unwrap().recovery_enabled);

    // The vault now opens with the new code and no longer with the old.
    d.lock();
    figlock_session::config::forget();
    assert_eq!(
        code_of(d.forgot_find(&folder, &[old], false).unwrap_err()),
        "recovery_failed"
    );
    assert!(d.forgot_find(&folder, &[c], false).is_ok());
}

#[test]
fn cards_say_how_many_are_needed_and_exactly_that_many_open_the_vault() {
    scenario("cards_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn cards_inner() {
    let d = desk();
    let f = fixture(&d, Some((true, 2, 3)));
    let folder = folder(&vault_dir());
    for (i, card) in f.cards.iter().enumerate() {
        let info = Desk::forgot_card_info(card).unwrap();
        assert_eq!(
            (info.needed, usize::from(info.number)),
            (2, info.number as usize)
        );
        assert!(info.number >= 1 && usize::from(info.number) <= 3, "{i}");
    }
    assert_eq!(
        code_of(Desk::forgot_card_info("not a card").unwrap_err()),
        "bad_card"
    );
    // One card is not enough, and neither are two cards from different places in the set mixed with a typo.
    assert_eq!(
        code_of(d.forgot_find(&folder, &f.cards[..1], true).unwrap_err()),
        "bad_recovery_key"
    );
    let found = d
        .forgot_find(&folder, &[f.cards[2].clone(), f.cards[0].clone()], true)
        .unwrap();
    assert_eq!(found.items, 1);
}

#[test]
fn a_vault_without_a_recovery_key_says_so() {
    scenario("none_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn none_inner() {
    let d = desk();
    fixture(&d, None);
    let code = RecoveryKey::random().unwrap().format().to_string();
    assert_eq!(
        code_of(
            d.forgot_find(&folder(&vault_dir()), &[code], false)
                .unwrap_err()
        ),
        "no_recovery"
    );
}

#[test]
fn backing_out_changes_nothing() {
    scenario("cancel_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn cancel_inner() {
    let d = desk();
    let f = fixture(&d, Some((false, 0, 0)));
    let folder = folder(&vault_dir());
    let before = stored();
    d.forgot_find(&folder, &[f.code.clone().unwrap()], false)
        .unwrap();
    d.forgot_prepare(&password(), false).unwrap();
    d.forgot_cancel();
    assert_eq!(
        code_of(d.forgot_prepare(&password(), false).unwrap_err()),
        "failed"
    );
    assert_eq!(code_of(d.forgot_commit(false).unwrap_err()), "failed");
    assert_eq!(stored(), before);
    assert_eq!(d.state().unwrap().phase, "none");
}

#[test]
fn a_device_that_is_already_set_up_cannot_start_it() {
    scenario("initialised_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn initialised_inner() {
    let d = desk();
    let pw = password();
    let key = created_key(&d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    d.recovery_skip(true).unwrap();
    d.finish_create(false).unwrap();
    d.lock();
    let code = RecoveryKey::random().unwrap().format().to_string();
    assert_eq!(
        code_of(
            d.forgot_find(&folder(&vault_dir()), &[code], false)
                .unwrap_err()
        ),
        "failed"
    );
}

#[test]
fn a_failed_write_leaves_the_device_as_it_was_and_the_attempt_can_be_retried() {
    scenario("failed_write_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn failed_write_inner() {
    use std::os::unix::fs::PermissionsExt;
    let d = desk();
    let f = fixture(&d, Some((false, 0, 0)));
    let code = f.code.clone().unwrap();
    let folder = folder(&vault_dir());
    d.forgot_find(&folder, &[code], false).unwrap();
    let new = d.forgot_prepare(&password(), false).unwrap();
    let groups: Vec<&str> = new.secret_key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    let before = stored();

    // Storage that cannot be written: nothing changes, and this device is still not set up.
    let dir_name = vault_dir();
    let dir = std::path::Path::new(&dir_name);
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    // A process that may write regardless of the mode (root) cannot run this scenario.
    if std::fs::write(dir.join("probe"), b"x").is_ok() {
        let _ = std::fs::remove_file(dir.join("probe"));
        return;
    }
    let err = d.forgot_commit(false).unwrap_err();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(err.code, "failed");
    assert_eq!(stored(), before);
    assert_eq!(d.state().unwrap().phase, "none");

    // The key is still held, so the same attempt can be prepared again and finished.
    let new_pw = password();
    let again = d.forgot_prepare(&new_pw, false).unwrap();
    let groups: Vec<&str> = again.secret_key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    assert_eq!(d.forgot_commit(false).unwrap().phase, "unlocked");
}
