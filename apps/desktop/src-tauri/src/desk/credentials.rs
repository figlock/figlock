//! Changing the master password and rotating the Secret Key without revoking a device (ADR-0022, ADR-0027).
//! Both rewrap the same vault key under new credentials, so they ask for the master password and Secret Key
//! again instead of trusting an open session. A rotation keeps the new key here until the person has saved the
//! new Emergency Kit; other devices find out at their next unlock (`settings_changed`) and ask for the new key.

use figlock_core::{CredentialChange, Vault};
use figlock_session::session::now_ms;
use figlock_session::{quick, secret_key_store};
use serde::Serialize;

use super::devices::{credentials_key, revoke_credentials_error};
use super::{CmdError, CmdRes, Desk, MIN_PASSWORD_CHARS, Reissued};

/// What a credential change hands back to the window.
#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    /// The new Secret Key, shown for the Emergency Kit. Only set by a rotation.
    pub secret_key: Option<String>,
}

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

impl Desk {
    /// Set a new master password, a new Secret Key, or both. `secret_key` is blank when this device keeps it.
    /// A rotation leaves the new key to be confirmed with `check_kit_key` and cleared by `end_rotation`.
    pub fn change_credentials(
        &self,
        password: &str,
        secret_key: Option<&str>,
        new_password: Option<&str>,
        rotate: bool,
    ) -> CmdRes<Changed> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        if new_password.is_none() && !rotate {
            return Err(CmdError::new("failed", "Choose what to change."));
        }
        if let Some(p) = new_password
            && p.chars().count() < MIN_PASSWORD_CHARS
        {
            return Err(CmdError::new(
                "weak_password",
                "Use at least 8 characters for the new master password.",
            ));
        }
        let (secret_key, typed) = credentials_key(secret_key, "change your credentials")?;
        let vault_id = figlock_session::session::peek_vault_id()?;
        let touch_was_on = quick::system().is_ok_and(|q| q.status(&vault_id).0);
        // Other devices may have changed the vault since the last sync; rewrapping from a stale view would
        // drop their changes to the vault's settings.
        self.run_sync(false);
        let mut inner = self.inner();
        let session = inner.session.as_mut().ok_or_else(locked)?;
        let refreshed = session.refresh()?;
        if refreshed.settings_changed || refreshed.stopped {
            return Err(CmdError::new(
                "needs_unlock",
                "Another device changed this vault. Lock and unlock, then try again.",
            ));
        }
        let changed = session
            .vault
            .change_credentials(
                password,
                &secret_key,
                CredentialChange {
                    new_password,
                    rotate_secret_key: rotate,
                    ..CredentialChange::default()
                },
            )
            .map_err(|_| revoke_credentials_error(typed))?;
        if session.replace_vault_json(&changed.vault_json).is_err() {
            // This vault already moved on and storage may or may not have the change: lock, so the next unlock
            // reads what is really there.
            inner.drop_session();
            return Err(CmdError::new(
                "change_uncertain",
                "The change may be only partly saved, so the vault was locked. Unlock it again; if the old password still works, repeat the change.",
            ));
        }
        inner.cache = None;
        inner.dirty = true;
        // The key a quick unlock would need to keep working here: the new credentials' Account Unlock Key.
        let unlock_key = touch_was_on
            .then(|| {
                let sk = changed.secret_key.as_ref().unwrap_or(&secret_key);
                Vault::unlock_key(&changed.vault_json, new_password.unwrap_or(password), sk).ok()
            })
            .flatten();
        let new_key = changed.secret_key;
        let shown = new_key.as_ref().map(|k| k.format().to_string());
        if let Some(key) = new_key {
            // The old key opens nothing now: keep the new one here in its place (ADR-0027). When that fails
            // the old one must not stay, or unlock would use it and never ask for the key.
            if let Ok(keys) = secret_key_store::system()
                && !(keys.available() && keys.put(&vault_id, &key).is_ok())
            {
                let _ = keys.forget(&vault_id);
            }
            inner.reissued = Some(Reissued {
                key,
                confirmed: false,
            });
        }
        drop(inner);
        // The quick unlock key opened the old credentials; it must not outlive them. This device just proved
        // the new ones, so Touch ID stays on here; failing to turn it back on only costs one typed unlock.
        if let Ok(q) = quick::system() {
            let _ = q.forget(&vault_id);
            if let Some(key) = unlock_key {
                let _ = q.enable(&vault_id, &key, now_ms() / 1000);
            }
        }
        Ok(Changed { secret_key: shown })
    }

    /// Close the new-Emergency-Kit step. Refused until the person typed the end of the new key back.
    pub fn end_rotation(&self) -> CmdRes<()> {
        let mut inner = self.inner();
        match inner.reissued.as_ref() {
            Some(r) if !r.confirmed => Err(CmdError::new(
                "key_not_confirmed",
                "Confirm your new Secret Key before you continue.",
            )),
            _ => {
                inner.reissued = None;
                Ok(())
            }
        }
    }
}
