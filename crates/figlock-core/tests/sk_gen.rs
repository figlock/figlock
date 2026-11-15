//! The Secret Key generation `sk_gen` (`crypto.md` 9.4, ADR-0034): rotation raises it, a new password and a
//! revocation leave it, and a device compares it with the one its kept key belongs to.

mod common;

use common::{T0, new_vault, pw, sk_copy};
use figlock_core::{CredentialChange, DeviceKeys, KeptKey, Recover, RecoveryKey, Vault, kept_key};
use serde_json::{Value, json};

const NEW_PW: &str = "a-different-generated-passphrase-for-tests";

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

fn rotate() -> CredentialChange<'static> {
    CredentialChange {
        rotate_secret_key: true,
        ..CredentialChange::default()
    }
}

#[test]
fn a_new_vault_has_generation_one_and_the_member_is_absent() {
    let (v, created) = new_vault();
    assert_eq!(v.sk_gen(), 1);
    assert_eq!(Vault::sk_gen_in(&created.vault_json).unwrap(), 1);
    let body = json(&created.vault_json);
    assert!(body.get("sk_gen").is_none());
    assert_eq!(body["min_format_version"], 1);
}

#[test]
fn rotation_raises_it_and_a_password_change_does_not() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);

    let changed = v
        .change_credentials(
            pw(),
            &sk,
            CredentialChange {
                new_password: Some(NEW_PW),
                ..CredentialChange::default()
            },
        )
        .unwrap();
    assert_eq!(v.sk_gen(), 1, "a new password keeps the generation");
    assert_eq!(Vault::sk_gen_in(&changed.vault_json).unwrap(), 1);

    let rotated = v.change_credentials(NEW_PW, &sk, rotate()).unwrap();
    assert_eq!(v.sk_gen(), 2);
    assert_eq!(Vault::sk_gen_in(&rotated.vault_json).unwrap(), 2);
    let body = json(&rotated.vault_json);
    assert_eq!(body["sk_gen"], 2);
    assert_eq!(body["min_format_version"], 2, "an old client refuses it");

    let new_sk = rotated.secret_key.unwrap();
    let again = v
        .change_credentials(
            NEW_PW,
            &new_sk,
            CredentialChange {
                new_password: Some(pw()),
                ..CredentialChange::default()
            },
        )
        .unwrap();
    assert_eq!(
        Vault::sk_gen_in(&again.vault_json).unwrap(),
        2,
        "a later password change keeps the raised generation"
    );
    assert_eq!(json(&again.vault_json)["min_format_version"], 2);

    let third = v.change_credentials(pw(), &new_sk, rotate()).unwrap();
    assert_eq!(Vault::sk_gen_in(&third.vault_json).unwrap(), 3);
}

#[test]
fn unlocking_reads_the_generation_back() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let rotated = v.change_credentials(pw(), &sk, rotate()).unwrap();
    let new_sk = rotated.secret_key.unwrap();
    let (opened, _) = Vault::unlock(
        &rotated.vault_json,
        pw(),
        &new_sk,
        DeviceKeys::generate().unwrap(),
        "Reader",
        std::slice::from_ref(&created.device_record),
        T0 + 900,
    )
    .unwrap();
    assert_eq!(opened.sk_gen(), 2);
}

#[test]
fn a_revocation_keeps_it_and_one_that_rotates_raises_it() {
    let (mut a, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let records = vec![created.device_record.clone()];
    let (c, _) = common::join(
        &mut a,
        &created.vault_json,
        &records,
        &sk,
        "Phone",
        T0 + 100,
    );
    let c_id = *c.vault.device_id();
    let plain = a.revoke_device(pw(), &sk, &c_id, T0 + 300).unwrap();
    assert_eq!(Vault::sk_gen_in(&plain.vault_json).unwrap(), 1);
    assert_eq!(a.sk_gen(), 1);

    let (mut b, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let records = vec![created.device_record.clone()];
    let (c, _) = common::join(
        &mut b,
        &created.vault_json,
        &records,
        &sk,
        "Phone",
        T0 + 100,
    );
    let c_id = *c.vault.device_id();
    let both = b
        .revoke_device_and_change_credentials(pw(), &sk, &c_id, rotate(), T0 + 300)
        .unwrap();
    assert_eq!(Vault::sk_gen_in(&both.vault_json).unwrap(), 2);
    assert_eq!(b.sk_gen(), 2);
}

#[test]
fn recovery_makes_a_new_secret_key_and_counts_as_a_rotation() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let change = v.set_recovery(pw(), &sk).unwrap();
    let rk: RecoveryKey = change.key.unwrap();
    let (vault, out) = Vault::recover(
        Recover {
            vault_json: &change.vault_json,
            recovery_key: &rk,
            new_password: NEW_PW,
            new_recovery_key: None,
        },
        DeviceKeys::generate().unwrap(),
        "Reinstalled Mac",
        std::slice::from_ref(&created.device_record),
        T0 + 1_000,
    )
    .unwrap();
    assert_eq!(vault.sk_gen(), 2);
    assert_eq!(Vault::sk_gen_in(&out.vault_json).unwrap(), 2);
}

#[test]
fn enabling_the_recovery_key_leaves_the_generation() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let rotated = v.change_credentials(pw(), &sk, rotate()).unwrap();
    let new_sk = rotated.secret_key.unwrap();
    let change = v.set_recovery(pw(), &new_sk).unwrap();
    assert_eq!(Vault::sk_gen_in(&change.vault_json).unwrap(), 2);
}

#[test]
fn a_generation_that_is_not_written_canonically_is_refused() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let rotated = v.change_credentials(pw(), &sk, rotate()).unwrap();
    let body = json(&rotated.vault_json);

    let with = |value: Value, min: u64| {
        let mut b = body.clone();
        b["sk_gen"] = value;
        b["min_format_version"] = json!(min);
        serde_json::to_vec(&b).unwrap()
    };
    assert_eq!(Vault::sk_gen_in(&with(json!(7), 2)).unwrap(), 7);
    // 1 is the absent value, 0 and negatives and text are not generations, and the version must say 2.
    for bad in [
        json!(1),
        json!(0),
        json!(-1),
        json!("2"),
        json!(2.5),
        json!(u64::MAX),
    ] {
        assert!(Vault::sk_gen_in(&with(bad.clone(), 2)).is_err(), "{bad}");
    }
    assert!(Vault::sk_gen_in(&with(json!(2), 1)).is_err());
    // Dropping the member while the version still says 2 is not a vault without a rotation either.
    let mut dropped = body.clone();
    dropped.as_object_mut().unwrap().remove("sk_gen");
    assert!(Vault::sk_gen_in(&serde_json::to_vec(&dropped).unwrap()).is_err());
}

#[test]
fn a_device_compares_its_kept_generation_with_the_vaults() {
    assert_eq!(kept_key(1, 1), KeptKey::Current);
    assert_eq!(kept_key(3, 3), KeptKey::Current);
    assert_eq!(kept_key(1, 2), KeptKey::Stale);
    assert_eq!(kept_key(2, 5), KeptKey::Stale);
    assert_eq!(
        kept_key(3, 2),
        KeptKey::RolledBack,
        "a vault.json older than this device has seen is refused"
    );
}

#[test]
fn credentials_are_checked_without_unlocking() {
    let (mut v, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let rotated = v.change_credentials(pw(), &sk, rotate()).unwrap();
    let new_sk = rotated.secret_key.unwrap();
    assert!(Vault::verify_credentials(&rotated.vault_json, pw(), &new_sk).is_ok());
    assert!(Vault::verify_credentials(&rotated.vault_json, pw(), &sk).is_err());
    assert!(Vault::verify_credentials(&rotated.vault_json, NEW_PW, &new_sk).is_err());
}

// ---- the committed vectors (`docs/spec/vectors/sk-gen.json`) ----

fn vectors() -> Vec<Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/vectors/sk-gen.json");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(doc["format"], "figlock-vectors-1");
    doc["cases"].as_array().unwrap().clone()
}

/// A vault whose `sk_gen` is `before`, built by rotating that many times.
fn vault_at(before: u64) -> (Vault, figlock_core::SecretKey, Vec<u8>, Vec<u8>) {
    let (mut v, created) = new_vault();
    let mut sk = sk_copy(&created.secret_key);
    let mut json = created.vault_json.clone();
    for _ in 1..before {
        let r = v.change_credentials(pw(), &sk, rotate()).unwrap();
        sk = r.secret_key.unwrap();
        json = r.vault_json;
    }
    assert_eq!(u64::from(v.sk_gen()), before);
    (v, sk, json, created.device_record)
}

#[test]
fn the_vectors_say_what_each_event_does_to_the_generation() {
    let mut checked = 0;
    for c in vectors() {
        let (i, e, name) = (&c["inputs"], &c["expected"], c["name"].as_str().unwrap());
        let Some(event) = i.get("event").and_then(Value::as_str) else {
            continue;
        };
        let before = i["sk_gen_before"].as_u64().unwrap();
        if e == "reject" {
            // Generation 0 cannot be built; the ceiling is a counter overflow, which `checked_add` refuses.
            assert!(before == 0 || before == u64::from(u32::MAX), "{name}");
            continue;
        }
        // Building a vault at a high generation means rotating that many times; the reference covers the
        // rule for any number, and the Rust core is exercised from 1 to 3.
        if before > 3 {
            continue;
        }
        let (mut v, sk, json, record) = vault_at(before);
        let after = match event {
            "change_password" => {
                let r = v
                    .change_credentials(
                        pw(),
                        &sk,
                        CredentialChange {
                            new_password: Some(NEW_PW),
                            ..CredentialChange::default()
                        },
                    )
                    .unwrap();
                Vault::sk_gen_in(&r.vault_json).unwrap()
            }
            "change_kdf_params" => {
                let r = v
                    .change_credentials(
                        pw(),
                        &sk,
                        CredentialChange {
                            params: Some(figlock_core::KdfParams {
                                t: figlock_core::KdfParams::FLOOR.t + 1,
                                ..figlock_core::KdfParams::FLOOR
                            }),
                            ..CredentialChange::default()
                        },
                    )
                    .unwrap();
                Vault::sk_gen_in(&r.vault_json).unwrap()
            }
            "rotate_secret_key" => {
                let r = v.change_credentials(pw(), &sk, rotate()).unwrap();
                Vault::sk_gen_in(&r.vault_json).unwrap()
            }
            "set_recovery" => {
                Vault::sk_gen_in(&v.set_recovery(pw(), &sk).unwrap().vault_json).unwrap()
            }
            "clear_recovery" => {
                v.set_recovery(pw(), &sk).unwrap();
                Vault::sk_gen_in(&v.clear_recovery().unwrap().vault_json).unwrap()
            }
            "revoke_device" | "revoke_device_and_rotate_secret_key" => {
                let device = common::join(&mut v, &json, &[record], &sk, "Phone", T0 + 100);
                let id = *device.0.vault.device_id();
                let r = if event == "revoke_device" {
                    v.revoke_device(pw(), &sk, &id, T0 + 300)
                        .unwrap()
                        .vault_json
                } else {
                    v.revoke_device_and_change_credentials(pw(), &sk, &id, rotate(), T0 + 300)
                        .unwrap()
                        .vault_json
                };
                Vault::sk_gen_in(&r).unwrap()
            }
            "recover_with_recovery_key" => {
                let change = v.set_recovery(pw(), &sk).unwrap();
                let rk: RecoveryKey = change.key.unwrap();
                let (_, out) = Vault::recover(
                    Recover {
                        vault_json: &change.vault_json,
                        recovery_key: &rk,
                        new_password: NEW_PW,
                        new_recovery_key: None,
                    },
                    DeviceKeys::generate().unwrap(),
                    "Reinstalled Mac",
                    &[record],
                    T0 + 1_000,
                )
                .unwrap();
                Vault::sk_gen_in(&out.vault_json).unwrap()
            }
            other => panic!("{name}: no Rust check for {other}"),
        };
        assert_eq!(
            u64::from(after),
            e["sk_gen_after"].as_u64().unwrap(),
            "{name}"
        );
        checked += 1;
    }
    assert!(checked >= 10, "{checked}");
}

#[test]
fn the_vectors_say_how_the_member_is_written_and_read() {
    let (_, _, json, _) = vault_at(2);
    let body = |member: &Value, version: u64, recovery: bool| {
        let mut b: Value = serde_json::from_slice(&json).unwrap();
        b.as_object_mut().unwrap().remove("sk_gen");
        if !member.is_null() {
            b["sk_gen"] = member.clone();
        }
        b["min_format_version"] = serde_json::json!(version);
        if recovery {
            // A body that is version 2 for another reason (the Recovery Key members).
            let (mut other, created) = new_vault();
            let sk = sk_copy(&created.secret_key);
            let wrapped: Value =
                serde_json::from_slice(&other.set_recovery(pw(), &sk).unwrap().vault_json).unwrap();
            b["rk_pub"] = wrapped["rk_pub"].clone();
            b["wrapped_vk_rk"] = wrapped["wrapped_vk_rk"].clone();
        }
        serde_json::to_vec(&b).unwrap()
    };
    let mut checked = 0;
    for c in vectors() {
        let (i, e, name) = (&c["inputs"], &c["expected"], c["name"].as_str().unwrap());
        if let Some(gen_in) = i.get("sk_gen") {
            let n = gen_in.as_u64().unwrap();
            if n > 3 {
                continue;
            }
            let (_, _, written, _) = vault_at(n);
            let w: Value = serde_json::from_slice(&written).unwrap();
            let want = e["body"].as_object().unwrap();
            assert_eq!(w.get("sk_gen"), want.get("sk_gen"), "{name}");
            assert_eq!(
                w["min_format_version"],
                want.get("min_format_version")
                    .cloned()
                    .unwrap_or(serde_json::json!(1)),
                "{name}"
            );
            checked += 1;
        } else if let Some(member) = i.get("member") {
            let version = i["min_format_version"].as_u64().unwrap();
            let recovery = i["other_version_2_members"].as_bool().unwrap();
            let read = Vault::sk_gen_in(&body(member, version, recovery));
            if e == "reject" {
                assert!(read.is_err(), "{name}");
            } else {
                assert_eq!(
                    u64::from(read.expect(name)),
                    e["sk_gen"].as_u64().unwrap(),
                    "{name}"
                );
            }
            checked += 1;
        } else if let Some(kept) = i.get("kept_gen") {
            let (kept, vault) = (
                kept.as_u64().unwrap() as u32,
                i["vault_gen"].as_u64().unwrap() as u32,
            );
            match (
                kept_key(kept, vault),
                e.as_str(),
                e.get("kept_key").and_then(Value::as_str),
            ) {
                (KeptKey::RolledBack, Some("reject"), _) => {}
                (KeptKey::Current, _, Some("current")) | (KeptKey::Stale, _, Some("stale")) => {}
                other => panic!("{name}: {other:?}"),
            }
            checked += 1;
        }
    }
    assert!(checked >= 10, "{checked}");
}
