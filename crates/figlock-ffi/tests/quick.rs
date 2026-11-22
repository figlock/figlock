//! Quick unlock through the exported API, with a software P-256 key standing in for the Secure Enclave
//! (the chip only does the one ECDH step; the wrap is `figlock-core`). Values are generated.

use std::sync::Arc;

use figlock_ffi::{
    DeviceKeysHandle, FigLockError, UnlockKey, create_vault, derive_unlock_key,
    minimum_kdf_settings, open_quick_unlock, quick_unlock_ephemeral_public, unlock_vault_with_key,
    vault_identity,
};
use p256::{PublicKey, SecretKey, ecdh::diffie_hellman, elliptic_curve::sec1::ToSec1Point};

const T0: u64 = 1_800_000_000_000;
const PASSWORD: &str = "generated-test-password-1";

/// A stand-in for `QK`: the public point the phone seals to, and the ECDH step it does when opening.
struct FakeChip(SecretKey);

impl FakeChip {
    fn new() -> Self {
        // 32 random bytes are a valid P-256 scalar except with negligible probability
        let seed = DeviceKeysHandle::generate().unwrap().to_bytes();
        FakeChip(SecretKey::from_slice(&seed[16..48]).unwrap())
    }
    fn public(&self) -> Vec<u8> {
        self.0.public_key().to_sec1_point(false).as_bytes().to_vec()
    }
    fn agree(&self, eph_public: &[u8]) -> Vec<u8> {
        let eph = PublicKey::from_sec1_bytes(eph_public).unwrap();
        diffie_hellman(self.0.to_nonzero_scalar(), eph.as_affine())
            .raw_secret_bytes()
            .to_vec()
    }
}

struct Rig {
    vault_json: Vec<u8>,
    record: Vec<u8>,
    device: Arc<DeviceKeysHandle>,
    secret_key: String,
    vault_id: String,
}

fn rig() -> Rig {
    let c = create_vault(
        PASSWORD.into(),
        "test phone".into(),
        minimum_kdf_settings(),
        T0,
    )
    .unwrap();
    let vault_json = c.vault_json().unwrap();
    Rig {
        vault_id: vault_identity(vault_json.clone()).unwrap().vault_id,
        record: c.device_record().unwrap(),
        device: Arc::new(DeviceKeysHandle::from_bytes(c.device_keys().to_bytes()).unwrap()),
        secret_key: c.secret_key_text().unwrap(),
        vault_json,
    }
}

fn auk(r: &Rig) -> Arc<UnlockKey> {
    derive_unlock_key(r.vault_json.clone(), PASSWORD.into(), r.secret_key.clone()).unwrap()
}

fn open_with(r: &Rig, key: Arc<UnlockKey>) -> Result<(), FigLockError> {
    unlock_vault_with_key(
        r.vault_json.clone(),
        key,
        r.device.clone(),
        "test phone".into(),
        vec![r.record.clone()],
        T0 + 10,
    )
    .map(|_| ())
}

#[test]
fn seal_then_open_unlocks_the_vault() {
    let r = rig();
    let chip = FakeChip::new();
    let blob = auk(&r)
        .seal_for_quick_unlock(chip.public(), r.vault_id.clone(), r.device.device_id())
        .unwrap();

    let eph = quick_unlock_ephemeral_public(blob.clone()).unwrap();
    assert_eq!(eph.len(), 65);
    let opened = open_quick_unlock(
        blob,
        chip.public(),
        chip.agree(&eph),
        r.vault_id.clone(),
        r.device.device_id(),
    )
    .unwrap();
    open_with(&r, opened).unwrap();
}

#[test]
fn wrong_key_ids_or_tampered_blob_do_not_open() {
    let r = rig();
    let chip = FakeChip::new();
    let blob = auk(&r)
        .seal_for_quick_unlock(chip.public(), r.vault_id.clone(), r.device.device_id())
        .unwrap();
    let eph = quick_unlock_ephemeral_public(blob.clone()).unwrap();
    let open = |blob: Vec<u8>, chip: &FakeChip, vault: &str, device: &str| {
        open_quick_unlock(
            blob,
            chip.public(),
            chip.agree(&eph),
            vault.into(),
            device.into(),
        )
        .map(|_| ())
    };

    // another chip (a key from a restored backup or another phone)
    assert_eq!(
        open(
            blob.clone(),
            &FakeChip::new(),
            &r.vault_id,
            &r.device.device_id()
        ),
        Err(FigLockError::Failed)
    );
    // another vault, another device
    let other = "00000000000000000000000000000000";
    assert_eq!(
        open(blob.clone(), &chip, other, &r.device.device_id()),
        Err(FigLockError::Failed)
    );
    assert_eq!(
        open(blob.clone(), &chip, &r.vault_id, other),
        Err(FigLockError::Failed)
    );
    // a flipped bit in the sealed part, a wrong version byte, a truncated blob
    let mut flipped = blob.clone();
    *flipped.last_mut().unwrap() ^= 1;
    assert_eq!(
        open(flipped, &chip, &r.vault_id, &r.device.device_id()),
        Err(FigLockError::Failed)
    );
    let mut version = blob.clone();
    version[0] = 2;
    assert!(quick_unlock_ephemeral_public(version).is_err());
    assert!(quick_unlock_ephemeral_public(blob[..40].to_vec()).is_err());
    // malformed ids are input errors, not secret-dependent failures
    assert_eq!(
        open(blob, &chip, "zz", &r.device.device_id()),
        Err(FigLockError::InvalidInput)
    );
}

#[test]
fn a_key_for_another_vault_does_not_unlock() {
    let r = rig();
    let other = rig();
    assert_eq!(open_with(&r, auk(&other)), Err(FigLockError::Failed));
}

#[test]
fn wrong_password_or_secret_key_fails_at_unlock() {
    let r = rig();
    let bad_pw = derive_unlock_key(
        r.vault_json.clone(),
        "not-the-password".into(),
        r.secret_key.clone(),
    )
    .unwrap();
    assert_eq!(open_with(&r, bad_pw), Err(FigLockError::Failed));
    let other = rig();
    let bad_sk = derive_unlock_key(
        r.vault_json.clone(),
        PASSWORD.into(),
        other.secret_key.clone(),
    )
    .unwrap();
    assert_eq!(open_with(&r, bad_sk), Err(FigLockError::Failed));
    assert!(matches!(
        derive_unlock_key(r.vault_json.clone(), PASSWORD.into(), "nonsense".into()),
        Err(FigLockError::InvalidInput)
    ));
}
