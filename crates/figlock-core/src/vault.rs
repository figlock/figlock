//! The vault: create, unlock, lock, items and op files (`crypto.md` 5-7, `sync.md` 4-6).
//!
//! Pure logic over bytes: it produces `vault.json`, device records and op files, and consumes them, but never
//! touches storage. The CLI (FIG-29) and the sync engine move the bytes through a `BlobStore`.

use std::collections::BTreeMap;

use ciborium::Value as Cbor;
use ed25519_dalek::SigningKey;
use serde_json::{Map, Value as Json, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::file::{FileId, Kind, build_file, read_file};
use crate::hlc::{Hlc, Stamp, is_held};
use crate::keys::{VaultKeys, derive_auk, derive_vault_keys, unwrap_vk, wrap_vk};
use crate::merge::{DeviceId, State, Write};
use crate::payload::{ItemId, Op, OpBatch};
use crate::schema::{
    FIELD_DELETED, FIELD_IK, FIELD_TYPE, FIELD_VAULT, ItemType, MAX_VALUE_BYTES, validate_field,
};
use crate::signing::{sign_json, verify_json};
use crate::{
    Error, KdfParams, Key, Result, SecretKey, open_field, seal_field, unwrap_item_key,
    wrap_item_key,
};

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| Error)?;
    Ok(b)
}

/// A device's private keys. Held by the platform keystore in apps; never leaves the device.
pub struct DeviceKeys {
    pub device_id: DeviceId,
    pub sign_seed: Key,
    pub agree_priv: Key,
}

impl DeviceKeys {
    pub fn generate() -> Result<Self> {
        Ok(DeviceKeys {
            device_id: random()?,
            sign_seed: Key::random()?,
            agree_priv: Key::random()?,
        })
    }

    pub fn sign_public(&self) -> [u8; 32] {
        SigningKey::from_bytes(self.sign_seed.expose())
            .verifying_key()
            .to_bytes()
    }
}

/// What creating a vault produces: files to store and the Secret Key for the Emergency Kit.
pub struct Created {
    pub vault_json: Vec<u8>,
    pub device_record: Vec<u8>,
    pub secret_key: SecretKey,
}

/// A committed op file ready to be written with `put_new`.
#[derive(Clone, Debug)]
pub struct Committed {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// What a state register holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldValue {
    Blob(Vec<u8>),
    Removed,
    Flag(bool),
}

/// A decrypted item for display. Secret values zeroize on drop.
pub struct ItemView {
    pub id: ItemId,
    pub item_type: ItemType,
    pub vault: String,
    pub deleted: bool,
    pub fields: BTreeMap<String, Zeroizing<String>>,
    pub updated: Stamp,
}

pub struct Vault {
    vault_id: [u8; 16],
    epoch: u32,
    revision: u64,
    params: KdfParams,
    keys: VaultKeys,
    device: DeviceKeys,
    hlc: Hlc,
    state: State<ItemId, String, FieldValue>,
    item_keys: BTreeMap<ItemId, Key>,
    pending: Vec<Op>,
    own_seq: u64,
    own_prev: [u8; 32],
    applied: BTreeMap<DeviceId, (u64, [u8; 32])>,
    devices: BTreeMap<DeviceId, [u8; 32]>,
}

// ---- JSON files ----
fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex<const N: usize>(v: &Json) -> Result<[u8; N]> {
    let s = v.as_str().ok_or(Error)?;
    if s.len() != N * 2 || !s.is_ascii() {
        return Err(Error);
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| Error)?;
    }
    Ok(out)
}

fn unhex_vec(v: &Json) -> Result<Vec<u8>> {
    let s = v.as_str().ok_or(Error)?;
    if s.len() % 2 != 0 || !s.is_ascii() || s.len() > 4096 {
        return Err(Error);
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| Error))
        .collect()
}

fn with_signature(mut body: Json, sig: [u8; 64]) -> Result<Vec<u8>> {
    body.as_object_mut()
        .ok_or(Error)?
        .insert("signature".into(), Json::String(hexs(&sig)));
    serde_json::to_vec_pretty(&body).map_err(|_| Error)
}

/// Split a signed JSON file into its body (without the signature member) and the signature.
fn split_signature(bytes: &[u8]) -> Result<(Json, [u8; 64])> {
    let mut v: Json = serde_json::from_slice(bytes).map_err(|_| Error)?;
    let sig = unhex::<64>(
        &v.as_object_mut()
            .ok_or(Error)?
            .remove("signature")
            .ok_or(Error)?,
    )?;
    Ok((v, sig))
}

fn vault_json_body(v: &Vault, salt: &[u8; 16], wrapped_vk: &[u8]) -> Json {
    json!({
        "v": 1, "suite": "fl1", "vault_id": hexs(&v.vault_id), "revision": v.revision, "key_epoch": v.epoch,
        "min_format_version": 1,
        "kdf": { "m_kib": v.params.m_kib, "t": v.params.t, "p": v.params.p, "salt": hexs(salt) },
        "wrapped_vk": hexs(wrapped_vk), "authority_pub": hexs(&v.keys.authority_public()),
    })
}

fn device_record_body(
    d: &DeviceKeys,
    agree_pub: &[u8; 32],
    name: &str,
    now_ms: u64,
    epoch: u32,
) -> Json {
    json!({
        "device_id": hexs(&d.device_id), "name": name, "sign_pub": hexs(&d.sign_public()),
        "agree_pub": hexs(agree_pub), "added_at": now_ms, "epoch_added": epoch,
    })
}

// ---- CBOR field values (text) ----
fn enc_text(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(&Cbor::Text(s.to_owned()), &mut out).expect("vec write");
    out
}

fn dec_text(b: &[u8]) -> Result<Zeroizing<String>> {
    let v: Cbor = ciborium::from_reader(b).map_err(|_| Error)?;
    let s = v.into_text().map_err(|_| Error)?;
    if enc_text(&s) != b {
        return Err(Error);
    }
    Ok(Zeroizing::new(s))
}

impl Vault {
    /// Create a vault and its first device. Returns the vault (unlocked) and the files to store.
    pub fn create(
        password: &str,
        device_name: &str,
        params: KdfParams,
        now_ms: u64,
    ) -> Result<(Vault, Created)> {
        params.check()?;
        let vault_id = random::<16>()?;
        let salt = random::<16>()?;
        let secret_key = SecretKey::random()?;
        let vk = Key::random()?;
        let auk = derive_auk(password, &secret_key, &vault_id, &salt, &params)?;
        let wrapped_vk = wrap_vk(&auk, &vk, &vault_id, 1, &params.bytes_with_salt(&salt))?;
        let keys = derive_vault_keys(&vk)?;
        let device = DeviceKeys::generate()?;
        let mut vault = Vault::assemble(vault_id, 1, 1, params, keys, device, Stamp::ZERO);
        vault.hlc = Hlc::new(Stamp {
            wall: now_ms,
            counter: 0,
        });

        let body = vault_json_body(&vault, &salt, &wrapped_vk);
        let vault_json = with_signature(
            body.clone(),
            sign_json(&vault.keys.authority_seed, "vault-json", &body)?,
        )?;
        let device_record = vault.sign_own_record(device_name, now_ms)?;
        Ok((
            vault,
            Created {
                vault_json,
                device_record,
                secret_key,
            },
        ))
    }

    /// Unlock with the master password and Secret Key. `device` is this device's keys; if it has no record
    /// among `known_records`, a signed record is created for it (`pairing-recovery.md` 3.5) and returned.
    pub fn unlock(
        vault_json: &[u8],
        password: &str,
        secret_key: &SecretKey,
        device: DeviceKeys,
        device_name: &str,
        known_records: &[Vec<u8>],
        now_ms: u64,
    ) -> Result<(Vault, Option<Vec<u8>>)> {
        let (body, sig) = split_signature(vault_json)?;
        let o = body.as_object().ok_or(Error)?;
        if o.get("v") != Some(&json!(1))
            || o.get("suite") != Some(&json!("fl1"))
            || o.get("min_format_version") != Some(&json!(1))
        {
            return Err(Error);
        }
        let kdf = o.get("kdf").and_then(Json::as_object).ok_or(Error)?;
        let num = |m: &Map<String, Json>, k: &str| -> Result<u64> {
            m.get(k).and_then(Json::as_u64).ok_or(Error)
        };
        let params = KdfParams {
            m_kib: u32::try_from(num(kdf, "m_kib")?).map_err(|_| Error)?,
            t: u32::try_from(num(kdf, "t")?).map_err(|_| Error)?,
            p: u32::try_from(num(kdf, "p")?).map_err(|_| Error)?,
        };
        let salt = unhex::<16>(kdf.get("salt").ok_or(Error)?)?;
        let vault_id = unhex::<16>(o.get("vault_id").ok_or(Error)?)?;
        let epoch = u32::try_from(num(o, "key_epoch")?).map_err(|_| Error)?;
        let revision = num(o, "revision")?;
        let wrapped = unhex_vec(o.get("wrapped_vk").ok_or(Error)?)?;

        let auk = derive_auk(password, secret_key, &vault_id, &salt, &params)?;
        let vk = unwrap_vk(
            &auk,
            &wrapped,
            &vault_id,
            epoch,
            &params.bytes_with_salt(&salt),
        )?;
        let keys = derive_vault_keys(&vk)?;
        // The signature is checked against the authority key *derived* from VK, not one read from storage.
        verify_json(&keys.authority_public(), "vault-json", &body, &sig)?;
        if unhex::<32>(o.get("authority_pub").ok_or(Error)?)? != keys.authority_public() {
            return Err(Error);
        }

        let mut vault =
            Vault::assemble(vault_id, epoch, revision, params, keys, device, Stamp::ZERO);
        vault.hlc = Hlc::new(Stamp {
            wall: now_ms,
            counter: 0,
        });
        for r in known_records {
            vault.add_device_record(r)?;
        }
        let own = vault.device.device_id;
        let new_record = if vault.devices.contains_key(&own) {
            None
        } else {
            let rec = vault.sign_own_record(device_name, now_ms)?;
            Some(rec)
        };
        Ok((vault, new_record))
    }

    fn assemble(
        vault_id: [u8; 16],
        epoch: u32,
        revision: u64,
        params: KdfParams,
        keys: VaultKeys,
        device: DeviceKeys,
        _s: Stamp,
    ) -> Vault {
        Vault {
            vault_id,
            epoch,
            revision,
            params,
            keys,
            device,
            hlc: Hlc::new(Stamp::ZERO),
            state: State::new(),
            item_keys: BTreeMap::new(),
            pending: Vec::new(),
            own_seq: 0,
            own_prev: [0; 32],
            applied: BTreeMap::new(),
            devices: BTreeMap::new(),
        }
    }

    /// Lock: drop every key and plaintext. Consuming `self` makes using a locked vault a compile error.
    pub fn lock(self) {}

    fn sign_own_record(&mut self, name: &str, now_ms: u64) -> Result<Vec<u8>> {
        let agree_pub = crate::pairing::x25519_public(&self.device.agree_priv);
        let body = device_record_body(&self.device, &agree_pub, name, now_ms, self.epoch);
        let rec = with_signature(
            body.clone(),
            sign_json(&self.keys.authority_seed, "device-record", &body)?,
        )?;
        self.devices
            .insert(self.device.device_id, self.device.sign_public());
        Ok(rec)
    }

    /// Accept a device record signed by this vault's authority key.
    pub fn add_device_record(&mut self, bytes: &[u8]) -> Result<()> {
        let (body, sig) = split_signature(bytes)?;
        verify_json(&self.keys.authority_public(), "device-record", &body, &sig)?;
        let o = body.as_object().ok_or(Error)?;
        self.devices.insert(
            unhex(o.get("device_id").ok_or(Error)?)?,
            unhex(o.get("sign_pub").ok_or(Error)?)?,
        );
        Ok(())
    }

    pub fn vault_id(&self) -> &[u8; 16] {
        &self.vault_id
    }

    pub fn device_keys(&self) -> &DeviceKeys {
        &self.device
    }

    pub fn device_id(&self) -> &DeviceId {
        &self.device.device_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    // ---- writing ----
    fn write(&mut self, item: ItemId, field: &str, stamp: Stamp, value: FieldValue) {
        let device = self.device.device_id;
        self.state.apply(Write {
            item,
            field: field.to_owned(),
            stamp,
            device,
            value,
        });
    }

    fn item_key(&mut self, id: &ItemId) -> Result<&Key> {
        if !self.item_keys.contains_key(id) {
            let reg = self.state.get(id, &FIELD_IK.to_owned()).ok_or(Error)?;
            let FieldValue::Blob(w) = &reg.value else {
                return Err(Error);
            };
            let ik = unwrap_item_key(&self.keys.wk, w, &self.vault_id, id, self.epoch)?;
            self.item_keys.insert(*id, ik);
        }
        self.item_keys.get(id).ok_or(Error)
    }

    fn set_text(
        &mut self,
        id: ItemId,
        field: &str,
        value: Option<&str>,
        now_ms: u64,
    ) -> Result<()> {
        let stamp = self.hlc.local(now_ms)?;
        let blob = match value {
            Some(v) => {
                if v.len() > MAX_VALUE_BYTES {
                    return Err(Error);
                }
                let vid = self.vault_id;
                let ik = self.item_key(&id)?;
                Some(seal_field(
                    ik,
                    &enc_text(v),
                    &vid,
                    &id,
                    field,
                    &stamp.to_bytes(),
                )?)
            }
            None => None,
        };
        let fv = blob.clone().map_or(FieldValue::Removed, FieldValue::Blob);
        self.write(id, field, stamp, fv);
        self.pending.push(Op::FieldSet {
            item: id,
            field: field.to_owned(),
            hlc: stamp,
            blob,
        });
        Ok(())
    }

    /// Add an item. `fields` are validated against the item type's schema.
    pub fn add_item(
        &mut self,
        t: ItemType,
        vault_name: &str,
        fields: &[(&str, &str)],
        now_ms: u64,
    ) -> Result<ItemId> {
        for (f, _) in fields {
            validate_field(t, f)?;
        }
        let id = random::<16>()?;
        let ik = Key::random()?;
        let wrapped = wrap_item_key(&self.keys.wk, &ik, &self.vault_id, &id, self.epoch)?;
        let stamp = self.hlc.local(now_ms)?;
        self.write(id, FIELD_IK, stamp, FieldValue::Blob(wrapped.clone()));
        self.pending.push(Op::ItemCreate {
            item: id,
            hlc: stamp,
            wrapped_ik: wrapped,
        });
        self.item_keys.insert(id, ik);
        self.set_text(id, FIELD_TYPE, Some(t.as_str()), now_ms)?;
        self.set_text(id, FIELD_VAULT, Some(vault_name), now_ms)?;
        for (f, v) in fields {
            self.set_text(id, f, Some(v), now_ms)?;
        }
        Ok(id)
    }

    /// Set (or with `None`, remove) one field of an existing item.
    pub fn set_field(
        &mut self,
        id: &ItemId,
        field: &str,
        value: Option<&str>,
        now_ms: u64,
    ) -> Result<()> {
        let t = self.item_type(id)?;
        validate_field(t, field)?;
        self.set_text(*id, field, value, now_ms)
    }

    pub fn delete_item(&mut self, id: &ItemId, now_ms: u64) -> Result<()> {
        self.item_type(id)?;
        let stamp = self.hlc.local(now_ms)?;
        self.write(*id, FIELD_DELETED, stamp, FieldValue::Flag(true));
        self.pending.push(Op::ItemDelete {
            item: *id,
            hlc: stamp,
        });
        Ok(())
    }

    pub fn restore_item(&mut self, id: &ItemId, now_ms: u64) -> Result<()> {
        self.item_type(id)?;
        let stamp = self.hlc.local(now_ms)?;
        self.write(*id, FIELD_DELETED, stamp, FieldValue::Flag(false));
        self.pending.push(Op::ItemRestore {
            item: *id,
            hlc: stamp,
        });
        Ok(())
    }

    /// Turn pending ops into one signed, encrypted op file. `None` if there is nothing to write.
    pub fn commit(&mut self) -> Result<Option<Committed>> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        let seq = self.own_seq + 1;
        let batch = OpBatch {
            device: self.device.device_id,
            seq,
            prev: self.own_prev,
            ops: self.pending.clone(),
        };
        let id = FileId {
            vault_id: &self.vault_id,
            device_id: &self.device.device_id,
            seq,
        };
        let bytes = build_file(
            Kind::Op,
            &self.keys.lk,
            &self.device.sign_seed,
            &id,
            self.epoch,
            &batch.encode(),
        )?;
        self.own_prev = Sha256::digest(&bytes).into();
        self.own_seq = seq;
        self.pending.clear();
        Ok(Some(Committed {
            path: format!("log/{}/{seq:08}.op", hexs(&self.device.device_id)),
            bytes,
        }))
    }

    // ---- reading ----
    /// Verify, decrypt and apply an op file, this device's own included (to restore state and the chain
    /// head after an unlock). Files must arrive in order per device (`sync.md` 8); a file stamped too far
    /// in the future is refused so the caller can retry later. A file is applied whole or not at all.
    pub fn apply_file(
        &mut self,
        bytes: &[u8],
        device: &DeviceId,
        seq: u64,
        now_ms: u64,
    ) -> Result<()> {
        let own = *device == self.device.device_id;
        let sign_pub = self.devices.get(device).copied();
        let id = FileId {
            vault_id: &self.vault_id,
            device_id: device,
            seq,
        };
        let payload = read_file(bytes, &self.keys.lk, sign_pub.as_ref(), &id, Kind::Op)?;
        let batch = OpBatch::decode(&payload)?;
        let (last_seq, last_hash) = if own {
            (self.own_seq, self.own_prev)
        } else {
            self.applied.get(device).copied().unwrap_or((0, [0; 32]))
        };
        if batch.device != *device
            || batch.seq != seq
            || seq != last_seq + 1
            || batch.prev != last_hash
        {
            return Err(Error);
        }
        // Validate everything and fold the clock on a copy before touching state.
        let mut hlc = self.hlc;
        let mut writes = Vec::with_capacity(batch.ops.len());
        for op in batch.ops {
            let (item, stamp, field, value) = match op {
                Op::ItemCreate {
                    item,
                    hlc,
                    wrapped_ik,
                } => (item, hlc, FIELD_IK.to_owned(), FieldValue::Blob(wrapped_ik)),
                Op::FieldSet {
                    item,
                    field,
                    hlc,
                    blob,
                } => {
                    if blob
                        .as_ref()
                        .is_some_and(|b| b.len() > MAX_VALUE_BYTES + 64)
                        || !self.field_allowed(&item, &field)
                    {
                        return Err(Error);
                    }
                    (
                        item,
                        hlc,
                        field,
                        blob.map_or(FieldValue::Removed, FieldValue::Blob),
                    )
                }
                Op::ItemDelete { item, hlc } => {
                    (item, hlc, FIELD_DELETED.to_owned(), FieldValue::Flag(true))
                }
                Op::ItemRestore { item, hlc } => {
                    (item, hlc, FIELD_DELETED.to_owned(), FieldValue::Flag(false))
                }
            };
            if is_held(stamp.wall, now_ms) {
                return Err(Error);
            }
            hlc.receive(stamp, now_ms)?;
            writes.push(Write {
                item,
                field,
                stamp,
                device: *device,
                value,
            });
        }
        for w in writes {
            self.state.apply(w);
        }
        self.hlc = hlc;
        let head = (seq, Sha256::digest(bytes).into());
        if own {
            (self.own_seq, self.own_prev) = head;
        } else {
            self.applied.insert(*device, head);
        }
        Ok(())
    }

    /// Whether a remote `FieldSet` may write `field`: the vault's own `_type` and `_vault`, or a schema
    /// field of the item's type (any type while the item's type is not yet known).
    fn field_allowed(&mut self, item: &ItemId, field: &str) -> bool {
        if field == FIELD_TYPE || field == FIELD_VAULT {
            return true;
        }
        match self.item_type(item) {
            Ok(t) => validate_field(t, field).is_ok(),
            Err(_) => [
                ItemType::Login,
                ItemType::Token,
                ItemType::Note,
                ItemType::EnvBundle,
                ItemType::SshKey,
            ]
            .into_iter()
            .any(|t| validate_field(t, field).is_ok()),
        }
    }

    fn read_text(&mut self, id: &ItemId, field: &str) -> Result<Option<Zeroizing<String>>> {
        let Some(reg) = self.state.get(id, &field.to_owned()).cloned() else {
            return Ok(None);
        };
        match reg.value {
            FieldValue::Blob(b) => {
                let vid = self.vault_id;
                let ik = self.item_key(id)?;
                let plain = open_field(ik, &b, &vid, id, field, &reg.stamp.to_bytes())?;
                Ok(Some(dec_text(&plain)?))
            }
            _ => Ok(None),
        }
    }

    fn item_type(&mut self, id: &ItemId) -> Result<ItemType> {
        let t = self.read_text(id, FIELD_TYPE)?.ok_or(Error)?;
        ItemType::parse(&t)
    }

    /// Decrypt one item.
    pub fn item(&mut self, id: &ItemId) -> Result<ItemView> {
        let names: Vec<(String, Stamp)> = self
            .state
            .items()
            .get(id)
            .ok_or(Error)?
            .iter()
            .map(|(f, r)| (f.clone(), r.stamp))
            .collect();
        let deleted = matches!(
            self.state
                .get(id, &FIELD_DELETED.to_owned())
                .map(|r| &r.value),
            Some(FieldValue::Flag(true))
        );
        let item_type = self.item_type(id)?;
        let vault = self
            .read_text(id, FIELD_VAULT)?
            .map(|s| s.to_string())
            .unwrap_or_default();
        let mut fields = BTreeMap::new();
        let mut updated = Stamp::ZERO;
        for (name, stamp) in names {
            updated = updated.max(stamp);
            if name.starts_with('_') {
                continue;
            }
            if let Some(v) = self.read_text(id, &name)? {
                fields.insert(name, v);
            }
        }
        Ok(ItemView {
            id: *id,
            item_type,
            vault,
            deleted,
            fields,
            updated,
        })
    }

    /// All readable items, optionally including deleted ones, ordered by id, plus the ids of items that
    /// could not be read (corrupt or hostile writes) so one bad item never hides the rest.
    pub fn items(&mut self, include_deleted: bool) -> (Vec<ItemView>, Vec<ItemId>) {
        let ids: Vec<ItemId> = self.state.items().keys().copied().collect();
        let (mut out, mut corrupt) = (Vec::new(), Vec::new());
        for id in ids {
            match self.item(&id) {
                Ok(view) if include_deleted || !view.deleted => out.push(view),
                Ok(_) => {}
                Err(_) => corrupt.push(id),
            }
        }
        (out, corrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PW: &str = "correct horse battery staple";
    const T0: u64 = 1_700_000_000_000;

    fn pair() -> (Vault, Vault) {
        let (a, c) = Vault::create(PW, "A", KdfParams::FLOOR, T0).unwrap();
        let sk = SecretKey::parse(&c.secret_key.format()).unwrap();
        let (b, _) = Vault::unlock(
            &c.vault_json,
            PW,
            &sk,
            DeviceKeys::generate().unwrap(),
            "B",
            &[c.device_record],
            T0 + 1,
        )
        .unwrap();
        (a, b)
    }

    fn forge(a: &Vault, seq: u64, prev: [u8; 32], ops: Vec<Op>) -> Vec<u8> {
        let batch = OpBatch {
            device: a.device.device_id,
            seq,
            prev,
            ops,
        };
        let id = FileId {
            vault_id: &a.vault_id,
            device_id: &a.device.device_id,
            seq,
        };
        build_file(
            Kind::Op,
            &a.keys.lk,
            &a.device.sign_seed,
            &id,
            a.epoch,
            &batch.encode(),
        )
        .unwrap()
    }

    fn set(item: ItemId, field: &str, wall: u64, counter: u16) -> Op {
        Op::FieldSet {
            item,
            field: field.into(),
            hlc: Stamp { wall, counter },
            blob: Some(vec![7; 40]),
        }
    }

    #[test]
    fn counter_overflow_applies_nothing_and_a_later_retry_works() {
        let (a, mut b) = pair();
        let ops = vec![
            set([1; 16], "title", T0 + 1, 0),
            set([1; 16], "notes", T0 + 1, 0xFFFF),
        ];
        let file = forge(&a, 1, [0; 32], ops);
        let dev = a.device.device_id;
        assert!(b.apply_file(&file, &dev, 1, T0 + 1).is_err());
        assert!(b.state.items().is_empty(), "no partial writes");
        assert!(b.applied.is_empty());
        b.apply_file(&file, &dev, 1, T0 + 5).unwrap();
        assert_eq!(b.state.items()[&[1; 16]].len(), 2);
    }

    #[test]
    fn remote_reserved_and_schema_invalid_fields_are_refused() {
        let (a, mut b) = pair();
        let dev = a.device.device_id;
        for field in ["_ik", "_deleted", "_other", "bogus", "url/ABC"] {
            let file = forge(&a, 1, [0; 32], vec![set([1; 16], field, T0 + 2, 0)]);
            assert!(b.apply_file(&file, &dev, 1, T0 + 5).is_err(), "{field}");
        }
        assert!(b.state.items().is_empty());
    }

    #[test]
    fn a_hostile_type_overwrite_corrupts_only_that_item() {
        let (mut a, mut b) = pair();
        let good = a
            .add_item(ItemType::Note, "p", &[("title", "good")], T0 + 2)
            .unwrap();
        let bad = a
            .add_item(ItemType::Note, "p", &[("title", "bad")], T0 + 3)
            .unwrap();
        let f1 = a.commit().unwrap().unwrap();
        let dev = a.device.device_id;
        b.apply_file(&f1.bytes, &dev, 1, T0 + 10).unwrap();
        let hostile = forge(&a, 2, a.own_prev, vec![set(bad, FIELD_TYPE, T0 + 50, 0)]);
        b.apply_file(&hostile, &dev, 2, T0 + 60).unwrap();
        let (items, corrupt) = b.items(false);
        assert_eq!(corrupt, vec![bad]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, good);
    }
}
