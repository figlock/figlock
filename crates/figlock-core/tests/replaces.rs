//! A phone added to a vault again after it was removed marks its earlier device entry as its own (ADR-0044), so
//! the lists show one device. The hint only labels an entry: it is signed with the record, changes no key and
//! revokes nothing. Every credential is generated per run.

mod common;

use common::{T0, new_vault, pw, sk_copy};
use figlock_core::{DeviceKeys, Vault};
use serde_json::Value;

fn unlock_as(
    vault_json: &[u8],
    sk: &figlock_core::SecretKey,
    keys: DeviceKeys,
    records: &[Vec<u8>],
    now: u64,
) -> (Vault, Option<Vec<u8>>) {
    Vault::unlock(vault_json, pw(), sk, keys, "iPhone", records, now).unwrap()
}

fn id_hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_phone_added_again_replaces_its_earlier_entry_in_the_list() {
    let (_laptop, c) = new_vault();
    let sk = sk_copy(&c.secret_key);

    // First install: the phone writes its record.
    let first = DeviceKeys::generate().unwrap();
    let first_id = first.device_id;
    let (_, r1) = unlock_as(
        &c.vault_json,
        &sk,
        first,
        std::slice::from_ref(&c.device_record),
        T0 + 10,
    );
    let r1 = r1.expect("first record");
    assert!(
        serde_json::from_slice::<Value>(&r1)
            .unwrap()
            .get("replaces")
            .is_none(),
        "a first install replaces nothing"
    );

    // Removed and added again: new keys, with the earlier id as the hint.
    let mut second = DeviceKeys::generate().unwrap();
    second.replaces = Some(first_id);
    let second_id = second.device_id;
    let known = vec![c.device_record.clone(), r1.clone()];
    let (_, r2) = unlock_as(&c.vault_json, &sk, second, &known, T0 + 20);
    let r2 = r2.expect("second record");
    let body: Value = serde_json::from_slice(&r2).unwrap();
    assert_eq!(body["replaces"], id_hex(&first_id));

    // Another device reading all three records sees the first entry as the second's.
    let reader = DeviceKeys::generate().unwrap();
    let all = vec![c.device_record.clone(), r1, r2];
    let (reader, _) = unlock_as(&c.vault_json, &sk, reader, &all, T0 + 30);
    let list = reader.devices();
    let old = list.iter().find(|d| d.device_id == first_id).unwrap();
    let new = list.iter().find(|d| d.device_id == second_id).unwrap();
    assert_eq!(old.replaced_by, Some(second_id));
    assert_eq!(new.replaced_by, None);
    assert!(old.revoked_at.is_none(), "nothing is revoked by the hint");
}

#[test]
fn a_device_never_replaces_itself_and_an_unknown_id_hides_nothing() {
    let (_laptop, c) = new_vault();
    let sk = sk_copy(&c.secret_key);

    let mut own = DeviceKeys::generate().unwrap();
    own.replaces = Some(own.device_id);
    let (_, rec) = unlock_as(
        &c.vault_json,
        &sk,
        own,
        std::slice::from_ref(&c.device_record),
        T0 + 10,
    );
    let body: Value = serde_json::from_slice(&rec.unwrap()).unwrap();
    assert!(
        body.get("replaces").is_none(),
        "pointing at itself is dropped"
    );

    // An id no record has: the entry is written, and no other device is marked.
    let mut stray = DeviceKeys::generate().unwrap();
    stray.replaces = Some([7; 16]);
    let (_, rec) = unlock_as(
        &c.vault_json,
        &sk,
        stray,
        std::slice::from_ref(&c.device_record),
        T0 + 20,
    );
    let reader = DeviceKeys::generate().unwrap();
    let all = vec![c.device_record.clone(), rec.unwrap()];
    let (reader, _) = unlock_as(&c.vault_json, &sk, reader, &all, T0 + 30);
    assert!(reader.devices().iter().all(|d| d.replaced_by.is_none()));
}

#[test]
fn a_record_with_a_changed_replaces_member_is_not_a_device() {
    let (_laptop, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let victim = DeviceKeys::generate().unwrap();
    let victim_id = victim.device_id;
    let (_, victim_rec) = unlock_as(
        &c.vault_json,
        &sk,
        victim,
        std::slice::from_ref(&c.device_record),
        T0 + 10,
    );

    let mut phone = DeviceKeys::generate().unwrap();
    let phone_id = phone.device_id;
    phone.replaces = None;
    let (_, rec) = unlock_as(
        &c.vault_json,
        &sk,
        phone,
        std::slice::from_ref(&c.device_record),
        T0 + 20,
    );
    // Someone adds a `replaces` to a signed record to hide another device: the signature no longer holds.
    let mut forged: Value = serde_json::from_slice(&rec.unwrap()).unwrap();
    forged["replaces"] = Value::String(id_hex(&victim_id));
    let forged = serde_json::to_vec(&forged).unwrap();

    let reader = DeviceKeys::generate().unwrap();
    let all = vec![c.device_record.clone(), victim_rec.unwrap(), forged];
    let (reader, _) = unlock_as(&c.vault_json, &sk, reader, &all, T0 + 30);
    let list = reader.devices();
    assert!(
        list.iter().all(|d| d.device_id != phone_id),
        "forged record is skipped"
    );
    assert!(list.iter().all(|d| d.replaced_by.is_none()));
}
