//! Revocation tests (FIG-33, `pairing-recovery.md` 4, `crypto.md` 9, `sync.md` 9): the vault key rotates,
//! the revoked device loses the new epoch but its earlier work survives, and the negative cases from
//! `AGENTS.md` (revoked device, unknown device, tampered files, wrong credentials).

mod common;

use common::{T0, add_login, clone_keys, join, new_vault, pw, sk_copy, titles};
use figlock_core::{DeviceKeys, Invite, JoinAttempt, Revocation, SecretKey, Vault};
use serde_json::Value;

/// A vault with three devices: A (trusted, `a`), B and C. Each knows the others' records.
struct Fleet {
    a: Vault,
    b: Vault,
    c: Vault,
    sk: SecretKey,
    records: Vec<Vec<u8>>,
}

fn fleet() -> Fleet {
    let (mut a, created) = new_vault();
    let sk = sk_copy(&created.secret_key);
    let mut records = vec![created.device_record.clone()];
    let (b, rec_b) = join(
        &mut a,
        &created.vault_json,
        &records,
        &sk,
        "Phone",
        T0 + 100,
    );
    records.push(rec_b.bytes);
    let (c, rec_c) = join(
        &mut a,
        &created.vault_json,
        &records,
        &sk,
        "Work PC",
        T0 + 200,
    );
    records.push(rec_c.bytes);
    let (mut b, mut c) = (b.vault, c.vault);
    for r in &records {
        // Everyone learns every record, as the sync engine does from devices/.
        let _ = b.add_device_record(r);
        let _ = c.add_device_record(r);
    }
    Fleet {
        a,
        b,
        c,
        sk,
        records,
    }
}

fn vault_json_value(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

/// Everything a remaining device reads after a revocation: new vault.json, its records, the snapshot.
fn unlock_after(r: &Revocation, sk: &SecretKey, name: &str, now: u64) -> Vault {
    let records: Vec<Vec<u8>> = r.records.iter().map(|c| c.bytes.clone()).collect();
    let (v, own) = Vault::unlock(
        &r.vault_json,
        pw(),
        sk,
        DeviceKeys::generate().unwrap(),
        name,
        &records,
        now,
    )
    .unwrap();
    // A brand new device identity has no record; that is expected for a reader-only check.
    assert!(own.is_some());
    v
}

#[test]
fn revoking_rotates_the_epoch_and_signs_the_revoked_list() {
    let mut f = fleet();
    add_login(&mut f.a, "Cloudflare", T0 + 300);
    f.a.commit().unwrap().unwrap();
    let c_id = *f.c.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();

    assert_eq!(f.a.epoch(), 2);
    let v = vault_json_value(&r.vault_json);
    assert_eq!(v["key_epoch"], 2);
    assert_eq!(v["revision"], 2);
    let revoked = v["revoked"].as_array().unwrap();
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0]["device_id"], hex(&c_id));
    assert_eq!(revoked[0]["epoch"], 2);
    assert_eq!(v["epoch_keys"].as_array().unwrap().len(), 1);
    // Only the devices that stay are re-signed.
    assert_eq!(r.records.len(), 2);
    assert!(r.records.iter().all(|c| !c.path.contains(&hex(&c_id))));
    assert_eq!(r.remove, [format!("devices/{}.json", hex(&c_id))]);
    assert!(
        r.snapshot
            .path
            .starts_with(&format!("snap/{}-", hex(f.a.device_id())))
    );

    let list = f.a.devices();
    assert_eq!(list.len(), 3);
    assert!(
        list.iter()
            .find(|d| d.device_id == c_id)
            .unwrap()
            .revoked_at
            .is_some()
    );
}

#[test]
fn remaining_devices_unlock_the_new_epoch_and_read_old_and_new_data() {
    let mut f = fleet();
    add_login(&mut f.a, "Before", T0 + 300);
    let old = f.a.commit().unwrap().unwrap();
    f.b.apply_file(&old.bytes, f.a.device_id(), 1, T0 + 310)
        .unwrap();
    add_login(&mut f.b, "From phone", T0 + 320);
    let from_b = f.b.commit().unwrap().unwrap();
    f.a.apply_file(&from_b.bytes, f.b.device_id(), 1, T0 + 330)
        .unwrap();

    let c_id = *f.c.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();
    add_login(&mut f.a, "After", T0 + 500);
    let new = f.a.commit().unwrap().unwrap();
    assert_eq!(
        new.path,
        "log/".to_owned() + &hex(f.a.device_id()) + "/00000002.op"
    );

    // B, back from offline, does a full unlock with the same credentials and its own device keys.
    let records: Vec<Vec<u8>> = r.records.iter().map(|c| c.bytes.clone()).collect();
    let b_keys = clone_keys(&f.b);
    let (mut b2, own) = Vault::unlock(
        &r.vault_json,
        pw(),
        &f.sk,
        b_keys,
        "Phone",
        &records,
        T0 + 600,
    )
    .unwrap();
    assert!(own.is_none(), "B keeps its record through the rotation");
    assert_eq!(b2.epoch(), 2);
    // Old-epoch files open through the epoch chain, new ones through the new key, items of both epochs read.
    b2.apply_file(&old.bytes, f.a.device_id(), 1, T0 + 610)
        .unwrap();
    b2.apply_file(&new.bytes, f.a.device_id(), 2, T0 + 620)
        .unwrap();
    b2.apply_file(&from_b.bytes, f.b.device_id(), 1, T0 + 630)
        .unwrap();
    assert_eq!(titles(&mut b2), ["After", "Before", "From phone"]);
}

#[test]
fn the_revoked_device_cannot_read_or_write_the_new_epoch() {
    let mut f = fleet();
    add_login(&mut f.a, "Before", T0 + 300);
    let old = f.a.commit().unwrap().unwrap();
    f.c.apply_file(&old.bytes, f.a.device_id(), 1, T0 + 310)
        .unwrap();

    let c_id = *f.c.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();
    add_login(&mut f.a, "Secret after revocation", T0 + 500);
    let new = f.a.commit().unwrap().unwrap();

    // C still holds the old Vault Key, but no file of the new epoch opens with it.
    assert!(
        f.c.apply_file(&new.bytes, f.a.device_id(), 2, T0 + 600)
            .is_err()
    );
    assert!(
        f.c.apply_snapshot(&r.snapshot.bytes, f.a.device_id(), 1, T0 + 600)
            .is_err()
    );
    // C cannot be re-admitted with its old record: it is not signed by the new authority key.
    let mut b2 = unlock_after(&r, &f.sk, "Phone", T0 + 600);
    assert!(b2.add_device_record(&f.records[2]).is_err());
    // And the new epoch's files do not contain the plaintext.
    assert!(!contains(&new.bytes, b"Secret after revocation"));
}

#[test]
fn files_the_revoked_device_wrote_before_revocation_stay_readable() {
    let mut f = fleet();
    add_login(&mut f.c, "Written by C early", T0 + 250);
    let early = f.c.commit().unwrap().unwrap();
    let c_id = *f.c.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();

    let mut b2 = unlock_after(&r, &f.sk, "Phone", T0 + 600);
    b2.apply_file(&early.bytes, &c_id, 1, T0 + 610).unwrap();
    assert_eq!(titles(&mut b2), ["Written by C early"]);
}

#[test]
fn files_the_revoked_device_writes_after_revocation_are_ignored() {
    let mut f = fleet();
    add_login(&mut f.c, "Written by C early", T0 + 250);
    let early = f.c.commit().unwrap().unwrap();
    let c_id = *f.c.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();

    // C still has the epoch 1 keys and keeps writing, stamped after the revocation.
    add_login(&mut f.c, "Written by C late", T0 + 800);
    let late = f.c.commit().unwrap().unwrap();

    let mut b2 = unlock_after(&r, &f.sk, "Phone", T0 + 900);
    b2.apply_file(&early.bytes, &c_id, 1, T0 + 910).unwrap();
    assert!(b2.apply_file(&late.bytes, &c_id, 2, T0 + 920).is_err());
    assert_eq!(titles(&mut b2), ["Written by C early"]);
}

#[test]
fn a_new_device_can_start_from_the_snapshot_alone() {
    let mut f = fleet();
    add_login(&mut f.a, "One", T0 + 300);
    let doomed = add_login(&mut f.a, "Two (deleted)", T0 + 301);
    f.a.commit().unwrap().unwrap();
    f.a.delete_item(&doomed, T0 + 310).unwrap();
    add_login(&mut f.b, "Three from phone", T0 + 320);
    let from_b = f.b.commit().unwrap().unwrap();
    f.a.apply_file(&from_b.bytes, f.b.device_id(), 1, T0 + 330)
        .unwrap();
    f.a.commit().unwrap().unwrap();

    let c_id = *f.c.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();

    let mut d = unlock_after(&r, &f.sk, "Tablet", T0 + 600);
    d.apply_snapshot(&r.snapshot.bytes, f.a.device_id(), 2, T0 + 610)
        .unwrap();
    assert_eq!(titles(&mut d), ["One", "Three from phone"]);
    let (all, corrupt) = d.items(true);
    assert!(corrupt.is_empty());
    assert_eq!(all.len(), 3);
    assert!(
        all.iter()
            .any(|i| i.deleted && i.fields["title"].as_str() == "Two (deleted)")
    );

    // A snapshot is only taken from the device that signed it, at its own sequence number.
    assert!(
        d.apply_snapshot(&r.snapshot.bytes, f.b.device_id(), 2, T0 + 620)
            .is_err()
    );
    assert!(
        d.apply_snapshot(&r.snapshot.bytes, f.a.device_id(), 1, T0 + 620)
            .is_err()
    );
    let mut flipped = r.snapshot.bytes.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 1;
    assert!(
        d.apply_snapshot(&flipped, f.a.device_id(), 2, T0 + 620)
            .is_err()
    );
}

#[test]
fn a_revocation_that_cannot_proceed_changes_nothing() {
    let mut f = fleet();
    let (b_id, c_id) = (*f.b.device_id(), *f.c.device_id());
    let own = *f.a.device_id();
    // Not yourself, not a stranger, not with the wrong password or Secret Key, not with unsent changes.
    assert!(f.a.revoke_device(pw(), &f.sk, &own, T0 + 400).is_err());
    assert!(
        f.a.revoke_device(pw(), &f.sk, &[9u8; 16], T0 + 400)
            .is_err()
    );
    assert!(
        f.a.revoke_device("not the password", &f.sk, &c_id, T0 + 400)
            .is_err()
    );
    let other = SecretKey::random().unwrap();
    assert!(f.a.revoke_device(pw(), &other, &c_id, T0 + 400).is_err());
    add_login(&mut f.a, "Unsent", T0 + 410);
    assert!(f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 420).is_err());
    f.a.commit().unwrap().unwrap();
    assert_eq!(f.a.epoch(), 1, "every refusal left the vault at epoch 1");
    assert!(f.a.devices().iter().all(|d| d.revoked_at.is_none()));

    // The vault is still fully usable: the same revocation now works, once, and cannot be repeated.
    f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 430).unwrap();
    assert!(f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 440).is_err());
    assert!(f.a.revoke_device(pw(), &f.sk, &b_id, T0 + 450).is_ok());
}

#[test]
fn a_revoked_device_id_cannot_join_again() {
    let mut f = fleet();
    let c_keys = clone_keys(&f.c);
    let c_id = c_keys.device_id;
    f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();
    let (mut invite, offer) = Invite::create(f.a.vault_id(), T0 + 500).unwrap();
    let (_, req) =
        JoinAttempt::begin(&offer.code, &offer.inv, c_keys, "Work PC", T0 + 501).unwrap();
    let opened = invite.open_request(&req.bytes, T0 + 502).unwrap();
    assert!(f.a.approve_join(&invite, &opened, &f.sk, T0 + 503).is_err());
}

#[test]
fn rotations_chain_and_items_from_every_epoch_stay_readable() {
    let mut f = fleet();
    add_login(&mut f.a, "Epoch one", T0 + 300);
    let one = f.a.commit().unwrap().unwrap();
    let c_id = *f.c.device_id();
    let first = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();
    add_login(&mut f.a, "Epoch two", T0 + 500);
    let two = f.a.commit().unwrap().unwrap();

    // A new device joins at epoch 2 and is then revoked in turn.
    let records: Vec<Vec<u8>> = first.records.iter().map(|c| c.bytes.clone()).collect();
    let (d, _) = join(
        &mut f.a,
        &first.vault_json,
        &records,
        &f.sk,
        "Tablet",
        T0 + 600,
    );
    let d_id = *d.vault.device_id();
    add_login(&mut f.a, "Epoch two later", T0 + 700);
    let two_b = f.a.commit().unwrap().unwrap();
    let second = f.a.revoke_device(pw(), &f.sk, &d_id, T0 + 800).unwrap();
    assert_eq!(f.a.epoch(), 3);
    let v = vault_json_value(&second.vault_json);
    assert_eq!(v["epoch_keys"].as_array().unwrap().len(), 2);
    assert_eq!(v["revoked"].as_array().unwrap().len(), 2);
    add_login(&mut f.a, "Epoch three", T0 + 900);
    let three = f.a.commit().unwrap().unwrap();

    let mut b3 = unlock_after(&second, &f.sk, "Phone", T0 + 1000);
    for (i, file) in [&one, &two, &two_b, &three].into_iter().enumerate() {
        b3.apply_file(&file.bytes, f.a.device_id(), (i + 1) as u64, T0 + 1010)
            .unwrap();
    }
    assert_eq!(
        titles(&mut b3),
        ["Epoch one", "Epoch three", "Epoch two", "Epoch two later"]
    );
}

#[test]
fn a_vault_json_with_the_revocation_removed_or_chain_cut_is_refused() {
    let mut f = fleet();
    let c_id = *f.c.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();
    let records: Vec<Vec<u8>> = r.records.iter().map(|c| c.bytes.clone()).collect();
    let try_unlock = |json: &[u8]| {
        Vault::unlock(
            json,
            pw(),
            &f.sk,
            DeviceKeys::generate().unwrap(),
            "Phone",
            &records,
            T0 + 500,
        )
        .is_ok()
    };
    assert!(try_unlock(&r.vault_json));
    for member in ["revoked", "epoch_keys"] {
        let mut v = vault_json_value(&r.vault_json);
        v.as_object_mut().unwrap().remove(member);
        assert!(
            !try_unlock(&serde_json::to_vec(&v).unwrap()),
            "removing {member} breaks the signature"
        );
    }
    let mut v = vault_json_value(&r.vault_json);
    v["revoked"][0]["revoked_at"] = serde_json::json!([T0 + 10_000_000, 0]);
    assert!(!try_unlock(&serde_json::to_vec(&v).unwrap()));
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn unlock_with_records(r: &Revocation, sk: &SecretKey, extra: &[Vec<u8>]) -> Vault {
    let mut records: Vec<Vec<u8>> = r.records.iter().map(|c| c.bytes.clone()).collect();
    records.extend(extra.iter().cloned());
    Vault::unlock(
        &r.vault_json,
        pw(),
        sk,
        DeviceKeys::generate().unwrap(),
        "Tablet",
        &records,
        T0 + 600,
    )
    .unwrap()
    .0
}

fn trusted(v: &Vault, id: &[u8; 16]) -> bool {
    v.devices()
        .iter()
        .any(|d| &d.device_id == id && d.revoked_at.is_none())
}

#[test]
fn unlock_skips_stale_records_and_never_trusts_them() {
    let mut f = fleet();
    let c_id = *f.c.device_id();
    let b_id = *f.b.device_id();
    let r = f.a.revoke_device(pw(), &f.sk, &c_id, T0 + 400).unwrap();

    // The revoked device's old record and a survivor's old-signature record are still lying around.
    let stale = vec![f.records[2].clone(), f.records[1].clone()];
    let v = unlock_with_records(&r, &f.sk, &stale);
    assert!(!trusted(&v, &c_id));
    assert!(trusted(&v, &b_id));

    // A record whose body was edited fails verification and is skipped.
    let mut forged = r.records[0].bytes.clone();
    let at = forged.iter().position(|&b| b == b'"').unwrap() + 1;
    forged[at] ^= 0x01;
    let v = unlock_with_records(&r, &f.sk, &[forged]);
    assert!(trusted(&v, &b_id));
}
