//! The Recovery Key at vault level (FIG-103, `crypto.md` 9.3, ADR-0029): enabling, using it after the
//! password is lost, rotating it, and what must never work (wrong key, tampered or half-present members, a
//! vault without one). Every key and password is generated per run.

mod common;

use common::{T0, add_login, join, new_vault, pw, sk_copy, titles};
use figlock_core::{DeviceKeys, Recover, RecoveryKey, SecretKey, Vault};
use serde_json::Value;

const NEW_PW: &str = "a-new-generated-passphrase-after-the-loss";

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn fresh_device() -> DeviceKeys {
    DeviceKeys::generate().unwrap()
}

fn unlock(vault_json: &[u8], password: &str, sk: &SecretKey) -> figlock_core::Result<Vault> {
    Vault::unlock(
        vault_json,
        password,
        sk,
        fresh_device(),
        "Reader",
        &[],
        T0 + 900,
    )
    .map(|(v, _)| v)
}

/// A vault with a Recovery Key already enabled.
struct Setup {
    vault: Vault,
    sk: SecretKey,
    rk: RecoveryKey,
    vault_json: Vec<u8>,
    record: Vec<u8>,
}

fn setup() -> Setup {
    let (mut vault, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let change = vault.set_recovery(pw(), &sk).unwrap();
    Setup {
        vault,
        sk,
        rk: change.key.unwrap(),
        vault_json: change.vault_json,
        record: created.device_record,
    }
}

fn recover(
    s: &Setup,
    rk: &RecoveryKey,
    rotate: bool,
) -> figlock_core::Result<(Vault, figlock_core::Recovered)> {
    Vault::recover(
        Recover {
            vault_json: &s.vault_json,
            recovery_key: rk,
            new_password: NEW_PW,
            rotate_recovery: rotate,
        },
        fresh_device(),
        "Reinstalled Mac",
        std::slice::from_ref(&s.record),
        T0 + 1_000,
    )
}

#[test]
fn enabling_adds_the_members_and_bumps_the_format_and_revision() {
    let (mut v, created) = new_vault();
    let before = json(&created.vault_json);
    assert!(before.get("rk_pub").is_none());
    assert_eq!(before["min_format_version"], 1);
    assert!(!v.has_recovery());

    let sk = sk_copy(&created.secret_key);
    let change = v.set_recovery(pw(), &sk).unwrap();
    assert!(v.has_recovery());
    let after = json(&change.vault_json);
    assert_eq!(after["min_format_version"], 2);
    assert_eq!(after["revision"], before["revision"].as_u64().unwrap() + 1);
    assert_eq!(after["authority_pub"], before["authority_pub"]);
    assert_eq!(after["wrapped_vk"], before["wrapped_vk"]);
    let key = change.key.unwrap();
    assert_eq!(after["rk_pub"], common_hex(&key.public()));
    // Normal credentials still open it, with the same vault.
    let again = unlock(&change.vault_json, pw(), &sk).unwrap();
    assert_eq!(again.vault_id(), v.vault_id());
    assert!(again.has_recovery());
}

fn common_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn wrong_credentials_do_not_enable_it() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    assert!(v.set_recovery("not the password", &sk).is_err());
    assert!(v.set_recovery(pw(), &SecretKey::random().unwrap()).is_err());
    assert!(!v.has_recovery());
    // Nothing changed: the next write is still revision 2.
    let ok = v.set_recovery(pw(), &sk).unwrap();
    assert_eq!(json(&ok.vault_json)["revision"], 2);
}

#[test]
fn the_code_recovers_the_vault_with_a_new_password_and_secret_key() {
    let mut s = setup();
    add_login(&mut s.vault, "Kept", T0 + 10);
    let op = s.vault.commit().unwrap().unwrap();

    let code = s.rk.format().to_string();
    let typed = RecoveryKey::parse(&code.to_lowercase()).unwrap();
    let (mut r, out) = recover(&s, &typed, false).unwrap();
    assert_eq!(r.vault_id(), s.vault.vault_id());
    assert!(out.recovery_key.is_none());
    assert!(out.new_record.is_some(), "a fresh install gets its record");

    // The old files still open under the recovered keys.
    r.apply_file(&op.bytes, s.vault.device_id(), 1, T0 + 1_100)
        .unwrap();
    assert_eq!(titles(&mut r), ["Kept"]);

    // The new vault.json opens with the new password and the new Secret Key, and with nothing old.
    let after = json(&out.vault_json);
    assert_eq!(after["revision"], 3);
    assert!(unlock(&out.vault_json, NEW_PW, &out.secret_key).is_ok());
    assert!(unlock(&out.vault_json, pw(), &out.secret_key).is_err());
    assert!(unlock(&out.vault_json, NEW_PW, &s.sk).is_err());
    assert!(unlock(&out.vault_json, pw(), &s.sk).is_err());
    // The same Recovery Key keeps working on the new file.
    assert_eq!(after["rk_pub"], json(&s.vault_json)["rk_pub"]);
    let recovered = unlock(&out.vault_json, NEW_PW, &out.secret_key).unwrap();
    assert!(recovered.has_recovery());
    // An old copy of vault.json still opens with the old credentials: recovery does not revoke devices.
    assert!(unlock(&s.vault_json, pw(), &s.sk).is_ok());
}

#[test]
fn shares_recover_it_too() {
    let s = setup();
    let shares = s.rk.split(2, 3).unwrap();
    let back = RecoveryKey::from_shares(&[&shares[0], &shares[2]]).unwrap();
    let (_, out) = recover(&s, &back, false).unwrap();
    assert!(unlock(&out.vault_json, NEW_PW, &out.secret_key).is_ok());
    assert!(RecoveryKey::from_shares(&[&shares[1]]).is_err());
}

#[test]
fn a_wrong_key_and_a_vault_without_one_recover_nothing() {
    let s = setup();
    assert!(recover(&s, &RecoveryKey::random().unwrap(), false).is_err());

    // A vault that never had a Recovery Key.
    let (_, created) = new_vault();
    let r = Vault::recover(
        Recover {
            vault_json: &created.vault_json,
            recovery_key: &s.rk,
            new_password: NEW_PW,
            rotate_recovery: false,
        },
        fresh_device(),
        "Mac",
        &[],
        T0 + 1,
    );
    assert!(r.is_err());
}

#[test]
fn rotating_the_recovery_key_in_the_same_write_retires_the_old_one() {
    let s = setup();
    let (_, out) = recover(&s, &s.rk, true).unwrap();
    let fresh = out.recovery_key.as_ref().expect("a new key was made");
    assert_ne!(fresh.public(), s.rk.public());
    assert_eq!(json(&out.vault_json)["rk_pub"], common_hex(&fresh.public()));

    let next = |rk: &RecoveryKey| {
        Vault::recover(
            Recover {
                vault_json: &out.vault_json,
                recovery_key: rk,
                new_password: "yet-another-generated-passphrase",
                rotate_recovery: false,
            },
            fresh_device(),
            "Mac",
            &[],
            T0 + 2_000,
        )
    };
    assert!(next(fresh).is_ok());
    assert!(next(&s.rk).is_err(), "the old cards open nothing now");
}

#[test]
fn replacing_and_removing_the_key() {
    let mut s = setup();
    let old = std::mem::replace(&mut s.rk, RecoveryKey::random().unwrap());
    let replaced = s.vault.set_recovery(pw(), &s.sk).unwrap();
    let new = replaced.key.unwrap();
    s.vault_json = replaced.vault_json;
    assert_eq!(json(&s.vault_json)["revision"], 3);
    assert_ne!(new.public(), old.public());
    assert!(recover(&s, &new, false).is_ok());
    assert!(recover(&s, &old, false).is_err());

    let removed = s.vault.clear_recovery().unwrap();
    assert!(removed.key.is_none());
    let v = json(&removed.vault_json);
    assert!(v.get("rk_pub").is_none() && v.get("wrapped_vk_rk").is_none());
    assert_eq!(v["min_format_version"], 1);
    assert_eq!(v["revision"], 4);
    assert!(!s.vault.has_recovery());
    assert!(s.vault.clear_recovery().is_err());
    s.vault_json = removed.vault_json;
    assert!(recover(&s, &new, false).is_err());
    assert!(unlock(&s.vault_json, pw(), &s.sk).is_ok());
}

#[test]
fn a_password_change_keeps_the_recovery_key_working() {
    let mut s = setup();
    let rotated = s
        .vault
        .change_credentials(
            pw(),
            &s.sk,
            figlock_core::CredentialChange {
                new_password: Some("changed-by-the-owner-in-settings"),
                rotate_secret_key: true,
                ..Default::default()
            },
        )
        .unwrap();
    s.vault_json = rotated.vault_json;
    let v = json(&s.vault_json);
    assert_eq!(v["min_format_version"], 2);
    assert!(v.get("rk_pub").is_some());
    assert!(recover(&s, &s.rk, false).is_ok());
}

#[test]
fn a_revocation_rewraps_the_new_vault_key_for_the_same_recovery_key() {
    let (mut a, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let mut records = vec![created.device_record.clone()];
    let rk = a.set_recovery(pw(), &sk).unwrap();
    let key = rk.key.unwrap();
    let base_json = rk.vault_json;
    let (b, rec_b) = join(&mut a, &base_json, &records, &sk, "Phone", T0 + 100);
    records.push(rec_b.bytes);
    add_login(&mut a, "Before", T0 + 150);
    let early = a.commit().unwrap().unwrap();

    let b_id = *b.vault.device_id();
    let rev = a.revoke_device(pw(), &sk, &b_id, T0 + 400).unwrap();
    let after = json(&rev.vault_json);
    assert_eq!(after["key_epoch"], 2);
    assert_eq!(after["min_format_version"], 2);
    assert_eq!(after["rk_pub"], common_hex(&key.public()));
    assert_ne!(after["wrapped_vk_rk"], json(&base_json)["wrapped_vk_rk"]);

    // Recovering from the new file gives the new epoch, and old files still open through the epoch chain.
    let a_records: Vec<Vec<u8>> = rev.records.iter().map(|c| c.bytes.clone()).collect();
    let (mut r, out) = Vault::recover(
        Recover {
            vault_json: &rev.vault_json,
            recovery_key: &key,
            new_password: NEW_PW,
            rotate_recovery: false,
        },
        fresh_device(),
        "Mac",
        &a_records,
        T0 + 1_000,
    )
    .unwrap();
    assert_eq!(r.epoch(), 2);
    r.apply_file(&early.bytes, a.device_id(), 1, T0 + 1_100)
        .unwrap();
    assert_eq!(titles(&mut r), ["Before"]);
    assert!(unlock(&out.vault_json, NEW_PW, &out.secret_key).is_ok());
}

#[test]
fn tampered_or_half_present_members_are_refused() {
    let s = setup();
    let original = json(&s.vault_json);
    let open = |v: &Value| unlock(&serde_json::to_vec(v).unwrap(), pw(), &s.sk);
    assert!(open(&original).is_ok());

    // Swapping in someone else's public key breaks the signature.
    let mut swapped = original.clone();
    swapped["rk_pub"] = common_hex(&RecoveryKey::random().unwrap().public()).into();
    assert!(open(&swapped).is_err());

    // Only one of the two members.
    for member in ["rk_pub", "wrapped_vk_rk"] {
        let mut half = original.clone();
        half.as_object_mut().unwrap().remove(member);
        assert!(open(&half).is_err(), "{member}");
    }

    // The members and the format version must agree, in both directions.
    let mut low = original.clone();
    low["min_format_version"] = 1.into();
    assert!(open(&low).is_err());
    let (_, plain) = new_vault();
    let mut high = json(&plain.vault_json);
    high["min_format_version"] = 2.into();
    assert!(
        unlock(
            &serde_json::to_vec(&high).unwrap(),
            pw(),
            &sk_copy(&plain.secret_key)
        )
        .is_err()
    );

    // A flipped byte of the wrap opens nothing (the signature covers it).
    let mut flipped = original.clone();
    let w = flipped["wrapped_vk_rk"].as_str().unwrap().to_owned();
    let mut chars: Vec<char> = w.chars().collect();
    chars[10] = if chars[10] == '0' { '1' } else { '0' };
    flipped["wrapped_vk_rk"] = chars.into_iter().collect::<String>().into();
    assert!(open(&flipped).is_err());
}
