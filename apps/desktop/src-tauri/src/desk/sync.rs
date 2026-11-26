//! Keeping the local vault matched to Google Drive, as the window sees it (ADR-0026).
//!
//! Sync works on ciphertext only, so it runs whether the vault is locked or not. A background thread calls
//! [`Desk::sync_tick`] every second; it syncs every 15 seconds, soon after a local save, and backs off when
//! something fails. What the person needs to act on (sign in again, confirm an upload) stops the retries until
//! they do.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use figlock_session::config::{self, Remote};
use figlock_session::sync::{self, BACKOFF_SECS, INTERVAL, Report, SyncError, Warning, backoff};
use figlock_session::{drive, icloud};
use serde::Serialize;

use super::{CmdError, CmdRes, Desk};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Idle,
    Offline,
    /// Needs the person: sign in to Google again.
    SignIn,
    /// Needs the person: the vault is gone from this remote; confirm an upload.
    Lost,
    /// Needs the person: this remote holds a different vault.
    Foreign,
    /// A synced folder that is not there: iCloud Drive is off or signed out, or the drive is not connected. It
    /// comes back by itself, so sync keeps trying.
    Unavailable,
    /// iCloud has not finished downloading the vault yet. Not an error: it keeps trying.
    Waiting,
    Failed,
}

/// One remote's sync, kept apart from the others: one failing never slows or blocks another (ADR-0043).
pub(super) struct RemoteState {
    pub(super) kind: Kind,
    message: Option<String>,
    /// This remote's last sync found two devices changing the vault's settings at once. Kept per remote, so a
    /// clean sync of another one does not hide it.
    fork: bool,
    failures: usize,
    next_at: Instant,
}

impl RemoteState {
    fn new() -> Self {
        RemoteState {
            kind: Kind::Idle,
            message: None,
            fork: false,
            failures: 0,
            next_at: Instant::now(),
        }
    }

    /// Stops retrying until the person acts.
    fn blocked(&self) -> bool {
        matches!(self.kind, Kind::SignIn | Kind::Lost | Kind::Foreign)
    }
}

pub(super) struct SyncState {
    running: bool,
    /// By remote id.
    remotes: BTreeMap<String, RemoteState>,
    needs_unlock: bool,
    /// When the open vault last looked for changes another process made in the local folder.
    local_at: Instant,
}

impl SyncState {
    pub(super) fn new() -> Self {
        SyncState {
            running: false,
            remotes: BTreeMap::new(),
            needs_unlock: false,
            local_at: Instant::now(),
        }
    }

    fn remote(&mut self, id: &str) -> &mut RemoteState {
        self.remotes
            .entry(id.to_owned())
            .or_insert_with(RemoteState::new)
    }

    fn kind_of(&self, id: &str) -> Kind {
        self.remotes.get(id).map_or(Kind::Idle, |r| r.kind)
    }

    /// Every place that was tried failed (or is waiting): what was just written has reached none of them.
    pub(super) fn none_synced(&self) -> bool {
        !self.remotes.is_empty() && self.remotes.values().all(|r| r.kind != Kind::Idle)
    }

    /// How long the sync thread may sleep: until the nearest place that is due, or the open vault's next look at
    /// the local folder, never longer than one interval and never in a tight loop (FIG-261).
    fn next_wait(&self, unlocked: bool) -> Duration {
        let now = Instant::now();
        let mut wait = INTERVAL;
        for rs in self.remotes.values().filter(|rs| !rs.blocked()) {
            wait = wait.min(rs.next_at.saturating_duration_since(now));
        }
        if unlocked {
            wait = wait.min((self.local_at + INTERVAL).saturating_duration_since(now));
        }
        wait.max(Duration::from_millis(250))
    }

    /// Forget one remote, as turning it off does.
    fn drop_remote(&mut self, id: &str) {
        self.remotes.remove(id);
    }
}

/// What the window shows for one place the vault syncs to.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteView {
    /// Stable name of the remote, to act on it: `drive`, or `folder:<path>`.
    pub id: String,
    /// `drive`, `icloud` or `folder`.
    pub kind: &'static str,
    /// "Google Drive", "iCloud Drive", or the folder's name.
    pub name: String,
    /// The folder, for `icloud` and `folder`.
    pub path: Option<String>,
    /// `idle`, `syncing`, `offline`, `sign_in`, `lost`, `foreign`, `unavailable`, `waiting` or `failed`.
    pub state: &'static str,
    pub last_sync_secs_ago: Option<u64>,
    /// Words for the person when this place needs their attention.
    pub message: Option<String>,
}

/// iCloud Drive on this Mac, without touching it.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IcloudView {
    /// This is an Apple computer: iCloud Drive can exist here at all.
    pub available: bool,
    /// iCloud Drive is on and signed in.
    pub ready: bool,
    /// `iCloud Drive/FigLock` already exists.
    pub folder_exists: bool,
    /// Words for what to switch on when it is not ready.
    pub help: Option<String>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncView {
    /// Sync is on to at least one place.
    pub enabled: bool,
    /// All places in one word, for the sidebar: `off`, `idle`, `syncing`, or the one that most needs attention.
    pub state: &'static str,
    /// Google sign-in. It is Drive's alone; iCloud has none.
    pub signed_in: bool,
    /// How long ago this Mac signed in to Google. The sign-in carries no account name (the app folder scope
    /// gives no email), so this is all the window can say about it.
    pub signed_in_secs_ago: Option<u64>,
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
    /// Every place it syncs to, each with its own state.
    pub remotes: Vec<RemoteView>,
    pub icloud: IcloudView,
}

/// iCloud Drive on this Mac, with no vault needed: the setup screens ask before there is a config.
pub fn icloud_view() -> IcloudView {
    let ic = icloud::status();
    IcloudView {
        available: ic.apple,
        ready: ic.ready,
        folder_exists: ic.folder_exists,
        help: if ic.ready {
            None
        } else {
            icloud::folder().err().map(|f| f.0)
        },
    }
}

fn name_of(remote: &Remote) -> String {
    match remote {
        Remote::Drive => "Google Drive".to_owned(),
        Remote::Folder(p) if icloud::is_icloud_path(p) => "iCloud Drive".to_owned(),
        Remote::Folder(p) => p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
    }
}

fn kind_name(remote: &Remote) -> &'static str {
    match remote {
        Remote::Drive => "drive",
        Remote::Folder(p) if icloud::is_icloud_path(p) => "icloud",
        Remote::Folder(_) => "folder",
    }
}

fn state_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Idle => "idle",
        Kind::Offline => "offline",
        Kind::SignIn => "sign_in",
        Kind::Lost => "lost",
        Kind::Foreign => "foreign",
        Kind::Unavailable => "unavailable",
        Kind::Waiting => "waiting",
        Kind::Failed => "failed",
    }
}

fn words(kind: Kind, remote: &Remote) -> Option<String> {
    let name = name_of(remote);
    let icloud = kind_name(remote) == "icloud";
    match kind {
        Kind::Idle => None,
        Kind::Offline => Some(format!(
            "Can't reach {name}. The vault works here as normal and will sync when you're back online."
        )),
        Kind::SignIn => {
            Some("Your Google sign-in has expired. Sign in again to keep syncing.".to_owned())
        }
        Kind::Lost => Some(format!(
            "The vault is no longer in {name}. You can upload it again from this Mac."
        )),
        Kind::Foreign => Some(format!(
            "{name} holds a different vault, so sync to it is paused. Nothing was changed."
        )),
        Kind::Unavailable if icloud => Some(icloud::missing_message(match remote {
            Remote::Folder(p) => p,
            Remote::Drive => std::path::Path::new(""),
        })),
        Kind::Unavailable => Some(format!(
            "{name} is not available. If it is on a drive, connect it; sync resumes by itself."
        )),
        Kind::Waiting => Some(
            "Waiting for iCloud to finish syncing the vault to this Mac. It keeps trying."
                .to_owned(),
        ),
        Kind::Failed => Some("Sync ran into a problem. It will try again.".to_owned()),
    }
}

/// How much a state needs the person, to pick the one word for the sidebar. Higher first.
fn severity(kind: Kind) -> u8 {
    match kind {
        Kind::SignIn => 7,
        Kind::Lost => 6,
        Kind::Foreign => 5,
        Kind::Unavailable => 4,
        Kind::Failed => 3,
        Kind::Offline => 2,
        Kind::Waiting => 1,
        Kind::Idle => 0,
    }
}

/// What a failed sync means for `remote`, in the states the window knows.
fn classify(remote: &Remote, e: &SyncError) -> (Kind, Option<String>) {
    match e {
        SyncError::Offline | SyncError::Busy => (Kind::Offline, None),
        SyncError::SignIn => (Kind::SignIn, None),
        SyncError::RemoteLost => (Kind::Lost, None),
        SyncError::ForeignVault => (Kind::Foreign, None),
        SyncError::NotDownloaded => (Kind::Waiting, None),
        // A synced folder that is gone is the drive being away or iCloud being off, not a bug.
        SyncError::Other(_) if matches!(remote, Remote::Folder(p) if !p.is_dir()) => {
            (Kind::Unavailable, None)
        }
        SyncError::Other(m) => (Kind::Failed, Some(m.clone())),
        _ => (Kind::Failed, None),
    }
}

impl Desk {
    pub(super) fn sync_state(&self) -> std::sync::MutexGuard<'_, SyncState> {
        self.sync.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn sync_status(&self) -> CmdRes<SyncView> {
        let cfg = config::load().ok();
        let remotes_cfg: Vec<Remote> = cfg
            .as_ref()
            .map(|c| c.place.remotes.clone())
            .unwrap_or_default();
        let enabled = !remotes_cfg.is_empty();
        let drive = drive::status()?;
        let s = self.sync_state();
        // A stored sign-in Google has stopped honouring is not a sign-in: say so, here and in the CLI.
        let signed_in = drive.signed_in && !sync::sign_in_refused();
        let now = figlock_session::session::now_ms();
        let mut views = Vec::new();
        let mut worst = Kind::Idle;
        let mut message = None;
        for r in &remotes_cfg {
            let id = r.id();
            let mut kind = s.kind_of(&id);
            // A folder that went away shows as such at once, not only after the next sync fails.
            if matches!(r, Remote::Folder(p) if !p.is_dir()) && !s.running {
                kind = Kind::Unavailable;
            }
            let own_message = s
                .remotes
                .get(&id)
                .and_then(|rs| rs.message.clone())
                .filter(|_| kind != Kind::Unavailable)
                .or_else(|| words(kind, r));
            if severity(kind) > severity(worst) {
                worst = kind;
                message = own_message.clone();
            }
            views.push(RemoteView {
                id,
                kind: kind_name(r),
                name: name_of(r),
                path: match r {
                    Remote::Folder(p) => Some(p.display().to_string()),
                    Remote::Drive => None,
                },
                state: if s.running && kind == Kind::Idle {
                    "syncing"
                } else {
                    state_word(kind)
                },
                last_sync_secs_ago: sync::last_sync_ms_of(r).map(|t| now.saturating_sub(t) / 1000),
                message: own_message,
            });
        }
        let state = if !enabled {
            "off"
        } else if s.running {
            "syncing"
        } else {
            state_word(worst)
        };
        Ok(SyncView {
            enabled,
            state,
            signed_in,
            signed_in_secs_ago: config::drive_signed_in_ms()
                .filter(|_| signed_in)
                .map(|t| now.saturating_sub(t) / 1000),
            can_sign_in: drive.can_sign_in,
            last_sync_secs_ago: sync::last_sync_ms().map(|t| now.saturating_sub(t) / 1000),
            message: if enabled { message } else { None },
            fork: enabled && s.remotes.values().any(|r| r.fork),
            needs_unlock: enabled && s.needs_unlock,
            local_folder: cfg.map(|c| c.place.local.display().to_string()),
            remotes: views,
            icloud: icloud_view(),
        })
    }

    /// Sign out of Google: turn Drive sync off first (a Mac with Drive sync on and no sign-in would only sit at
    /// "sign in again"), then revoke the token and forget it. The vault on this Mac, the copy in Drive and any
    /// other place it syncs to are left alone.
    pub fn sync_sign_out(&self) -> CmdRes<super::drive::SignedOut> {
        if config::load().is_ok_and(|c| c.place.has_drive()) {
            config::remove_remote(&Remote::Drive)?;
            sync::forget_state_of(&Remote::Drive);
            self.sync_state().drop_remote(&Remote::Drive.id());
        }
        super::drive::sign_out()
    }

    /// Sync once now: every place, or only `remote` (its id). `reupload` is the person's yes to uploading
    /// everything again after the vault was lost from a place. Resolves with the state afterwards; a sync that
    /// failed is a state, not an error.
    pub fn sync_now(&self, remote: Option<&str>, reupload: bool) -> CmdRes<SyncView> {
        self.run_remotes(Which::Only(remote), reupload);
        self.sync_status()
    }

    /// Run one sync of every place and record how each went. Returns false when none was run (sync is off).
    pub(super) fn run_sync(&self, reupload: bool) -> bool {
        self.run_remotes(Which::Only(None), reupload)
    }

    fn run_remotes(&self, which: Which, reupload: bool) -> bool {
        let Ok(cfg) = config::load() else {
            return false;
        };
        let targets: Vec<Remote> = {
            let s = self.sync_state();
            cfg.place
                .remotes
                .iter()
                .filter(|r| match which {
                    Which::Only(None) => true,
                    Which::Only(Some(id)) => r.id() == id,
                    // Something was saved: every place that is not waiting for the person.
                    Which::Unblocked => s.remotes.get(&r.id()).is_none_or(|rs| !rs.blocked()),
                    // The timer: places whose turn it is, which a failing place pushes back by itself.
                    Which::Due => s
                        .remotes
                        .get(&r.id())
                        .is_none_or(|rs| !rs.blocked() && Instant::now() >= rs.next_at),
                })
                .cloned()
                .collect()
        };
        if targets.is_empty() {
            return false;
        }
        self.sync_state().running = true;
        let results: Vec<(Remote, Result<Report, SyncError>)> = targets
            .into_iter()
            .map(|r| {
                let result = sync::run_remote(&cfg, &r, reupload);
                (r, result)
            })
            .collect();
        // Taking in what was pulled needs the vault; do it before the state says "idle".
        let pulled = results
            .iter()
            .any(|(_, r)| r.as_ref().is_ok_and(|r| r.pulled > 0));
        let absorbed = pulled.then(|| self.absorb_pull());
        let mut s = self.sync_state();
        s.running = false;
        for (remote, result) in results {
            match result {
                Ok(report) => {
                    let rs = s.remote(&remote.id());
                    rs.kind = Kind::Idle;
                    rs.message = None;
                    rs.failures = 0;
                    rs.fork = report.warnings.contains(&Warning::VaultJsonFork);
                    rs.next_at = Instant::now() + INTERVAL;
                    if let Some(needs_unlock) = absorbed {
                        s.needs_unlock |= needs_unlock;
                    }
                }
                Err(SyncError::Busy) => {
                    // Another sync (the CLI's) holds the lock; it is doing this work.
                    let rs = s.remote(&remote.id());
                    rs.next_at = Instant::now() + Duration::from_secs(BACKOFF_SECS[0]);
                }
                Err(e) => {
                    let (kind, message) = classify(&remote, &e);
                    let rs = s.remote(&remote.id());
                    rs.kind = kind;
                    rs.message = message;
                    rs.next_at = Instant::now() + backoff(rs.failures);
                    rs.failures += 1;
                }
            }
        }
        true
    }

    /// A fresh unlock reloads everything, so the notice has been acted on.
    pub(super) fn clear_needs_unlock(&self) {
        self.sync_state().needs_unlock = false;
    }

    #[cfg(test)]
    pub(super) fn flag_needs_unlock_for_test(&self) {
        self.sync_state().needs_unlock = true;
    }

    #[cfg(test)]
    pub(super) fn absorb_pull_for_test(&self) -> bool {
        self.absorb_pull()
    }

    /// Bring what a sync pulled into the unlocked vault. Returns true when the vault needs a fresh unlock to
    /// take in the rest (settings changed elsewhere, or a device's changes could not be applied yet).
    fn absorb_pull(&self) -> bool {
        self.absorb_local().1
    }

    /// Take in what is in the local folder and not yet in the open vault, whoever wrote it: a sync, or another
    /// process on this Mac such as the CLI (FIG-126, ADR-0026). Returns whether the list changed and whether the
    /// vault needs a fresh unlock.
    fn absorb_local(&self) -> (bool, bool) {
        let (changed, needs_unlock, stale) = self.absorb_in_session();
        // The broker hears of it with no session lock held.
        self.tell_broker(changed, stale);
        (changed, needs_unlock)
    }

    /// `(list changed, needs a fresh unlock, the vault's settings changed or this device was removed)`.
    fn absorb_in_session(&self) -> (bool, bool, bool) {
        let mut inner = self.inner();
        let Some(session) = inner.session.as_mut() else {
            return (false, false, false);
        };
        // A revocation made elsewhere shows up as a settings change; the window must hear of it at once and
        // block writes, not at the next unlock (FIG-138).
        let was_removed = session.removed_at_ms().is_some();
        match session.refresh() {
            Ok(r) => {
                let removed = !was_removed && session.removed_at_ms().is_some();
                if r.applied > 0 {
                    inner.cache = None;
                }
                (
                    r.applied > 0 || removed,
                    r.settings_changed || r.stopped,
                    r.settings_changed || removed,
                )
            }
            Err(_) => (false, false, false),
        }
    }

    /// Look for changes another process made to the local folder, now. Returns true when the list changed.
    /// The window calls this when it gains focus, so what the CLI did while it was in the background shows
    /// at once.
    pub fn refresh_local(&self) -> bool {
        self.sync_state().local_at = Instant::now();
        let (changed, needs_unlock) = self.absorb_local();
        if needs_unlock {
            self.sync_state().needs_unlock = true;
        }
        changed
    }

    /// Called every second with the sync tick: while unlocked, looks for local changes every `INTERVAL`, with
    /// or without Drive, like the broker does. Returns true when the list changed.
    pub fn local_tick(&self) -> bool {
        let due = {
            let mut s = self.sync_state();
            let due = s.local_at.elapsed() >= INTERVAL;
            if due {
                s.local_at = Instant::now();
            }
            due
        };
        if !due || self.inner().session.is_none() {
            return false;
        }
        let (changed, needs_unlock) = self.absorb_local();
        if needs_unlock {
            self.sync_state().needs_unlock = true;
        }
        changed
    }

    /// Turn Drive sync on and upload what is already here. Only stays on if the first sync works, so a
    /// different vault in Drive or a dead sign-in leaves the device as it was.
    pub fn sync_enable(&self) -> CmdRes<SyncView> {
        if !drive::status()?.signed_in {
            return Err(CmdError::new("drive_sign_in", "Sign in to Google first."));
        }
        self.enable_remote(Remote::Drive)
    }

    /// Turn iCloud Drive sync on: make `iCloud Drive/FigLock` when it is not there, then upload what is already
    /// here. There is no sign-in; iCloud Drive off or signed out is a clear error that says what to switch on.
    pub fn sync_enable_icloud(&self) -> CmdRes<SyncView> {
        let folder = icloud::folder()?;
        // Never next to another device's vault that is still arriving.
        icloud::refuse_partial_folder(&folder)?;
        let folder = icloud::prepare()?;
        self.enable_remote(Remote::Folder(folder))
    }

    fn enable_remote(&self, remote: Remote) -> CmdRes<SyncView> {
        let cfg = config::load()?;
        if cfg.place.remotes.contains(&remote) {
            return self.sync_status();
        }
        sync::forget_state_of(&remote);
        let cfg = config::add_remote(remote.clone())?;
        // Only this place's first sync decides whether it stays on.
        match sync::run_remote(&cfg, &remote, false) {
            Ok(_) => {
                self.sync_state().drop_remote(&remote.id());
                self.sync_status()
            }
            Err(e) => {
                config::remove_remote(&remote)?;
                sync::forget_state_of(&remote);
                Err(CmdError::new(
                    "sync_failed",
                    format!(
                        "Could not turn on sync, so nothing was changed. {}",
                        plain(&remote, &e)
                    ),
                ))
            }
        }
    }

    /// Delete this vault's copy from Google Drive (FIG-122, ADR-0035). Needs the typed phrase, checked here and not
    /// only by the window, and a Google sign-in; it refuses a Drive that holds a different vault. The vault on this
    /// Mac is never touched, and sync ends up off, whether it was on or not.
    pub fn sync_delete_remote(&self, confirmation: &str) -> CmdRes<SyncView> {
        let cfg = config::load()?;
        match sync::delete_remote(&cfg, confirmation) {
            Ok(_) => {
                *self.sync_state() = SyncState::new();
                self.sync_status()
            }
            Err(SyncError::NotConfirmed) => Err(CmdError::new(
                "not_confirmed",
                format!(
                    "Type \"{}\" exactly to delete the vault from Google Drive. Nothing was deleted.",
                    sync::DELETE_REMOTE_PHRASE
                ),
            )),
            Err(SyncError::ForeignVault) => Err(CmdError::new(
                "foreign_vault",
                "Your Google Drive holds a different vault than this Mac's, so nothing was deleted.",
            )),
            Err(SyncError::SignIn) => Err(CmdError::new(
                "drive_sign_in",
                "Sign in to Google first. Nothing was deleted.",
            )),
            Err(SyncError::Busy) => Err(CmdError::new(
                "sync_failed",
                "A sync is running right now. Wait a moment and try again. Nothing was deleted.",
            )),
            Err(e) => Err(CmdError::new(
                "sync_failed",
                format!(
                    "Could not finish deleting the vault from Google Drive; some files may already be gone. Try again. {}",
                    plain(&Remote::Drive, &e)
                ),
            )),
        }
    }

    /// Stop syncing to Google Drive. The vault on this Mac and the copy in Drive are left alone.
    pub fn sync_disable(&self) -> CmdRes<SyncView> {
        self.sync_disable_remote(&Remote::Drive.id())
    }

    /// Stop syncing to one place (its id), leaving the others. The vault on this Mac and the copy stored there
    /// are left alone.
    pub fn sync_disable_remote(&self, id: &str) -> CmdRes<SyncView> {
        let cfg = config::load()?;
        if let Some(remote) = cfg.place.remotes.iter().find(|r| r.id() == id) {
            config::remove_remote(remote)?;
            sync::forget_state_of(remote);
        }
        self.sync_state().drop_remote(id);
        self.sync_status()
    }

    /// Called every second by the sync thread. Returns true when a sync ran, so the window can refresh what it
    /// shows.
    /// Sleep until the next sync round is due, or until a save wakes it. The sync thread calls this between
    /// rounds instead of waking every second (FIG-261).
    pub fn wait_for_sync_work(&self) {
        let unlocked = self.inner().session.is_some();
        let wait = self.sync_state().next_wait(unlocked);
        self.wake.wait(wait);
    }

    pub fn sync_tick(&self) -> bool {
        let dirty = std::mem::take(&mut self.inner().dirty);
        if self.sync_state().running {
            return false;
        }
        self.run_remotes(if dirty { Which::Unblocked } else { Which::Due }, false)
    }
}

/// Which places a sync round covers.
#[derive(Clone, Copy)]
enum Which<'a> {
    /// All of them, or the one with this id: the person asked.
    Only(Option<&'a str>),
    /// Every one that is not waiting for the person: something was saved.
    Unblocked,
    /// Those whose turn it is.
    Due,
}

/// A sentence for the person from a sync error.
fn plain(remote: &Remote, e: &SyncError) -> String {
    let name = name_of(remote);
    match e {
        SyncError::ForeignVault => format!("{name} already holds a different vault."),
        SyncError::SignIn => "Google refused the sign-in. Sign in again.".to_owned(),
        SyncError::Offline | SyncError::Busy => {
            format!("{name} could not be reached. Check your connection and try again.")
        }
        e => e.to_string(),
    }
}

#[cfg(test)]
mod wait_tests {
    use super::*;

    #[test]
    fn the_wait_ends_at_the_nearest_deadline_and_never_spins() {
        let mut s = SyncState::new();
        // Nothing known and locked: look again in one interval.
        assert_eq!(s.next_wait(false), INTERVAL);
        // A place due in two seconds pulls the wait in.
        s.remote("a").next_at = Instant::now() + Duration::from_secs(2);
        let w = s.next_wait(false);
        assert!(w <= Duration::from_secs(2) && w > Duration::from_millis(1500));
        // A place already due still waits a beat: a blocked loop must not spin.
        s.remote("a").next_at = Instant::now();
        assert_eq!(s.next_wait(false), Duration::from_millis(250));
    }

    #[test]
    fn a_place_waiting_for_the_person_does_not_wake_the_thread() {
        let mut s = SyncState::new();
        let r = s.remote("a");
        r.kind = Kind::SignIn;
        r.next_at = Instant::now();
        assert_eq!(s.next_wait(false), INTERVAL);
    }

    #[test]
    fn an_open_vault_also_wakes_for_the_local_folder_check() {
        let mut s = SyncState::new();
        s.local_at = Instant::now() - INTERVAL + Duration::from_secs(3);
        let w = s.next_wait(true);
        assert!(w <= Duration::from_secs(3) && w > Duration::from_secs(2));
        // Locked, the local folder check is not due, so it does not matter.
        assert_eq!(s.next_wait(false), INTERVAL);
    }
}
