//! Sync tests (FIG-26): the HLC, merge and reader vectors from `docs/spec/vectors/`, the payload codec, and
//! property tests for convergence (`sync.md` 12).

use fc::{Hlc, Op, OpBatch, Snapshot, SnapshotField, SnapshotItem, Stamp, State, Write};
use figlock_core as fc;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn load(name: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/vectors")
        .join(name);
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    doc["cases"].as_array().unwrap().clone()
}

fn dev(hex_id: &str) -> [u8; 16] {
    hex::decode(hex_id).unwrap().try_into().unwrap()
}

fn st(v: &Value) -> Stamp {
    Stamp {
        wall: v[0].as_u64().unwrap(),
        counter: v[1].as_u64().unwrap() as u16,
    }
}

// ---------------------------------------------------------------------------------------------- vectors
#[test]
fn hlc_vectors() {
    for c in load("hlc.json") {
        let (i, name) = (&c["inputs"], c["name"].as_str().unwrap());
        if name == "drift-hold" {
            let now = i["now"].as_u64().unwrap();
            let held: Vec<bool> = i["op_wall_ms"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| fc::is_held(w.as_u64().unwrap(), now))
                .collect();
            assert_eq!(json!(held), c["expected"]["held"], "{name}");
            continue;
        }
        let mut h = Hlc::new(st(&i["start"]));
        let mut out = Vec::new();
        let mut failed = false;
        for step in i["steps"].as_array().unwrap() {
            let r = match step[0].as_str().unwrap() {
                "local" => h.local(step[1].as_u64().unwrap()),
                _ => h.receive(
                    Stamp {
                        wall: step[1].as_u64().unwrap(),
                        counter: step[2].as_u64().unwrap() as u16,
                    },
                    step[3].as_u64().unwrap(),
                ),
            };
            match r {
                Ok(s) => out.push(json!([s.wall, s.counter])),
                Err(_) => failed = true,
            }
        }
        if c["expected"] == "reject" {
            assert!(failed, "{name}: counter overflow must be an error");
        } else {
            assert!(!failed, "{name}");
            assert_eq!(Value::Array(out), c["expected"], "{name}");
        }
    }
}

type JState = State<String, String, Value>;

fn write_of(o: &Value) -> Write<String, String, Value> {
    Write {
        item: o["item"].as_str().unwrap().to_owned(),
        field: o["field"].as_str().unwrap().to_owned(),
        stamp: Stamp {
            wall: o["wall"].as_u64().unwrap(),
            counter: o["counter"].as_u64().unwrap() as u16,
        },
        device: dev(o["device"].as_str().unwrap()),
        value: o["value"].clone(),
    }
}

fn to_json(s: &JState) -> Value {
    let mut items = serde_json::Map::new();
    for (item, fields) in s.items() {
        let mut m = serde_json::Map::new();
        for (f, r) in fields {
            m.insert(
                f.clone(),
                json!([
                    r.stamp.wall,
                    r.stamp.counter,
                    hex::encode(r.device),
                    r.value
                ]),
            );
        }
        items.insert(item.clone(), Value::Object(m));
    }
    Value::Object(items)
}

#[test]
fn merge_vectors() {
    for c in load("merge.json") {
        let (i, name) = (&c["inputs"], c["name"].as_str().unwrap());
        let writes: Vec<_> = i["ops"].as_array().unwrap().iter().map(write_of).collect();
        let mut state = match i.get("purge_horizon") {
            Some(h) => {
                let known = i["known_items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned());
                JState::with_snapshot_scope(st(h), known)
            }
            None => JState::new(),
        };
        state.apply_all(writes.clone());
        assert_eq!(to_json(&state), c["expected"], "{name}");

        if i.get("purge_horizon").is_none() {
            // Every permutation and a version with duplicates must give the same state (up to 6 ops).
            let mut idx: Vec<usize> = (0..writes.len()).collect();
            permute(&mut idx, 0, &mut |p| {
                let mut s = JState::new();
                s.apply_all(p.iter().map(|&k| writes[k].clone()));
                assert_eq!(to_json(&s), c["expected"], "{name}: permutation");
            });
            let mut dup = JState::new();
            dup.apply_all(writes.iter().cloned().chain(writes.iter().cloned()));
            assert_eq!(to_json(&dup), c["expected"], "{name}: duplicates");
        }
        if let Some(n) = i.get("snapshot_from_first_ops") {
            let mut prefix = JState::new();
            prefix.apply_all(writes[..n.as_u64().unwrap() as usize].iter().cloned());
            let mut s = JState::new();
            s.apply_all(prefix.to_writes());
            s.apply_all(writes.iter().cloned());
            assert_eq!(to_json(&s), c["expected"], "{name}: snapshot + all ops");
        }
    }
}

fn permute(idx: &mut Vec<usize>, k: usize, f: &mut impl FnMut(&[usize])) {
    if k == idx.len() {
        f(idx);
        return;
    }
    for i in k..idx.len() {
        idx.swap(k, i);
        permute(idx, k + 1, f);
        idx.swap(k, i);
    }
}

#[test]
fn reader_vectors() {
    for c in load("sync-reader.json") {
        let (i, e, name) = (&c["inputs"], &c["expected"], c["name"].as_str().unwrap());
        let first = |m: &Value| -> (String, u64) {
            let (k, v) = m.as_object().unwrap().iter().next().unwrap();
            (k.clone(), v.as_u64().unwrap())
        };
        let seqs = |m: &Value| -> Vec<u64> {
            m.as_object()
                .unwrap()
                .values()
                .next()
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect()
        };
        match name {
            "gap-blocks-later-files" | "files-covered-by-snapshot-are-not-a-gap" => {
                let (d, applied) = first(&i["applied"]);
                let hw = i.get("snapshot_hw").map_or(0, |m| first(m).1);
                let plan = fc::plan_log(applied, hw, &seqs(&i["present_files"]), &[], None);
                assert_eq!(json!({d.clone(): plan.applied}), e["applied"], "{name}");
                if let Some(w) = e.get("waiting_for") {
                    assert_eq!(json!({d: plan.waiting_for.unwrap()}), *w, "{name}");
                } else {
                    assert_eq!(plan.waiting_for, None, "{name}");
                }
                assert!(!plan.stopped);
            }
            "fork-same-seq-two-files" | "prev-hash-mismatch" => {
                let (d, applied) = first(&i["applied"]);
                let plan = if name == "fork-same-seq-two-files" {
                    fc::plan_log(applied, 0, &[4], &[4], None)
                } else {
                    fc::plan_log(applied, 0, &[4], &[], Some(4))
                };
                assert!(
                    plan.stopped && plan.warning == Some(fc::Warning::Fork),
                    "{name}"
                );
                assert_eq!(json!({d.clone(): plan.applied}), e["applied"], "{name}");
                assert_eq!(e["device_stopped"], json!(d), "{name}");
            }
            "rollback-shorter-listing" => {
                let (_, applied) = first(&i["applied"]);
                let max = *seqs(&i["listing"]).iter().max().unwrap();
                assert_eq!(
                    fc::check_listing(applied, max),
                    Some(fc::Warning::Rollback),
                    "{name}"
                );
                assert_eq!(first(&e["applied"]).1, applied, "never lower local state");
            }
            "vault-json-lower-revision-ignored" => {
                let (rev, w) = fc::accept_vault_json(
                    i["accepted_revision"].as_u64().unwrap(),
                    i["seen_revision"].as_u64().unwrap(),
                );
                assert_eq!(rev, e["accepted_revision"].as_u64().unwrap(), "{name}");
                assert_eq!(w, Some(fc::Warning::Rollback), "{name}");
            }
            _ => {
                let ok = fc::accept_from_revoked(
                    st(&i["revoked_at"]),
                    st(&i["op_stamp"]),
                    i["file_epoch"].as_u64().unwrap() as u32,
                    i["rotation_epoch"].as_u64().unwrap() as u32,
                );
                assert_eq!(
                    if ok { "accept" } else { "ignore" },
                    e.as_str().unwrap(),
                    "{name}"
                );
            }
        }
    }
}

// ----------------------------------------------------------------------------------------------- payloads
fn sample_batch() -> OpBatch {
    let item = [7u8; 16];
    let hlc = Stamp {
        wall: 1_700_000_000_000,
        counter: 2,
    };
    OpBatch {
        device: [1; 16],
        seq: 5,
        prev: [9; 32],
        ops: vec![
            Op::ItemCreate {
                item,
                hlc,
                wrapped_ik: vec![1, 2, 3],
            },
            Op::FieldSet {
                item,
                field: "title".into(),
                hlc,
                blob: Some(vec![4, 5]),
            },
            Op::FieldSet {
                item,
                field: "url/abc".into(),
                hlc,
                blob: None,
            },
            Op::ItemDelete { item, hlc },
            Op::ItemRestore { item, hlc },
        ],
    }
}

#[test]
fn op_batch_roundtrip_is_padded_and_canonical() {
    let b = sample_batch();
    let enc = b.encode();
    assert_eq!(enc.len(), fc::padded_size(enc.len()), "padded to a bucket");
    assert_eq!(enc.len(), 1024);
    assert_eq!(OpBatch::decode(&enc).unwrap(), b);
    // Encoding is deterministic.
    assert_eq!(enc, b.encode());
}

#[test]
fn padding_buckets() {
    for (n, want) in [
        (1, 1024),
        (1024, 1024),
        (1025, 2048),
        (4097, 8192),
        (512 * 1024, 512 * 1024),
        (512 * 1024 + 1, 1024 * 1024),
        (1024 * 1024 + 1, 2 * 1024 * 1024),
    ] {
        assert_eq!(fc::padded_size(n), want, "{n}");
    }
}

#[test]
fn large_batch_pads_to_exact_bucket() {
    let mut b = sample_batch();
    b.ops.push(Op::FieldSet {
        item: [7; 16],
        field: "notes".into(),
        hlc: Stamp {
            wall: 1,
            counter: 0,
        },
        blob: Some(vec![0xAB; 5000]),
    });
    let enc = b.encode();
    assert_eq!(enc.len(), 8192);
    assert_eq!(OpBatch::decode(&enc).unwrap(), b);
}

#[test]
fn decode_rejects_noncanonical_and_tampered() {
    let enc = sample_batch().encode();
    // Non-zero padding: the `pad` member is a byte string of zeros; change one of its bytes.
    let mut bad = enc.clone();
    let key = bad
        .windows(4)
        .position(|w| w == [0x63, b'p', b'a', b'd'])
        .unwrap();
    assert_eq!(
        bad[key + 4],
        0x59,
        "2-byte length header expected for ~900 pad bytes"
    );
    bad[key + 7 + 10] = 1;
    assert!(OpBatch::decode(&bad).is_err(), "non-zero padding");
    // Truncated, and trailing garbage.
    assert!(OpBatch::decode(&enc[..enc.len() - 1]).is_err());
    let mut longer = enc.clone();
    longer.push(0);
    assert!(OpBatch::decode(&longer).is_err());
    // Wrong version.
    let mut v = enc.clone();
    let pos = v.windows(2).position(|w| w == [0x61, b'v']).unwrap();
    v[pos + 2] = 0x02;
    assert!(OpBatch::decode(&v).is_err(), "version 2");
    // A snapshot is not an op batch.
    let snap = Snapshot {
        device: [1; 16],
        seq: 1,
        hw: BTreeMap::new(),
        purge_horizon: Stamp::ZERO,
        items: vec![],
    };
    assert!(OpBatch::decode(&snap.encode()).is_err());
}

fn cbor(v: &ciborium::Value) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(v, &mut out).unwrap();
    out
}

fn text(s: &str) -> ciborium::Value {
    ciborium::Value::Text(s.into())
}

fn raw_map(mut entries: Vec<(ciborium::Value, ciborium::Value)>) -> ciborium::Value {
    entries.sort_by_key(|(k, _)| cbor(k));
    ciborium::Value::Map(entries)
}

/// A canonical op batch payload with a zero `pad` member sized to exactly 1024 bytes, built independently
/// of the codec so entries the codec would never write (extra or duplicate keys) can be injected.
fn raw_batch(extra: Vec<(ciborium::Value, ciborium::Value)>, total: usize) -> Vec<u8> {
    use ciborium::Value::{Array, Bytes, Integer};
    for pad in 0..=total {
        let mut e = vec![
            (text("v"), Integer(1.into())),
            (text("device"), Bytes(vec![1; 16])),
            (text("seq"), Integer(1.into())),
            (text("prev"), Bytes(vec![0; 32])),
            (text("ops"), Array(vec![])),
            (text("pad"), Bytes(vec![0; pad])),
        ];
        e.extend(extra.clone());
        let enc = cbor(&raw_map(e));
        if enc.len() == total {
            return enc;
        }
    }
    panic!("no pad fits");
}

#[test]
fn decode_requires_one_exact_encoding() {
    use ciborium::Value::Integer;
    let ok = raw_batch(vec![], 1024);
    assert!(
        OpBatch::decode(&ok).is_ok(),
        "the helper builds a valid payload"
    );
    let extra = raw_batch(vec![(text("extra"), Integer(1.into()))], 1024);
    assert!(OpBatch::decode(&extra).is_err(), "unknown top-level key");
    let dup = raw_batch(vec![(text("seq"), Integer(2.into()))], 1024);
    assert!(OpBatch::decode(&dup).is_err(), "duplicate top-level key");
    assert!(
        OpBatch::decode(&raw_batch(vec![], 1000)).is_err(),
        "not a padding bucket"
    );
    assert!(
        OpBatch::decode(&raw_batch(vec![], 100)).is_err(),
        "unpadded"
    );
}

#[test]
fn decode_rejects_duplicate_and_unknown_nested_keys() {
    use ciborium::Value::{Array, Bytes, Integer};
    let op = |extra: Vec<(ciborium::Value, ciborium::Value)>| {
        let mut e = vec![
            (text("op"), text("item.delete")),
            (text("item"), Bytes(vec![7; 16])),
            (text("hlc"), Bytes(vec![0; 8])),
        ];
        e.extend(extra);
        raw_map(e)
    };
    let batch = |op: ciborium::Value| {
        let mut ops = raw_batch(vec![], 1024);
        // rebuild with the op inside
        for pad in 0..=1024 {
            let e = vec![
                (text("v"), Integer(1.into())),
                (text("device"), Bytes(vec![1; 16])),
                (text("seq"), Integer(1.into())),
                (text("prev"), Bytes(vec![0; 32])),
                (text("ops"), Array(vec![op.clone()])),
                (text("pad"), Bytes(vec![0; pad])),
            ];
            let enc = cbor(&raw_map(e));
            if enc.len() == 1024 {
                ops = enc;
                break;
            }
        }
        ops
    };
    assert!(OpBatch::decode(&batch(op(vec![]))).is_ok());
    assert!(OpBatch::decode(&batch(op(vec![(text("x"), Integer(1.into()))]))).is_err());
    assert!(OpBatch::decode(&batch(op(vec![(text("item"), Bytes(vec![8; 16]))]))).is_err());
}

#[test]
fn snapshot_rejects_duplicate_hw_device() {
    use ciborium::Value::{Array, Bytes, Integer};
    for pad in 0..=1024 {
        let hw = ciborium::Value::Map(vec![
            (Bytes(vec![1; 16]), Integer(1.into())),
            (Bytes(vec![1; 16]), Integer(2.into())),
        ]);
        let e = vec![
            (text("v"), Integer(1.into())),
            (text("device"), Bytes(vec![1; 16])),
            (text("seq"), Integer(1.into())),
            (text("hw"), hw),
            (text("purge_horizon"), Bytes(vec![0; 8])),
            (text("items"), Array(vec![])),
            (text("pad"), Bytes(vec![0; pad])),
        ];
        let enc = cbor(&raw_map(e));
        if enc.len() == 1024 {
            assert!(Snapshot::decode(&enc).is_err());
            return;
        }
    }
    panic!("no pad fits");
}

#[test]
fn stamp_wall_is_bounded_to_48_bits() {
    let max = Stamp {
        wall: Stamp::MAX_WALL,
        counter: 0,
    };
    assert_eq!(Stamp::from_bytes(&max.to_bytes()), max);
    let mut h = Hlc::new(Stamp::ZERO);
    assert!(h.local(Stamp::MAX_WALL + 1).is_err());
    assert_eq!(
        h.last(),
        Stamp::ZERO,
        "a failed stamp leaves the clock untouched"
    );
    let far = Stamp {
        wall: Stamp::MAX_WALL + 1,
        counter: 0,
    };
    assert!(h.receive(far, 1000).is_err());
    assert_eq!(h.last(), Stamp::ZERO);
    assert!(h.local(Stamp::MAX_WALL).is_ok());
}

#[test]
fn plan_log_survives_max_high_water_mark() {
    let p = fc::plan_log(0, u64::MAX, &[], &[], None);
    assert!(p.stopped);
    assert_eq!(p.applied, u64::MAX);
    let p = fc::plan_log(u64::MAX, 0, &[1], &[], None);
    assert!(p.stopped);
}

#[test]
fn vector_payloads_decode() {
    for c in load("files.json") {
        if c["name"] == "op-file-build" {
            let p = hex::decode(c["inputs"]["payload_cbor"].as_str().unwrap()).unwrap();
            let b = OpBatch::decode(&p).unwrap();
            assert_eq!(b.seq, 1);
            assert_eq!(b.ops.len(), 1);
        }
        if c["name"] == "snapshot-file-build" {
            let p = hex::decode(c["inputs"]["payload_cbor"].as_str().unwrap()).unwrap();
            let s = Snapshot::decode(&p).unwrap();
            assert_eq!(s.hw.len(), 1);
            assert!(s.items.is_empty());
        }
    }
}

#[test]
fn snapshot_roundtrip() {
    let mut fields = BTreeMap::new();
    fields.insert(
        "title".to_string(),
        SnapshotField {
            hlc: Stamp {
                wall: 10,
                counter: 1,
            },
            device: [2; 16],
            blob: Some(vec![1]),
        },
    );
    fields.insert(
        "url/x".to_string(),
        SnapshotField {
            hlc: Stamp {
                wall: 11,
                counter: 0,
            },
            device: [3; 16],
            blob: None,
        },
    );
    let s = Snapshot {
        device: [1; 16],
        seq: 40,
        hw: BTreeMap::from([([1; 16], 40), ([2; 16], 7)]),
        purge_horizon: Stamp {
            wall: 5,
            counter: 0,
        },
        items: vec![SnapshotItem {
            item: [9; 16],
            wrapped_ik: vec![1, 2],
            deleted: Some((
                Stamp {
                    wall: 12,
                    counter: 0,
                },
                [2; 16],
                true,
            )),
            fields,
        }],
    };
    let enc = s.encode();
    assert_eq!(Snapshot::decode(&enc).unwrap(), s);
}

// ------------------------------------------------------------------------------------------------ properties
fn arb_write() -> impl Strategy<Value = Write<u8, u8, u8>> {
    (0u8..3, 0u8..3, 0u64..6, 0u16..3, 0u8..3, 0u8..4).prop_map(
        |(item, field, wall, counter, d, value)| Write {
            item,
            field,
            stamp: Stamp { wall, counter },
            device: [d; 16],
            value,
        },
    )
}

// A write with the same (stamp, device) but a different value would be two writes by one device with the
// same stamp, which the clock forbids; the value is derived from the key so equal keys carry equal values.
fn normalised(mut w: Write<u8, u8, u8>) -> Write<u8, u8, u8> {
    w.value = (w.stamp.wall as u8)
        .wrapping_mul(7)
        .wrapping_add(w.stamp.counter as u8)
        .wrapping_add(w.device[0]);
    w
}

fn state_of(ws: &[Write<u8, u8, u8>]) -> State<u8, u8, u8> {
    let mut s = State::new();
    s.apply_all(ws.iter().cloned());
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn convergence_any_order_any_duplication(ws in prop::collection::vec(arb_write().prop_map(normalised), 0..24), seed in any::<u64>()) {
        let base = state_of(&ws);
        let mut shuffled = ws.clone();
        shuffled.extend(ws.iter().take(ws.len() / 2).cloned()); // duplicates
        // deterministic Fisher-Yates from `seed`
        let mut x = seed | 1;
        for i in (1..shuffled.len()).rev() {
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            shuffled.swap(i, (x % (i as u64 + 1)) as usize);
        }
        prop_assert_eq!(base, state_of(&shuffled));
    }

    #[test]
    fn snapshot_plus_remaining_equals_replay(ws in prop::collection::vec(arb_write().prop_map(normalised), 0..24), cut in 0usize..24) {
        let cut = cut.min(ws.len());
        let full = state_of(&ws);
        let prefix = state_of(&ws[..cut]);
        let mut s = State::new();
        s.apply_all(prefix.to_writes());
        s.apply_all(ws[cut..].iter().cloned());
        prop_assert_eq!(&full, &s);
        // applying ops already folded into the snapshot changes nothing
        s.apply_all(ws[..cut].iter().cloned());
        prop_assert_eq!(&full, &s);
        // merging whole states is also order independent
        let (a, b) = (state_of(&ws[..cut]), state_of(&ws[cut..]));
        let (mut ab, mut ba) = (a.clone(), b.clone());
        ab.merge_state(&b);
        ba.merge_state(&a);
        prop_assert_eq!(&ab, &ba);
        prop_assert_eq!(&ab, &full);
    }

    #[test]
    fn own_stamps_strictly_increase(start in (0u64..1000, 0u16..5), steps in prop::collection::vec((any::<bool>(), 0u64..1500, 0u64..1500, 0u16..5), 0..60)) {
        let mut h = Hlc::new(Stamp { wall: start.0, counter: start.1 });
        let mut last = h.last();
        for (local, now, msg_wall, msg_counter) in steps {
            let r = if local { h.local(now) } else { h.receive(Stamp { wall: msg_wall, counter: msg_counter }, now) };
            if let Ok(s) = r {
                prop_assert!(s > last, "stamps never repeat or decrease: {:?} then {:?}", last, s);
                last = s;
            }
        }
    }

    #[test]
    fn decoders_never_panic_on_arbitrary_bytes(data in prop::collection::vec(any::<u8>(), 0..2048)) {
        let _ = OpBatch::decode(&data);
        let _ = Snapshot::decode(&data);
    }

    #[test]
    fn decoders_never_panic_on_mutated_valid_payloads(pos in 0usize..1024, bit in 0u8..8) {
        let mut enc = sample_batch().encode();
        enc[pos % 1024] ^= 1 << bit;
        let r = OpBatch::decode(&enc);
        // Any change to a canonical payload is either rejected or decodes to something else, and re-encodes
        // to exactly the mutated bytes only if it is itself canonical.
        if let Ok(b) = r { prop_assert_eq!(b.encode().len() % 1024, 0); }
    }
}
