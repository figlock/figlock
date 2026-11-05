use figlock_core::{DeviceKeys, Key};
use zeroize::Zeroizing;

use crate::error::{FigLockError, Result, id_hex};

/// Length of the serialised device keys: device id (16) + signing seed (32) + agreement key (32).
const DEVICE_KEYS_LEN: usize = 16 + 32 + 32;

/// This device's private keys. The platform keystore (Keychain, Android Keystore) owns the stored copy;
/// this handle only carries them into `unlock`. The bytes cross the boundary only through `to_bytes`, which
/// exists so the app can put them in the keystore, and `from_bytes`, which loads them back.
#[derive(uniffi::Object)]
pub struct DeviceKeysHandle {
    id: [u8; 16],
    sign_seed: Key,
    agree_priv: Key,
}

impl DeviceKeysHandle {
    pub(crate) fn from_core(keys: &DeviceKeys) -> Self {
        DeviceKeysHandle {
            id: keys.device_id,
            sign_seed: Key::from_bytes(*keys.sign_seed.expose()),
            agree_priv: Key::from_bytes(*keys.agree_priv.expose()),
        }
    }

    /// A fresh `DeviceKeys` for one `unlock` call; core takes ownership, so the handle keeps its own copy.
    pub(crate) fn to_core(&self) -> DeviceKeys {
        DeviceKeys {
            device_id: self.id,
            sign_seed: Key::from_bytes(*self.sign_seed.expose()),
            agree_priv: Key::from_bytes(*self.agree_priv.expose()),
        }
    }
}

#[uniffi::export]
impl DeviceKeysHandle {
    /// New random device keys for first use of the app on this device.
    #[uniffi::constructor]
    pub fn generate() -> Result<Self> {
        let keys = DeviceKeys::generate()?;
        Ok(Self::from_core(&keys))
    }

    /// Load keys previously saved with `to_bytes`. Any other length is `InvalidInput`.
    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let bytes = Zeroizing::new(bytes);
        if bytes.len() != DEVICE_KEYS_LEN {
            return Err(FigLockError::InvalidInput);
        }
        let id: [u8; 16] = bytes[..16]
            .try_into()
            .map_err(|_| FigLockError::InvalidInput)?;
        Ok(DeviceKeysHandle {
            id,
            sign_seed: Key::from_slice(&bytes[16..48])?,
            agree_priv: Key::from_slice(&bytes[48..])?,
        })
    }

    /// The private key material, for the platform keystore only. The copy on the foreign side is the
    /// caller's to wipe.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DEVICE_KEYS_LEN);
        out.extend_from_slice(&self.id);
        out.extend_from_slice(self.sign_seed.expose());
        out.extend_from_slice(self.agree_priv.expose());
        out
    }

    /// The public device id, 32 hex characters.
    pub fn device_id(&self) -> String {
        id_hex(&self.id)
    }
}
