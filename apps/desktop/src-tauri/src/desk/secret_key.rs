//! The Secret Key kept on this device (ADR-0027): showing it again and letting it go. Reading it asks for Touch
//! ID or the device password, so a person at an unlocked, unattended Mac cannot read it out.

use figlock_core::{SecretKey, Vault};
use figlock_session::kit;
use figlock_session::secret_key_store;
use figlock_session::session::{now_ms, peek_vault_id};
use zeroize::Zeroizing;

use super::{CmdError, CmdRes, Desk, VaultState, parse_secret_key, wrong_credentials};

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
        let key = keys.get(
            &vault,
            "show your FigLock Secret Key",
            secret_key_store::quick_on(&vault),
        )?;
        match key {
            Some(k) => Ok(k.format().to_string()),
            None => Err(CmdError::new(
                "not_confirmed",
                "The Secret Key was not shown because Touch ID or the device password was not confirmed.",
            )),
        }
    }

    /// The Emergency Kit PDF made now (FIG-134), to be written where the person chooses. With a key kept on this
    /// Mac it asks for the same Touch ID or device password as showing the key. With none kept, `typed` (the
    /// master password and the Secret Key from the old kit) is checked against the vault first. Nothing is made
    /// without one of the two, and nothing is cached: the caller writes the bytes and drops them.
    pub fn kit_again(&self, typed: Option<(&str, &str)>) -> CmdRes<Zeroizing<Vec<u8>>> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        let place = figlock_session::config::load()?.place;
        if let Some((password, secret_key)) = typed {
            let key = parse_secret_key(secret_key)?;
            let vault_json = figlock_session::session::peek_vault_json()?;
            Vault::verify_credentials(&vault_json, password, &key)
                .map_err(|_| wrong_credentials())?;
            return Ok(kit::kit_pdf(&place, &key, now_ms())?);
        }
        let vault = peek_vault_id()?;
        let keys = secret_key_store::system()?;
        if !keys.has(&vault) {
            return Err(not_kept());
        }
        kit::kit_from_kept_key(&keys, &vault, secret_key_store::quick_on(&vault), &place, now_ms())?
            .ok_or_else(|| {
                CmdError::new(
                    "not_confirmed",
                    "The Emergency Kit was not saved because Touch ID or the device password was not confirmed.",
                )
            })
    }

    /// Keep the Secret Key another device just made, in a window that is already open (ADR-0034). It asks for the
    /// master password too, because the key can only be checked against the vault with it, and the open session
    /// keeps working: it holds the Vault Key, which a Secret Key rotation does not change.
    pub fn update_secret_key(&self, password: &str, secret_key: &str) -> CmdRes<VaultState> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        let key = parse_secret_key(secret_key)?;
        let vault_json = figlock_session::session::peek_vault_json()?;
        Vault::verify_credentials(&vault_json, password, &key).map_err(|_| wrong_credentials())?;
        let sk_gen = Vault::sk_gen_in(&vault_json).map_err(|_| wrong_credentials())?;
        let vault = peek_vault_id()?;
        let keys = secret_key_store::system()?;
        if keys.available() {
            keys.put(&vault, &key, sk_gen)?;
        }
        self.inner().held_key = Some(key);
        self.state()
    }

    /// Stop keeping the Secret Key on this Mac. The choice is remembered: every full unlock asks for the key
    /// until the person keeps it again (FIG-142). The open session keeps holding it until it locks (ADR-0030).
    pub fn forget_secret_key(&self) -> CmdRes<VaultState> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        secret_key_store::system()?.opt_out(&peek_vault_id()?)?;
        self.state()
    }

    /// Keep the Secret Key on this Mac again. The open session already holds the key it unlocked with, so then
    /// nothing is typed; otherwise the master password and Secret Key are asked for once and checked.
    pub fn keep_secret_key(&self, password: &str, secret_key: &str) -> CmdRes<VaultState> {
        if self.inner().session.is_none() {
            return Err(locked());
        }
        let typed = secret_key.trim();
        let key = if typed.is_empty() {
            let needs_key = || {
                CmdError::new(
                    "needs_secret_key",
                    "Type your Secret Key from your Emergency Kit to continue.",
                )
            };
            let current = Vault::sk_gen_in(&figlock_session::session::peek_vault_json()?)
                .map_err(|_| wrong_credentials())?;
            let inner = self.inner();
            let unlocked_with = inner.session.as_ref().map(|s| s.vault.sk_gen());
            if unlocked_with != Some(current) {
                return Err(needs_key());
            }
            inner
                .held_key
                .as_ref()
                .map(|k| SecretKey::from_bytes(*k.expose()))
                .ok_or_else(needs_key)?
        } else {
            let key = parse_secret_key(typed)?;
            let vault_json = figlock_session::session::peek_vault_json()?;
            Vault::verify_credentials(&vault_json, password, &key)
                .map_err(|_| wrong_credentials())?;
            key
        };
        let vault_json = figlock_session::session::peek_vault_json()?;
        let sk_gen = Vault::sk_gen_in(&vault_json).map_err(|_| wrong_credentials())?;
        let keys = secret_key_store::system()?;
        if !keys.available() {
            return Err(CmdError::new(
                "not_kept",
                "This Mac has no secure place to keep the Secret Key.",
            ));
        }
        keys.keep_again(&peek_vault_id()?, &key, sk_gen)?;
        self.state()
    }
}
