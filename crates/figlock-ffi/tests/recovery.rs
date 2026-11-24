//! "I forgot my password" through the exported API: the Recovery Key opens the vault, a new master password and
//! Secret Key replace the old ones, and the old ones stop working. Values are generated.

use std::sync::Arc;

use figlock_core::{KdfParams, RecoveryKey, Vault};
use figlock_ffi::{
    DeviceKeysHandle, FigLockError, recovery_key_is_valid, start_recovery, unlock_vault,
    vault_has_recovery_key,
};

const T0: u64 = 1_800_000_000_000;
const OLD_PASSWORD: &str = "generated-old-password-1";
const NEW_PASSWORD: &str = "generated-new-password-2";

struct World {
    vault_json: Vec<u8>,
    old_secret: String,
    code: String,
    device: Arc<DeviceKeysHandle>,
    record: Vec<u8>,
}

fn keys_bytes(k: &figlock_core::DeviceKeys) -> Vec<u8> {
    let mut out = k.device_id.to_vec();
    out.extend_from_slice(k.sign_seed.expose());
    out.extend_from_slice(k.agree_priv.expose());
    out
}

/// A vault with a Recovery Key, and this phone's own device and record.
fn world() -> World {
    let (mut vault, created) = Vault::create(OLD_PASSWORD, "phone", KdfParams::FLOOR, T0).unwrap();
    let key = RecoveryKey::random().unwrap();
    let change = vault
        .set_recovery_with(&key, OLD_PASSWORD, &created.secret_key)
        .unwrap();
    World {
        vault_json: change.vault_json,
        old_secret: created.secret_key.format().to_string(),
        code: key.format().to_string(),
        device: Arc::new(DeviceKeysHandle::from_bytes(keys_bytes(vault.device_keys())).unwrap()),
        record: created.device_record,
    }
}

fn start(w: &World, code: &str) -> Result<(), FigLockError> {
    start_recovery(
        code.to_owned(),
        w.vault_json.clone(),
        vec![w.record.clone()],
        w.device.clone(),
        "phone".to_owned(),
        T0 + 1,
    )
    .map(|_| ())
}

#[test]
fn the_recovery_key_sets_a_new_password_and_secret_key() {
    let w = world();
    assert!(vault_has_recovery_key(w.vault_json.clone()).unwrap());
    assert!(recovery_key_is_valid(w.code.clone()));
    let attempt = start_recovery(
        w.code.clone(),
        w.vault_json.clone(),
        vec![w.record.clone()],
        w.device.clone(),
        "phone".to_owned(),
        T0 + 1,
    )
    .unwrap();
    let recovered = attempt.prepare(NEW_PASSWORD.to_owned(), T0 + 2).unwrap();
    let new_secret = recovered.secret_key_text();
    assert_ne!(new_secret, w.old_secret);
    // The kit carries the new key, and the forced check is against it.
    assert!(
        !recovered
            .render_kit(
                "My vault".into(),
                "On this iPhone".into(),
                "2026-11-25".into()
            )
            .unwrap()
            .is_empty()
    );
    assert!(recovered.verify_kit_groups(new_secret[new_secret.len() - 9..].to_owned()));
    assert!(!recovered.verify_kit_groups("AAAA-AAAA".to_owned()));
    // This phone's device record was already there, so none is added.
    assert!(recovered.new_record().is_none());

    let stored = recovered.vault_json();
    let opened = unlock_vault(
        stored.clone(),
        NEW_PASSWORD.to_owned(),
        new_secret,
        w.device.clone(),
        "phone".to_owned(),
        vec![w.record.clone()],
        T0 + 3,
    );
    assert!(opened.is_ok());
    // The old password and Secret Key no longer open the vault.
    let old = unlock_vault(
        stored,
        OLD_PASSWORD.to_owned(),
        w.old_secret.clone(),
        w.device.clone(),
        "phone".to_owned(),
        vec![w.record.clone()],
        T0 + 3,
    );
    assert!(matches!(old, Err(FigLockError::Failed)));
    // The session can be taken once.
    assert!(recovered.take_session().is_ok());
    assert!(matches!(
        recovered.take_session(),
        Err(FigLockError::Locked)
    ));
}

#[test]
fn a_wrong_or_malformed_recovery_key_is_refused() {
    let w = world();
    let other = RecoveryKey::random().unwrap().format().to_string();
    assert!(matches!(start(&w, &other), Err(FigLockError::Failed)));
    assert!(matches!(
        start(&w, "not a recovery key"),
        Err(FigLockError::InvalidInput)
    ));
    assert!(!recovery_key_is_valid("not a recovery key".into()));
    // A tampered vault.json is refused too, whatever the key.
    let mut tampered = world();
    let mid = tampered.vault_json.len() / 2;
    tampered.vault_json[mid] ^= 0x01;
    let code = tampered.code.clone();
    assert!(start(&tampered, &code).is_err());
}

#[test]
fn a_vault_without_a_recovery_key_says_so() {
    let (_, created) = Vault::create(OLD_PASSWORD, "phone", KdfParams::FLOOR, T0).unwrap();
    assert!(!vault_has_recovery_key(created.vault_json).unwrap());
}
