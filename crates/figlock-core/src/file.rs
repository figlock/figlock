//! The op-file and snapshot container, and its signature (`sync.md` 4, `crypto.md` 7.2 and 7.3).
//!
//! ```text
//! "FL" 0x01 kind u32(key_epoch) | u32(len N) | envelope (N bytes) | Ed25519 signature (64 bytes)
//! ```

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::aead::{open, seal};
use crate::wire::{aad, u32be, u64be};
use crate::{Error, Key, Result};

/// Length of the plaintext header that the signature covers.
pub const FILE_HEADER_LEN: usize = 8;
const SIG_LEN: usize = 64;

/// Where a file lives: vault, writing device and its sequence number (all signed and in the AAD).
#[derive(Clone, Copy, Debug)]
pub struct FileId<'a> {
    pub vault_id: &'a [u8; 16],
    pub device_id: &'a [u8; 16],
    pub seq: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Op = 1,
    Snapshot = 2,
}

fn header(kind: Kind, epoch: u32) -> [u8; FILE_HEADER_LEN] {
    let e = u32be(epoch);
    [b'F', b'L', 1, kind as u8, e[0], e[1], e[2], e[3]]
}

fn file_aad(kind: Kind, id: &FileId, epoch: u32) -> Vec<u8> {
    let ctx = if kind == Kind::Op { "log" } else { "snap" };
    aad(
        ctx,
        &[id.vault_id, id.device_id, &u64be(id.seq), &u32be(epoch)],
    )
}

fn sig_input(id: &FileId, header: &[u8], envelope: &[u8]) -> Vec<u8> {
    let mut m = b"figlock/v1/file-sig\x00".to_vec();
    m.extend_from_slice(id.vault_id);
    m.extend_from_slice(id.device_id);
    m.extend_from_slice(&u64be(id.seq));
    let mut h = Sha256::new();
    h.update(header);
    h.update(envelope);
    m.extend_from_slice(&h.finalize());
    m
}

fn assemble(
    kind: Kind,
    epoch: u32,
    signer: &Key,
    id: &FileId,
    envelope: Vec<u8>,
) -> Result<Vec<u8>> {
    let hdr = header(kind, epoch);
    let sig = SigningKey::from_bytes(signer.expose()).sign(&sig_input(id, &hdr, &envelope));
    let mut out = Vec::with_capacity(FILE_HEADER_LEN + 4 + envelope.len() + SIG_LEN);
    out.extend_from_slice(&hdr);
    out.extend_from_slice(&u32be(u32::try_from(envelope.len()).map_err(|_| Error)?));
    out.extend_from_slice(&envelope);
    out.extend_from_slice(&sig.to_bytes());
    Ok(out)
}

/// Encrypt `payload` (CBOR, already padded) under the log key and sign the container with the device key.
pub fn build_file(
    kind: Kind,
    lk: &Key,
    device_sign_seed: &Key,
    id: &FileId,
    epoch: u32,
    payload: &[u8],
) -> Result<Vec<u8>> {
    let envelope = seal(lk, payload, &file_aad(kind, id, epoch))?;
    assemble(kind, epoch, device_sign_seed, id, envelope)
}

/// Test-vector entry point; see [`crate::vector_hooks`].
#[cfg(feature = "vector-hooks")]
pub fn build_file_with_nonce(
    kind: Kind,
    lk: &Key,
    device_sign_seed: &Key,
    id: &FileId,
    epoch: u32,
    payload: &[u8],
    nonce: &[u8; 24],
) -> Result<Vec<u8>> {
    let envelope = crate::aead::seal_with_nonce(lk, payload, &file_aad(kind, id, epoch), nonce)?;
    assemble(kind, epoch, device_sign_seed, id, envelope)
}

/// Verify, then decrypt. `device_sign_pub` is `None` when the device has no valid, non-revoked record; the
/// file is then refused without looking at it. Order: kind and length, signature, then decryption.
pub fn read_file(
    data: &[u8],
    lk: &Key,
    device_sign_pub: Option<&[u8; 32]>,
    id: &FileId,
    expect: Kind,
) -> Result<Zeroizing<Vec<u8>>> {
    let pubkey = device_sign_pub.ok_or(Error)?;
    if data.len() < FILE_HEADER_LEN + 4 + SIG_LEN {
        return Err(Error);
    }
    let hdr = &data[..FILE_HEADER_LEN];
    if hdr[..3] != [b'F', b'L', 1] || hdr[3] != expect as u8 {
        return Err(Error);
    }
    let epoch = u32::from_be_bytes(hdr[4..8].try_into().map_err(|_| Error)?);
    let n = u32::from_be_bytes(data[8..12].try_into().map_err(|_| Error)?) as usize;
    if n != data.len() - (FILE_HEADER_LEN + 4) - SIG_LEN {
        return Err(Error);
    }
    let (envelope, sig) = (&data[12..12 + n], &data[12 + n..]);
    let sig: [u8; SIG_LEN] = sig.try_into().map_err(|_| Error)?;
    VerifyingKey::from_bytes(pubkey)
        .map_err(|_| Error)?
        .verify_strict(&sig_input(id, hdr, envelope), &Signature::from_bytes(&sig))
        .map_err(|_| Error)?;
    open(lk, envelope, &file_aad(expect, id, epoch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_length_field_is_rejected() {
        let key = Key::from_bytes([7u8; 32]);
        let id = FileId {
            vault_id: &[1; 16],
            device_id: &[2; 16],
            seq: 1,
        };
        let mut data = vec![b'F', b'L', 1, 1, 0, 0, 0, 1];
        data.extend_from_slice(&u32::MAX.to_be_bytes());
        data.extend_from_slice(&[0u8; 80]);
        assert!(read_file(&data, &key, Some(&[0u8; 32]), &id, Kind::Op).is_err());
    }
}
