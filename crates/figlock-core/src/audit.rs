//! Broker audit records (`crypto.md` 7.5, `broker.md` 5).
//!
//! A record is one file container of kind `Audit`, sealed under the audit key `AK` and signed by the writing
//! device. Its plaintext is canonical JSON `{"d":..,"ev":..,"prev":..,"t":..,"v":1}`. `prev` is the SHA-256
//! of the previous record's container bytes, so a record cannot be changed, dropped, inserted or reordered
//! without the chain breaking. The detail member `d` has a fixed set of members with fixed types: nothing
//! else can be written, so a vault value cannot get into the log by accident.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::file::{FILE_HEADER_LEN, FileId, Kind, read_file};
use crate::signing::jcs;
use crate::vault::{hexs, unhex};
use crate::{Error, Key, Result};

/// `prev` of the first record.
pub const ZERO_PREV: [u8; 32] = [0; 32];

/// One record's container is at most this many bytes; a longer length prefix is treated as damage.
const MAX_RECORD_BYTES: usize = 64 * 1024;

pub const AUDIT_EVENTS: [&str; 10] = [
    "broker.start",
    "broker.lock",
    "conn.refused",
    "grant.requested",
    "grant.approved",
    "grant.denied",
    "grant.revoked",
    "grant.expired",
    "use.allowed",
    "use.refused",
];

const MODES: [&str; 3] = ["http", "run", "reveal"];
const TTLS: [&str; 5] = ["once", "15m", "session", "1d", "until-lock"];
const CODES: [&str; 9] = [
    "bad_request",
    "denied",
    "no_grant",
    "expired",
    "not_allowed",
    "locked",
    "busy",
    "audit_failed",
    "internal",
];

/// A verified record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    pub seq: u64,
    pub prev: [u8; 32],
    /// Unix milliseconds.
    pub t: u64,
    pub ev: String,
    pub d: Map<String, Value>,
}

/// The records that verified, the hash a new record must chain to, and where verification stopped.
#[derive(Debug, Default)]
pub struct AuditChain {
    pub records: Vec<AuditRecord>,
    /// SHA-256 of the last good record's container; [`ZERO_PREV`] when there is none.
    pub head: [u8; 32],
    /// The `seq` of the first record that failed; later records are not shown.
    pub first_bad: Option<u64>,
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn text(v: &Value, max_chars: usize) -> Result<&str> {
    let s = v.as_str().ok_or(Error)?;
    if s.chars().count() > max_chars || s.chars().any(char::is_control) {
        return Err(Error);
    }
    Ok(s)
}

fn one_of(v: &Value, set: &[&str]) -> Result<()> {
    let s = v.as_str().ok_or(Error)?;
    set.contains(&s).then_some(()).ok_or(Error)
}

fn name_chars(s: &str, extra: &[u8]) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

fn check_scope(s: &str) -> Result<()> {
    let ok = s == "vault"
        || s.strip_prefix("item:").is_some_and(|h| is_hex(h, 32))
        || s.strip_prefix("tag:").is_some_and(|t| {
            t.chars().count() <= 64 && !t.is_empty() && !t.chars().any(char::is_control)
        });
    ok.then_some(()).ok_or(Error)
}

fn check_peer(v: &Value) -> Result<()> {
    let o = v.as_object().ok_or(Error)?;
    for (k, pv) in o {
        match k.as_str() {
            "uid" | "pid" => {
                pv.as_u64().ok_or(Error)?;
            }
            "exe" => {
                text(pv, 1024)?;
            }
            _ => return Err(Error),
        }
    }
    if o.contains_key("uid") && o.contains_key("pid") {
        Ok(())
    } else {
        Err(Error)
    }
}

/// Only these members, with these types. Free text (`label`, `reason`, `exe`) must already be free of
/// control characters; use [`clean_text`].
fn check_detail(d: &Map<String, Value>) -> Result<()> {
    for (k, v) in d {
        match k.as_str() {
            "peer" => check_peer(v)?,
            "label" => {
                text(v, 100)?;
            }
            "reason" => {
                text(v, 200)?;
            }
            "grant" => {
                is_hex(text(v, 16)?, 16).then_some(()).ok_or(Error)?;
            }
            "item" => {
                is_hex(text(v, 32)?, 32).then_some(()).ok_or(Error)?;
            }
            "mode" => one_of(v, &MODES)?,
            "ttl" => one_of(v, &TTLS)?,
            "code" => one_of(v, &CODES)?,
            "scope" => check_scope(text(v, 80)?)?,
            "field" => {
                name_chars(text(v, 64)?, b"_/.-")
                    .then_some(())
                    .ok_or(Error)?;
            }
            "host" => {
                name_chars(text(v, 253)?, b".-*_")
                    .then_some(())
                    .ok_or(Error)?;
            }
            _ => return Err(Error),
        }
    }
    Ok(())
}

/// Text from outside (a claimed label, a reason, a program path) made safe for a record: control
/// characters become spaces and the length is cut to `max_chars`.
pub fn clean_text(s: &str, max_chars: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max_chars)
        .collect()
}

/// The canonical plaintext of a record. Fails for an unknown event or detail member.
pub fn audit_payload(prev: &[u8; 32], t: u64, ev: &str, d: &Map<String, Value>) -> Result<Vec<u8>> {
    if !AUDIT_EVENTS.contains(&ev) {
        return Err(Error);
    }
    check_detail(d)?;
    let mut o = Map::new();
    o.insert("v".into(), Value::from(1));
    o.insert("prev".into(), Value::from(hexs(prev)));
    o.insert("t".into(), Value::from(t));
    o.insert("ev".into(), Value::from(ev));
    o.insert("d".into(), Value::Object(d.clone()));
    jcs(&Value::Object(o))
}

/// Strict inverse of [`audit_payload`]: the bytes must be exactly the canonical form of a valid record.
fn parse_payload(seq: u64, bytes: &[u8]) -> Result<AuditRecord> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| Error)?;
    let o = v.as_object().ok_or(Error)?;
    if o.len() != 5 || o.get("v") != Some(&Value::from(1)) {
        return Err(Error);
    }
    let prev_hex = o.get("prev").ok_or(Error)?;
    let prev: [u8; 32] = unhex(prev_hex)?;
    // `unhex` also accepts upper case; a record only ever holds lower case (it is canonical).
    if prev_hex.as_str() != Some(hexs(&prev).as_str()) {
        return Err(Error);
    }
    let t = o.get("t").and_then(Value::as_u64).ok_or(Error)?;
    let ev = o.get("ev").and_then(Value::as_str).ok_or(Error)?;
    let d = o.get("d").and_then(Value::as_object).ok_or(Error)?;
    if audit_payload(&prev, t, ev, d)? != bytes {
        return Err(Error);
    }
    Ok(AuditRecord {
        seq,
        prev,
        t,
        ev: ev.to_owned(),
        d: d.clone(),
    })
}

/// Verify a sequence of containers as one device's audit log, record by record: signature, then
/// decryption, then `seq` (1, 2, 3, ...) and the `prev` chain. It stops at the first record that fails.
///
/// `ak_for` returns the audit key of a record's `key_epoch` (the current key, or one derived from an earlier
/// epoch's Vault Key).
pub fn read_audit_chain<'k>(
    files: &[&[u8]],
    ak_for: impl Fn(u32) -> Option<&'k Key>,
    sign_pub: &[u8; 32],
    vault_id: &[u8; 16],
    device_id: &[u8; 16],
) -> AuditChain {
    let mut out = AuditChain {
        head: ZERO_PREV,
        ..AuditChain::default()
    };
    for (n, data) in files.iter().enumerate() {
        let seq = n as u64 + 1;
        let id = FileId {
            vault_id,
            device_id,
            seq,
        };
        let record = (|| {
            let epoch_bytes: [u8; 4] = data.get(4..FILE_HEADER_LEN)?.try_into().ok()?;
            let ak = ak_for(u32::from_be_bytes(epoch_bytes))?;
            let plain = read_file(data, ak, Some(sign_pub), &id, Kind::Audit).ok()?;
            let rec = parse_payload(seq, &plain).ok()?;
            (rec.prev == out.head).then_some(rec)
        })();
        match record {
            Some(rec) => {
                out.head = Sha256::digest(data).into();
                out.records.push(rec);
            }
            None => {
                out.first_bad = Some(seq);
                break;
            }
        }
    }
    out
}

/// `u32 length || container`, the on-disk form of one record.
pub fn audit_frame(container: &[u8]) -> Result<Vec<u8>> {
    if container.is_empty() || container.len() > MAX_RECORD_BYTES {
        return Err(Error);
    }
    let len = u32::try_from(container.len()).map_err(|_| Error)?;
    let mut out = Vec::with_capacity(4 + container.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(container);
    Ok(out)
}

/// Split a log into containers. The second value is how many bytes were consumed; if it is less than
/// `log.len()` the rest is an incomplete or damaged tail (a crash during a write, or tampering).
pub fn audit_split(log: &[u8]) -> (Vec<&[u8]>, usize) {
    let mut records = Vec::new();
    let mut at = 0;
    while let Some(len) = log.get(at..at + 4) {
        let n = u32::from_be_bytes([len[0], len[1], len[2], len[3]]) as usize;
        if n == 0 || n > MAX_RECORD_BYTES {
            break;
        }
        let Some(rec) = log.get(at + 4..at + 4 + n) else {
            break;
        };
        records.push(rec);
        at += 4 + n;
    }
    (records, at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::build_file;
    use serde_json::json;

    fn detail(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn rec(
        ak: &Key,
        sign: &Key,
        vault: &[u8; 16],
        device: &[u8; 16],
        seq: u64,
        prev: &[u8; 32],
        ev: &str,
    ) -> Vec<u8> {
        let payload = audit_payload(prev, 1_700_000_000_000 + seq, ev, &Map::new()).unwrap();
        let id = FileId {
            vault_id: vault,
            device_id: device,
            seq,
        };
        build_file(Kind::Audit, ak, sign, &id, 1, &payload).unwrap()
    }

    #[test]
    fn detail_members_are_an_allow_list_with_types() {
        let ok = detail(json!({
            "peer": {"uid": 501, "pid": 7, "exe": "/bin/agent"}, "label": "agent", "reason": "deploy",
            "grant": "0123456789abcdef", "item": "00112233445566778899aabbccddeeff", "mode": "http",
            "ttl": "15m", "scope": "tag:work", "field": "env/API_KEY", "host": "*.example.com",
            "code": "not_allowed"
        }));
        assert!(audit_payload(&ZERO_PREV, 1, "use.refused", &ok).is_ok());
        for bad in [
            json!({"secret": "x"}),
            json!({"value": "hunter2"}),
            json!({"mode": "root"}),
            json!({"ttl": "forever"}),
            json!({"code": "oops"}),
            json!({"grant": "NOTHEX0123456789"}),
            json!({"item": "short"}),
            json!({"scope": "everything"}),
            json!({"field": "a b"}),
            json!({"host": "evil.example/path"}),
            json!({"label": "tab\there"}),
            json!({"label": "x".repeat(101)}),
            json!({"reason": "y".repeat(201)}),
            json!({"peer": {"uid": 1}}),
            json!({"peer": {"uid": 1, "pid": 2, "extra": 3}}),
            json!({"peer": {"uid": "1", "pid": 2}}),
            json!({"grant": 5}),
        ] {
            assert!(
                audit_payload(&ZERO_PREV, 1, "use.allowed", &detail(bad.clone())).is_err(),
                "{bad}"
            );
        }
        assert!(audit_payload(&ZERO_PREV, 1, "made.up", &Map::new()).is_err());
    }

    #[test]
    fn clean_text_removes_controls_and_cuts() {
        assert_eq!(clean_text("a\u{1b}[31mb\nc", 100), "a [31mb c");
        assert_eq!(clean_text("abcdef", 3), "abc");
        let d = detail(json!({"label": clean_text("x\u{7}y", 10)}));
        assert!(audit_payload(&ZERO_PREV, 1, "grant.requested", &d).is_ok());
    }

    #[test]
    fn payload_is_canonical_and_strictly_parsed() {
        let d = detail(json!({"mode": "run", "ttl": "once"}));
        let p = audit_payload(&ZERO_PREV, 5, "grant.approved", &d).unwrap();
        assert_eq!(
            String::from_utf8(p.clone()).unwrap(),
            format!(
                r#"{{"d":{{"mode":"run","ttl":"once"}},"ev":"grant.approved","prev":"{}","t":5,"v":1}}"#,
                "0".repeat(64)
            )
        );
        assert_eq!(parse_payload(1, &p).unwrap().ev, "grant.approved");
        let mut spaced = p.clone();
        spaced.insert(1, b' ');
        assert!(parse_payload(1, &spaced).is_err(), "non canonical");
        let extra = br#"{"d":{},"ev":"broker.start","prev":"0000000000000000000000000000000000000000000000000000000000000000","t":1,"v":1,"x":1}"#;
        assert!(parse_payload(1, extra).is_err(), "unknown member");
    }

    #[test]
    fn chain_detects_every_kind_of_damage() {
        let ak = Key::random().unwrap();
        let sign = Key::random().unwrap();
        let sign_pub = ed25519_dalek::SigningKey::from_bytes(sign.expose())
            .verifying_key()
            .to_bytes();
        let (vault, device) = ([1u8; 16], [2u8; 16]);
        let r1 = rec(&ak, &sign, &vault, &device, 1, &ZERO_PREV, "broker.start");
        let h1: [u8; 32] = Sha256::digest(&r1).into();
        let r2 = rec(&ak, &sign, &vault, &device, 2, &h1, "grant.requested");
        let h2: [u8; 32] = Sha256::digest(&r2).into();
        let r3 = rec(&ak, &sign, &vault, &device, 3, &h2, "grant.denied");
        let read =
            |files: &[&[u8]]| read_audit_chain(files, |_| Some(&ak), &sign_pub, &vault, &device);

        let good = read(&[&r1, &r2, &r3]);
        assert_eq!(good.first_bad, None);
        assert_eq!(good.records.len(), 3);
        assert_eq!(good.head, <[u8; 32]>::from(Sha256::digest(&r3)));
        assert_eq!(read(&[]).head, ZERO_PREV);

        assert_eq!(read(&[&r1, &r3]).first_bad, Some(2), "dropped middle");
        assert_eq!(read(&[&r2, &r1]).first_bad, Some(1), "reordered");
        assert_eq!(read(&[&r1, &r1]).first_bad, Some(2), "duplicated");
        let mut flipped = r2.clone();
        flipped[20] ^= 1;
        let c = read(&[&r1, &flipped, &r3]);
        assert_eq!((c.first_bad, c.records.len()), (Some(2), 1), "flipped bit");
        assert!(
            read_audit_chain(&[&r1], |_| None, &sign_pub, &vault, &device).first_bad == Some(1)
        );
        let other = Key::random().unwrap();
        assert_eq!(
            read_audit_chain(&[&r1], |_| Some(&other), &sign_pub, &vault, &device).first_bad,
            Some(1),
            "wrong key"
        );
        // A validly signed record whose prev does not match.
        let bad_prev = rec(
            &ak,
            &sign,
            &vault,
            &device,
            2,
            &ZERO_PREV,
            "grant.requested",
        );
        assert_eq!(read(&[&r1, &bad_prev]).first_bad, Some(2));
        // Wrong signer.
        let other_sign = [9u8; 32];
        assert_eq!(
            read_audit_chain(&[&r1], |_| Some(&ak), &other_sign, &vault, &device).first_bad,
            Some(1)
        );
    }

    #[test]
    fn frames_round_trip_and_a_damaged_tail_is_reported() {
        let a = audit_frame(b"first").unwrap();
        let b = audit_frame(b"second record").unwrap();
        let mut log = [a.clone(), b.clone()].concat();
        let (recs, used) = audit_split(&log);
        assert_eq!(recs, vec![b"first".as_slice(), b"second record".as_slice()]);
        assert_eq!(used, log.len());
        log.truncate(log.len() - 3);
        let (recs, used) = audit_split(&log);
        assert_eq!(recs.len(), 1);
        assert_eq!(used, a.len());
        let (recs, used) = audit_split(&[0, 0, 0, 0, 1]);
        assert!(recs.is_empty());
        assert_eq!(used, 0, "zero length");
        let huge = (MAX_RECORD_BYTES as u32 + 1).to_be_bytes();
        assert_eq!(audit_split(&huge).1, 0, "oversized length");
        assert!(audit_frame(b"").is_err());
    }
}
