//! Keeping the local vault matched to Google Drive, as the window sees it (ADR-0026).
//!
//! Sync works on ciphertext only, so it runs whether the vault is locked or not. A background thread calls
//! [`Desk::sync_tick`] every second; it syncs every 15 seconds, soon after a local save, and backs off when
//! something fails. What the person needs to act on (sign in again, confirm an upload) stops the retries until
//! they do.

use std::time::{Duration, Instant};

use figlock_session::config::{self, Remote};
use figlock_session::drive;
use figlock_session::sync::{self, SyncError, Warning};
use serde::Serialize;

use super::{CmdError, CmdRes, Desk};

/// How often a healthy sync runs (`sync.md` 10).
const INTERVAL: Duration = Duration::from_secs(15);
/// Waits after consecutive failures: 5 s, 15 s, 60 s, then every 5 minutes.
const BACKOFF: [u64; 4] = [5, 15, 60, 300];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Idle,
    Offline,
    /// Needs the person: sign in to Google again.
    SignIn,
    /// Needs the person: the vault is gone from Drive; confirm an upload.
    Lost,
    /// Needs the person: Drive holds a different vault.
    Foreign,
    Failed,
}

pub(super) struct SyncState {
    running: bool,
    kind: Kind,
    message: Option<String>,
    fork: bool,
    needs_unlock: bool,
    failures: usize,
    next_at: Instant,
}

impl SyncState {
    pub(super) fn new() -> Self {
        SyncState {
            running: false,
            kind: Kind::Idle,
            message: None,
            fork: false,
            needs_unlock: false,
            failures: 0,
            next_at: Instant::now(),
        }
    }

    /// Stops retrying until the person acts.
    fn blocked(&self) -> bool {
        matches!(self.kind, Kind::SignIn | Kind::Lost | Kind::Foreign)
    }
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncView {
    pub enabled: bool,
    /// `off`, `idle`, `syncing`, `offline`, `sign_in`, `lost`, `foreign` or `failed`.
    pub state: &'static str,
    pub signed_in: bool,
    /// This build can sign in to Google at all.
    pub can_sign_in: bool,
    pub last_sync_secs_ago: Option<u64>,
    /// Words for the person when something needs their attention.
    pub message: Option<String>,
    /// Two devices changed the vault's settings at once and one copy was kept (ADR-0026).
    pub fork: bool,
    /// Another device changed the password, keys or devices: lock and unlock to take it in.
    pub needs_unlock: bool,
    /// Where the vault is kept on this Mac.
    pub local_folder: Option<String>,
}

fn words(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Idle => None,
        Kind::Offline => Some(
            "Can't reach Google Drive. The vault works here as normal and will sync when you're back online.",
        ),
        Kind::SignIn => Some("Your Google sign-in has expired. Sign in again to keep syncing."),
        Kind::Lost => Some(
            "The vault is no longer in your Google Drive. You can upload it again from this Mac.",
        ),
        Kind::Foreign => Some(
            "Your Google Drive holds a different vault, so sync is paused. Nothing was changed.",
        ),
        Kind::Failed => Some("Sync ran into a problem. It will try again."),
    }
}

fn kind_of(e: &SyncError) -> Kind {
    match e {
        SyncError::Offline | SyncError::Busy => Kind::Offline,
        SyncError::SignIn => Kind::SignIn,
        SyncError::RemoteLost => Kind::Lost,
        SyncError::ForeignVault => Kind::Foreign,
        _ => Kind::Failed,
    }
}

impl Desk {
    fn sync_state(&self) -> std::sync::MutexGuard<'_, SyncState> {
        self.sync.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn sync_status(&self) -> CmdRes<SyncView> {
        let cfg = config::load().ok();
        let enabled = cfg.as_ref().is_some_and(|c| c.place.remote.is_some());
        let drive = drive::status()?;
        let s = self.sync_state();
        let state = if !enabled {
            "off"
        } else if s.running {
            "syncing"
        } else {
            match s.kind {
                Kind::Idle => "idle",
                Kind::Offline => "offline",
                Kind::SignIn => "sign_in",
                Kind::Lost => "lost",
                Kind::Foreign => "foreign",
                Kind::Failed => "failed",
            }
        };
        Ok(SyncView {
            enabled,
            state,
            signed_in: drive.signed_in,
            can_sign_in: drive.can_sign_in,
            last_sync_secs_ago: sync::last_sync_ms()
                .map(|t| figlock_session::session::now_ms().saturating_sub(t) / 1000),
            message: if enabled {
                s.message
                    .clone()
                    .or_else(|| words(s.kind).map(str::to_owned))
            } else {
                None
            },
            fork: enabled && s.fork,
            needs_unlock: enabled && s.needs_unlock,
            local_folder: cfg.map(|c| c.place.local.display().to_string()),
        })
    }

    /// Sync once now. `reupload` is the person's yes to uploading everything again after the vault was lost
    /// from Drive. Resolves with the state afterwards; a sync that failed is a state, not an error.
    pub fn sync_now(&self, reupload: bool) -> CmdRes<SyncView> {
        self.run_sync(reupload);
        self.sync_status()
    }

    /// Run one sync and record how it went. Returns false when none was run (sync is off).
    fn run_sync(&self, reupload: bool) -> bool {
        let Ok(cfg) = config::load() else {
            return false;
        };
        if cfg.place.remote.is_none() {
            return false;
        }
        self.sync_state().running = true;
        let result = sync::run(&cfg, reupload);
        // Taking in what was pulled needs the vault; do it before the state says "idle".
        let absorbed = match &result {
            Ok(r) if r.pulled > 0 => Some(self.absorb_pull()),
            _ => None,
        };
        let mut s = self.sync_state();
        s.running = false;
        match result {
            Ok(report) => {
                s.kind = Kind::Idle;
                s.message = None;
                s.failures = 0;
                s.fork = report.warnings.contains(&Warning::VaultJsonFork);
                s.next_at = Instant::now() + INTERVAL;
                if let Some(needs_unlock) = absorbed {
                    s.needs_unlock |= needs_unlock;
                }
            }
            Err(SyncError::Busy) => {
                // Another sync (the CLI's) holds the lock; it is doing this work.
                s.next_at = Instant::now() + Duration::from_secs(BACKOFF[0]);
            }
            Err(e) => {
                s.kind = kind_of(&e);
                s.message = match e {
                    SyncError::Other(m) if s.kind == Kind::Failed => Some(m),
                    _ => None,
                };
                let wait = BACKOFF[s.failures.min(BACKOFF.len() - 1)];
                s.failures += 1;
                s.next_at = Instant::now() + Duration::from_secs(wait);
            }
        }
        true
    }

    #[cfg(test)]
    pub(super) fn absorb_pull_for_test(&self) -> bool {
        self.absorb_pull()
    }

    /// Bring what a sync pulled into the unlocked vault. Returns true when the vault needs a fresh unlock to
    /// take in the rest (settings changed elsewhere, or a device's changes could not be applied yet).
    fn absorb_pull(&self) -> bool {
        let mut inner = self.inner();
        let Some(session) = inner.session.as_mut() else {
            return false;
        };
        match session.refresh() {
            Ok(r) => {
                if r.applied > 0 {
                    inner.cache = None;
                }
                r.settings_changed || r.stopped
            }
            Err(_) => true,
        }
    }

    /// Turn Drive sync on and upload what is already here. Only stays on if the first sync works, so a
    /// different vault in Drive or a dead sign-in leaves the device as it was.
    pub fn sync_enable(&self) -> CmdRes<SyncView> {
        let cfg = config::load()?;
        if cfg.place.remote.is_some() {
            return self.sync_status();
        }
        if !drive::status()?.signed_in {
            return Err(CmdError::new("drive_sign_in", "Sign in to Google first."));
        }
        sync::forget_state();
        let cfg = config::set_remote(Some(Remote::Drive))?;
        match sync::run(&cfg, false) {
            Ok(_) => {
                *self.sync_state() = SyncState::new();
                self.sync_status()
            }
            Err(e) => {
                config::set_remote(None)?;
                sync::forget_state();
                Err(CmdError::new(
                    "sync_failed",
                    format!(
                        "Could not turn on sync, so nothing was changed. {}",
                        plain(&e)
                    ),
                ))
            }
        }
    }

    /// Stop syncing. The vault on this Mac and the copy in Drive are left alone.
    pub fn sync_disable(&self) -> CmdRes<SyncView> {
        config::set_remote(None)?;
        sync::forget_state();
        *self.sync_state() = SyncState::new();
        self.sync_status()
    }

    /// Called every second by the sync thread. Returns true when a sync ran, so the window can refresh what it
    /// shows.
    pub fn sync_tick(&self) -> bool {
        let dirty = std::mem::take(&mut self.inner().dirty);
        let due = {
            let s = self.sync_state();
            !s.running && !s.blocked() && (dirty || Instant::now() >= s.next_at)
        };
        due && self.run_sync(false)
    }
}

/// A sentence for the person from a sync error.
fn plain(e: &SyncError) -> String {
    match e {
        SyncError::ForeignVault => "Your Google Drive already holds a different vault.".to_owned(),
        SyncError::SignIn => "Google refused the sign-in. Sign in again.".to_owned(),
        SyncError::Offline | SyncError::Busy => {
            "Google Drive could not be reached. Check your connection and try again.".to_owned()
        }
        e => e.to_string(),
    }
}
