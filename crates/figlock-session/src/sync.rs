//! Keeping the local vault folder matched to its remote (ADR-0026).
//!
//! The algorithm lives in `figlock-sync` (ADR-0041 decision 3); this module runs it for this device: it opens
//! the stores from the config, keeps the sync state in `sync-state.json`, and holds the lock that stops two
//! syncs from mirroring into the local folder at once.

use std::fs;
use std::path::{Path, PathBuf};

use figlock_sync::{Clock, SyncState as SyncStateStore};

pub use figlock_sync::{
    BACKOFF_SECS, DELETE_REMOTE_PHRASE, Deleted, INTERVAL, Opts, Report, SyncError, Warning,
    backoff, delete_confirmed, mirror,
};

use crate::config::{self, Config, Place};
use crate::error::{Fail, FailKind};

impl From<Fail> for SyncError {
    fn from(f: Fail) -> Self {
        SyncError::Other(f.0)
    }
}

impl From<SyncError> for Fail {
    /// The message stays the CLI's text; the kind lets a window word the cases a person can act on itself.
    fn from(e: SyncError) -> Self {
        let kind = match e {
            SyncError::SignIn => FailKind::StorageSignedOut,
            SyncError::Offline => FailKind::StorageUnreachable,
            _ => FailKind::Other,
        };
        Fail::of(kind, e.to_string())
    }
}

/// The system clock.
struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        crate::session::now_ms()
    }
}

/// The sync state in `sync-state.json` in the config folder.
struct FileState;

fn state_path() -> Result<PathBuf, Fail> {
    Ok(config::home()?.join("sync-state.json"))
}

fn state() -> Option<serde_json::Value> {
    let bytes = fs::read(state_path().ok()?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

impl SyncStateStore for FileState {
    fn synced_before(&self) -> bool {
        state().is_some_and(|v| v["synced"] == true)
    }

    fn last_sync_ms(&self) -> Option<u64> {
        state().and_then(|v| v["last_sync_ms"].as_u64())
    }

    fn sign_in_refused(&self) -> bool {
        state().is_some_and(|v| v["sign_in_refused"] == true)
    }

    fn mark_synced(&self, now_ms: u64) {
        if let Ok(p) = state_path() {
            let _ = fs::write(
                p,
                serde_json::json!({ "synced": true, "last_sync_ms": now_ms }).to_string(),
            );
        }
    }

    fn mark_sign_in_refused(&self) {
        let Ok(p) = state_path() else { return };
        let mut v = state().unwrap_or_else(|| serde_json::json!({}));
        v["sign_in_refused"] = true.into();
        let _ = fs::write(p, v.to_string());
    }

    fn clear_sign_in_refused(&self) {
        let Some(mut v) = state() else { return };
        if v.as_object_mut()
            .and_then(|o| o.remove("sign_in_refused"))
            .is_some()
            && let Ok(p) = state_path()
        {
            let _ = fs::write(p, v.to_string());
        }
    }

    fn forget(&self) {
        if let Ok(p) = state_path() {
            let _ = fs::remove_file(p);
        }
    }
}

/// Whether the last sync was refused because Google no longer honours this device's sign-in. Kept in the sync
/// state so every screen and the CLI agree, without a network call, until a sign-in or a sync works again.
pub fn sign_in_refused() -> bool {
    FileState.sign_in_refused()
}

/// A new sign-in replaces the refused one.
pub fn clear_sign_in_refused() {
    FileState.clear_sign_in_refused();
}

/// When this device last finished a sync, for the status line.
pub fn last_sync_ms() -> Option<u64> {
    FileState.last_sync_ms()
}

/// Forget that this device synced, as turning sync off does, so turning it on again is a fresh upload.
pub fn forget_state() {
    FileState.forget();
}

/// Held for the whole of a sync so two never mirror into the local folder at once.
struct SyncLock(#[expect(dead_code)] fs::File);

impl SyncLock {
    fn acquire_in(dir: &Path) -> Result<Self, SyncError> {
        fs::create_dir_all(dir)?;
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("sync.lock"))?;
        match f.try_lock() {
            Ok(()) => Ok(SyncLock(f)),
            Err(fs::TryLockError::WouldBlock) => Err(SyncError::Busy),
            Err(fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
}

/// Sync this device's local folder with its remote. `confirm_reupload` is the person's yes to uploading
/// everything again after the remote was lost.
pub fn run(cfg: &Config, confirm_reupload: bool) -> Result<Report, SyncError> {
    let remote_kind = cfg.place.remote.ok_or(SyncError::NotEnabled)?;
    let _lock = SyncLock::acquire_in(&config::home()?)?;
    let local = crate::store::open(&cfg.place)?;
    let remote = crate::store::open_remote(remote_kind)?;
    let own = config::device_id().map(hex::encode);
    figlock_sync::run(
        local.as_ref(),
        remote.as_ref(),
        &FileState,
        &SystemClock,
        own.as_deref(),
        confirm_reupload,
    )
}

/// Called whenever a vault is opened. Makes sure the local folder holds a vault when a remote is set (a
/// Drive-only device from before ADR-0026). Pulls only, and touches the network only when the local folder has
/// no vault yet: a device with a local vault must open offline.
pub fn fill_local(place: &Place) -> Result<(), SyncError> {
    let Some(remote_kind) = place.remote else {
        return Ok(());
    };
    fs::create_dir_all(&place.local)?;
    let local = crate::store::open_local(&place.local)?;
    // Only a local folder without a vault needs the remote, so a vault that opens offline never opens it.
    if figlock_sync::has_vault(local.as_ref())? {
        return Ok(());
    }
    figlock_sync::fill_local(
        local.as_ref(),
        crate::store::open_remote(remote_kind)?.as_ref(),
    )
}

/// Opening an existing vault from the remote during setup (open or join): always looks at the remote, and
/// refuses when the local folder already holds a different vault, so a stale copy is never unlocked by mistake.
pub fn open_from_remote(place: &Place) -> Result<(), SyncError> {
    let Some(remote_kind) = place.remote else {
        return Ok(());
    };
    fs::create_dir_all(&place.local)?;
    let local = crate::store::open_local(&place.local)?;
    let remote = crate::store::open_remote(remote_kind)?;
    figlock_sync::open_from_remote(local.as_ref(), remote.as_ref())
}

/// Take files out of the remote too (see `figlock_sync::remove_remote`).
pub fn remove_remote(place: &Place, paths: &[String]) {
    let Some(kind) = place.remote else {
        return;
    };
    let Ok(remote) = crate::store::open_remote(kind) else {
        return;
    };
    figlock_sync::remove_remote(remote.as_ref(), paths);
}

/// Push and pull for a join in progress, before this device has a config or id: it sends its request and
/// collects the answer. Only the `first` exchange pushes every local file; later ones push the inbox only.
pub fn exchange(place: &Place, first: bool) -> Result<Report, SyncError> {
    let Some(remote_kind) = place.remote else {
        return Ok(Report::default());
    };
    let local = crate::store::open_local(&place.local)?;
    let remote = crate::store::open_remote(remote_kind)?;
    figlock_sync::exchange(local.as_ref(), remote.as_ref(), first)
}

/// Delete this vault's copy in Google Drive: every file the app folder holds for it, and nothing of this device
/// (ADR-0035). Needs the typed confirmation and a sign-in. It works whether sync is on or off, and turns sync
/// off on this device afterwards: a device left syncing would at once offer to upload the vault again.
/// Another device that still has sync on finds the vault gone and asks its person before uploading it again.
pub fn delete_remote(cfg: &Config, typed: &str) -> Result<Deleted, SyncError> {
    if !delete_confirmed(typed) {
        return Err(SyncError::NotConfirmed);
    }
    let _lock = SyncLock::acquire_in(&config::home()?)?;
    let local = crate::store::open(&cfg.place)?;
    let remote = crate::store::open_drive().map_err(|_| SyncError::SignIn)?;
    let deleted = figlock_sync::delete_remote(local.as_ref(), remote.as_ref(), &FileState, typed)?;
    if cfg.place.remote.is_some() {
        config::set_remote(None)?;
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_sync_cannot_start_while_one_holds_the_lock() {
        let d = tempfile::tempdir().unwrap();
        let first = SyncLock::acquire_in(d.path()).unwrap();
        assert!(matches!(
            SyncLock::acquire_in(d.path()),
            Err(SyncError::Busy)
        ));
        drop(first);
        assert!(SyncLock::acquire_in(d.path()).is_ok());
    }

    #[test]
    fn no_confirmation_deletes_nothing_and_asks_for_nothing_else() {
        let cfg = Config {
            place: Place {
                local: PathBuf::from("/no/such/folder"),
                remote: Some(config::Remote::Drive),
            },
            device_name: "x".into(),
            autolock_secs: None,
        };
        for typed in ["", "yes", "Personal"] {
            assert_eq!(delete_remote(&cfg, typed), Err(SyncError::NotConfirmed));
        }
    }

    #[test]
    fn a_failed_io_step_keeps_the_cli_wording() {
        let e = SyncError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(e.to_string(), "i/o error: permission denied");
    }
}
