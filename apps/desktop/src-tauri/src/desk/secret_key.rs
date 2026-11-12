//! The Secret Key kept on this device (ADR-0027): showing it again and letting it go. Reading it asks for Touch
//! ID or the device password, so a person at an unlocked, unattended Mac cannot read it out.

use figlock_session::secret_key_store;
use figlock_session::session::peek_vault_id;

use super::{CmdError, CmdRes, Desk, VaultState};

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

fn not_kept() -> CmdError {
    CmdError::new(
        "no_secret_key",
        "This Mac is not keeping your Secret Key. It is on your Emergency Kit, and it is kept again the next time you unlock with it typed.",
    )
}

impl Desk {
    /// The Secret Key, for the person to read or copy. Needs an unlocked vault and a confirmed prompt.
    pub fn show_secret_key(&self) -> CmdRes<String> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        let vault = peek_vault_id()?;
        let keys = secret_key_store::system()?;
        if !keys.has(&vault) {
            return Err(not_kept());
        }
        let key = keys.get(&vault, "show your FigLock Secret Key")?;
        match key {
            Some(k) => Ok(k.format().to_string()),
            None => Err(CmdError::new(
                "not_confirmed",
                "The Secret Key was not shown because Touch ID or the device password was not confirmed.",
            )),
        }
    }

    /// Stop keeping the Secret Key on this Mac. The next full unlock asks for it.
    pub fn forget_secret_key(&self) -> CmdRes<VaultState> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        secret_key_store::system()?.forget(&peek_vault_id()?)?;
        self.state()
    }
}
