//! Adding a device from this one, the trusted side of pairing (`pairing-recovery.md` 3, FIG-102). Settings opens
//! an invite and shows its code; the new device answers with a request; the person compares the device name and
//! key fingerprint and approves with Touch ID, which releases the Secret Key that goes to the new device
//! inside the sealed answer. The invite secret and its keys live only in this process.

use figlock_core::{DeviceKeys, Invite, JoinRequest, SecretKey, Vault, qr_rows};
use figlock_session::session::now_ms as wall_ms;
use figlock_session::{config, sync};
use serde::Serialize;

use super::devices::DeviceRow;
use super::sync::Kind;
use super::{CmdError, CmdRes, Desk, parse_secret_key, wrong_credentials};

/// An open invite, and the request that arrived for it. Not `Debug`: it holds the invite secret.
pub(super) struct Inviting {
    invite: Invite,
    request: Option<JoinRequest>,
}

/// What the window shows to start: the code to type or scan on the new device.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InviteView {
    /// `XXXX-XXXX-XXXX-XXXX-XXXX`. Shown once; never stored.
    pub code: String,
    /// QR modules of the code, one string of `1` and `0` per row.
    pub qr: Vec<String>,
    pub expires_at_ms: u64,
}

/// Where the invite stands. `state` is `none`, `waiting`, `request` or `expired`.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingStatus {
    pub state: &'static str,
    pub expires_at_ms: Option<u64>,
    /// With `request`: the name the new device gave itself, and its key fingerprint to compare with its screen.
    pub device_name: Option<String>,
    pub fingerprint: Option<String>,
}

impl PairingStatus {
    fn bare(state: &'static str) -> Self {
        PairingStatus {
            state,
            expires_at_ms: None,
            device_name: None,
            fingerprint: None,
        }
    }
}

/// The wall clock. Tests move it forward to run out an invite's 10 minutes without waiting.
#[cfg(not(test))]
fn now_ms() -> u64 {
    wall_ms()
}

#[cfg(test)]
static SKEW_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
fn now_ms() -> u64 {
    wall_ms() + SKEW_MS.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
pub(super) fn advance_clock_for_test(ms: u64) {
    SKEW_MS.fetch_add(ms, std::sync::atomic::Ordering::Relaxed);
}

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

fn no_invite() -> CmdError {
    CmdError::new(
        "no_invite",
        "There is no open invite. Start again with Add a device.",
    )
}

impl Desk {
    /// Open an invite: write its `.inv` and share it with the storage so the new device can find it. Only one
    /// invite is open at a time; a new one replaces the old.
    pub fn add_device_start(&self) -> CmdRes<InviteView> {
        self.add_device_cancel()?;
        let (offer, expires_at_ms) = {
            let mut inner = self.inner();
            let session = inner.session.as_mut().ok_or_else(locked)?;
            let now = now_ms();
            let (invite, offer) = Invite::create(session.vault.vault_id(), now)
                .map_err(|_| CmdError::new("failed", "Could not make an invite."))?;
            session.publish_invite(&offer)?;
            let expires = invite.expires_at();
            inner.inviting = Some(Inviting {
                invite,
                request: None,
            });
            (offer, expires)
        };
        // With Drive sync the new device finds the invite there, so it has to arrive before the code is shown.
        if self.run_sync(false) && self.sync_state().kind != Kind::Idle {
            self.add_device_cancel()?;
            return Err(CmdError::new(
                "offline",
                "FigLock could not reach Google Drive, so the new device could not find the invite. Check your connection and try again.",
            ));
        }
        let qr = qr_rows(&offer.code)
            .map_err(|_| CmdError::new("failed", "Could not make the QR code."))?;
        Ok(InviteView {
            code: offer.code.to_string(),
            qr,
            expires_at_ms,
        })
    }

    /// Look for the new device's request (and sync first, so one written to Drive is seen). Once a valid
    /// request opens, the invite is used up and the person is asked to approve it.
    pub fn add_device_status(&self) -> CmdRes<PairingStatus> {
        if self.inner().inviting.is_none() {
            return Ok(PairingStatus::bare("none"));
        }
        self.run_sync(false);
        let mut guard = self.inner();
        let inner = &mut *guard;
        let session = inner.session.as_mut().ok_or_else(locked)?;
        let Some(open) = inner.inviting.as_mut() else {
            return Ok(PairingStatus::bare("none"));
        };
        let now = now_ms();
        let expires_at_ms = Some(open.invite.expires_at());
        if open.invite.is_expired(now) {
            session.close_invite(&open.invite);
            let paths = vec![open.invite.inv_path(), open.invite.req_path()];
            inner.inviting = None;
            drop(guard);
            self.forget_remote(&paths);
            return Ok(PairingStatus::bare("expired"));
        }
        if open.request.is_none() {
            let found = session.take_request(&mut open.invite, now)?;
            if let Some(req) = found {
                open.request = Some(req);
                let inv = open.invite.inv_path();
                session.retire_invite(&open.invite);
                drop(guard);
                self.forget_remote(&[inv]);
                return self.add_device_status();
            }
        }
        Ok(match open.request.as_ref() {
            Some(req) => PairingStatus {
                state: "request",
                expires_at_ms,
                device_name: Some(req.device_name.clone()),
                fingerprint: Some(req.fingerprint()),
            },
            None => PairingStatus {
                state: "waiting",
                expires_at_ms,
                device_name: None,
                fingerprint: None,
            },
        })
    }

    /// Approve the request. A typed Secret Key, or the one the session holds (ADR-0030), must open the vault
    /// together with the master password. With neither, Touch ID (or the device password) releases the Secret
    /// Key this device keeps.
    pub fn add_device_approve(
        &self,
        password: Option<&str>,
        secret_key: Option<&str>,
    ) -> CmdRes<Vec<DeviceRow>> {
        if self
            .inner()
            .inviting
            .as_ref()
            .is_none_or(|i| i.request.is_none())
        {
            return Err(no_invite());
        }
        let typed = secret_key.map(str::trim).filter(|k| !k.is_empty());
        let held = self
            .inner()
            .held_key
            .as_ref()
            .map(|k| SecretKey::from_bytes(*k.expose()));
        let checked = match typed {
            Some(text) => Some(parse_secret_key(text)?),
            None => held,
        };
        let key: SecretKey = match checked {
            Some(key) => {
                let vault_json = {
                    let inner = self.inner();
                    inner.session.as_ref().ok_or_else(locked)?.vault_json()?
                };
                // The answer carries this key to the new device, so a wrong one must not get that far. Opening
                // the vault with both is the check; the throwaway device keys and the result are dropped.
                let opened = Vault::unlock_key(&vault_json, password.unwrap_or_default(), &key)
                    .and_then(|auk| {
                        Vault::unlock_with_key(
                            &vault_json,
                            &auk,
                            DeviceKeys::generate()?,
                            "check",
                            &[],
                            now_ms(),
                        )
                    });
                opened.map_err(|_| wrong_credentials())?;
                key
            }
            None => self.credentials_key(None, "add a device").map(|(k, _)| k)?,
        };
        let mut guard = self.inner();
        let inner = &mut *guard;
        let session = inner.session.as_mut().ok_or_else(locked)?;
        let open = inner.inviting.as_mut().ok_or_else(no_invite)?;
        let req = open.request.take().ok_or_else(no_invite)?;
        let now = now_ms();
        if let Err(e) = session.approve_join(&open.invite, &req, &key, now) {
            let expired = open.invite.is_expired(now);
            if expired {
                session.close_invite(&open.invite);
                inner.inviting = None;
            } else {
                open.request = Some(req);
            }
            return Err(if session.is_uncertain() {
                CmdError::new(
                    "needs_unlock",
                    "The new device could not be saved. Lock and unlock FigLock, then try again.",
                )
            } else if expired {
                CmdError::new(
                    "expired",
                    "The invite expired. Start again with Add a device.",
                )
            } else {
                e.into()
            });
        }
        let paths = vec![open.invite.inv_path(), open.invite.req_path()];
        inner.inviting = None;
        inner.cache = None;
        drop(guard);
        self.forget_remote(&paths);
        // Share the new device's record and the answer; a failure shows in the sync status and is retried.
        self.run_sync(false);
        self.devices()
    }

    /// Close the open invite, whether or not a request came.
    pub fn add_device_cancel(&self) -> CmdRes<()> {
        let mut guard = self.inner();
        let inner = &mut *guard;
        let Some(open) = inner.inviting.take() else {
            return Ok(());
        };
        if let Some(session) = inner.session.as_ref() {
            session.close_invite(&open.invite);
        }
        let paths = vec![open.invite.inv_path(), open.invite.req_path()];
        drop(guard);
        self.forget_remote(&paths);
        Ok(())
    }

    fn forget_remote(&self, paths: &[String]) {
        if let Ok(cfg) = config::load() {
            sync::remove_remote(&cfg.place, paths);
        }
    }
}
