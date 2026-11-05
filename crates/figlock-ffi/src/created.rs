use std::sync::{Arc, Mutex};

use figlock_core::{Created, KdfParams, KitInfo, SecretKey, Vault, render_kit, verify_kit_groups};
use zeroize::Zeroizing;

use crate::device::DeviceKeysHandle;
use crate::error::{FigLockError, Result};
use crate::session::VaultSession;

/// Argon2id cost settings stored in `vault.json`. Core refuses anything outside its floor and ceiling.
#[derive(uniffi::Record)]
pub struct KdfSettings {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl From<KdfParams> for KdfSettings {
    fn from(p: KdfParams) -> Self {
        KdfSettings {
            m_kib: p.m_kib,
            t: p.t,
            p: p.p,
        }
    }
}

/// The profile to use for a real vault.
#[uniffi::export]
pub fn default_kdf_settings() -> KdfSettings {
    KdfParams::DEFAULT.into()
}

/// The weakest profile core accepts. For tests and very constrained devices only.
#[uniffi::export]
pub fn minimum_kdf_settings() -> KdfSettings {
    KdfParams::FLOOR.into()
}

struct Pending {
    vault: Option<Vault>,
    created: Created,
}

/// A freshly created vault: the files to store, the Secret Key for the Emergency Kit, and the unlocked
/// session. The Secret Key never leaves this handle except inside the kit PDF or through
/// `secret_key_text`, which exists for the on-screen "write this down" step.
#[derive(uniffi::Object)]
pub struct CreatedVault {
    inner: Mutex<Pending>,
    device: Arc<DeviceKeysHandle>,
}

#[uniffi::export]
impl CreatedVault {
    /// Contents of `vault.json`, to store at that path.
    pub fn vault_json(&self) -> Result<Vec<u8>> {
        Ok(self.lock()?.created.vault_json.clone())
    }

    /// Contents of this device's record, to store at `devices/<device id>.json`.
    pub fn device_record(&self) -> Result<Vec<u8>> {
        Ok(self.lock()?.created.device_record.clone())
    }

    /// This device's new private keys; the app must save them in the platform keystore.
    pub fn device_keys(&self) -> Arc<DeviceKeysHandle> {
        self.device.clone()
    }

    /// The Secret Key as `FL1-XXXX-...`. Reveal only to the user, once, during setup.
    pub fn secret_key_text(&self) -> Result<String> {
        Ok(self.lock()?.created.secret_key.format().to_string())
    }

    /// The Emergency Kit as a one-page PDF. `created` is `YYYY-MM-DD`.
    pub fn render_kit(
        &self,
        vault_name: String,
        storage: String,
        created: String,
    ) -> Result<Vec<u8>> {
        let st = self.lock()?;
        let info = KitInfo {
            vault_name: &vault_name,
            storage: &storage,
            created: &created,
        };
        let pdf: Zeroizing<Vec<u8>> =
            render_kit(&info, &st.created.secret_key).map_err(|_| FigLockError::InvalidInput)?;
        Ok(pdf.to_vec())
    }

    /// The forced check that the user saved the kit: do the last two groups they typed match?
    pub fn verify_kit_groups(&self, typed: String) -> Result<bool> {
        let typed = Zeroizing::new(typed);
        Ok(verify_kit_groups(&self.lock()?.created.secret_key, &typed))
    }

    /// Take the unlocked session. Works once; afterwards it is `Locked`.
    pub fn take_session(&self) -> Result<Arc<VaultSession>> {
        let vault = self.lock()?.vault.take().ok_or(FigLockError::Locked)?;
        Ok(Arc::new(VaultSession::new(vault)))
    }
}

impl CreatedVault {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Pending>> {
        self.inner.lock().map_err(|_| FigLockError::Failed)
    }
}

/// Create a vault and its first device. Nothing is written anywhere; the caller stores the files.
#[uniffi::export]
pub fn create_vault(
    password: String,
    device_name: String,
    kdf: KdfSettings,
    now_ms: u64,
) -> Result<Arc<CreatedVault>> {
    let password = Zeroizing::new(password);
    let params = KdfParams {
        m_kib: kdf.m_kib,
        t: kdf.t,
        p: kdf.p,
    };
    let (vault, created) = Vault::create(&password, &device_name, params, now_ms)?;
    let device = Arc::new(DeviceKeysHandle::from_core(vault.device_keys()));
    Ok(Arc::new(CreatedVault {
        inner: Mutex::new(Pending {
            vault: Some(vault),
            created,
        }),
        device,
    }))
}

/// Parse Secret Key text and report whether it is well formed (prefix, length, check digits), so the UI can
/// flag a typo before the slow unlock.
#[uniffi::export]
pub fn secret_key_is_valid(text: String) -> bool {
    let text = Zeroizing::new(text);
    SecretKey::parse(&text).is_ok()
}
