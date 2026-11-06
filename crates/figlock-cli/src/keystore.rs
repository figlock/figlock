//! Where the device's private keys live when the platform has a keystore: the macOS login keychain. Other
//! platforms have no backend yet (FIG-71) and keep the keys in `device.json`, owner-only.
//!
//! `FIGLOCK_DEVICE_KEYS=file` keeps them in the file on any platform, for headless machines, CI and tests.

use zeroize::Zeroizing;

use crate::error::Res;

/// A place to keep one small secret per device, found by an account name that is not secret.
pub trait SecretStore {
    fn put(&self, account: &str, secret: &[u8]) -> Res<()>;
    fn get(&self, account: &str) -> Res<Option<Zeroizing<Vec<u8>>>>;
    /// Missing is fine.
    fn delete(&self, account: &str) -> Res<()>;
}

/// The platform keystore, unless `FIGLOCK_DEVICE_KEYS=file` or this platform has none.
pub fn system() -> Option<Box<dyn SecretStore>> {
    if std::env::var("FIGLOCK_DEVICE_KEYS").is_ok_and(|v| v == "file") {
        return None;
    }
    platform()
}

#[cfg(target_os = "macos")]
fn platform() -> Option<Box<dyn SecretStore>> {
    Some(Box::new(Keychain))
}

#[cfg(not(target_os = "macos"))]
fn platform() -> Option<Box<dyn SecretStore>> {
    None
}

#[cfg(target_os = "macos")]
struct Keychain;

#[cfg(target_os = "macos")]
const SERVICE: &str = "com.figlock.cli.device";
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
impl SecretStore for Keychain {
    fn put(&self, account: &str, secret: &[u8]) -> Res<()> {
        security_framework::passwords::set_generic_password(SERVICE, account, secret).map_err(
            |_| crate::error::Fail::new("could not save the device keys in the macOS keychain"),
        )
    }

    fn get(&self, account: &str) -> Res<Option<Zeroizing<Vec<u8>>>> {
        match security_framework::passwords::get_generic_password(SERVICE, account) {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(_) => Err(crate::error::Fail::new(
                "could not read the device keys from the macOS keychain",
            )),
        }
    }

    fn delete(&self, account: &str) -> Res<()> {
        match security_framework::passwords::delete_generic_password(SERVICE, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(_) => Err(crate::error::Fail::new(
                "could not remove the device keys from the macOS keychain",
            )),
        }
    }
}
