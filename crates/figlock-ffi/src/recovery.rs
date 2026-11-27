//! "I forgot my password" (`pairing-recovery.md` 7.3, ADR-0029): open the vault with a Recovery Key, choose a new
//! password and get a new Secret Key. The same `Vault::recover` the desktop uses; nothing is stored here.
//! The caller stores the returned files only after the new Emergency Kit was saved and confirmed.

use std::sync::Arc;

use figlock_core::{
    DeviceKeys, KitInfo, Recover, Recovered, RecoveryKey, SecretKey, Vault, render_kit,
    render_recovery_kit, verify_kit_groups,
};
use zeroize::Zeroizing;

use crate::device::DeviceKeysHandle;
use crate::error::{FigLockError, Result};
use crate::session::VaultSession;

/// Whether `vault.json` carries a Recovery Key, so the screen can say "this vault has none" before asking.
#[uniffi::export]
pub fn vault_has_recovery_key(vault_json: Vec<u8>) -> Result<bool> {
    Vault::recovery_in(&vault_json).map_err(|_| FigLockError::InvalidInput)
}

/// Whether `code` is a well formed Recovery Key code, so a typo is flagged before any storage access.
#[uniffi::export]
pub fn recovery_key_is_valid(code: String) -> bool {
    let code = Zeroizing::new(code);
    RecoveryKey::parse(&code).is_ok()
}

/// A Recovery Key that opened the vault, waiting for the new credentials. Holds keys in memory only.
#[derive(uniffi::Object)]
pub struct RecoveryAttempt {
    vault_json: Vec<u8>,
    records: Vec<Vec<u8>>,
    key: RecoveryKey,
    device: Arc<DeviceKeysHandle>,
    device_name: String,
}

/// The new credentials, made in memory. Store `vault_json` (and `new_record` when there is one) only after the
/// person confirmed the new Emergency Kit.
#[derive(uniffi::Object)]
pub struct RecoveredVault {
    out: Recovered,
    secret: SecretKey,
    session: std::sync::Mutex<Option<Vault>>,
    device: Arc<DeviceKeysHandle>,
}

#[uniffi::export]
impl RecoveredVault {
    /// The new `vault.json`, to replace the stored one.
    pub fn vault_json(&self) -> Vec<u8> {
        self.out.vault_json.clone()
    }

    /// This device's record when the vault had none (`devices/<device id>.json`), else nothing.
    pub fn new_record(&self) -> Option<Vec<u8>> {
        self.out.new_record.clone()
    }

    pub fn device_keys(&self) -> Arc<DeviceKeysHandle> {
        self.device.clone()
    }

    /// The new Secret Key as `FL1-...`, for the screen and the keychain.
    pub fn secret_key_text(&self) -> String {
        self.secret.format().to_string()
    }

    /// The new Emergency Kit as a one-page PDF. `created` is `YYYY-MM-DD`.
    pub fn render_kit(
        &self,
        vault_name: String,
        storage: String,
        created: String,
    ) -> Result<Vec<u8>> {
        let info = KitInfo {
            vault_name: &vault_name,
            storage: &storage,
            created: &created,
        };
        let pdf: Zeroizing<Vec<u8>> =
            render_kit(&info, &self.secret).map_err(|_| FigLockError::InvalidInput)?;
        Ok(pdf.to_vec())
    }

    /// The forced check that the new kit was saved: the last two groups the person typed.
    pub fn verify_kit_groups(&self, typed: String) -> bool {
        let typed = Zeroizing::new(typed);
        verify_kit_groups(&self.secret, &typed)
    }

    /// Take the unlocked session. Works once; afterwards it is `Locked`.
    pub fn take_session(&self) -> Result<Arc<VaultSession>> {
        let vault = self
            .session
            .lock()
            .map_err(|_| FigLockError::Failed)?
            .take()
            .ok_or(FigLockError::Locked)?;
        Ok(Arc::new(VaultSession::new(vault)))
    }
}

#[uniffi::export]
impl RecoveryAttempt {
    /// Choose the new password and make the new Secret Key. The old Recovery Key keeps working (no new
    /// one is made on the phone). `Failed` when the key no longer opens the vault.
    pub fn prepare(&self, new_password: String, now_ms: u64) -> Result<Arc<RecoveredVault>> {
        let new_password = Zeroizing::new(new_password);
        let (vault, out) = Vault::recover(
            Recover {
                vault_json: &self.vault_json,
                recovery_key: &self.key,
                new_password: &new_password,
                new_recovery_key: None,
            },
            self.device.to_core(),
            &self.device_name,
            &self.records,
            now_ms,
        )?;
        let secret = SecretKey::from_bytes(*out.secret_key.expose());
        Ok(Arc::new(RecoveredVault {
            out,
            secret,
            session: std::sync::Mutex::new(Some(vault)),
            device: self.device.clone(),
        }))
    }
}

/// Prove that the Recovery Key `code` opens the vault (a trial run with a throwaway password) and hold it for
/// `prepare`. One `InvalidInput` for a malformed code, `Failed` when it does not open this vault, so a stranger's
/// key cannot be told apart from a typo in the vault. `records` are the files of `devices/`.
#[uniffi::export]
pub fn start_recovery(
    code: String,
    vault_json: Vec<u8>,
    records: Vec<Vec<u8>>,
    device: Arc<DeviceKeysHandle>,
    device_name: String,
    now_ms: u64,
) -> Result<Arc<RecoveryAttempt>> {
    let code = Zeroizing::new(code);
    let key = RecoveryKey::parse(&code).map_err(|_| FigLockError::InvalidInput)?;
    let throwaway = SecretKey::random()?.format();
    Vault::recover(
        Recover {
            vault_json: &vault_json,
            recovery_key: &key,
            new_password: throwaway.as_str(),
            new_recovery_key: None,
        },
        device.to_core(),
        &device_name,
        &records,
        now_ms,
    )?;
    Ok(Arc::new(RecoveryAttempt {
        vault_json,
        records,
        key,
        device,
        device_name,
    }))
}

/// A new Recovery Key made for Settings (`pairing-recovery.md` 7, ADR-0029): shown once as a code, checked by a
/// forced re-entry, and only then written into `vault.json` by `VaultSession::set_recovery_key`. Never stored.
#[derive(uniffi::Object)]
pub struct RecoveryDraft {
    key: RecoveryKey,
}

/// Make a new Recovery Key to show. Nothing changes in any vault until it is applied.
#[uniffi::export]
pub fn new_recovery_draft() -> Result<Arc<RecoveryDraft>> {
    let key = RecoveryKey::random().map_err(|_| FigLockError::Failed)?;
    Ok(Arc::new(RecoveryDraft { key }))
}

/// The last two groups of `code` and a typed answer, squashed the way a typed answer is (case, dashes, spaces
/// and the look-alike characters).
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            u => u,
        })
        .collect()
}

#[uniffi::export]
impl RecoveryDraft {
    /// The code, `FLR1-...`, for the screen.
    pub fn code(&self) -> String {
        self.key.format().to_string()
    }

    /// The forced check that the code was saved: its last two groups, as typed.
    pub fn verify_tail(&self, typed: String) -> bool {
        let typed = Zeroizing::new(typed);
        let code = self.key.format();
        let groups: Vec<&str> = code.split('-').collect();
        let tail = groups[groups.len().saturating_sub(2)..].concat();
        squash(&typed) == squash(&tail)
    }

    /// The printable Recovery Key page as a PDF. `created` is `YYYY-MM-DD`.
    pub fn render_kit(
        &self,
        vault_name: String,
        storage: String,
        created: String,
    ) -> Result<Vec<u8>> {
        let info = KitInfo {
            vault_name: &vault_name,
            storage: &storage,
            created: &created,
        };
        let code = self.key.format();
        let pdf = render_recovery_kit(&info, &code).map_err(|_| FigLockError::InvalidInput)?;
        Ok(pdf.to_vec())
    }
}

#[uniffi::export]
impl VaultSession {
    /// Whether this vault has a Recovery Key (`rk_pub` in its `vault.json`).
    pub fn has_recovery_key(&self) -> Result<bool> {
        self.with_vault(|v| Ok(v.has_recovery()))
    }

    /// Turn the Recovery Key on, or replace it with `draft` (ADR-0029). The password and Secret Key are
    /// checked by opening the Vault Key; a wrong pair is `Failed`, a malformed Secret Key `InvalidInput`, and
    /// nothing changes. Returns the new `vault.json` for the caller to store.
    pub fn set_recovery_key(
        &self,
        draft: Arc<RecoveryDraft>,
        password: String,
        secret_key: String,
    ) -> Result<Vec<u8>> {
        let password = Zeroizing::new(password);
        let secret_key = Zeroizing::new(secret_key);
        let secret = SecretKey::parse(&secret_key).map_err(|_| FigLockError::InvalidInput)?;
        self.with_vault(|v| {
            Ok(v.set_recovery_with(&draft.key, &password, &secret)?
                .vault_json)
        })
    }

    /// Turn the Recovery Key off. Removing it needs no credentials of the vault, so this is a downgrade the
    /// password and Secret Key are proven for first, against the stored `vault_json`. `Failed` for a
    /// wrong pair or a vault with no Recovery Key. Returns the new `vault.json` for the caller to store.
    pub fn clear_recovery_key(
        &self,
        vault_json: Vec<u8>,
        password: String,
        secret_key: String,
    ) -> Result<Vec<u8>> {
        let password = Zeroizing::new(password);
        let secret_key = Zeroizing::new(secret_key);
        let secret = SecretKey::parse(&secret_key).map_err(|_| FigLockError::InvalidInput)?;
        let auk = Vault::unlock_key(&vault_json, &password, &secret)?;
        Vault::unlock_with_key(&vault_json, &auk, DeviceKeys::generate()?, "check", &[], 0)?;
        self.with_vault(|v| Ok(v.clear_recovery()?.vault_json))
    }
}
