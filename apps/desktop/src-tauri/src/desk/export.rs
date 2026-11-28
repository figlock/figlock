//! Exporting the vault as JSON (FIG-287). The document is the one `figlock export` writes, built by
//! `figlock_core::export_json`. Without secrets it needs nothing but an unlocked vault. With secrets every
//! export asks again, because an unlocked window left alone must not be enough to read every password out:
//! Touch ID or the device password when this Mac keeps the Secret Key behind quick unlock, otherwise the
//! account password. The text stays here: it goes to the file the person chose and is dropped, never to the window.

use figlock_core::Vault;
use figlock_session::secret_key_store;
use figlock_session::session::peek_vault_id;
use zeroize::Zeroizing;

use super::{CmdError, CmdRes, Desk, wrong_credentials};

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

impl Desk {
    /// The export document. `password` (with `secret_key`, when this Mac keeps none) is the proof for
    /// `include_secrets`; without it the proof is a Touch ID or device-password prompt, which exists only
    /// while quick unlock is on. Nothing is returned unless the proof holds.
    pub fn export_vault(
        &self,
        include_secrets: bool,
        include_deleted: bool,
        password: Option<&str>,
        secret_key: Option<&str>,
    ) -> CmdRes<Zeroizing<String>> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        if include_secrets {
            self.prove_it_is_them(password, secret_key)?;
        }
        let mut inner = self.inner();
        let session = inner.session.as_mut().ok_or_else(locked)?;
        let items = session.items(include_deleted);
        figlock_core::export_json(&items, include_secrets)
            .ok_or_else(|| CmdError::new("failed", "The export could not be made."))
    }

    fn prove_it_is_them(&self, password: Option<&str>, secret_key: Option<&str>) -> CmdRes<()> {
        let vault = peek_vault_id()?;
        if let Some(password) = password.filter(|p| !p.is_empty()) {
            let (key, _) =
                self.credentials_key(secret_key, "export your vault with its secrets")?;
            let vault_json = figlock_session::session::peek_vault_json()?;
            return Vault::verify_credentials(&vault_json, password, &key)
                .map_err(|_| wrong_credentials());
        }
        let keys = secret_key_store::system()?;
        let quick_on = secret_key_store::quick_on(&vault);
        if !quick_on || !keys.has(&vault) {
            return Err(CmdError::new(
                "needs_password",
                "Type your password to export with secrets.",
            ));
        }
        match keys.get(&vault, "export your FigLock vault with its secrets", true)? {
            Some(_) => Ok(()),
            None => Err(CmdError::new(
                "not_confirmed",
                "Nothing was exported because Touch ID or the device password was not confirmed.",
            )),
        }
    }
}
