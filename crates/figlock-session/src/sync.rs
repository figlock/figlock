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

use crate::config::{self, Config, Place, Remote};
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
            SyncError::NotDownloaded => FailKind::StorageSyncing,
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

/// One remote's sync state, in `sync-state.json` in the config folder: `{"remotes": {"<remote id>": {...}}}`.
/// A file from before FIG-196 holds a single flat record, which was always Drive's; it is read as that and
/// rewritten in the new shape the next time anything is recorded.
struct FileState {
    key: String,
}

impl FileState {
    fn of(remote: &Remote) -> FileState {
        FileState { key: remote.id() }
    }
}

fn state_path() -> Result<PathBuf, Fail> {
    Ok(config::home()?.join("sync-state.json"))
}

fn state() -> Option<serde_json::Value> {
    let bytes = fs::read(state_path().ok()?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The records in `all`, by remote id, with a flat record from before FIG-196 filed under Drive.
fn records(all: Option<serde_json::Value>) -> serde_json::Map<String, serde_json::Value> {
    let Some(serde_json::Value::Object(mut o)) = all else {
        return serde_json::Map::new();
    };
    if let Some(serde_json::Value::Object(m)) = o.remove("remotes") {
        return m;
    }
    let mut m = serde_json::Map::new();
    if !o.is_empty() {
        m.insert(Remote::Drive.id(), serde_json::Value::Object(o));
    }
    m
}

fn save(m: serde_json::Map<String, serde_json::Value>) {
    let Ok(p) = state_path() else { return };
    if m.is_empty() {
        let _ = fs::remove_file(p);
    } else {
        let _ = fs::write(p, serde_json::json!({ "remotes": m }).to_string());
    }
}

impl FileState {
    fn record(&self) -> Option<serde_json::Value> {
        records(state()).remove(&self.key)
    }

    fn update(&self, f: impl FnOnce(&mut serde_json::Value)) {
        let mut m = records(state());
        f(m.entry(self.key.clone())
            .or_insert_with(|| serde_json::json!({})));
        save(m);
    }
}

impl FileState {
    /// Keep why the last sync failed, so a status screen can say so with no network call. A sync that works
    /// already replaced the whole record (`mark_synced`), which drops an old error. Another sync running here
    /// is not a failure of the remote.
    fn note_result(&self, result: &Result<Report, SyncError>) {
        if let Err(e) = result
            && !matches!(e, SyncError::Busy)
        {
            let text = e.to_string();
            self.update(|v| v["last_error"] = text.into());
        }
    }
}

impl SyncStateStore for FileState {
    fn synced_before(&self) -> bool {
        self.record().is_some_and(|v| v["synced"] == true)
    }

    fn last_sync_ms(&self) -> Option<u64> {
        self.record().and_then(|v| v["last_sync_ms"].as_u64())
    }

    fn sign_in_refused(&self) -> bool {
        self.record().is_some_and(|v| v["sign_in_refused"] == true)
    }

    fn mark_synced(&self, now_ms: u64) {
        // A sync that works ends a refusal too, as it always has: the record is replaced, not merged.
        self.update(|v| *v = serde_json::json!({ "synced": true, "last_sync_ms": now_ms }));
    }

    fn mark_sign_in_refused(&self) {
        self.update(|v| v["sign_in_refused"] = true.into());
    }

    fn clear_sign_in_refused(&self) {
        if self
            .record()
            .is_some_and(|v| v.get("sign_in_refused").is_some())
        {
            self.update(|v| {
                if let Some(o) = v.as_object_mut() {
                    o.remove("sign_in_refused");
                }
            });
        }
    }

    fn forget(&self) {
        let mut m = records(state());
        if m.remove(&self.key).is_some() {
            save(m);
        }
    }
}

/// Whether the last sync was refused because Google no longer honours this device's sign-in. Kept in the sync
/// state so every screen and the CLI agree, without a network call, until a sign-in or a sync works again.
pub fn sign_in_refused() -> bool {
    FileState::of(&Remote::Drive).sign_in_refused()
}

/// A new sign-in replaces the refused one.
pub fn clear_sign_in_refused() {
    FileState::of(&Remote::Drive).clear_sign_in_refused();
}

/// When this device last finished a sync to any remote, for the status line.
pub fn last_sync_ms() -> Option<u64> {
    records(state())
        .values()
        .filter_map(|v| v["last_sync_ms"].as_u64())
        .max()
}

/// Why the last sync to `remote` failed, or `None` when it worked (or has not run).
pub fn last_error_of(remote: &Remote) -> Option<String> {
    FileState::of(remote)
        .record()
        .and_then(|v| v["last_error"].as_str().map(str::to_owned))
}

/// When this device last finished a sync to `remote`.
pub fn last_sync_ms_of(remote: &Remote) -> Option<u64> {
    FileState::of(remote).last_sync_ms()
}

/// Forget that this device synced anywhere, so turning sync on again is a fresh upload.
pub fn forget_state() {
    if let Ok(p) = state_path() {
        let _ = fs::remove_file(p);
    }
}

/// Forget that this device synced to `remote`, as turning that remote off does. The others keep theirs.
pub fn forget_state_of(remote: &Remote) {
    FileState::of(remote).forget();
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

/// How one remote's sync went.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub remote: Remote,
    pub result: Result<Report, SyncError>,
}

/// Sync this device's local folder with one remote: its own same-vault check, remote-lost confirmation and
/// state. `confirm_reupload` is the person's yes to uploading everything again after this remote was lost.
pub fn run_remote(
    cfg: &Config,
    remote: &Remote,
    confirm_reupload: bool,
) -> Result<Report, SyncError> {
    let _lock = SyncLock::acquire_in(&config::home()?)?;
    let state = FileState::of(remote);
    let result = mirror_remote(cfg, remote, &state, confirm_reupload);
    state.note_result(&result);
    result
}

fn mirror_remote(
    cfg: &Config,
    remote: &Remote,
    state: &FileState,
    confirm_reupload: bool,
) -> Result<Report, SyncError> {
    let local = crate::store::open(&cfg.place)?;
    let store = crate::store::open_remote(remote, &cfg.place.local)?;
    let own = config::device_id().map(hex::encode);
    figlock_sync::run(
        local.as_ref(),
        store.as_ref(),
        state,
        &SystemClock,
        own.as_deref(),
        confirm_reupload,
    )
}

/// Sync every remote, one after the other and each on its own: one that fails, is offline or holds another
/// vault never stops the rest. `confirm_reupload` applies to each remote that turns out to be lost.
pub fn run_all(cfg: &Config, confirm_reupload: bool) -> Vec<Outcome> {
    cfg.place
        .remotes
        .iter()
        .map(|remote| Outcome {
            remote: remote.clone(),
            result: run_remote(cfg, remote, confirm_reupload),
        })
        .collect()
}

/// Sync every remote (see [`run_all`]) and sum up: the combined report when all went well, else the first
/// error, after every remote has had its turn.
pub fn run(cfg: &Config, confirm_reupload: bool) -> Result<Report, SyncError> {
    if !cfg.place.syncs() {
        return Err(SyncError::NotEnabled);
    }
    let mut total = Report::default();
    let mut first_err = None;
    for Outcome { result, .. } in run_all(cfg, confirm_reupload) {
        match result {
            Ok(r) => {
                total.pulled += r.pulled;
                total.pushed += r.pushed;
                total.waiting += r.waiting;
                for w in r.warnings {
                    if !total.warnings.contains(&w) {
                        total.warnings.push(w);
                    }
                }
            }
            Err(e) => first_err = first_err.or(Some(e)),
        }
    }
    first_err.map_or(Ok(total), Err)
}

/// Called whenever a vault is opened. Makes sure the local folder holds a vault when a remote is set (a
/// Drive-only device from before ADR-0026). Pulls only, and touches the network only when the local folder has
/// no vault yet: a device with a local vault must open offline. With several remotes the first that has the
/// vault fills it.
pub fn fill_local(place: &Place) -> Result<(), SyncError> {
    if !place.syncs() {
        return Ok(());
    }
    fs::create_dir_all(&place.local)?;
    let local = crate::store::open_local(&place.local)?;
    // Only a local folder without a vault needs a remote, so a vault that opens offline never opens one.
    if figlock_sync::has_vault(local.as_ref())? {
        return Ok(());
    }
    let mut first_err = None;
    for remote in &place.remotes {
        let step = crate::store::open_remote(remote, &place.local)
            .map_err(SyncError::from)
            .and_then(|r| figlock_sync::fill_local(local.as_ref(), r.as_ref()));
        match step {
            Ok(()) if figlock_sync::has_vault(local.as_ref())? => return Ok(()),
            Ok(()) => {}
            Err(e) => first_err = first_err.or(Some(e)),
        }
    }
    first_err.map_or(Ok(()), Err)
}

/// Opening an existing vault from a remote during setup (open or join): always looks at the remote, and
/// refuses when the local folder already holds a different vault, so a stale copy is never unlocked by mistake.
pub fn open_from_remote(place: &Place) -> Result<(), SyncError> {
    if !place.syncs() {
        return Ok(());
    }
    fs::create_dir_all(&place.local)?;
    let local = crate::store::open_local(&place.local)?;
    for remote in &place.remotes {
        let store = crate::store::open_remote(remote, &place.local)?;
        figlock_sync::open_from_remote(local.as_ref(), store.as_ref())?;
    }
    Ok(())
}

/// Take files out of the remotes too (see `figlock_sync::remove_remote`).
pub fn remove_remote(place: &Place, paths: &[String]) {
    for remote in &place.remotes {
        if let Ok(store) = crate::store::open_remote(remote, &place.local) {
            figlock_sync::remove_remote(store.as_ref(), paths);
        }
    }
}

/// Push and pull for a join in progress, before this device has a config or id: it sends its request and
/// collects the answer. Only the `first` exchange pushes every local file; later ones push the inbox only.
pub fn exchange(place: &Place, first: bool) -> Result<Report, SyncError> {
    let local = crate::store::open_local(&place.local)?;
    let mut total = Report::default();
    let mut first_err = None;
    for remote in &place.remotes {
        let step = crate::store::open_remote(remote, &place.local)
            .map_err(SyncError::from)
            .and_then(|r| figlock_sync::exchange(local.as_ref(), r.as_ref(), first));
        match step {
            Ok(r) => {
                total.pulled += r.pulled;
                total.pushed += r.pushed;
                total.waiting += r.waiting;
            }
            Err(e) => first_err = first_err.or(Some(e)),
        }
    }
    first_err.map_or(Ok(total), Err)
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
    let deleted = figlock_sync::delete_remote(local.as_ref(), remote.as_ref(), typed)?;
    if cfg.place.has_drive() {
        config::remove_remote(&Remote::Drive)?;
    }
    forget_state_of(&Remote::Drive);
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
                remotes: vec![config::Remote::Drive],
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
