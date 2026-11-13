//! The device list and revoking a device (ADR-0018, ADR-0022 amendment). Revoking rotates the vault key, so
//! it asks for the master password and Secret Key again instead of trusting an open session. What crosses to
//! the window is names, fingerprints and dates, never keys.

use figlock_core::{CredentialChange, DeviceId, DeviceInfo, SecretKey};
use figlock_session::session::now_ms;
use figlock_session::{quick, secret_key_store};
use serde::Serialize;

use super::{CmdError, CmdRes, Desk, KeyFrom, parse_secret_key, wrong_credentials, wrong_password};

/// One row of the device list.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRow {
    /// The full id; the window sends it back to revoke.
    pub id: String,
    /// Unknown for a revoked device: its record is not re-signed.
    pub name: Option<String>,
    /// Short fingerprint to compare with what the other device shows.
    pub fingerprint: String,
    pub this_device: bool,
    pub revoked: bool,
}

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

fn row(d: &DeviceInfo, this: &DeviceId) -> DeviceRow {
    DeviceRow {
        id: hex::encode(d.device_id),
        name: d.name.clone(),
        fingerprint: d.fingerprint.clone(),
        this_device: d.device_id == *this,
        revoked: d.revoked_at.is_some(),
    }
}

/// Credentials that did not open the vault for a step that rewraps the vault key. A kept key that fails cannot
/// be told apart from a wrong password, so the window is asked to show the key field; a typed key that fails is
/// a plain mismatch of the pair; a held key was never asked for, so only the password is blamed.
pub(super) fn revoke_credentials_error(from: KeyFrom) -> CmdError {
    match from {
        KeyFrom::Typed => wrong_credentials(),
        KeyFrom::Held => wrong_password(),
        KeyFrom::Kept => CmdError::new(
            "needs_secret_key",
            "The kept Secret Key or the password did not open the vault. If the password is right, the Secret Key was changed: type the new one from your Emergency Kit.",
        ),
    }
}

impl Desk {
    /// The Secret Key for a step that rewraps the vault key, in the order of ADR-0030: typed in the window,
    /// held by this unlocked session, or the one this device keeps (after Touch ID). The flag says the key
    /// came from, which decides how a failure reads. The window asks for the key only when none of the three
    /// exists.
    pub(super) fn credentials_key(
        &self,
        typed: Option<&str>,
        why: &str,
    ) -> CmdRes<(SecretKey, KeyFrom)> {
        let typed = typed.map(str::trim).filter(|k| !k.is_empty());
        if let Some(text) = typed {
            return Ok((parse_secret_key(text)?, KeyFrom::Typed));
        }
        if let Some(held) = self
            .inner()
            .held_key
            .as_ref()
            .map(|k| SecretKey::from_bytes(*k.expose()))
        {
            return Ok((held, KeyFrom::Held));
        }
        let vault = figlock_session::session::peek_vault_id()?;
        let key = secret_key_store::system()?
            .get(&vault, why)?
            .ok_or_else(|| {
                CmdError::new(
                    "needs_secret_key",
                    "Type your Secret Key from your Emergency Kit to continue.",
                )
            })?;
        Ok((key, KeyFrom::Kept))
    }
}

impl Desk {
    /// The devices that can open this vault, then the revoked ones. This device comes first.
    pub fn devices(&self) -> CmdRes<Vec<DeviceRow>> {
        let inner = self.inner();
        let session = inner.session.as_ref().ok_or_else(locked)?;
        let this = *session.vault.device_id();
        let mut rows: Vec<DeviceRow> = session
            .vault
            .devices()
            .iter()
            .map(|d| row(d, &this))
            .collect();
        // Live devices before revoked ones, this device first; the sort is stable so the core's order stays.
        rows.sort_by_key(|r| (r.revoked, !r.this_device));
        Ok(rows)
    }

    /// Stop `device` from opening anything written from now on. `new_password` also changes the master
    /// password in the same write, which is what to do when the device may have been compromised. Needs the
    /// credentials again: the vault key is rewrapped under them.
    pub fn revoke_device(
        &self,
        device: &str,
        password: &str,
        secret_key: Option<&str>,
        new_password: Option<&str>,
    ) -> CmdRes<Vec<DeviceRow>> {
        let (secret_key, from) = self.credentials_key(secret_key, "revoke a device")?;
        let target: DeviceId = hex::decode(device)
            .ok()
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| CmdError::new("failed", "That device is not in this vault."))?;
        if let Some(p) = new_password
            && p.chars().count() < super::MIN_PASSWORD_CHARS
        {
            return Err(CmdError::new(
                "weak_password",
                "Use at least 8 characters for the new master password.",
            ));
        }
        // Other devices may have changed the vault since the last sync; a revocation built on a stale view
        // would drop their records.
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
        let this = *session.vault.device_id();
        let known = session
            .vault
            .devices()
            .into_iter()
            .any(|d| d.device_id == target && d.revoked_at.is_none());
        if !known {
            return Err(CmdError::new(
                "failed",
                "That device is not in this vault any more.",
            ));
        }
        if target == this {
            return Err(CmdError::new(
                "failed",
                "This is the device you are using. Revoke it from another device.",
            ));
        }
        let now = now_ms();
        let revocation = match new_password {
            Some(p) => session.vault.revoke_device_and_change_credentials(
                password,
                &secret_key,
                &target,
                CredentialChange {
                    new_password: Some(p),
                    ..CredentialChange::default()
                },
                now,
            ),
            None => session
                .vault
                .revoke_device(password, &secret_key, &target, now),
        }
        .map_err(|_| revoke_credentials_error(from))?;
        if session.write_revocation(&revocation).is_err() {
            // Part of the revocation may be stored and this vault already moved on: lock, so the next unlock
            // reads what is really there, and let the person look at the list again.
            inner.drop_session();
            return Err(CmdError::new(
                "revoke_uncertain",
                "The revocation may be only partly saved, so the vault was locked. Unlock it again and check the device list.",
            ));
        }
        inner.cache = None;
        inner.dirty = true;
        drop(inner);
        if new_password.is_some() {
            // The quick unlock key opened the old credentials; it must not outlive them.
            if let (Ok(vault), Ok(q)) = (figlock_session::session::peek_vault_id(), quick::system())
            {
                let _ = q.forget(&vault);
            }
        }
        self.devices()
    }
}
