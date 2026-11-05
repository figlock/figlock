//! The invite flow of `pairing-recovery.md` 3: the trusted device's [`Invite`], the new device's
//! [`JoinAttempt`], and the strict parsing of every file that lands in `inbox/`.
//!
//! Pure logic over bytes. Each step returns the path and bytes to store (or consumes bytes read from
//! storage); the caller moves them through a `BlobStore`. Everything in `inbox/` is untrusted: it is size
//! limited, parsed strictly, and only acted on after it opens under the keys derived from the invite code.

use ciborium::Value;
use ed25519_dalek::VerifyingKey;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::aead::{open, seal};
use crate::pairing::{INVITE_BYTES, invite_code_format, invite_code_parse, invite_id, invite_keys};
use crate::payload::{arr, as_map, canonical, check_keys, encode_value, get, map, text};
use crate::secret_key::b32_encode;
use crate::vault::{DeviceKeys, hexs, unhex};
use crate::wire::aad;
use crate::{Error, Key, Result, SecretKey, Vault, x25519_public};

/// An invite lives 10 minutes (`pairing-recovery.md` 3.4).
pub const INVITE_TTL_MS: u64 = 10 * 60 * 1000;
/// Largest `inbox/` file that is parsed at all (`pairing-recovery.md` 8).
pub const INBOX_MAX_BYTES: usize = 16 * 1024;
/// Files in `inbox/` older than this are cleaned up by any device.
pub const INBOX_STALE_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_NAME_BYTES: usize = 64;

/// `Crockford(SHA-256("figlock/v1/device-fingerprint" || 0x00 || sign_pub)[0..10])` as four groups of four.
/// Both screens show it so the user can compare them (`pairing-recovery.md` 3.3 step 4).
pub fn device_fingerprint(sign_pub: &[u8; 32]) -> String {
    let mut h = Sha256::new();
    h.update(b"figlock/v1/device-fingerprint");
    h.update([0u8]);
    h.update(sign_pub);
    let t = b32_encode(&h.finalize()[..10]);
    (0..4)
        .map(|i| &t[i * 4..i * 4 + 4])
        .collect::<Vec<_>>()
        .join("-")
}

/// Where the `.inv` for a typed `code` lives in the storage whose `vault.json` is `vault_json`. The vault id
/// is read from the file without verifying it: it only locates the invite, which [`JoinAttempt::begin`]
/// then checks against the code.
pub fn invite_path(code: &str, vault_json: &[u8]) -> Result<String> {
    let s = Zeroizing::new(invite_code_parse(code)?);
    let v: Json = serde_json::from_slice(vault_json).map_err(|_| Error)?;
    let vault_id: [u8; 16] = unhex(v.get("vault_id").ok_or(Error)?)?;
    Ok(inbox_path(&invite_id(&s, &vault_id)?, "inv"))
}

/// Whether a file in `inbox/` should be cleaned up regardless of what it is.
pub fn inbox_is_stale(age_ms: u64) -> bool {
    age_ms > INBOX_STALE_MS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxKind {
    Invite,
    Request,
    Response,
}

/// Classify an `inbox/<invite_id>.<ext>` path. Other names (later shares, strangers) return `None`.
pub fn parse_inbox_path(path: &str) -> Option<(InboxKind, [u8; 16])> {
    let name = path.strip_prefix("inbox/")?;
    let (id, ext) = name.split_once('.')?;
    let kind = match ext {
        "inv" => InboxKind::Invite,
        "req" => InboxKind::Request,
        "resp" => InboxKind::Response,
        _ => return None,
    };
    let id = unhex::<16>(&Json::String(id.to_owned())).ok()?;
    Some((kind, id))
}

fn inbox_path(id: &[u8; 16], ext: &str) -> String {
    format!("inbox/{}.{ext}", hexs(id))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control)
}

/// Decode a CBOR map that must be in its one canonical encoding, with nothing after it.
fn decode_exact(data: &[u8]) -> Result<Vec<(Value, Value)>> {
    let v: Value = ciborium::from_reader(data).map_err(|_| Error)?;
    if encode_value(&canonical(v.clone())) != data {
        return Err(Error);
    }
    as_map(v)
}

fn encode_map(entries: Vec<(&str, Value)>) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(encode_value(&canonical(map(entries))))
}

/// The plaintext `.inv` file.
struct InvFile {
    vault_id: [u8; 16],
    invite_id: [u8; 16],
    et_pub: [u8; 32],
    expires_at: u64,
}

fn parse_inv(data: &[u8]) -> Result<InvFile> {
    if data.len() > INBOX_MAX_BYTES {
        return Err(Error);
    }
    let v: Json = serde_json::from_slice(data).map_err(|_| Error)?;
    let o = v.as_object().ok_or(Error)?;
    if o.len() != 5 || o.get("v") != Some(&json!(1)) {
        return Err(Error);
    }
    Ok(InvFile {
        vault_id: unhex(o.get("vault_id").ok_or(Error)?)?,
        invite_id: unhex(o.get("invite_id").ok_or(Error)?)?,
        et_pub: unhex(o.get("et_pub").ok_or(Error)?)?,
        expires_at: o.get("expires_at").and_then(Json::as_u64).ok_or(Error)?,
    })
}

/// What the trusted device shows and stores when it opens an invite.
pub struct InviteOffer {
    /// `XXXX-XXXX-XXXX-XXXX-XXXX`, shown as text and as a QR code. Never stored or logged.
    pub code: Zeroizing<String>,
    pub inv_path: String,
    pub inv: Vec<u8>,
}

/// The trusted device's side of one open invite. Holds the invite secret and the ephemeral key in memory
/// only; they are never written anywhere.
pub struct Invite {
    s: Zeroizing<[u8; INVITE_BYTES]>,
    et_priv: Key,
    vault_id: [u8; 16],
    invite_id: [u8; 16],
    expires_at: u64,
    consumed: bool,
}

impl Invite {
    pub fn create(vault_id: &[u8; 16], now_ms: u64) -> Result<(Invite, InviteOffer)> {
        let mut s = Zeroizing::new([0u8; INVITE_BYTES]);
        getrandom::fill(s.as_mut()).map_err(|_| Error)?;
        s[0] &= 0x0f; // 100 bits: the top four bits of the 13 bytes are zero
        let et_priv = Key::random()?;
        let invite_id = invite_id(&s, vault_id)?;
        let expires_at = now_ms.checked_add(INVITE_TTL_MS).ok_or(Error)?;
        let inv = serde_json::to_vec_pretty(&json!({
            "v": 1, "vault_id": hexs(vault_id), "invite_id": hexs(&invite_id),
            "et_pub": hexs(&x25519_public(&et_priv)), "expires_at": expires_at,
        }))
        .map_err(|_| Error)?;
        let offer = InviteOffer {
            code: Zeroizing::new(invite_code_format(&s)?),
            inv_path: inbox_path(&invite_id, "inv"),
            inv,
        };
        let invite = Invite {
            s,
            et_priv,
            vault_id: *vault_id,
            invite_id,
            expires_at,
            consumed: false,
        };
        Ok((invite, offer))
    }

    pub fn invite_id(&self) -> &[u8; 16] {
        &self.invite_id
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at
    }

    /// Whether a valid request already used this invite.
    pub fn is_consumed(&self) -> bool {
        self.consumed
    }

    pub fn inv_path(&self) -> String {
        inbox_path(&self.invite_id, "inv")
    }

    pub fn req_path(&self) -> String {
        inbox_path(&self.invite_id, "req")
    }

    pub fn resp_path(&self) -> String {
        inbox_path(&self.invite_id, "resp")
    }

    /// Open a `.req` read from storage. A request that opens under this invite's keys consumes the invite
    /// (even if the user then declines); anything else, including garbage anyone could have written, changes
    /// nothing, so a stranger cannot burn an open invite.
    pub fn open_request(&mut self, data: &[u8], now_ms: u64) -> Result<JoinRequest> {
        if self.consumed || self.is_expired(now_ms) || !(32..=INBOX_MAX_BYTES).contains(&data.len())
        {
            return Err(Error);
        }
        let en_pub: [u8; 32] = data[..32].try_into().map_err(|_| Error)?;
        let keys = invite_keys(&self.s, &self.vault_id, &self.et_priv, &en_pub)?;
        let plain = open(
            &keys.k_req,
            &data[32..],
            &aad("invite-req", &[&self.vault_id, &self.invite_id]),
        )?;
        let m = decode_exact(&plain)?;
        check_request_keys(&m)?;
        let device_name = get(&m, "device_name")?.as_text().ok_or(Error)?.to_owned();
        let sign_pub: [u8; 32] = arr(get(&m, "sign_pub")?)?;
        VerifyingKey::from_bytes(&sign_pub).map_err(|_| Error)?;
        if !valid_name(&device_name) {
            return Err(Error);
        }
        self.consumed = true;
        Ok(JoinRequest {
            device_id: arr(get(&m, "device_id")?)?,
            device_name,
            sign_pub,
            agree_pub: arr(get(&m, "agree_pub")?)?,
            invite_id: self.invite_id,
            k_resp: keys.k_resp,
        })
    }

    /// Seal the response for an opened request: the Secret Key and the device id.
    pub(crate) fn seal_response(
        &self,
        req: &JoinRequest,
        secret_key: &SecretKey,
    ) -> Result<Vec<u8>> {
        if req.invite_id != self.invite_id {
            return Err(Error);
        }
        let plain = encode_map(vec![
            ("secret_key", Value::Bytes(secret_key.expose().to_vec())),
            ("device_id", Value::Bytes(req.device_id.to_vec())),
        ]);
        seal(
            &req.k_resp,
            &plain,
            &aad("invite-resp", &[&self.vault_id, &self.invite_id]),
        )
    }
}

fn check_request_keys(m: &[(Value, Value)]) -> Result<()> {
    check_keys(m, &["device_id", "device_name", "sign_pub", "agree_pub"])?;
    if m.len() != 4 {
        return Err(Error);
    }
    Ok(())
}

/// A request that opened under an invite: what the user is asked to approve.
pub struct JoinRequest {
    pub device_id: [u8; 16],
    pub device_name: String,
    pub sign_pub: [u8; 32],
    pub agree_pub: [u8; 32],
    invite_id: [u8; 16],
    pub(crate) k_resp: Key,
}

impl JoinRequest {
    /// Shown on both devices for comparison.
    pub fn fingerprint(&self) -> String {
        device_fingerprint(&self.sign_pub)
    }
}

/// The new device's side: its keys, and what it needs to read the answer.
pub struct JoinAttempt {
    vault_id: [u8; 16],
    invite_id: [u8; 16],
    k_resp: Key,
    device: DeviceKeys,
    device_name: String,
}

/// The `.req` the new device stores.
pub struct JoinRequestFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A device that has joined: the unlocked vault and the Secret Key to keep in the platform keystore.
pub struct Joined {
    pub vault: Vault,
    pub secret_key: SecretKey,
}

impl JoinAttempt {
    /// Start joining from the typed or scanned `code` and the `.inv` read from storage. Refuses an invite
    /// that is expired, malformed, or whose id does not belong to the code.
    pub fn begin(
        code: &str,
        inv: &[u8],
        device: DeviceKeys,
        device_name: &str,
        now_ms: u64,
    ) -> Result<(JoinAttempt, JoinRequestFile)> {
        if !valid_name(device_name) {
            return Err(Error);
        }
        let s = Zeroizing::new(invite_code_parse(code)?);
        let inv = parse_inv(inv)?;
        if invite_id(&s, &inv.vault_id)? != inv.invite_id || now_ms >= inv.expires_at {
            return Err(Error);
        }
        let en_priv = Key::random()?;
        let keys = invite_keys(&s, &inv.vault_id, &en_priv, &inv.et_pub)?;
        let plain = encode_map(vec![
            ("device_id", Value::Bytes(device.device_id.to_vec())),
            ("device_name", text(device_name)),
            ("sign_pub", Value::Bytes(device.sign_public().to_vec())),
            (
                "agree_pub",
                Value::Bytes(x25519_public(&device.agree_priv).to_vec()),
            ),
        ]);
        let mut bytes = x25519_public(&en_priv).to_vec();
        bytes.extend_from_slice(&seal(
            &keys.k_req,
            &plain,
            &aad("invite-req", &[&inv.vault_id, &inv.invite_id]),
        )?);
        let file = JoinRequestFile {
            path: inbox_path(&inv.invite_id, "req"),
            bytes,
        };
        let attempt = JoinAttempt {
            vault_id: inv.vault_id,
            invite_id: inv.invite_id,
            k_resp: keys.k_resp,
            device,
            device_name: device_name.to_owned(),
        };
        Ok((attempt, file))
    }

    pub fn resp_path(&self) -> String {
        inbox_path(&self.invite_id, "resp")
    }

    /// The device record path the answer is expected at.
    pub fn record_path(&self) -> String {
        format!("devices/{}.json", hexs(&self.device.device_id))
    }

    /// Open the `.resp`, check it is for this device, and unlock with the typed master password. The device
    /// record read from storage must verify against the authority key derived from the unlocked vault; a
    /// vault that would have to create a fresh record for this device is refused (`pairing-recovery.md` 3.4).
    pub fn finish(
        self,
        resp: &[u8],
        vault_json: &[u8],
        records: &[Vec<u8>],
        password: &str,
        now_ms: u64,
    ) -> Result<Joined> {
        if resp.len() > INBOX_MAX_BYTES {
            return Err(Error);
        }
        let plain = open(
            &self.k_resp,
            resp,
            &aad("invite-resp", &[&self.vault_id, &self.invite_id]),
        )?;
        let m = decode_exact(&plain)?;
        check_keys(&m, &["secret_key", "device_id"])?;
        if m.len() != 2 || arr::<16>(get(&m, "device_id")?)? != self.device.device_id {
            return Err(Error);
        }
        let secret_key = SecretKey::from_bytes(arr(get(&m, "secret_key")?)?);
        let (vault, own_record) = Vault::unlock(
            vault_json,
            password,
            &secret_key,
            self.device,
            &self.device_name,
            records,
            now_ms,
        )?;
        if own_record.is_some() || *vault.vault_id() != self.vault_id {
            return Err(Error);
        }
        Ok(Joined { vault, secret_key })
    }
}
