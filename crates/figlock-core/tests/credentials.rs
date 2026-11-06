//! Password change and Secret Key rotation (FIG-63, `crypto.md` 9): the Vault Key is rewrapped under a new
//! Account Unlock Key, nothing is re-encrypted, and whoever holds only the old credentials is locked out of
//! the new `vault.json`.

mod common;

use common::{T0, add_login, new_vault, pw, sk_copy, titles};
use figlock_core::{CredentialChange, DeviceKeys, KdfParams, SecretKey, Vault};
use serde_json::Value;

const NEW_PW: &str = "a-different-generated-passphrase-for-tests";

fn unlock(vault_json: &[u8], password: &str, sk: &SecretKey) -> figlock_core::Result<Vault> {
    Vault::unlock(
        vault_json,
        password,
        sk,
        DeviceKeys::generate().unwrap(),
        "Reader",
        &[],
        T0 + 900,
    )
    .map(|(v, _)| v)
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn password_only(new: &str) -> CredentialChange<'_> {
    CredentialChange {
        new_password: Some(new),
        ..CredentialChange::default()
    }
}

#[test]
fn a_new_password_unlocks_and_the_old_one_does_not() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    add_login(&mut v, "Kept", T0 + 10);

    let r = v
        .change_credentials(pw(), &sk, password_only(NEW_PW))
        .unwrap();
    assert!(r.secret_key.is_none());
    assert!(unlock(&r.vault_json, pw(), &sk).is_err());
    let mut again = unlock(&r.vault_json, NEW_PW, &sk).unwrap();
    assert_eq!(again.epoch(), 1);
    assert_eq!(again.vault_id(), v.vault_id());

    let before = json(&created.vault_json);
    let after = json(&r.vault_json);
    assert_eq!(after["revision"], before["revision"].as_u64().unwrap() + 1);
    assert_eq!(after["authority_pub"], before["authority_pub"]);
    assert_ne!(after["kdf"]["salt"], before["kdf"]["salt"]);
    assert_ne!(after["wrapped_vk"], before["wrapped_vk"]);
    let _ = titles(&mut again);
}

#[test]
fn a_rotated_secret_key_replaces_the_old_one() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);

    let r = v
        .change_credentials(
            pw(),
            &sk,
            CredentialChange {
                rotate_secret_key: true,
                ..CredentialChange::default()
            },
        )
        .unwrap();
    let new_sk = r.secret_key.expect("a new Secret Key is returned");
    assert_ne!(new_sk.format(), sk.format());
    assert!(unlock(&r.vault_json, pw(), &sk).is_err());
    assert!(unlock(&r.vault_json, pw(), &new_sk).is_ok());
}

#[test]
fn password_secret_key_and_kdf_parameters_change_together() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let params = KdfParams {
        t: KdfParams::FLOOR.t + 1,
        ..KdfParams::FLOOR
    };

    let r = v
        .change_credentials(
            pw(),
            &sk,
            CredentialChange {
                new_password: Some(NEW_PW),
                rotate_secret_key: true,
                params: Some(params),
            },
        )
        .unwrap();
    let new_sk = r.secret_key.unwrap();
    assert_eq!(json(&r.vault_json)["kdf"]["t"], params.t);
    assert!(unlock(&r.vault_json, NEW_PW, &new_sk).is_ok());
    assert!(unlock(&r.vault_json, NEW_PW, &sk).is_err());
    assert!(unlock(&r.vault_json, pw(), &new_sk).is_err());
}

#[test]
fn items_and_history_survive_a_change() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    add_login(&mut v, "Before", T0 + 10);
    let op = v.commit().unwrap().unwrap();

    let r = v
        .change_credentials(pw(), &sk, password_only(NEW_PW))
        .unwrap();
    assert_eq!(titles(&mut v), ["Before"]);

    // Another device unlocks with the new credentials and reads the same op file.
    let (mut again, _) = Vault::unlock(
        &r.vault_json,
        NEW_PW,
        &sk,
        DeviceKeys::generate().unwrap(),
        "Reader",
        std::slice::from_ref(&created.device_record),
        T0 + 900,
    )
    .unwrap();
    again
        .apply_file(&op.bytes, v.device_id(), 1, T0 + 920)
        .unwrap();
    assert_eq!(titles(&mut again), ["Before"]);
}

#[test]
fn the_old_unlock_key_does_not_open_the_new_vault_json() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let old_auk = Vault::unlock_key(&created.vault_json, pw(), &sk).unwrap();

    let r = v
        .change_credentials(pw(), &sk, password_only(NEW_PW))
        .unwrap();
    // This is what a device with quick unlock holds; it must be sent back to a full unlock.
    let stale = Vault::unlock_with_key(
        &r.vault_json,
        &old_auk,
        DeviceKeys::generate().unwrap(),
        "Reader",
        &[],
        T0 + 900,
    );
    assert!(stale.is_err());
    let fresh = Vault::unlock_key(&r.vault_json, NEW_PW, &sk).unwrap();
    assert!(
        Vault::unlock_with_key(
            &r.vault_json,
            &fresh,
            DeviceKeys::generate().unwrap(),
            "Reader",
            &[],
            T0 + 900,
        )
        .is_ok()
    );
}

#[test]
fn wrong_current_credentials_change_nothing() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let other_sk = SecretKey::random().unwrap();

    assert!(
        v.change_credentials("not-the-password", &sk, password_only(NEW_PW))
            .is_err()
    );
    assert!(
        v.change_credentials(pw(), &other_sk, password_only(NEW_PW))
            .is_err()
    );
    let bad_params = KdfParams {
        m_kib: 1,
        ..KdfParams::FLOOR
    };
    assert!(
        v.change_credentials(
            pw(),
            &sk,
            CredentialChange {
                params: Some(bad_params),
                ..CredentialChange::default()
            },
        )
        .is_err()
    );

    // The refusals left the vault as it was: the next valid change is revision 2, not 4.
    let r = v
        .change_credentials(pw(), &sk, password_only(NEW_PW))
        .unwrap();
    assert_eq!(json(&r.vault_json)["revision"], 2);
    assert!(unlock(&r.vault_json, NEW_PW, &sk).is_ok());
}

#[test]
fn credentials_can_change_again_and_again() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let first = v
        .change_credentials(pw(), &sk, password_only(NEW_PW))
        .unwrap();
    let second = v
        .change_credentials(
            NEW_PW,
            &sk,
            CredentialChange {
                rotate_secret_key: true,
                ..CredentialChange::default()
            },
        )
        .unwrap();
    assert_eq!(json(&first.vault_json)["revision"], 2);
    assert_eq!(json(&second.vault_json)["revision"], 3);
    let sk3 = second.secret_key.unwrap();
    assert!(unlock(&second.vault_json, NEW_PW, &sk3).is_ok());
    assert!(unlock(&second.vault_json, NEW_PW, &sk).is_err());
}

#[test]
fn a_revoked_device_with_the_old_credentials_cannot_unlock_after_a_change() {
    let (mut a, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let mut records = vec![created.device_record.clone()];
    let (c, rec_c) = common::join(
        &mut a,
        &created.vault_json,
        &records,
        &sk,
        "Lost phone",
        T0 + 100,
    );
    records.push(rec_c.bytes);
    let c_id = *c.vault.device_id();
    add_login(&mut a, "Before", T0 + 200);
    a.commit().unwrap();

    let revocation = a.revoke_device(pw(), &sk, &c_id, T0 + 300).unwrap();
    // The lost phone knew the password and Secret Key. Right after revocation they still open the new Vault
    // Key, which is why a change follows.
    assert!(unlock(&revocation.vault_json, pw(), &sk).is_ok());

    let r = a
        .change_credentials(
            pw(),
            &sk,
            CredentialChange {
                new_password: Some(NEW_PW),
                rotate_secret_key: true,
                ..CredentialChange::default()
            },
        )
        .unwrap();
    let new_sk = r.secret_key.unwrap();

    // Old password and old Secret Key, in every combination, against the vault.json after the change.
    assert!(unlock(&r.vault_json, pw(), &sk).is_err());
    assert!(unlock(&r.vault_json, NEW_PW, &sk).is_err());
    assert!(unlock(&r.vault_json, pw(), &new_sk).is_err());
    assert!(
        Vault::unlock_key(&r.vault_json, pw(), &sk).is_ok_and(|auk| {
            Vault::unlock_with_key(
                &r.vault_json,
                &auk,
                DeviceKeys::generate().unwrap(),
                "Thief",
                &[],
                T0 + 900,
            )
            .is_err()
        })
    );

    // The devices that stay unlock with the new credentials, in the new epoch, with the revocation intact.
    let records: Vec<Vec<u8>> = revocation.records.iter().map(|c| c.bytes.clone()).collect();
    let (mut again, _) = Vault::unlock(
        &r.vault_json,
        NEW_PW,
        &new_sk,
        DeviceKeys::generate().unwrap(),
        "Laptop 2",
        &records,
        T0 + 900,
    )
    .unwrap();
    assert_eq!(again.epoch(), 2);
    assert_eq!(json(&r.vault_json)["revoked"].as_array().unwrap().len(), 1);
    again
        .apply_snapshot(&revocation.snapshot.bytes, a.device_id(), 1, T0 + 910)
        .unwrap();
    assert_eq!(titles(&mut again), ["Before"]);
}
