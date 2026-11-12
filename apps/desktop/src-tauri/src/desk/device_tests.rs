//! The device list and revoking, against a real folder (see `tests.rs` for how scenarios run).

use figlock_core::{DeviceKeys, Invite, JoinAttempt};

use super::tests::{code_of, created_key, desk, password, scenario_in, vault_dir};
use super::*;

fn scenario(name: &str) {
    scenario_in("device_tests", name);
}

/// A desk with a new vault, unlocked, and the credentials it was made with.
fn unlocked(d: &Desk) -> (String, String) {
    let pw = password();
    let key = created_key(d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    assert_eq!(d.finish_create(false).unwrap().phase, "unlocked");
    (pw, key)
}

/// Pair a second device called `name` the way the core does, store its record in the vault folder, and
/// return its id.
fn add_device(d: &Desk, key: &str, name: &str) -> String {
    let secret_key = SecretKey::parse(key).unwrap();
    let mut inner = d.inner();
    let vault = &mut inner.session.as_mut().unwrap().vault;
    let now = now_ms();
    let (mut invite, offer) = Invite::create(vault.vault_id(), now).unwrap();
    let (_, req) = JoinAttempt::begin(
        &offer.code,
        &offer.inv,
        DeviceKeys::generate().unwrap(),
        name,
        now + 1,
    )
    .unwrap();
    let opened = invite.open_request(&req.bytes, now + 2).unwrap();
    let approval = vault
        .approve_join(&invite, &opened, &secret_key, now + 3)
        .unwrap();
    std::fs::write(
        std::path::Path::new(&vault_dir()).join(&approval.record.path),
        &approval.record.bytes,
    )
    .unwrap();
    opened_id(&approval.record.path)
}

fn opened_id(path: &str) -> String {
    path.trim_start_matches("devices/")
        .trim_end_matches(".json")
        .to_owned()
}

#[test]
fn the_device_list_shows_this_device_and_revoking_another_keeps_the_vault_open() {
    scenario("list_and_revoke_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn list_and_revoke_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    let rows = d.devices().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].this_device && !rows[0].revoked);

    let phone = add_device(&d, &key, "Phone");
    let rows = d.devices().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].this_device, "this device is listed first");
    assert_eq!(rows[1].name.as_deref(), Some("Phone"));

    // Negative cases first: nothing is revoked by a wrong password, a wrong id or this device.
    let wrong = password();
    assert_eq!(
        code_of(d.revoke_device(&phone, &wrong, &key, None).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.revoke_device(&rows[0].id, &pw, &key, None).unwrap_err()),
        "failed"
    );
    assert_eq!(
        code_of(
            d.revoke_device(&"00".repeat(16), &pw, &key, None)
                .unwrap_err()
        ),
        "failed"
    );
    assert_eq!(
        code_of(
            d.revoke_device(&phone, &pw, &key, Some("short"))
                .unwrap_err()
        ),
        "weak_password"
    );
    assert!(d.devices().unwrap().iter().all(|r| !r.revoked));

    let rows = d.revoke_device(&phone, &pw, &key, None).unwrap();
    assert!(rows[0].this_device && !rows[0].revoked);
    let gone = rows.iter().find(|r| r.id == phone).unwrap();
    assert!(gone.revoked && gone.name.is_none());

    // Revoking twice is refused, and the vault still unlocks with the same credentials.
    assert_eq!(
        code_of(d.revoke_device(&phone, &pw, &key, None).unwrap_err()),
        "failed"
    );
    d.lock();
    assert_eq!(d.unlock(&pw, &key).unwrap().phase, "unlocked");
}

#[test]
fn revoking_with_a_new_password_replaces_the_old_one() {
    scenario("revoke_and_change_password_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn revoke_and_change_password_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    let tablet = add_device(&d, &key, "Tablet");
    let new_pw = password();
    d.revoke_device(&tablet, &pw, &key, Some(&new_pw)).unwrap();
    d.lock();
    assert_eq!(
        code_of(d.unlock(&pw, &key).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(d.unlock(&new_pw, &key).unwrap().phase, "unlocked");
}

#[test]
fn the_device_list_needs_an_unlocked_vault() {
    let d = desk();
    assert_eq!(code_of(d.devices().unwrap_err()), "locked");
}
