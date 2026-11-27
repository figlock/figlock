//! The optional Recovery Key (`pairing-recovery.md` 7, FIG-103): made and shown once as a code, checked by a
//! forced re-entry, and only then written into `vault.json`. During setup the choice is
//! required: add one, or acknowledge that a forgotten password cannot be recovered. Later, Settings can set up,
//! replace or turn it off. The key is held here only between "show" and "saved"; FigLock never stores it.

use figlock_core::{KitInfo, RecoveryKey, SecretKey, Vault, render_recovery_kit};
use figlock_session::session::now_ms;
use serde::Serialize;
use zeroize::Zeroizing;

use super::devices::revoke_credentials_error;
use super::{CmdError, CmdRes, Desk, KeyFrom, nothing_to_do, parse_secret_key};

/// The Recovery Key being shown and checked. Not `Debug`: it holds the key.
pub(super) struct Draft {
    key: RecoveryKey,
    confirmed: bool,
}

impl Draft {
    pub(super) fn confirmed(&self) -> bool {
        self.confirmed
    }

    pub(super) fn key(&self) -> &RecoveryKey {
        &self.key
    }
}

/// What setup chose: the first screen asks for one of these before a vault is created.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Choice {
    Undecided,
    /// The person acknowledged that a forgotten password cannot be recovered.
    Acknowledged,
    /// A Recovery Key was shown and confirmed; it is written with the vault.
    Key,
}

/// Everything the window shows for a new Recovery Key.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryMaterial {
    /// The `FLR1-` code. The person types its last two groups back to prove it is readable.
    pub code: String,
}

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

/// The last two groups of `code`, squashed the way a typed answer is.
fn tail_of(code: &str) -> String {
    let groups: Vec<&str> = code.split('-').collect();
    groups[groups.len() - 2..].concat()
}

fn squash_typed(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            u => u,
        })
        .collect()
}

impl Desk {
    /// Make a new Recovery Key and keep it as the draft to show and confirm. Nothing is written to the vault yet.
    /// Replaces an earlier draft.
    pub fn recovery_prepare(&self) -> CmdRes<RecoveryMaterial> {
        let mut inner = self.inner();
        let setup_open =
            inner.creating.as_ref().is_some_and(|c| c.key_confirmed) || inner.recovering.is_some();
        if !setup_open && inner.session.is_none() {
            return Err(locked());
        }
        let key = RecoveryKey::random()
            .map_err(|_| CmdError::new("failed", "Could not get randomness from the system."))?;
        let material = RecoveryMaterial {
            code: key.format().to_string(),
        };
        inner.recovery = Some(Draft {
            key,
            confirmed: false,
        });
        Ok(material)
    }

    /// The forced check that what was shown is readable: the last two groups of the code.
    pub fn recovery_check(&self, answer: &str) -> CmdRes<bool> {
        let mut inner = self.inner();
        let draft = inner.recovery.as_mut().ok_or_else(nothing_to_do)?;
        let ok = squash_typed(answer) == squash_typed(&tail_of(&draft.key.format()));
        draft.confirmed |= ok;
        Ok(ok)
    }

    /// The printable Recovery Key: a page with the code and its QR.
    pub fn recovery_kit(&self) -> CmdRes<Zeroizing<Vec<u8>>> {
        let inner = self.inner();
        let draft = inner.recovery.as_ref().ok_or_else(nothing_to_do)?;
        let place = match (inner.creating.as_ref(), inner.recovering.as_ref()) {
            (Some(c), _) => c.place.clone(),
            (None, Some(r)) => r.place().clone(),
            (None, None) => figlock_session::config::load()?.place,
        };
        let storage = figlock_session::kit::storage_label(&place);
        let info = KitInfo {
            vault_name: "Personal",
            storage: &storage,
            created: &super::civil_date(now_ms()),
        };
        let code = draft.key.format();
        render_recovery_kit(&info, &code)
            .map_err(|_| CmdError::new("failed", "Could not make the Recovery Key pages."))
    }

    /// Drop the draft (the person backed out).
    pub fn recovery_cancel(&self) {
        self.inner().recovery = None;
    }

    /// Setup only: the person chose not to add a Recovery Key and acknowledged what that means.
    pub fn recovery_skip(&self, acknowledged: bool) -> CmdRes<()> {
        if !acknowledged {
            return Err(CmdError::new(
                "not_acknowledged",
                "Confirm that you understand a forgotten password cannot be recovered.",
            ));
        }
        let mut inner = self.inner();
        let c = inner.creating.as_mut().ok_or_else(nothing_to_do)?;
        c.recovery = Choice::Acknowledged;
        inner.recovery = None;
        Ok(())
    }

    /// Setup only: use the confirmed draft, to be written with the vault.
    pub fn recovery_use_draft(&self) -> CmdRes<()> {
        let mut inner = self.inner();
        match inner.recovery.as_ref() {
            Some(d) if d.confirmed => {}
            Some(_) => {
                return Err(CmdError::new(
                    "recovery_not_confirmed",
                    "Confirm you can read the Recovery Key before you continue.",
                ));
            }
            None => return Err(nothing_to_do()),
        }
        let c = inner.creating.as_mut().ok_or_else(nothing_to_do)?;
        c.recovery = Choice::Key;
        Ok(())
    }

    /// Setup, when the vault is written: add the confirmed Recovery Key to `vault.json`, or refuse when the
    /// required choice was not made.
    pub(super) fn apply_recovery_at_create(
        draft: Option<Draft>,
        choice: Choice,
        vault: &mut Vault,
        password: &str,
        secret_key: &SecretKey,
    ) -> CmdRes<Option<Vec<u8>>> {
        match (choice, draft) {
            (Choice::Undecided, _) => Err(CmdError::new(
                "recovery_choice_needed",
                "Choose whether to add a Recovery Key before the vault is created.",
            )),
            (Choice::Acknowledged, _) => Ok(None),
            (Choice::Key, Some(d)) if d.confirmed => vault
                .set_recovery_with(&d.key, password, secret_key)
                .map(|change| Some(change.vault_json))
                .map_err(|_| CmdError::new("failed", "Could not add the Recovery Key.")),
            (Choice::Key, _) => Err(CmdError::new(
                "recovery_not_confirmed",
                "Confirm you can read the Recovery Key before you continue.",
            )),
        }
    }

    /// Settings: write the confirmed draft into the vault, replacing any Recovery Key it has. Asks for the
    /// password and the Secret Key (kept here, or typed) because the vault key is rewrapped.
    pub fn recovery_enable(&self, password: &str, secret_key: Option<&str>) -> CmdRes<()> {
        {
            let inner = self.inner();
            if inner.session.is_none() {
                return Err(locked());
            }
            if !inner.recovery.as_ref().is_some_and(|d| d.confirmed) {
                return Err(CmdError::new(
                    "recovery_not_confirmed",
                    "Confirm you can read the Recovery Key before you continue.",
                ));
            }
        }
        let (sk, from) = self.credentials_key(secret_key, "set up the Recovery Key")?;
        self.run_sync(false);
        let mut guard = self.inner();
        let inner = &mut *guard;
        let session = inner.session.as_mut().ok_or_else(locked)?;
        session.ensure_writable()?;
        let refreshed = session.refresh()?;
        if refreshed.settings_changed || refreshed.stopped {
            return Err(CmdError::new(
                "needs_unlock",
                "Another device changed this vault. Lock and unlock, then try again.",
            ));
        }
        let key = &inner.recovery.as_ref().ok_or_else(nothing_to_do)?.key;
        let change = session
            .vault
            .set_recovery_with(key, password, &sk)
            .map_err(|_| revoke_credentials_error(from))?;
        self.write_recovery(guard, &change.vault_json)
    }

    /// Settings: turn the Recovery Key off. The password confirms it, with the Secret Key kept here or
    /// typed; a code made earlier opens nothing from the next `vault.json`.
    pub fn recovery_disable(&self, password: &str, secret_key: Option<&str>) -> CmdRes<()> {
        let vault_json = {
            let inner = self.inner();
            let session = inner.session.as_ref().ok_or_else(locked)?;
            if !session.vault.has_recovery() {
                return Err(CmdError::new(
                    "failed",
                    "There is no Recovery Key to turn off.",
                ));
            }
            session.vault_json()?
        };
        let (sk, from) = match secret_key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(text) => (parse_secret_key(text)?, KeyFrom::Typed),
            None => self.credentials_key(None, "turn off the Recovery Key")?,
        };
        // Removing the key needs no credentials of the vault, so prove them here: this is a downgrade.
        Vault::unlock_key(&vault_json, password, &sk)
            .and_then(|auk| {
                Vault::unlock_with_key(
                    &vault_json,
                    &auk,
                    figlock_core::DeviceKeys::generate()?,
                    "check",
                    &[],
                    now_ms(),
                )
            })
            .map_err(|_| revoke_credentials_error(from))?;
        self.run_sync(false);
        let mut guard = self.inner();
        let inner = &mut *guard;
        let session = inner.session.as_mut().ok_or_else(locked)?;
        session.ensure_writable()?;
        let refreshed = session.refresh()?;
        if refreshed.settings_changed || refreshed.stopped {
            return Err(CmdError::new(
                "needs_unlock",
                "Another device changed this vault. Lock and unlock, then try again.",
            ));
        }
        let change = session
            .vault
            .clear_recovery()
            .map_err(|_| CmdError::new("failed", "There is no Recovery Key to turn off."))?;
        self.write_recovery(guard, &change.vault_json)
    }

    /// Store the new `vault.json` and sync it. A failed write leaves it unknown whether storage has it, so the
    /// session is dropped and the next unlock reads what is really there (as for a credential change).
    fn write_recovery(
        &self,
        mut guard: std::sync::MutexGuard<'_, super::Inner>,
        vault_json: &[u8],
    ) -> CmdRes<()> {
        let inner = &mut *guard;
        let session = inner.session.as_mut().ok_or_else(locked)?;
        if session.replace_vault_json(vault_json).is_err() {
            inner.drop_session();
            return Err(CmdError::new(
                "change_uncertain",
                "The change may be only partly saved, so the vault was locked. Unlock it again and check the Recovery Key setting.",
            ));
        }
        inner.recovery = None;
        inner.cache = None;
        inner.mark_dirty();
        drop(guard);
        self.run_sync(false);
        Ok(())
    }
}
