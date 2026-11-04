//! Op-batch and snapshot payloads: deterministic CBOR (RFC 8949 4.2) with zero padding (`sync.md` 6.1, 7, 11).
//!
//! Decoding is strict: the bytes must be exactly the canonical encoding of what they decode to, so there is
//! one valid encoding of every payload (no malleability) and padding must be all zeros.

use std::collections::BTreeMap;

use ciborium::Value;

use crate::hlc::Stamp;
use crate::merge::DeviceId;
use crate::{Error, Result};

pub type ItemId = [u8; 16];

const KIB: usize = 1024;

/// One operation in an op file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    ItemCreate {
        item: ItemId,
        hlc: Stamp,
        wrapped_ik: Vec<u8>,
    },
    /// `blob` is `None` for a removal.
    FieldSet {
        item: ItemId,
        field: String,
        hlc: Stamp,
        blob: Option<Vec<u8>>,
    },
    ItemDelete {
        item: ItemId,
        hlc: Stamp,
    },
    ItemRestore {
        item: ItemId,
        hlc: Stamp,
    },
}

/// The plaintext of an op file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpBatch {
    pub device: DeviceId,
    pub seq: u64,
    /// SHA-256 of the previous op file of this device; zeros for `seq == 1`.
    pub prev: [u8; 32],
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotField {
    pub hlc: Stamp,
    pub device: DeviceId,
    pub blob: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotItem {
    pub item: ItemId,
    pub wrapped_ik: Vec<u8>,
    pub deleted: Option<(Stamp, DeviceId, bool)>,
    pub fields: BTreeMap<String, SnapshotField>,
}

/// The plaintext of a snapshot file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub device: DeviceId,
    pub seq: u64,
    pub hw: BTreeMap<DeviceId, u64>,
    pub purge_horizon: Stamp,
    pub items: Vec<SnapshotItem>,
}

// ---- value helpers ----
fn text(s: &str) -> Value {
    Value::Text(s.to_owned())
}
fn bytes(b: &[u8]) -> Value {
    Value::Bytes(b.to_vec())
}
fn uint(n: u64) -> Value {
    Value::Integer(n.into())
}
fn opt_bytes(b: &Option<Vec<u8>>) -> Value {
    b.as_ref().map_or(Value::Null, |b| bytes(b))
}
fn map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(entries.into_iter().map(|(k, v)| (text(k), v)).collect())
}

fn encode_value(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(v, &mut out).expect("writing to a Vec cannot fail");
    out
}

/// Sort every map by the bytewise order of its encoded keys.
fn canonical(v: Value) -> Value {
    match v {
        Value::Map(entries) => {
            let mut e: Vec<(Vec<u8>, Value, Value)> = entries
                .into_iter()
                .map(|(k, v)| {
                    (
                        encode_value(&canonical(k.clone())),
                        canonical(k),
                        canonical(v),
                    )
                })
                .collect();
            e.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Map(e.into_iter().map(|(_, k, v)| (k, v)).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(canonical).collect()),
        other => other,
    }
}

/// Padded size: the next of 1, 2, 4 ... 512 KiB, then the next multiple of 1 MiB.
pub fn padded_size(n: usize) -> usize {
    if n <= 512 * KIB {
        n.next_power_of_two().max(KIB)
    } else {
        n.div_ceil(KIB * KIB) * KIB * KIB
    }
}

/// Encode with a zero `pad` member so the total length is exactly a padding bucket.
fn encode_padded(mut entries: Vec<(&str, Value)>) -> Vec<u8> {
    entries.push(("pad", bytes(&[])));
    let base = encode_value(&canonical(map(entries.clone())));
    let target = padded_size(base.len());
    let mut size = target;
    loop {
        // The pad length changes the size of its own CBOR header, so search for an exact fit.
        for p in (0..=size.saturating_sub(base.len()) + 8).rev() {
            let mut e = entries.clone();
            let last = e.len() - 1;
            e[last] = ("pad", bytes(&vec![0u8; p]));
            let enc = encode_value(&canonical(map(e)));
            if enc.len() == size {
                return enc;
            }
            if enc.len() < size && p + 16 < size.saturating_sub(base.len()) {
                break;
            }
        }
        size = padded_size(size + 1);
    }
}

fn stamp_bytes(s: Stamp) -> Value {
    bytes(&s.to_bytes())
}

fn op_to_value(op: &Op) -> Value {
    match op {
        Op::ItemCreate {
            item,
            hlc,
            wrapped_ik,
        } => map(vec![
            ("op", text("item.create")),
            ("item", bytes(item)),
            ("hlc", stamp_bytes(*hlc)),
            ("wrapped_ik", bytes(wrapped_ik)),
        ]),
        Op::FieldSet {
            item,
            field,
            hlc,
            blob,
        } => map(vec![
            ("op", text("field.set")),
            ("item", bytes(item)),
            ("field", text(field)),
            ("hlc", stamp_bytes(*hlc)),
            ("blob", opt_bytes(blob)),
        ]),
        Op::ItemDelete { item, hlc } => map(vec![
            ("op", text("item.delete")),
            ("item", bytes(item)),
            ("hlc", stamp_bytes(*hlc)),
        ]),
        Op::ItemRestore { item, hlc } => map(vec![
            ("op", text("item.restore")),
            ("item", bytes(item)),
            ("hlc", stamp_bytes(*hlc)),
        ]),
    }
}

impl OpBatch {
    /// Canonical, padded CBOR of this batch.
    pub fn encode(&self) -> Vec<u8> {
        encode_padded(vec![
            ("v", uint(1)),
            ("device", bytes(&self.device)),
            ("seq", uint(self.seq)),
            ("prev", bytes(&self.prev)),
            (
                "ops",
                Value::Array(self.ops.iter().map(op_to_value).collect()),
            ),
        ])
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let m = decode_canonical(data)?;
        check_keys(&m, &["v", "device", "seq", "prev", "ops", "pad"])?;
        check_version_and_pad(&m)?;
        let ops = get(&m, "ops")?
            .clone()
            .into_array()
            .map_err(|_| Error)?
            .iter()
            .map(op_from_value)
            .collect::<Result<_>>()?;
        Ok(OpBatch {
            device: arr(get(&m, "device")?)?,
            seq: get_u64(&m, "seq")?,
            prev: arr(get(&m, "prev")?)?,
            ops,
        })
    }
}

/// Decode, requiring the exact canonical encoding and a total length that is a padding bucket.
fn decode_canonical(data: &[u8]) -> Result<Vec<(Value, Value)>> {
    if padded_size(data.len()) != data.len() {
        return Err(Error);
    }
    let v: Value = ciborium::from_reader(data).map_err(|_| Error)?;
    if encode_value(&canonical(v.clone())) != data {
        return Err(Error);
    }
    as_map(v)
}

/// Canonical maps are sorted, so equal keys are adjacent.
fn distinct_keys(m: &[(Value, Value)]) -> Result<()> {
    if m.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(Error);
    }
    Ok(())
}

/// Only the listed text keys, each at most once.
fn check_keys(m: &[(Value, Value)], allowed: &[&str]) -> Result<()> {
    distinct_keys(m)?;
    if m.iter()
        .any(|(k, _)| !k.as_text().is_some_and(|k| allowed.contains(&k)))
    {
        return Err(Error);
    }
    Ok(())
}

fn as_map(v: Value) -> Result<Vec<(Value, Value)>> {
    v.into_map().map_err(|_| Error)
}

fn get<'a>(m: &'a [(Value, Value)], key: &str) -> Result<&'a Value> {
    m.iter()
        .find(|(k, _)| k.as_text() == Some(key))
        .map(|(_, v)| v)
        .ok_or(Error)
}

fn get_u64(m: &[(Value, Value)], key: &str) -> Result<u64> {
    let i = get(m, key)?.as_integer().ok_or(Error)?;
    u64::try_from(i).map_err(|_| Error)
}

fn arr<const N: usize>(v: &Value) -> Result<[u8; N]> {
    v.as_bytes()
        .ok_or(Error)?
        .as_slice()
        .try_into()
        .map_err(|_| Error)
}

fn opt_blob(v: &Value) -> Result<Option<Vec<u8>>> {
    match v {
        Value::Null => Ok(None),
        Value::Bytes(b) => Ok(Some(b.clone())),
        _ => Err(Error),
    }
}

fn check_version_and_pad(m: &[(Value, Value)]) -> Result<()> {
    if get_u64(m, "v")? != 1 {
        return Err(Error);
    }
    let pad = get(m, "pad")?.as_bytes().ok_or(Error)?;
    if pad.iter().any(|&b| b != 0) {
        return Err(Error);
    }
    Ok(())
}

fn stamp(v: &Value) -> Result<Stamp> {
    Ok(Stamp::from_bytes(&arr::<8>(v)?))
}

fn op_from_value(v: &Value) -> Result<Op> {
    let m = v.as_map().ok_or(Error)?;
    let kind = get(m, "op")?.as_text().ok_or(Error)?;
    let extra: &[&str] = match kind {
        "item.create" => &["wrapped_ik"],
        "field.set" => &["field", "blob"],
        "item.delete" | "item.restore" => &[],
        _ => return Err(Error),
    };
    let mut allowed = vec!["op", "item", "hlc"];
    allowed.extend_from_slice(extra);
    check_keys(m, &allowed)?;
    let item = arr(get(m, "item")?)?;
    let hlc = stamp(get(m, "hlc")?)?;
    Ok(match kind {
        "item.create" => Op::ItemCreate {
            item,
            hlc,
            wrapped_ik: get(m, "wrapped_ik")?.as_bytes().ok_or(Error)?.clone(),
        },
        "field.set" => Op::FieldSet {
            item,
            field: get(m, "field")?.as_text().ok_or(Error)?.to_owned(),
            hlc,
            blob: opt_blob(get(m, "blob")?)?,
        },
        "item.delete" => Op::ItemDelete { item, hlc },
        "item.restore" => Op::ItemRestore { item, hlc },
        _ => return Err(Error),
    })
}

impl Snapshot {
    pub fn encode(&self) -> Vec<u8> {
        let hw = Value::Map(self.hw.iter().map(|(d, s)| (bytes(d), uint(*s))).collect());
        let items = self
            .items
            .iter()
            .map(|it| {
                let deleted = it.deleted.map_or(Value::Null, |(s, d, f)| {
                    Value::Array(vec![stamp_bytes(s), bytes(&d), Value::Bool(f)])
                });
                let fields = Value::Map(
                    it.fields
                        .iter()
                        .map(|(k, f)| {
                            (
                                text(k),
                                Value::Array(vec![
                                    stamp_bytes(f.hlc),
                                    bytes(&f.device),
                                    opt_bytes(&f.blob),
                                ]),
                            )
                        })
                        .collect(),
                );
                map(vec![
                    ("item", bytes(&it.item)),
                    ("wrapped_ik", bytes(&it.wrapped_ik)),
                    ("deleted", deleted),
                    ("fields", fields),
                ])
            })
            .collect();
        encode_padded(vec![
            ("v", uint(1)),
            ("device", bytes(&self.device)),
            ("seq", uint(self.seq)),
            ("hw", hw),
            ("purge_horizon", stamp_bytes(self.purge_horizon)),
            ("items", Value::Array(items)),
        ])
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        let m = decode_canonical(data)?;
        check_keys(
            &m,
            &["v", "device", "seq", "hw", "purge_horizon", "items", "pad"],
        )?;
        check_version_and_pad(&m)?;
        let mut hw = BTreeMap::new();
        let hw_entries = get(&m, "hw")?.as_map().ok_or(Error)?;
        distinct_keys(hw_entries)?;
        for (k, s) in hw_entries {
            hw.insert(
                arr::<16>(k)?,
                u64::try_from(s.as_integer().ok_or(Error)?).map_err(|_| Error)?,
            );
        }
        let mut items = Vec::new();
        for it in get(&m, "items")?.as_array().ok_or(Error)? {
            let im = it.as_map().ok_or(Error)?;
            check_keys(im, &["item", "wrapped_ik", "deleted", "fields"])?;
            let deleted = match get(im, "deleted")? {
                Value::Null => None,
                Value::Array(a) if a.len() == 3 => {
                    Some((stamp(&a[0])?, arr(&a[1])?, a[2].as_bool().ok_or(Error)?))
                }
                _ => return Err(Error),
            };
            let mut fields = BTreeMap::new();
            let field_entries = get(im, "fields")?.as_map().ok_or(Error)?;
            distinct_keys(field_entries)?;
            for (k, f) in field_entries {
                let a = f.as_array().filter(|a| a.len() == 3).ok_or(Error)?;
                fields.insert(
                    k.as_text().ok_or(Error)?.to_owned(),
                    SnapshotField {
                        hlc: stamp(&a[0])?,
                        device: arr(&a[1])?,
                        blob: opt_blob(&a[2])?,
                    },
                );
            }
            items.push(SnapshotItem {
                item: arr(get(im, "item")?)?,
                wrapped_ik: get(im, "wrapped_ik")?.as_bytes().ok_or(Error)?.clone(),
                deleted,
                fields,
            });
        }
        Ok(Snapshot {
            device: arr(get(&m, "device")?)?,
            seq: get_u64(&m, "seq")?,
            hw,
            purge_horizon: stamp(get(&m, "purge_horizon")?)?,
            items,
        })
    }
}
