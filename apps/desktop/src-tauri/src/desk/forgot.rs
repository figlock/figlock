//! "I forgot my password" on a fresh install (`pairing-recovery.md` 7.3, FIG-103): find the vault, open it with
//! a Recovery Key (the code, or the needed cards), choose a new master password, get a new Secret Key and a new
//! Emergency Kit, and optionally a new Recovery Key. Nothing is written to storage or to this device until the
//! new Emergency Kit was confirmed, so an abandoned attempt changes nothing and the Recovery Key still works.

use figlock_core::{DeviceKeys, Recover, Recovered, RecoveryKey, Vault, slip39};
use figlock_session::config::{self, Config, Place};
use figlock_session::session::{Session, now_ms};
use serde::Serialize;
use zeroize::Zeroizing;

use super::{
    CmdError, CmdRes, Desk, MIN_PASSWORD_CHARS, Storage, Typed, nothing_to_do, storage_place,
};
use figlock_session::Fail;

/// A Recovery Key that opened a vault, waiting for the new credentials. Not `Debug`: it holds keys.
pub(super) struct Recovering {
    place: Place,
    vault_json: Vec<u8>,
    records: Vec<Vec<u8>>,
    key: RecoveryKey,
    prepared: Option<Prepared>,
}

/// The new credentials, made in memory and not yet stored.
pub(super) struct Prepared {
    vault: Vault,
    out: Recovered,
    password: Zeroizing<String>,
    /// The person typed the end of the new Secret Key back, so the new Emergency Kit was saved.
    pub(super) key_confirmed: bool,
}

impl Prepared {
    pub(super) fn secret_key(&self) -> &figlock_core::SecretKey {
        &self.out.secret_key
    }
}

impl Recovering {
    pub(super) fn place(&self) -> &Place {
        &self.place
    }

    pub(super) fn prepared_mut(&mut self) -> Option<&mut Prepared> {
        self.prepared.as_mut()
    }

    pub(super) fn prepared(&self) -> Option<&Prepared> {
        self.prepared.as_ref()
    }
}

/// What a card says about its set.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CardInfo {
    /// How many cards open the vault.
    pub needed: u8,
    /// This card's number.
    pub number: u8,
}

/// What the Recovery Key opened, to be shown before the person commits to anything.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub name: &'static str,
    pub items: usize,
    pub last_change_ms: Option<u64>,
    pub devices: usize,
}

/// The new Secret Key, to be written into a new Emergency Kit.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NewCredentials {
    pub secret_key: String,
}

fn fresh_only() -> CmdRes<()> {
    if config::is_initialised()? {
        return Err(CmdError::new(
            "failed",
            "This device already has a vault set up.",
        ));
    }
    Ok(())
}

fn bad_key() -> CmdError {
    CmdError::new(
        "bad_recovery_key",
        "That is not a valid Recovery Key. Check the code, or that every card is complete and from the same set.",
    )
}

impl Desk {
    /// What a card says: how many cards are needed. Lets the window say how many more to enter.
    pub fn forgot_card_info(text: &str) -> CmdRes<CardInfo> {
        let info = slip39::info(text).map_err(|_| {
            CmdError::new(
                "bad_card",
                "This card has a typo or is incomplete. Check every word.",
            )
        })?;
        Ok(CardInfo {
            needed: info.member_threshold,
            number: info.number,
        })
    }

    /// Open the vault in `storage` with a Recovery Key: `answers` is the one code, or the words of each card.
    /// Nothing is stored; this proves the key and shows what it found.
    pub fn forgot_find(
        &self,
        storage: &Storage,
        answers: &[String],
        shares: bool,
    ) -> CmdRes<Found> {
        fresh_only()?;
        let key = if shares {
            let refs: Vec<&str> = answers.iter().map(String::as_str).collect();
            RecoveryKey::from_shares(&refs).map_err(|_| bad_key())?
        } else {
            RecoveryKey::parse(answers.first().map_or("", String::as_str)).map_err(|_| bad_key())?
        };
        let place = storage_place(storage, false)?;
        figlock_session::sync::open_from_remote(&place).map_err(Fail::from)?;
        let store = figlock_session::store::open(&place)?;
        let vault_json = store.get("vault.json").map_err(|e| match e {
            figlock_store::StoreError::NotFound => CmdError::new(
                "no_vault",
                format!("There is no FigLock vault in {}.", storage.there()),
            ),
            e => Fail::from(e).into(),
        })?;
        match Vault::recovery_in(&vault_json) {
            Ok(true) => {}
            Ok(false) => {
                return Err(CmdError::new(
                    "no_recovery",
                    "That vault has no Recovery Key, so it cannot be opened this way. Use your Emergency Kit and master password, or a device that is already set up.",
                ));
            }
            Err(_) => return Err(Fail::new("That vault could not be read.").into()),
        }
        let mut records = Vec::new();
        for e in store.list("devices/").map_err(Fail::from)? {
            if let Ok(bytes) = store.get(&e.path) {
                records.push(bytes);
            }
        }
        // A trial run with a throwaway password proves the key opens this vault, and lets the vault be read.
        let throwaway = figlock_core::SecretKey::random()
            .map_err(Fail::from)?
            .format();
        let (vault, _) = Vault::recover(
            Recover {
                vault_json: &vault_json,
                recovery_key: &key,
                new_password: throwaway.as_str(),
                new_recovery_key: None,
            },
            DeviceKeys::generate().map_err(Fail::from)?,
            &Self::device_name(),
            &records,
            now_ms(),
        )
        .map_err(|_| {
            CmdError::new(
                "recovery_failed",
                "This Recovery Key does not open that vault. Check the code or cards, and that you picked the right storage.",
            )
        })?;
        let preview = Session::preview(vault, &place)?;
        self.inner().recovering = Some(Recovering {
            place,
            vault_json,
            records,
            key,
            prepared: None,
        });
        Ok(Found {
            name: "Personal",
            items: preview.items,
            last_change_ms: preview.last_change_ms,
            devices: preview.devices,
        })
    }

    /// Choose the new master password and make the new Secret Key, in memory. With `new_recovery_key` the
    /// confirmed draft (made with `recovery_prepare`) replaces the Recovery Key that was just used. The person
    /// then saves the new Emergency Kit and confirms it before `forgot_commit` writes anything.
    pub fn forgot_prepare(
        &self,
        new_password: &str,
        new_recovery_key: bool,
    ) -> CmdRes<NewCredentials> {
        if new_password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(CmdError::new(
                "weak_password",
                "Use at least 8 characters for the master password.",
            ));
        }
        let mut guard = self.inner();
        let inner = &mut *guard;
        let r = inner.recovering.as_mut().ok_or_else(nothing_to_do)?;
        let new_key = if new_recovery_key {
            let draft = inner.recovery.as_ref().ok_or_else(nothing_to_do)?;
            if !draft.confirmed() {
                return Err(CmdError::new(
                    "recovery_not_confirmed",
                    "Confirm you can read the new Recovery Key before you continue.",
                ));
            }
            Some(draft.key())
        } else {
            None
        };
        let (vault, out) = Vault::recover(
            Recover {
                vault_json: &r.vault_json,
                recovery_key: &r.key,
                new_password,
                new_recovery_key: new_key,
            },
            DeviceKeys::generate().map_err(Fail::from)?,
            &Self::device_name(),
            &r.records,
            now_ms(),
        )
        .map_err(|_| {
            CmdError::new(
                "recovery_failed",
                "This Recovery Key does not open that vault any more. Start again.",
            )
        })?;
        let secret_key = out.secret_key.format().to_string();
        r.prepared = Some(Prepared {
            vault,
            out,
            password: Zeroizing::new(new_password.to_owned()),
            key_confirmed: false,
        });
        Ok(NewCredentials { secret_key })
    }

    /// Store the new `vault.json`, remember this device and unlock. Refused until the new Emergency Kit was
    /// confirmed. This device's setup is written first and taken back if storage fails, so a failure leaves the
    /// vault as it was.
    pub fn forgot_commit(&self, touch_id: bool) -> CmdRes<super::VaultState> {
        fresh_only()?;
        let (r, prepared) = {
            let mut inner = self.inner();
            let r = inner.recovering.as_mut().ok_or_else(nothing_to_do)?;
            match r.prepared.as_ref() {
                Some(p) if p.key_confirmed => {}
                Some(_) => {
                    return Err(CmdError::new(
                        "key_not_confirmed",
                        "Confirm your new Secret Key before the vault is changed.",
                    ));
                }
                None => return Err(nothing_to_do()),
            }
            let prepared = r.prepared.take().ok_or_else(nothing_to_do)?;
            (inner.recovering.take().ok_or_else(nothing_to_do)?, prepared)
        };
        let Prepared {
            vault,
            out,
            password,
            ..
        } = prepared;
        let cfg = Config {
            place: r.place.clone(),
            device_name: Self::device_name(),
        };
        config::save(&cfg, vault.device_keys())?;
        let store = figlock_session::store::open(&cfg.place)?;
        let written = (|| -> Result<(), figlock_store::StoreError> {
            if let Some(record) = &out.new_record {
                let path = format!("devices/{}.json", hex::encode(vault.device_id()));
                match store.put_new(&path, record) {
                    Ok(()) | Err(figlock_store::StoreError::Exists) => {}
                    Err(e) => return Err(e),
                }
            }
            store.put_replace("vault.json", &out.vault_json)
        })();
        if let Err(e) = written {
            config::forget();
            // Put the attempt back so the person can try again with the same key.
            self.inner().recovering = Some(Recovering {
                prepared: None,
                ..r
            });
            return Err(Fail::from(e).into());
        }
        // The vault is safe on this device either way; a failed first upload is retried by the next sync.
        if cfg.place.remote.is_some() {
            let _ = figlock_session::sync::run(&cfg, false);
        }
        self.inner().recovery = None;
        let key = figlock_core::SecretKey::from_bytes(*out.secret_key.expose());
        self.enter(
            Typed {
                password,
                secret_key: Some(key),
            },
            touch_id,
        )
    }

    /// Drop the attempt (the person backed out).
    pub fn forgot_cancel(&self) {
        let mut inner = self.inner();
        inner.recovering = None;
        inner.recovery = None;
    }
}
