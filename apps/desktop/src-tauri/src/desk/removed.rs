//! A device the vault has removed (FIG-135, `pairing-recovery.md` 4). Opening the vault already tells the window
//! ([`VaultState::removed_at_ms`](super::VaultState)) and every write is refused; what is left here is what the
//! person can still do: stop using the vault on this Mac, then join again with a new invite.

use figlock_session::config;
use figlock_session::session::peek_vault_id;
use figlock_session::{quick, secret_key_store};

use super::items::locked;
use super::{CmdError, CmdRes, Desk};

impl Desk {
    /// Stop using the vault on this Mac after the vault removed it: lock, forget this device's config and keys
    /// and the Secret Key and Touch ID entries kept for the vault. The vault folder and the Google Drive copy are
    /// left alone, and a copy already read stays only where the person put it. Refused unless the vault really
    /// removed this device, so this is not a back door around the missing-folder confirmation.
    pub fn leave_vault(&self) -> CmdRes<()> {
        let mut inner = self.inner();
        let removed = inner
            .session
            .as_ref()
            .ok_or_else(locked)?
            .removed_at_ms()
            .is_some();
        if !removed {
            return Err(CmdError::new(
                "not_removed",
                "This Mac has not been removed from the vault.",
            ));
        }
        let vault = peek_vault_id().ok();
        inner.drop_session();
        drop(inner);
        if let Some(vault) = vault {
            if let Ok(keys) = secret_key_store::system() {
                let _ = keys.forget(&vault);
            }
            if let Ok(q) = quick::system() {
                let _ = q.forget(&vault);
            }
        }
        config::forget();
        Ok(())
    }
}
