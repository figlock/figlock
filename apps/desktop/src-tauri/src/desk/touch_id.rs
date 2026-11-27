//! Touch ID unlock from Settings (FIG-141): turn it off, or on again, for a vault that is already open.
//!
//! Turning it off drops the quick unlock key and its policy state and nothing else: the Secret Key stays kept
//! on this Mac in the login keychain (ADR-0034, ADR-0037), so the password is still all that is typed.
//! Turning it on needs the password, because the quick unlock key is derived from it.

use figlock_core::Vault;
use figlock_session::quick;
use figlock_session::session::{now_ms, peek_vault_id, peek_vault_json};

use super::{CmdError, CmdRes, Desk, VaultState, wrong_credentials};

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

impl Desk {
    /// Stop unlocking with Touch ID. The Unlock screen then shows the password only.
    pub fn turn_touch_id_off(&self) -> CmdRes<VaultState> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        quick::system()?.forget(&peek_vault_id()?)?;
        self.state()
    }

    /// Unlock with Touch ID from now on. The password confirms it; the Secret Key comes from the one
    /// typed, held by this window or kept on this Mac, and is asked for only when none of them exists.
    pub fn turn_touch_id_on(&self, password: &str, secret_key: &str) -> CmdRes<VaultState> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        if password.is_empty() {
            return Err(CmdError::new("empty_password", "Enter your password."));
        }
        let q = quick::system()?;
        if !q.available() {
            return Err(CmdError::new(
                "touch_unavailable",
                "This Mac cannot ask for Touch ID or the device password.",
            ));
        }
        let (key, _) = self.credentials_key(Some(secret_key), "turn on Touch ID")?;
        let vault_json = peek_vault_json()?;
        let auk =
            Vault::unlock_key(&vault_json, password, &key).map_err(|_| wrong_credentials())?;
        q.enable(&peek_vault_id()?, &auk, now_ms() / 1000)?;
        self.state()
    }
}
