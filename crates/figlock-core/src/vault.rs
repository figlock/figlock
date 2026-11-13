//! The vault: create, unlock, lock, items and op files (`crypto.md` 5-7, `sync.md` 4-6).
//!
//! Pure logic over bytes: it produces `vault.json`, device records and op files, and consumes them, but never
//! touches storage. The CLI (FIG-29) and the sync engine move the bytes through a `BlobStore`.

use std::collections::{BTreeMap, BTreeSet};

use ciborium::Value as Cbor;
use ed25519_dalek::SigningKey;
use serde_json::{Map, Value as Json, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::file::{FileId, Kind, build_file, read_file};
use crate::hlc::{Hlc, Stamp, is_held};
use crate::invite::{Invite, JoinRequest, device_fingerprint};
use crate::keys::{
    VaultKeys, derive_auk, derive_vault_keys, epoch_key, open_epoch_key, unwrap_vk, wrap_vk,
};
use crate::merge::{DeviceId, Register, State, Write};
use crate::pairing::{open_seal_to, seal_to};
use crate::payload::{ItemId, Op, OpBatch, Snapshot, SnapshotField, SnapshotItem};
use crate::reader::accept_from_revoked;
use crate::recovery::RecoveryKey;
use crate::schema::{
    FIELD_DELETED, FIELD_IK, FIELD_TYPE, FIELD_VAULT, ItemType, MAX_FIELDS_PER_ITEM,
    MAX_FILES_PER_ITEM, max_value_bytes, validate_field,
};
use crate::signing::{sign_json, verify_json};
use crate::wire::{aad, u32be};
use crate::{
    Error, KdfParams, Key, Result, SecretKey, open_field, seal_field, unwrap_item_key,
    wrap_item_key,
};

/// What sealing adds to a CBOR text value: the CBOR header (up to 9 bytes), the nonce and the tag, with room to spare.
const BLOB_OVERHEAD: usize = 64;

const ALL_TYPES: [ItemType; 6] = [
    ItemType::Login,
    ItemType::Token,
    ItemType::Note,
    ItemType::EnvBundle,
    ItemType::SshKey,
    ItemType::Document,
];

/// Whether an item with these field names stays within the per-item limits. A device checks its own writes;
/// the limits are not enforced on ops from other devices because a count across concurrent edits has no
/// single right answer (`docs/adr/0024-document-items.md`).
fn check_counts<'a>(fields: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut total = 0;
    let mut files = BTreeSet::new();
    for f in fields {
        total += 1;
        if let Some(id) = f.strip_prefix("file/").and_then(|r| r.split('/').next()) {
            files.insert(id.to_owned());
        }
    }
    if total > MAX_FIELDS_PER_ITEM || files.len() > MAX_FILES_PER_ITEM {
        return Err(Error);
    }
    Ok(())
}

fn record_path(id: &DeviceId) -> String {
    format!("devices/{}.json", hexs(id))
}

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

/// What a Vault knows about a device from its record.
#[derive(Clone)]
struct DeviceMeta {
    name: String,
    agree_pub: [u8; 32],
    added_at: u64,
    epoch_added: u32,
}

/// An entry of the signed `revoked` list (`crypto.md` 9.2).
#[derive(Clone)]
struct Revoked {
    sign_pub: [u8; 32],
    revoked_at: Stamp,
    epoch: u32,
}

/// An entry of `epoch_keys`: the key of epoch `from`, sealed under the key of epoch `to`.
#[derive(Clone)]
struct EpochKeyEntry {
    from: u32,
    to: u32,
    sealed: Vec<u8>,
}

/// The Recovery Key as `vault.json` carries it (`crypto.md` 9.3): the public key and the Vault Key sealed to it.
#[derive(Clone)]
struct RecoveryWrap {
    rk_pub: [u8; 32],
    wrapped: Vec<u8>,
}

/// `seal_to(rk_pub, VK, aad("vk-wrap-rk", vault_id, u32(epoch)))`.
fn wrap_vk_for_recovery(
    rk_pub: &[u8; 32],
    vk: &Key,
    vault_id: &[u8; 16],
    epoch: u32,
) -> Result<Vec<u8>> {
    seal_to(
        rk_pub,
        vk.expose(),
        &aad("vk-wrap-rk", &[vault_id, &u32be(epoch)]),
    )
}

/// One row of the device list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    /// Unknown for a revoked device: its record is not re-signed.
    pub name: Option<String>,
    pub fingerprint: String,
    pub revoked_at: Option<Stamp>,
}

/// Files to store after approving a join request. Write the record first, then the response, so the new
/// device finds its record as soon as it can open the answer.
pub struct Approval {
    pub record: Committed,
    pub response: Committed,
}

/// Files to store after revoking a device (`pairing-recovery.md` 4): write the snapshot (`put_new`),
/// replace the device records, delete the revoked device's record, and replace `vault.json`. Do `vault.json`
/// last so a device that sees the new epoch finds what it needs.
pub struct Revocation {
    pub vault_json: Vec<u8>,
    pub records: Vec<Committed>,
    /// Paths to delete: the revoked device's record, which no longer verifies.
    pub remove: Vec<String>,
    pub snapshot: Committed,
    /// The new Secret Key when the credentials changed with the revocation and it was rotated: give the
    /// person a new Emergency Kit (`pairing-recovery.md` 5).
    pub secret_key: Option<SecretKey>,
}

/// What [`Vault::change_credentials`] changes. Anything left out stays as it is.
#[derive(Clone, Copy, Default)]
pub struct CredentialChange<'a> {
    pub new_password: Option<&'a str>,
    pub rotate_secret_key: bool,
    pub params: Option<KdfParams>,
}

/// The result of a credential change: replace `vault.json` with `vault_json`, and give the person the new
/// Emergency Kit when `secret_key` is set (`pairing-recovery.md` 5).
pub struct Rotated {
    pub vault_json: Vec<u8>,
    pub secret_key: Option<SecretKey>,
}

/// A new `vault.json` after the Recovery Key was enabled, replaced or removed, and the new key to write down.
pub struct RecoveryChange {
    pub vault_json: Vec<u8>,
    /// Set when a Recovery Key was made: shown once, never stored.
    pub key: Option<RecoveryKey>,
}

/// What recovering with the Recovery Key produces (`pairing-recovery.md` 7.3): replace `vault.json`, and give the
/// person a new Emergency Kit for `secret_key`.
pub struct Recovered {
    pub vault_json: Vec<u8>,
    pub secret_key: SecretKey,
    /// A new Recovery Key when it was rotated in the same write.
    pub recovery_key: Option<RecoveryKey>,
    /// This device's own record, if it had none (a fresh install).
    pub new_record: Option<Vec<u8>>,
}

/// What [`Vault::recover`] is given besides this device: the stored `vault.json`, the Recovery Key and the new
/// master password. With `rotate_recovery` a new Recovery Key is made in the same write.
pub struct Recover<'a> {
    pub vault_json: &'a [u8],
    pub recovery_key: &'a RecoveryKey,
    pub new_password: &'a str,
    pub rotate_recovery: bool,
}

/// A local write that has not been committed: the register it replaced, and the stamp it wrote.
struct Undo {
    item: ItemId,
    field: String,
    written: Stamp,
    previous: Option<Register<FieldValue>>,
}

/// What it takes to put the vault back as it was before a commit.
struct Rollback {
    seq: u64,
    prev: [u8; 32],
    ops: Vec<Op>,
    undo: Vec<Undo>,
    new_keys: Vec<ItemId>,
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
    /// What each uncommitted local write replaced, so [`Vault::discard_pending`] can take it back.
    undo: Vec<Undo>,
    /// Item keys created by uncommitted `add_item` calls.
    new_keys: Vec<ItemId>,
    /// The most recent commit, kept until the next one so a failed upload can be taken back.
    last_commit: Option<Rollback>,
    own_seq: u64,
    own_prev: [u8; 32],
    applied: BTreeMap<DeviceId, (u64, [u8; 32])>,
    devices: BTreeMap<DeviceId, [u8; 32]>,
    device_meta: BTreeMap<DeviceId, DeviceMeta>,
    revoked: BTreeMap<DeviceId, Revoked>,
    epoch_keys: Vec<EpochKeyEntry>,
    /// Keys of earlier epochs, opened from `epoch_keys` at unlock.
    older: BTreeMap<u32, VaultKeys>,
    salt: [u8; 16],
    wrapped_vk: Vec<u8>,
    recovery: Option<RecoveryWrap>,
}

// ---- JSON files ----
pub(crate) fn hexs(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub(crate) fn unhex<const N: usize>(v: &Json) -> Result<[u8; N]> {
    let s = v.as_str().ok_or(Error)?;
    if s.len() != N * 2 || !s.is_ascii() {
        return Err(Error);
    }
    (0..N)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| Error))
        .collect::<Result<Vec<u8>>>()?
        .try_into()
        .map_err(|_| Error)
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
/// The fields of `vault.json` that unlocking reads before any key exists.
struct Header {
    params: KdfParams,
    salt: [u8; 16],
    vault_id: [u8; 16],
    epoch: u32,
    revision: u64,
    wrapped: Vec<u8>,
    recovery: Option<RecoveryWrap>,
}

impl Header {
    fn parse(o: &Map<String, Json>) -> Result<Header> {
        // Version 2 is a vault with a Recovery Key (`crypto.md` 9.3): the members and the version go together,
        // so a client that predates them refuses the file instead of dropping the key when it rewrites it.
        let recovery = match (o.get("rk_pub"), o.get("wrapped_vk_rk")) {
            (None, None) => None,
            (Some(p), Some(w)) => Some(RecoveryWrap {
                rk_pub: unhex::<32>(p)?,
                wrapped: unhex_vec(w)?,
            }),
            _ => return Err(Error),
        };
        let min_version = if recovery.is_some() { 2 } else { 1 };
        if o.get("v") != Some(&json!(1))
            || o.get("suite") != Some(&json!("fl1"))
            || o.get("min_format_version") != Some(&json!(min_version))
        {
            return Err(Error);
        }
        let kdf = o.get("kdf").and_then(Json::as_object).ok_or(Error)?;
        let num = |m: &Map<String, Json>, k: &str| -> Result<u64> {
            m.get(k).and_then(Json::as_u64).ok_or(Error)
        };
        Ok(Header {
            params: KdfParams {
                m_kib: u32::try_from(num(kdf, "m_kib")?).map_err(|_| Error)?,
                t: u32::try_from(num(kdf, "t")?).map_err(|_| Error)?,
                p: u32::try_from(num(kdf, "p")?).map_err(|_| Error)?,
            },
            salt: unhex::<16>(kdf.get("salt").ok_or(Error)?)?,
            vault_id: unhex::<16>(o.get("vault_id").ok_or(Error)?)?,
            epoch: u32::try_from(num(o, "key_epoch")?).map_err(|_| Error)?,
            revision: num(o, "revision")?,
            wrapped: unhex_vec(o.get("wrapped_vk").ok_or(Error)?)?,
            recovery,
        })
    }
}

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
    vault_json_body_with(
        v,
        salt,
        wrapped_vk,
        (v.epoch, v.revision, &v.keys.authority_public()),
        (&v.revoked, &v.epoch_keys),
        v.recovery.as_ref(),
    )
}

/// The signed body for explicit rotation state, so a rotation can build it before changing the vault.
fn vault_json_body_with(
    v: &Vault,
    salt: &[u8; 16],
    wrapped_vk: &[u8],
    (epoch, revision, authority_pub): (u32, u64, &[u8; 32]),
    (revoked, epoch_keys): (&BTreeMap<DeviceId, Revoked>, &[EpochKeyEntry]),
    recovery: Option<&RecoveryWrap>,
) -> Json {
    let mut body = json!({
        "v": 1, "suite": "fl1", "vault_id": hexs(&v.vault_id), "revision": revision, "key_epoch": epoch,
        "min_format_version": 1,
        "kdf": { "m_kib": v.params.m_kib, "t": v.params.t, "p": v.params.p, "salt": hexs(salt) },
        "wrapped_vk": hexs(wrapped_vk), "authority_pub": hexs(authority_pub),
    });
    // Both members are omitted while empty, so a vault that was never rotated keeps its epoch 1 shape.
    if let Some(o) = body.as_object_mut() {
        if let Some(r) = recovery {
            o.insert("min_format_version".into(), json!(2));
            o.insert("rk_pub".into(), json!(hexs(&r.rk_pub)));
            o.insert("wrapped_vk_rk".into(), json!(hexs(&r.wrapped)));
        }
        if !revoked.is_empty() {
            let list: Vec<Json> = revoked
                .iter()
                .map(|(id, r)| {
                    json!({
                        "device_id": hexs(id), "sign_pub": hexs(&r.sign_pub),
                        "revoked_at": [r.revoked_at.wall, r.revoked_at.counter], "epoch": r.epoch,
                    })
                })
                .collect();
            o.insert("revoked".into(), Json::Array(list));
        }
        if !epoch_keys.is_empty() {
            let list: Vec<Json> = epoch_keys
                .iter()
                .map(|e| json!({ "from": e.from, "to": e.to, "sealed": hexs(&e.sealed) }))
                .collect();
            o.insert("epoch_keys".into(), Json::Array(list));
        }
    }
    body
}

fn parse_revoked(o: &Map<String, Json>, epoch: u32) -> Result<BTreeMap<DeviceId, Revoked>> {
    let mut out = BTreeMap::new();
    let Some(list) = o.get("revoked") else {
        return Ok(out);
    };
    for e in list.as_array().ok_or(Error)? {
        let e = e.as_object().ok_or(Error)?;
        let at = e.get("revoked_at").and_then(Json::as_array).ok_or(Error)?;
        let (wall, counter) = match at.as_slice() {
            [w, c] => (w.as_u64().ok_or(Error)?, c.as_u64().ok_or(Error)?),
            _ => return Err(Error),
        };
        let rev_epoch = u32::try_from(e.get("epoch").and_then(Json::as_u64).ok_or(Error)?)
            .map_err(|_| Error)?;
        if e.len() != 4 || wall > Stamp::MAX_WALL || !(2..=epoch).contains(&rev_epoch) {
            return Err(Error);
        }
        let id: DeviceId = unhex(e.get("device_id").ok_or(Error)?)?;
        let entry = Revoked {
            sign_pub: unhex(e.get("sign_pub").ok_or(Error)?)?,
            revoked_at: Stamp {
                wall,
                counter: u16::try_from(counter).map_err(|_| Error)?,
            },
            epoch: rev_epoch,
        };
        if out.insert(id, entry).is_some() {
            return Err(Error);
        }
    }
    Ok(out)
}

/// The chain must hold one entry per rotation, `1 -> 2`, `2 -> 3`, ... up to the current epoch.
fn parse_epoch_keys(o: &Map<String, Json>, epoch: u32) -> Result<Vec<EpochKeyEntry>> {
    let mut out = Vec::new();
    if let Some(list) = o.get("epoch_keys") {
        for (i, e) in list.as_array().ok_or(Error)?.iter().enumerate() {
            let e = e.as_object().ok_or(Error)?;
            let num = |k: &str| -> Result<u32> {
                u32::try_from(e.get(k).and_then(Json::as_u64).ok_or(Error)?).map_err(|_| Error)
            };
            let (from, to) = (num("from")?, num("to")?);
            if e.len() != 3 || usize::try_from(from).ok() != Some(i + 1) || to != from + 1 {
                return Err(Error);
            }
            out.push(EpochKeyEntry {
                from,
                to,
                sealed: unhex_vec(e.get("sealed").ok_or(Error)?)?,
            });
        }
    }
    if out.len() as u64 + 1 != u64::from(epoch) {
        return Err(Error);
    }
    Ok(out)
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

        vault.salt = salt;
        vault.wrapped_vk = wrapped_vk.clone();
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
        let auk = Self::unlock_key(vault_json, password, secret_key)?;
        Self::unlock_with_key(vault_json, &auk, device, device_name, known_records, now_ms)
    }

    /// The Account Unlock Key for this `vault.json`, from the master password and Secret Key. A device keeps
    /// it only for quick unlock (`crypto.md` 8), sealed by a platform key.
    pub fn unlock_key(vault_json: &[u8], password: &str, secret_key: &SecretKey) -> Result<Key> {
        let (body, _) = split_signature(vault_json)?;
        let h = Header::parse(body.as_object().ok_or(Error)?)?;
        derive_auk(password, secret_key, &h.vault_id, &h.salt, &h.params)
    }

    /// Unlock with an Account Unlock Key from [`Vault::unlock_key`] or a quick-unlock store. A wrong key
    /// fails the same way a wrong password does.
    pub fn unlock_with_key(
        vault_json: &[u8],
        auk: &Key,
        device: DeviceKeys,
        device_name: &str,
        known_records: &[Vec<u8>],
        now_ms: u64,
    ) -> Result<(Vault, Option<Vec<u8>>)> {
        let (body, _) = split_signature(vault_json)?;
        let h = Header::parse(body.as_object().ok_or(Error)?)?;
        let vk = unwrap_vk(
            auk,
            &h.wrapped,
            &h.vault_id,
            h.epoch,
            &h.params.bytes_with_salt(&h.salt),
        )?;
        Self::open_with_vk(vault_json, &vk, device, device_name, known_records, now_ms)
    }

    /// Everything after the Vault Key is known: derive the keys, check the signature and the authority key,
    /// open the epoch chain and take in the device records.
    fn open_with_vk(
        vault_json: &[u8],
        vk: &Key,
        device: DeviceKeys,
        device_name: &str,
        known_records: &[Vec<u8>],
        now_ms: u64,
    ) -> Result<(Vault, Option<Vec<u8>>)> {
        let (body, sig) = split_signature(vault_json)?;
        let o = body.as_object().ok_or(Error)?;
        let Header {
            params,
            salt,
            vault_id,
            epoch,
            revision,
            wrapped,
            recovery,
        } = Header::parse(o)?;
        let keys = derive_vault_keys(vk)?;
        // The signature is checked against the authority key *derived* from VK, not one read from storage.
        verify_json(&keys.authority_public(), "vault-json", &body, &sig)?;
        if unhex::<32>(o.get("authority_pub").ok_or(Error)?)? != keys.authority_public() {
            return Err(Error);
        }

        let revoked = parse_revoked(o, epoch)?;
        let epoch_keys = parse_epoch_keys(o, epoch)?;
        // Open the chain newest first: each entry holds the previous epoch's VK under the next one.
        let mut older = BTreeMap::new();
        let mut newer = Key::from_bytes(*vk.expose());
        for e in epoch_keys.iter().rev() {
            let old = open_epoch_key(&newer, &e.sealed, &vault_id, e.from, e.to)?;
            older.insert(e.from, derive_vault_keys(&old)?);
            newer = old;
        }

        let mut vault =
            Vault::assemble(vault_id, epoch, revision, params, keys, device, Stamp::ZERO);
        vault.hlc = Hlc::new(Stamp {
            wall: now_ms,
            counter: 0,
        });
        vault.revoked = revoked;
        vault.epoch_keys = epoch_keys;
        vault.older = older;
        vault.salt = salt;
        vault.wrapped_vk = wrapped;
        vault.recovery = recovery;
        for r in known_records {
            let _ = vault.add_device_record(r);
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
            undo: Vec::new(),
            new_keys: Vec::new(),
            last_commit: None,
            own_seq: 0,
            own_prev: [0; 32],
            applied: BTreeMap::new(),
            devices: BTreeMap::new(),
            device_meta: BTreeMap::new(),
            revoked: BTreeMap::new(),
            epoch_keys: Vec::new(),
            older: BTreeMap::new(),
            salt: [0; 16],
            wrapped_vk: Vec::new(),
            recovery: None,
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
        self.device_meta.insert(
            self.device.device_id,
            DeviceMeta {
                name: name.to_owned(),
                agree_pub,
                added_at: now_ms,
                epoch_added: self.epoch,
            },
        );
        Ok(rec)
    }

    /// Accept a device record signed by this vault's authority key.
    pub fn add_device_record(&mut self, bytes: &[u8]) -> Result<()> {
        let (body, sig) = split_signature(bytes)?;
        verify_json(&self.keys.authority_public(), "device-record", &body, &sig)?;
        let o = body.as_object().ok_or(Error)?;
        let id: DeviceId = unhex(o.get("device_id").ok_or(Error)?)?;
        // A revoked device is not a device again, whatever the record says.
        if self.revoked.contains_key(&id) {
            return Err(Error);
        }
        let meta = DeviceMeta {
            name: o
                .get("name")
                .and_then(Json::as_str)
                .ok_or(Error)?
                .to_owned(),
            agree_pub: unhex(o.get("agree_pub").ok_or(Error)?)?,
            added_at: o.get("added_at").and_then(Json::as_u64).ok_or(Error)?,
            epoch_added: u32::try_from(o.get("epoch_added").and_then(Json::as_u64).ok_or(Error)?)
                .map_err(|_| Error)?,
        };
        self.devices
            .insert(id, unhex(o.get("sign_pub").ok_or(Error)?)?);
        self.device_meta.insert(id, meta);
        Ok(())
    }

    /// The highest op sequence of `device` applied here, this device's own included; 0 when there is none.
    pub fn applied_seq(&self, device: &DeviceId) -> u64 {
        if *device == self.device.device_id {
            self.own_seq
        } else {
            self.applied.get(device).map_or(0, |(seq, _)| *seq)
        }
    }

    /// The current `key_epoch`.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// Every device this vault knows: those with a valid record, then the revoked ones.
    pub fn devices(&self) -> Vec<DeviceInfo> {
        let live = self.devices.iter().map(|(id, sign_pub)| DeviceInfo {
            device_id: *id,
            name: self.device_meta.get(id).map(|m| m.name.clone()),
            fingerprint: device_fingerprint(sign_pub),
            revoked_at: None,
        });
        let gone = self.revoked.iter().map(|(id, r)| DeviceInfo {
            device_id: *id,
            name: None,
            fingerprint: device_fingerprint(&r.sign_pub),
            revoked_at: Some(r.revoked_at),
        });
        live.chain(gone).collect()
    }

    /// Approve a join request that opened under `invite` (`pairing-recovery.md` 3.2 step 3). The caller has
    /// already asked the user, with the device name and fingerprint, and checked a biometric. Signs the new
    /// device's record and seals the Secret Key for it. Refuses a device id that is already in use.
    pub fn approve_join(
        &mut self,
        invite: &Invite,
        req: &JoinRequest,
        secret_key: &SecretKey,
        now_ms: u64,
    ) -> Result<Approval> {
        if invite.is_expired(now_ms)
            || self.devices.contains_key(&req.device_id)
            || self.revoked.contains_key(&req.device_id)
        {
            return Err(Error);
        }
        let response = Committed {
            path: invite.resp_path(),
            bytes: invite.seal_response(req, secret_key)?,
        };
        let meta = DeviceMeta {
            name: req.device_name.clone(),
            agree_pub: req.agree_pub,
            added_at: now_ms,
            epoch_added: self.epoch,
        };
        let record = Committed {
            path: record_path(&req.device_id),
            bytes: self.sign_record(&req.device_id, &req.sign_pub, &meta)?,
        };
        self.devices.insert(req.device_id, req.sign_pub);
        self.device_meta.insert(req.device_id, meta);
        Ok(Approval { record, response })
    }

    fn sign_record(&self, id: &DeviceId, sign_pub: &[u8; 32], m: &DeviceMeta) -> Result<Vec<u8>> {
        let body = json!({
            "device_id": hexs(id), "name": m.name, "sign_pub": hexs(sign_pub),
            "agree_pub": hexs(&m.agree_pub), "added_at": m.added_at, "epoch_added": m.epoch_added,
        });
        with_signature(
            body.clone(),
            sign_json(&self.keys.authority_seed, "device-record", &body)?,
        )
    }

    /// Revoke `target` (`pairing-recovery.md` 4, `crypto.md` 9.1). Rotates `VK` to the next epoch, lists the
    /// device as revoked, re-signs the remaining device records, rewraps every item key and builds a
    /// snapshot under the new epoch. Needs the master password and Secret Key to rewrap `VK`; they are
    /// checked against the current wrapped `VK` first. This vault moves to the new epoch only if every step
    /// succeeds. Commit pending changes first, and load all current device records first: a device without
    /// a record here would lose its record in the rotation.
    pub fn revoke_device(
        &mut self,
        password: &str,
        secret_key: &SecretKey,
        target: &DeviceId,
        now_ms: u64,
    ) -> Result<Revocation> {
        self.revoke_with(password, secret_key, target, now_ms, None)
    }

    /// [`Vault::revoke_device`] and [`Vault::change_credentials`] in one write (ADR-0022 amendment). The new
    /// `VK` is wrapped straight under the new `AUK`, so no `vault.json` that the old password and Secret Key
    /// open is ever stored at the new epoch, and `revision` goes up once. A copy of the `vault.json` from
    /// before the revocation still opens the old epoch.
    pub fn revoke_device_and_change_credentials(
        &mut self,
        password: &str,
        secret_key: &SecretKey,
        target: &DeviceId,
        change: CredentialChange<'_>,
        now_ms: u64,
    ) -> Result<Revocation> {
        self.revoke_with(password, secret_key, target, now_ms, Some(change))
    }

    fn revoke_with(
        &mut self,
        password: &str,
        secret_key: &SecretKey,
        target: &DeviceId,
        now_ms: u64,
        change: Option<CredentialChange<'_>>,
    ) -> Result<Revocation> {
        if *target == self.device.device_id
            || !self.devices.contains_key(target)
            || !self.pending.is_empty()
        {
            return Err(Error);
        }
        let params_bytes = self.params.bytes_with_salt(&self.salt);
        let auk = derive_auk(
            password,
            secret_key,
            &self.vault_id,
            &self.salt,
            &self.params,
        )?;
        let old_vk = unwrap_vk(
            &auk,
            &self.wrapped_vk,
            &self.vault_id,
            self.epoch,
            &params_bytes,
        )?;

        let new_epoch = self.epoch.checked_add(1).ok_or(Error)?;
        let new_vk = Key::random()?;
        let new_keys = derive_vault_keys(&new_vk)?;
        // The `AUK` that wraps the new `VK`: the current one, or a fresh one when the credentials change.
        let (wrap_auk, salt, params, new_secret_key) = match change {
            None => (auk, self.salt, self.params, None),
            Some(c) => {
                let params = c.params.unwrap_or(self.params);
                params.check()?;
                let salt = random::<16>()?;
                let new_secret_key = if c.rotate_secret_key {
                    Some(SecretKey::random()?)
                } else {
                    None
                };
                let wrap_auk = derive_auk(
                    c.new_password.unwrap_or(password),
                    new_secret_key.as_ref().unwrap_or(secret_key),
                    &self.vault_id,
                    &salt,
                    &params,
                )?;
                (wrap_auk, salt, params, new_secret_key)
            }
        };
        let wrapped_vk = wrap_vk(
            &wrap_auk,
            &new_vk,
            &self.vault_id,
            new_epoch,
            &params.bytes_with_salt(&salt),
        )?;
        let mut epoch_keys = self.epoch_keys.clone();
        epoch_keys.push(EpochKeyEntry {
            from: self.epoch,
            to: new_epoch,
            sealed: epoch_key(&new_vk, &old_vk, &self.vault_id, self.epoch, new_epoch)?,
        });
        let mut hlc = self.hlc;
        let revoked_at = hlc.local(now_ms)?;
        let mut revoked = self.revoked.clone();
        revoked.insert(
            *target,
            Revoked {
                sign_pub: self.devices[target],
                revoked_at,
                epoch: new_epoch,
            },
        );

        // The Recovery Key keeps working: the new `VK` is sealed to the same public key (`crypto.md` 9.1).
        let new_recovery = match &self.recovery {
            Some(r) => Some(RecoveryWrap {
                rk_pub: r.rk_pub,
                wrapped: wrap_vk_for_recovery(&r.rk_pub, &new_vk, &self.vault_id, new_epoch)?,
            }),
            None => None,
        };
        let revision = self.revision + 1;
        let mut body = vault_json_body_with(
            self,
            &salt,
            &wrapped_vk,
            (new_epoch, revision, &new_keys.authority_public()),
            (&revoked, &epoch_keys),
            new_recovery.as_ref(),
        );
        // `vault_json_body_with` reads the KDF parameters from `self`; changed ones replace them.
        body["kdf"] = json!({
            "m_kib": params.m_kib, "t": params.t, "p": params.p, "salt": hexs(&salt),
        });
        let vault_json = with_signature(
            body.clone(),
            sign_json(&new_keys.authority_seed, "vault-json", &body)?,
        )?;

        // Records of the devices that stay, signed by the new authority key.
        let mut records = Vec::new();
        for (id, sign_pub) in self.devices.iter().filter(|(id, _)| *id != target) {
            let meta = self.device_meta.get(id).ok_or(Error)?;
            let body = json!({
                "device_id": hexs(id), "name": meta.name, "sign_pub": hexs(sign_pub),
                "agree_pub": hexs(&meta.agree_pub), "added_at": meta.added_at,
                "epoch_added": meta.epoch_added,
            });
            records.push(Committed {
                path: record_path(id),
                bytes: with_signature(
                    body.clone(),
                    sign_json(&new_keys.authority_seed, "device-record", &body)?,
                )?,
            });
        }

        let snapshot = self.build_snapshot(&new_keys, new_epoch)?;

        // Nothing below can fail.
        self.hlc = hlc;
        self.devices.remove(target);
        self.device_meta.remove(target);
        self.revoked = revoked;
        self.epoch_keys = epoch_keys;
        self.older
            .insert(self.epoch, std::mem::replace(&mut self.keys, new_keys));
        self.epoch = new_epoch;
        self.revision = revision;
        self.wrapped_vk = wrapped_vk;
        self.recovery = new_recovery;
        self.salt = salt;
        self.params = params;
        Ok(Revocation {
            vault_json,
            records,
            remove: vec![record_path(target)],
            snapshot,
            secret_key: new_secret_key,
        })
    }

    /// Change the master password, rotate the Secret Key, or both (`crypto.md` 9). The current credentials
    /// are checked against the wrapped `VK` first. A fresh salt and (when `rotate_secret_key`) a fresh
    /// Secret Key give a new `AUK`, and `VK` is rewrapped under it: nothing is re-encrypted and the epoch
    /// does not change. The replacement `vault.json` is signed with the same authority key, one `revision`
    /// higher. Every other device fails its next quick unlock and asks for a full unlock with the new
    /// credentials. This vault takes the new wrap only if every step succeeds.
    ///
    /// This does not rotate `VK`: someone holding the old credentials and a copy of the earlier `vault.json`
    /// can still open the `VK` of that copy. After a revocation, run it before anything new is written.
    pub fn change_credentials(
        &mut self,
        password: &str,
        secret_key: &SecretKey,
        change: CredentialChange<'_>,
    ) -> Result<Rotated> {
        let params = change.params.unwrap_or(self.params);
        params.check()?;
        let old_auk = derive_auk(
            password,
            secret_key,
            &self.vault_id,
            &self.salt,
            &self.params,
        )?;
        let vk = unwrap_vk(
            &old_auk,
            &self.wrapped_vk,
            &self.vault_id,
            self.epoch,
            &self.params.bytes_with_salt(&self.salt),
        )?;
        if derive_vault_keys(&vk)?.authority_public() != self.keys.authority_public() {
            return Err(Error);
        }

        let salt = random::<16>()?;
        let new_secret_key = if change.rotate_secret_key {
            Some(SecretKey::random()?)
        } else {
            None
        };
        let new_password = change.new_password.unwrap_or(password);
        let auk = derive_auk(
            new_password,
            new_secret_key.as_ref().unwrap_or(secret_key),
            &self.vault_id,
            &salt,
            &params,
        )?;
        let wrapped_vk = wrap_vk(
            &auk,
            &vk,
            &self.vault_id,
            self.epoch,
            &params.bytes_with_salt(&salt),
        )?;

        let revision = self.revision + 1;
        let mut body = vault_json_body_with(
            self,
            &salt,
            &wrapped_vk,
            (self.epoch, revision, &self.keys.authority_public()),
            (&self.revoked, &self.epoch_keys),
            self.recovery.as_ref(),
        );
        // `vault_json_body_with` reads the KDF parameters from `self`; the new ones replace them.
        body["kdf"] = json!({
            "m_kib": params.m_kib, "t": params.t, "p": params.p, "salt": hexs(&salt),
        });
        let vault_json = with_signature(
            body.clone(),
            sign_json(&self.keys.authority_seed, "vault-json", &body)?,
        )?;

        // Nothing below can fail.
        self.params = params;
        self.salt = salt;
        self.wrapped_vk = wrapped_vk;
        self.revision = revision;
        Ok(Rotated {
            vault_json,
            secret_key: new_secret_key,
        })
    }

    /// Whether a Recovery Key is enabled (`vault.json` carries `rk_pub`).
    pub fn has_recovery(&self) -> bool {
        self.recovery.is_some()
    }

    /// Enable the Recovery Key, or replace it with a new one (`crypto.md` 9.3, ADR-0029). Returns the new
    /// `vault.json` and the key to write down; this vault takes the change only if every step succeeds. The
    /// master password and Secret Key are needed because the Vault Key is not kept in memory: it is opened
    /// from the stored wrap and sealed to the new public key. After a replacement, cards of the old key stop
    /// working for the new `vault.json` and later ones.
    pub fn set_recovery(
        &mut self,
        password: &str,
        secret_key: &SecretKey,
    ) -> Result<RecoveryChange> {
        let auk = derive_auk(
            password,
            secret_key,
            &self.vault_id,
            &self.salt,
            &self.params,
        )?;
        let vk = unwrap_vk(
            &auk,
            &self.wrapped_vk,
            &self.vault_id,
            self.epoch,
            &self.params.bytes_with_salt(&self.salt),
        )?;
        if derive_vault_keys(&vk)?.authority_public() != self.keys.authority_public() {
            return Err(Error);
        }
        let key = RecoveryKey::random()?;
        let rk_pub = key.public();
        let wrap = RecoveryWrap {
            rk_pub,
            wrapped: wrap_vk_for_recovery(&rk_pub, &vk, &self.vault_id, self.epoch)?,
        };
        let vault_json = self.recovery_vault_json(Some(&wrap))?;
        self.recovery = Some(wrap);
        self.revision += 1;
        Ok(RecoveryChange {
            vault_json,
            key: Some(key),
        })
    }

    /// Turn the Recovery Key off: the next `vault.json` has no `rk_pub` and no wrap, and any card of the old
    /// key opens nothing from then on (a copy of an older `vault.json` still opens with it).
    pub fn clear_recovery(&mut self) -> Result<RecoveryChange> {
        if self.recovery.is_none() {
            return Err(Error);
        }
        let vault_json = self.recovery_vault_json(None)?;
        self.recovery = None;
        self.revision += 1;
        Ok(RecoveryChange {
            vault_json,
            key: None,
        })
    }

    /// The `vault.json` for revision + 1 with the Recovery Key members `recovery`, everything else unchanged.
    fn recovery_vault_json(&self, recovery: Option<&RecoveryWrap>) -> Result<Vec<u8>> {
        let body = vault_json_body_with(
            self,
            &self.salt,
            &self.wrapped_vk,
            (self.epoch, self.revision + 1, &self.keys.authority_public()),
            (&self.revoked, &self.epoch_keys),
            recovery,
        );
        with_signature(
            body.clone(),
            sign_json(&self.keys.authority_seed, "vault-json", &body)?,
        )
    }

    /// Use the Recovery Key after the master password is lost (`pairing-recovery.md` 7.3). Opens the Vault Key
    /// from `wrapped_vk_rk`, checks it exactly as an unlock does (authority key derived from it, signature),
    /// then writes a new `vault.json` in one go: a fresh salt and a fresh Secret Key wrapped under the new
    /// master password, with a new Recovery Key when `rotate_recovery` is set. Every wrong key, a vault without
    /// a Recovery Key and a tampered `vault.json` are the one generic error.
    ///
    /// Other devices are not revoked: whoever held the old credentials can still open an old copy of
    /// `vault.json`, which the flow tells the person.
    pub fn recover(
        request: Recover<'_>,
        device: DeviceKeys,
        device_name: &str,
        known_records: &[Vec<u8>],
        now_ms: u64,
    ) -> Result<(Vault, Recovered)> {
        let Recover {
            vault_json,
            recovery_key,
            new_password,
            rotate_recovery,
        } = request;
        let (body, _) = split_signature(vault_json)?;
        let h = Header::parse(body.as_object().ok_or(Error)?)?;
        let wrap = h.recovery.ok_or(Error)?;
        let opened = open_seal_to(
            recovery_key.private(),
            &wrap.wrapped,
            &aad("vk-wrap-rk", &[&h.vault_id, &u32be(h.epoch)]),
        )?;
        let vk = Key::from_slice(&opened)?;
        let (mut vault, new_record) =
            Self::open_with_vk(vault_json, &vk, device, device_name, known_records, now_ms)?;

        let salt = random::<16>()?;
        let secret_key = SecretKey::random()?;
        let auk = derive_auk(
            new_password,
            &secret_key,
            &vault.vault_id,
            &salt,
            &vault.params,
        )?;
        let wrapped_vk = wrap_vk(
            &auk,
            &vk,
            &vault.vault_id,
            vault.epoch,
            &vault.params.bytes_with_salt(&salt),
        )?;
        let (recovery, new_key) = if rotate_recovery {
            let key = RecoveryKey::random()?;
            let rk_pub = key.public();
            let wrap = RecoveryWrap {
                rk_pub,
                wrapped: wrap_vk_for_recovery(&rk_pub, &vk, &vault.vault_id, vault.epoch)?,
            };
            (wrap, Some(key))
        } else {
            (wrap, None)
        };
        let revision = vault.revision + 1;
        let mut body = vault_json_body_with(
            &vault,
            &salt,
            &wrapped_vk,
            (vault.epoch, revision, &vault.keys.authority_public()),
            (&vault.revoked, &vault.epoch_keys),
            Some(&recovery),
        );
        body["kdf"] = json!({
            "m_kib": vault.params.m_kib, "t": vault.params.t, "p": vault.params.p, "salt": hexs(&salt),
        });
        let new_vault_json = with_signature(
            body.clone(),
            sign_json(&vault.keys.authority_seed, "vault-json", &body)?,
        )?;
        vault.salt = salt;
        vault.wrapped_vk = wrapped_vk;
        vault.recovery = Some(recovery);
        vault.revision = revision;
        Ok((
            vault,
            Recovered {
                vault_json: new_vault_json,
                secret_key,
                recovery_key: new_key,
                new_record,
            },
        ))
    }

    /// A snapshot of the whole state sealed under `keys` for `epoch`, item keys rewrapped under its `WK`.
    /// An item whose key cannot be opened (corrupt or hostile) is left out rather than blocking the rotation.
    fn build_snapshot(&mut self, keys: &VaultKeys, epoch: u32) -> Result<Committed> {
        let ids: Vec<ItemId> = self.state.items().keys().copied().collect();
        let mut items = Vec::new();
        for id in ids {
            let Ok(ik) = self.item_key(&id).map(|k| Key::from_bytes(*k.expose())) else {
                continue;
            };
            let wrapped_ik = wrap_item_key(&keys.wk, &ik, &self.vault_id, &id, epoch)?;
            let mut deleted = None;
            let mut fields = BTreeMap::new();
            for (name, reg) in &self.state.items()[&id] {
                match (&reg.value, name.as_str()) {
                    (_, FIELD_IK) => {}
                    (FieldValue::Flag(f), FIELD_DELETED) => {
                        deleted = Some((reg.stamp, reg.device, *f));
                    }
                    (FieldValue::Blob(b), _) => {
                        fields.insert(
                            name.clone(),
                            SnapshotField {
                                hlc: reg.stamp,
                                device: reg.device,
                                blob: Some(b.clone()),
                            },
                        );
                    }
                    (FieldValue::Removed, _) => {
                        fields.insert(
                            name.clone(),
                            SnapshotField {
                                hlc: reg.stamp,
                                device: reg.device,
                                blob: None,
                            },
                        );
                    }
                    (FieldValue::Flag(_), _) => return Err(Error),
                }
            }
            items.push(SnapshotItem {
                item: id,
                wrapped_ik,
                deleted,
                fields,
            });
        }
        let mut hw: BTreeMap<DeviceId, u64> = self
            .applied
            .iter()
            .map(|(d, (seq, _))| (*d, *seq))
            .collect();
        hw.insert(self.device.device_id, self.own_seq);
        let snap = Snapshot {
            device: self.device.device_id,
            seq: self.own_seq,
            hw,
            purge_horizon: Stamp::ZERO,
            items,
        };
        let id = FileId {
            vault_id: &self.vault_id,
            device_id: &self.device.device_id,
            seq: self.own_seq,
        };
        Ok(Committed {
            path: format!(
                "snap/{}-{:08}-{:08}.snap",
                hexs(&self.device.device_id),
                self.own_seq,
                epoch
            ),
            bytes: build_file(
                Kind::Snapshot,
                &keys.lk,
                &self.device.sign_seed,
                &id,
                epoch,
                &snap.encode(),
            )?,
        })
    }

    /// Verify, decrypt and merge a snapshot file. It only adds state (the merge is idempotent); the op
    /// files still apply in order afterwards. Snapshots from revoked or unknown devices are refused.
    pub fn apply_snapshot(
        &mut self,
        bytes: &[u8],
        device: &DeviceId,
        seq: u64,
        now_ms: u64,
    ) -> Result<()> {
        let sign_pub = self.devices.get(device).copied();
        let id = FileId {
            vault_id: &self.vault_id,
            device_id: device,
            seq,
        };
        let epoch = self.file_epoch(bytes)?;
        let payload = read_file(
            bytes,
            self.log_key(epoch)?,
            sign_pub.as_ref(),
            &id,
            Kind::Snapshot,
        )?;
        let snap = Snapshot::decode(&payload)?;
        if snap.device != *device || snap.seq != seq {
            return Err(Error);
        }
        let mut hlc = self.hlc;
        let mut writes = Vec::new();
        for it in snap.items {
            // The item key has no stamp of its own in a snapshot; at zero it never beats an op's write.
            writes.push(Write {
                item: it.item,
                field: FIELD_IK.to_owned(),
                stamp: Stamp::ZERO,
                device: *device,
                value: FieldValue::Blob(it.wrapped_ik),
            });
            if let Some((stamp, dev, flag)) = it.deleted {
                writes.push(Write {
                    item: it.item,
                    field: FIELD_DELETED.to_owned(),
                    stamp,
                    device: dev,
                    value: FieldValue::Flag(flag),
                });
                hlc.receive(stamp, now_ms)?;
            }
            for (field, f) in it.fields {
                let known = [FIELD_TYPE, FIELD_VAULT].contains(&field.as_str())
                    || ALL_TYPES.iter().any(|t| validate_field(*t, &field).is_ok());
                if !known
                    || f.blob
                        .as_ref()
                        .is_some_and(|b| b.len() > max_value_bytes(&field) + BLOB_OVERHEAD)
                {
                    return Err(Error);
                }
                if is_held(f.hlc.wall, now_ms) {
                    return Err(Error);
                }
                hlc.receive(f.hlc, now_ms)?;
                writes.push(Write {
                    item: it.item,
                    field,
                    stamp: f.hlc,
                    device: f.device,
                    value: f.blob.map_or(FieldValue::Removed, FieldValue::Blob),
                });
            }
        }
        for w in writes {
            self.state.apply(w);
        }
        self.hlc = hlc;
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
        self.undo.push(Undo {
            item,
            field: field.to_owned(),
            written: stamp,
            previous: self.state.get(&item, &field.to_owned()).cloned(),
        });
        self.state.apply(Write {
            item,
            field: field.to_owned(),
            stamp,
            device,
            value,
        });
    }

    /// Unwrap an item key under the current epoch, then under earlier ones: an item created before a
    /// rotation keeps its original wrapping until a snapshot rewraps it (`crypto.md` 9.2).
    fn unwrap_ik(&self, wrapped: &[u8], id: &ItemId) -> Result<Key> {
        if let Ok(k) = unwrap_item_key(&self.keys.wk, wrapped, &self.vault_id, id, self.epoch) {
            return Ok(k);
        }
        self.older
            .iter()
            .rev()
            .find_map(|(epoch, keys)| {
                unwrap_item_key(&keys.wk, wrapped, &self.vault_id, id, *epoch).ok()
            })
            .ok_or(Error)
    }

    fn item_key(&mut self, id: &ItemId) -> Result<&Key> {
        if !self.item_keys.contains_key(id) {
            let reg = self.state.get(id, &FIELD_IK.to_owned()).ok_or(Error)?;
            let FieldValue::Blob(w) = &reg.value else {
                return Err(Error);
            };
            let ik = self.unwrap_ik(w, id)?;
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
                if v.len() > max_value_bytes(field) {
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
        check_counts(fields.iter().map(|(f, _)| *f))?;
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
        self.new_keys.push(id);
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
        if value.is_some() {
            let mut live = self.live_fields(id);
            live.insert(field.to_owned());
            check_counts(live.iter().map(String::as_str))?;
        }
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

    /// Throw away every change made since the last commit, as if it had not been made. For a save that
    /// failed: the person can edit and save again without the half-applied change riding along. A field that
    /// another device has overwritten since keeps the other device's value.
    pub fn discard_pending(&mut self) {
        let undo = std::mem::take(&mut self.undo);
        self.unwind(undo);
        for id in std::mem::take(&mut self.new_keys) {
            self.item_keys.remove(&id);
        }
        self.pending.clear();
        self.last_commit = None;
    }

    /// Record that the last `commit` is known to be written, so `rollback_commit` can no longer take it back.
    pub fn confirm_commit(&mut self) {
        self.last_commit = None;
    }

    /// Take back the last `commit` because its op file could not be written. The vault is then exactly as it
    /// was before the commit (the changes pending again, the chain head and sequence unchanged), so the
    /// caller can retry the commit or call `discard_pending`. Does nothing if there is no commit to take back, or
    /// if something was changed since it.
    pub fn rollback_commit(&mut self) {
        if !self.pending.is_empty() || !self.undo.is_empty() {
            return;
        }
        let Some(r) = self.last_commit.take() else {
            return;
        };
        self.own_seq = r.seq;
        self.own_prev = r.prev;
        self.pending = r.ops;
        self.undo = r.undo;
        self.new_keys = r.new_keys;
    }

    fn unwind(&mut self, undo: Vec<Undo>) {
        let device = self.device.device_id;
        for u in undo.into_iter().rev() {
            let ours = self
                .state
                .get(&u.item, &u.field)
                .is_some_and(|r| r.stamp == u.written && r.device == device);
            if ours {
                self.state.restore(&u.item, &u.field, u.previous);
            }
        }
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
        self.last_commit = Some(Rollback {
            seq: self.own_seq,
            prev: self.own_prev,
            ops: std::mem::take(&mut self.pending),
            undo: std::mem::take(&mut self.undo),
            new_keys: std::mem::take(&mut self.new_keys),
        });
        self.own_prev = Sha256::digest(&bytes).into();
        self.own_seq = seq;
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
        // A revoked device is known by its entry in the signed list; its files are checked below.
        let revoked = self.revoked.get(device).cloned();
        let sign_pub = self
            .devices
            .get(device)
            .copied()
            .or(revoked.as_ref().map(|r| r.sign_pub));
        let id = FileId {
            vault_id: &self.vault_id,
            device_id: device,
            seq,
        };
        let epoch = self.file_epoch(bytes)?;
        let payload = read_file(
            bytes,
            self.log_key(epoch)?,
            sign_pub.as_ref(),
            &id,
            Kind::Op,
        )?;
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
                        .is_some_and(|b| b.len() > max_value_bytes(&field) + BLOB_OVERHEAD)
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
            if let Some(r) = &revoked
                && !accept_from_revoked(r.revoked_at, stamp, epoch, r.epoch)
            {
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

    /// Seal one audit record (`crypto.md` 7.5): the next `seq` of this device's audit log, chained to the
    /// previous record by `prev` (its SHA-256, [`crate::ZERO_PREV`] for the first). Fails for an event or
    /// detail member the log does not allow. The caller appends the result with [`crate::audit_frame`].
    pub fn seal_audit(
        &self,
        seq: u64,
        prev: &[u8; 32],
        t: u64,
        ev: &str,
        d: &Map<String, Json>,
    ) -> Result<Vec<u8>> {
        let payload = crate::audit_payload(prev, t, ev, d)?;
        let id = FileId {
            vault_id: &self.vault_id,
            device_id: &self.device.device_id,
            seq,
        };
        build_file(
            Kind::Audit,
            &self.keys.ak,
            &self.device.sign_seed,
            &id,
            self.epoch,
            &payload,
        )
    }

    /// Verify this device's audit log. Returns the chain (records up to the first failure) and whether
    /// bytes remain after the last whole record (an incomplete write, or damage).
    pub fn read_audit_log(&self, log: &[u8]) -> (crate::AuditChain, bool) {
        let (files, used) = crate::audit_split(log);
        let chain = crate::read_audit_chain(
            &files,
            |epoch| {
                if epoch == self.epoch {
                    Some(&self.keys.ak)
                } else {
                    self.older.get(&epoch).map(|k| &k.ak)
                }
            },
            &self.device.sign_public(),
            &self.vault_id,
            &self.device.device_id,
        );
        (chain, used < log.len())
    }

    /// The `key_epoch` in a file's plaintext header (covered by its signature and AAD).
    fn file_epoch(&self, bytes: &[u8]) -> Result<u32> {
        let h: [u8; 4] = bytes
            .get(4..8)
            .and_then(|b| b.try_into().ok())
            .ok_or(Error)?;
        Ok(u32::from_be_bytes(h))
    }

    /// The log key for a file of `epoch`: the current one, or an earlier one from the chain. A file of a
    /// later epoch needs a full unlock first.
    fn log_key(&self, epoch: u32) -> Result<&Key> {
        if epoch == self.epoch {
            return Ok(&self.keys.lk);
        }
        self.older.get(&epoch).map(|k| &k.lk).ok_or(Error)
    }

    /// Whether a remote `FieldSet` may write `field`: the vault's own `_type` and `_vault`, or a schema
    /// field of the item's type (any type while the item's type is not yet known).
    fn field_allowed(&mut self, item: &ItemId, field: &str) -> bool {
        if field == FIELD_TYPE || field == FIELD_VAULT {
            return true;
        }
        match self.item_type(item) {
            Ok(t) => validate_field(t, field).is_ok(),
            Err(_) => ALL_TYPES.iter().any(|t| validate_field(*t, field).is_ok()),
        }
    }

    /// Names of the item's fields that currently hold a value.
    fn live_fields(&self, id: &ItemId) -> BTreeSet<String> {
        self.state
            .items()
            .get(id)
            .map(|fields| {
                fields
                    .iter()
                    .filter(|(f, r)| !f.starts_with('_') && !matches!(r.value, FieldValue::Removed))
                    .map(|(f, _)| f.clone())
                    .collect()
            })
            .unwrap_or_default()
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

    const T0: u64 = 1_700_000_000_000;

    fn pw() -> &'static str {
        static PW: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        PW.get_or_init(|| {
            let mut b = [0u8; 16];
            getrandom::fill(&mut b).unwrap();
            b.iter().map(|x| format!("{x:02x}")).collect()
        })
    }

    fn pair() -> (Vault, Vault) {
        let (a, c) = Vault::create(pw(), "A", KdfParams::FLOOR, T0).unwrap();
        let sk = SecretKey::parse(&c.secret_key.format()).unwrap();
        let (b, _) = Vault::unlock(
            &c.vault_json,
            pw(),
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
