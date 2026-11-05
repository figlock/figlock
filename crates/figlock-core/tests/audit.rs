//! Audit log tests (FIG-34, `broker.md` 5, `crypto.md` 7.5): records are sealed under the audit key, signed
//! by the device, hash-chained, and carry no vault value. Credentials and secrets are generated per run.

mod common;

use common::{T0, add_login, join, new_vault, pw, sk_copy};
use figlock_core::{ItemType, Vault, ZERO_PREV, audit_frame, audit_split};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

fn detail(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// What the broker does: keep `seq` and `prev`, append framed records.
struct Log {
    bytes: Vec<u8>,
    seq: u64,
    prev: [u8; 32],
}

impl Log {
    fn new() -> Self {
        Log {
            bytes: Vec::new(),
            seq: 0,
            prev: ZERO_PREV,
        }
    }

    fn append(&mut self, v: &Vault, ev: &str, d: Value) {
        self.seq += 1;
        let rec = v
            .seal_audit(self.seq, &self.prev, T0 + self.seq, ev, &detail(d))
            .unwrap();
        self.prev = Sha256::digest(&rec).into();
        self.bytes.extend_from_slice(&audit_frame(&rec).unwrap());
    }
}

fn three_records(v: &Vault) -> Log {
    let mut log = Log::new();
    log.append(v, "broker.start", json!({}));
    log.append(
        v,
        "grant.requested",
        json!({"peer": {"uid": 501, "pid": 99, "exe": "/bin/agent"}, "mode": "http", "ttl": "once",
               "scope": "vault"}),
    );
    log.append(v, "grant.denied", json!({"code": "denied"}));
    log
}

#[test]
fn records_round_trip_and_chain() {
    let (v, _) = new_vault();
    let log = three_records(&v);
    let (chain, damaged_tail) = v.read_audit_log(&log.bytes);
    assert!(!damaged_tail);
    assert_eq!(chain.first_bad, None);
    assert_eq!(chain.records.len(), 3);
    assert_eq!(chain.records[1].ev, "grant.requested");
    assert_eq!(chain.records[1].d["peer"]["pid"], 99);
    assert_eq!(chain.head, log.prev);
    assert_eq!(chain.records[0].prev, ZERO_PREV);
    let (empty, damaged) = v.read_audit_log(&[]);
    assert!(empty.records.is_empty() && !damaged);
    assert_eq!(empty.head, ZERO_PREV);
}

#[test]
fn only_known_events_and_members_can_be_written() {
    let (v, _) = new_vault();
    for (ev, d) in [
        ("made.up", json!({})),
        ("use.allowed", json!({"value": "x"})),
        ("use.allowed", json!({"password": "x"})),
        ("use.allowed", json!({"field": "password value"})),
    ] {
        assert!(
            v.seal_audit(1, &ZERO_PREV, T0, ev, &detail(d.clone()))
                .is_err(),
            "{ev} {d}"
        );
    }
}

#[test]
fn a_secret_value_never_reaches_the_log() {
    let (mut v, _) = new_vault();
    let mut raw = [0u8; 12];
    getrandom::fill(&mut raw).unwrap();
    let secret: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let id = v
        .add_item(
            ItemType::Login,
            "personal",
            &[("title", "Example"), ("password", secret.as_str())],
            T0,
        )
        .unwrap();
    let mut log = Log::new();
    log.append(
        &v,
        "use.allowed",
        json!({"item": hex(&id), "field": "password", "mode": "http", "host": "api.example.com"}),
    );
    let text = String::from_utf8_lossy(&log.bytes).into_owned();
    assert!(!text.contains(&secret));
    let (chain, _) = v.read_audit_log(&log.bytes);
    assert!(!format!("{:?}", chain.records).contains(&secret));
    // The log has no member that holds a value.
    let attempt = detail(json!({"value": secret.as_str()}));
    assert!(
        v.seal_audit(2, &log.prev, T0, "use.allowed", &attempt)
            .is_err()
    );
}

#[test]
fn ciphertext_has_no_plaintext() {
    let (v, _) = new_vault();
    let mut log = Log::new();
    log.append(
        &v,
        "grant.requested",
        json!({"label": "very recognisable label", "reason": "very recognisable reason"}),
    );
    let text = String::from_utf8_lossy(&log.bytes).into_owned();
    assert!(!text.contains("recognisable") && !text.contains("grant.requested"));
}

#[test]
fn tampering_is_detected() {
    let (v, _) = new_vault();
    let log = three_records(&v);
    let (files, _) = audit_split(&log.bytes);
    let frame = |parts: &[&[u8]]| -> Vec<u8> {
        parts.iter().flat_map(|p| audit_frame(p).unwrap()).collect()
    };

    // A bit flipped in the middle record.
    let mut flipped = files[1].to_vec();
    flipped[30] ^= 1;
    let (chain, _) = v.read_audit_log(&frame(&[files[0], &flipped, files[2]]));
    assert_eq!((chain.first_bad, chain.records.len()), (Some(2), 1));

    // Dropped, reordered and duplicated records.
    for (parts, bad) in [
        (vec![files[0], files[2]], 2),
        (vec![files[1], files[0], files[2]], 1),
        (vec![files[0], files[0]], 2),
    ] {
        let (chain, _) = v.read_audit_log(&frame(&parts));
        assert_eq!(chain.first_bad, Some(bad));
    }

    // Truncated tail and trailing junk are reported as a damaged tail, never skipped silently.
    let (chain, damaged) = v.read_audit_log(&log.bytes[..log.bytes.len() - 5]);
    assert_eq!(
        (chain.first_bad, chain.records.len(), damaged),
        (None, 2, true)
    );
    let mut junk = log.bytes.clone();
    junk.extend_from_slice(b"\x00\x00\x00\x07junk");
    let (chain, damaged) = v.read_audit_log(&junk);
    assert_eq!((chain.records.len(), damaged), (3, true));
}

#[test]
fn another_device_cannot_forge_or_adopt_a_log() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let records = vec![c.device_record.clone()];
    let (joined, _) = join(&mut a, &c.vault_json, &records, &sk, "Phone", T0 + 100);
    let b = joined.vault;
    let log_a = three_records(&a);
    // B holds the same Vault Key, so it derives the same audit key, but the log is bound to A's device.
    assert_eq!(b.read_audit_log(&log_a.bytes).0.first_bad, Some(1));
    let log_b = three_records(&b);
    assert_eq!(a.read_audit_log(&log_b.bytes).0.first_bad, Some(1));
}

#[test]
fn a_different_vault_cannot_read_the_log() {
    let (v, _) = new_vault();
    let log = three_records(&v);
    let (other, _) = new_vault();
    assert_eq!(other.read_audit_log(&log.bytes).0.first_bad, Some(1));
}

#[test]
fn records_from_before_a_rotation_stay_readable() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let records = vec![c.device_record.clone()];
    let (joined, _) = join(&mut a, &c.vault_json, &records, &sk, "Phone", T0 + 100);
    add_login(&mut a, "Example", T0 + 200);
    a.commit().unwrap();
    let mut log = Log::new();
    log.append(&a, "broker.start", json!({}));
    let b_id = *joined.vault.device_id();
    a.revoke_device(pw(), &sk, &b_id, T0 + 300).unwrap();
    assert_eq!(a.epoch(), 2);
    log.append(&a, "grant.revoked", json!({"grant": "0123456789abcdef"}));
    let (chain, damaged) = a.read_audit_log(&log.bytes);
    assert_eq!(
        (chain.first_bad, damaged, chain.records.len()),
        (None, false, 2)
    );
}
