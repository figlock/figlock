//! The optional Recovery Key (`pairing-recovery.md` 7, FIG-103): made and shown once, as a code or as Shamir
//! cards, checked by a forced re-entry, and only then written into `vault.json`. During setup the choice is
//! required: add one, or acknowledge that a forgotten password cannot be recovered. Later, Settings can set up,
//! replace or turn it off. The key is held here only between "show" and "saved"; FigLock never stores it.

use figlock_core::{KitInfo, RecoveryCards, RecoveryKey, SecretKey, Vault, render_recovery_kit};
use figlock_session::session::now_ms;
use serde::Serialize;
use zeroize::Zeroizing;

use super::devices::revoke_credentials_error;
use super::{
    CmdError, CmdRes, Desk, kit_storage, nothing_to_do, parse_secret_key, wrong_credentials,
};

/// The Recovery Key being shown and checked. Not `Debug`: it holds the key.
pub(super) struct Draft {
    key: RecoveryKey,
    /// Shamir cards and their threshold, or `None` for the single code.
    cards: Option<(Vec<Zeroizing<String>>, u8)>,
    /// For each card, the word the person is asked to type back (1-based); empty for a code.
    challenge: Vec<usize>,
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
    /// `code` or `shares`.
    pub mode: &'static str,
    pub code: Option<String>,
    /// The words of each card.
    pub cards: Vec<Vec<String>>,
    pub threshold: u8,
    pub count: u8,
    /// For shares: which word of each card (1-based) the person types back to prove the cards are readable.
    /// For a code the person types the last two groups.
    pub challenge: Vec<usize>,
}

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

fn random_word_positions(n: usize, words: usize) -> CmdRes<Vec<usize>> {
    let mut bytes = vec![0u8; n * 2];
    getrandom::fill(&mut bytes)
        .map_err(|_| CmdError::new("failed", "Could not get randomness from the system."))?;
    Ok(bytes
        .chunks(2)
        .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])) % words + 1)
        .collect())
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
    /// Make a new Recovery Key, as one code or as `count` cards of which `threshold` open the vault, and keep
    /// it as the draft to show and confirm. Nothing is written to the vault yet. Replaces an earlier draft.
    pub fn recovery_prepare(
        &self,
        shares: bool,
        threshold: u8,
        count: u8,
    ) -> CmdRes<RecoveryMaterial> {
        let mut inner = self.inner();
        let setup_open =
            inner.creating.as_ref().is_some_and(|c| c.key_confirmed) || inner.recovering.is_some();
        if !setup_open && inner.session.is_none() {
            return Err(locked());
        }
        let key = RecoveryKey::random()
            .map_err(|_| CmdError::new("failed", "Could not get randomness from the system."))?;
        let (material, draft) = if shares {
            if !(2..=16).contains(&count) || threshold < 2 || threshold > count {
                return Err(CmdError::new(
                    "bad_split",
                    "Choose 2 to 16 cards, and a threshold of at least 2 that is not more than the number of cards.",
                ));
            }
            let cards = key
                .split(threshold, count)
                .map_err(|_| CmdError::new("failed", "Could not split the Recovery Key."))?;
            let words: Vec<Vec<String>> = cards
                .iter()
                .map(|c| c.split(' ').map(str::to_owned).collect())
                .collect();
            let challenge = random_word_positions(cards.len(), words[0].len())?;
            (
                RecoveryMaterial {
                    mode: "shares",
                    code: None,
                    cards: words,
                    threshold,
                    count,
                    challenge: challenge.clone(),
                },
                Draft {
                    key,
                    cards: Some((cards, threshold)),
                    challenge,
                    confirmed: false,
                },
            )
        } else {
            (
                RecoveryMaterial {
                    mode: "code",
                    code: Some(key.format().to_string()),
                    cards: Vec::new(),
                    threshold: 0,
                    count: 0,
                    challenge: Vec::new(),
                },
                Draft {
                    key,
                    cards: None,
                    challenge: Vec::new(),
                    confirmed: false,
                },
            )
        };
        inner.recovery = Some(draft);
        Ok(material)
    }

    /// The forced check that what was shown is readable: for a code, its last two groups; for cards, the asked
    /// word of each card, in card order.
    pub fn recovery_check(&self, answers: &[String]) -> CmdRes<bool> {
        let mut inner = self.inner();
        let draft = inner.recovery.as_mut().ok_or_else(nothing_to_do)?;
        let ok = match &draft.cards {
            None => {
                answers.len() == 1
                    && squash_typed(&answers[0]) == squash_typed(&tail_of(&draft.key.format()))
            }
            Some((cards, _)) => {
                answers.len() == cards.len()
                    && cards.iter().zip(&draft.challenge).zip(answers).all(
                        |((card, pos), typed)| {
                            card.split(' ').nth(pos - 1)
                                == Some(typed.trim().to_lowercase().as_str())
                        },
                    )
            }
        };
        draft.confirmed |= ok;
        Ok(ok)
    }

    /// The printable Recovery Key (a page for the code, a card per page for shares).
    pub fn recovery_kit(&self) -> CmdRes<Zeroizing<Vec<u8>>> {
        let inner = self.inner();
        let draft = inner.recovery.as_ref().ok_or_else(nothing_to_do)?;
        let place = match (inner.creating.as_ref(), inner.recovering.as_ref()) {
            (Some(c), _) => c.place.clone(),
            (None, Some(r)) => r.place().clone(),
            (None, None) => figlock_session::config::load()?.place,
        };
        let storage = kit_storage(&place);
        let info = KitInfo {
            vault_name: "Personal",
            storage: &storage,
            created: &super::civil_date(now_ms()),
        };
        let code = draft.key.format();
        let refs: Vec<&str> = draft
            .cards
            .iter()
            .flat_map(|(c, _)| c.iter().map(|s| s.as_str()))
            .collect();
        let cards = match &draft.cards {
            None => RecoveryCards::Code(&code),
            Some((_, threshold)) => RecoveryCards::Shares {
                shares: &refs,
                threshold: *threshold,
            },
        };
        render_recovery_kit(&info, &cards)
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
    /// master password and the Secret Key (kept here, or typed) because the vault key is rewrapped.
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
        let (sk, typed) = self.credentials_key(secret_key, "set up the Recovery Key")?;
        self.run_sync(false);
        let mut guard = self.inner();
        let inner = &mut *guard;
        let session = inner.session.as_mut().ok_or_else(locked)?;
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
            .map_err(|_| revoke_credentials_error(typed))?;
        self.write_recovery(guard, &change.vault_json)
    }

    /// Settings: turn the Recovery Key off. The master password confirms it, with the Secret Key kept here or
    /// typed; cards made earlier open nothing from the next `vault.json`.
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
        let (sk, typed) = match secret_key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(text) => (parse_secret_key(text)?, true),
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
            .map_err(|_| {
                if typed {
                    wrong_credentials()
                } else {
                    revoke_credentials_error(false)
                }
            })?;
        self.run_sync(false);
        let mut guard = self.inner();
        let inner = &mut *guard;
        let session = inner.session.as_mut().ok_or_else(locked)?;
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
        inner.dirty = true;
        drop(guard);
        self.run_sync(false);
        Ok(())
    }
}
