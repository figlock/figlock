//! Keeping a local vault folder matched to its remote (ADR-0026, ADR-0041 decision 3).
//!
//! Sync copies ciphertext between two [`BlobStore`]s and never interprets it: everything it moves is verified
//! later, when the vault reads it (`sync.md` 8). Because every file except `vault.json` and the `devices/`
//! records is create-only and named by its owner, merging the two sides is a set union; the two exceptions
//! follow the signed `revision`. Deletions are only mirrored for `devices/` (a revocation removes a record).
//!
//! Order matters: create-only files first, then `devices/`, then `vault.json` last, so a device that sees a
//! new revision already finds the records and snapshot it names.
//!
//! This crate holds the algorithm only. It has no paths, no environment and no globals: the stores, the
//! [`Clock`] and the [`SyncState`] are passed in, so the CLI and desktop app (`figlock-session`) and the iOS app
//! (`figlock-ffi`, with the parts that need Apple APIs supplied by Swift) run the same code.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use figlock_store::{BlobStore, StoreError};
use sha2::{Digest, Sha256};

/// Files copied at once. Enough to hide Drive's per-request latency, few enough to stay under its rate limits.
const PARALLEL_COPIES: usize = 6;

/// Directories whose files are written once and never replaced.
const CREATE_ONLY: [&str; 3] = ["log/", "snap/", "inbox/"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Warning {
    /// Two devices changed `vault.json` at once (same `revision`, different bytes). One copy was chosen by a
    /// fixed rule; the device whose change lost re-applies it on its next unlock (ADR-0026).
    VaultJsonFork,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Files written to the local folder.
    pub pulled: usize,
    /// Files written to the remote.
    pub pushed: usize,
    /// Files the storage lists but has not downloaded yet (iCloud). A download was asked for each; they are
    /// not missing, and a sync that left any behind is not complete (FIG-226).
    pub waiting: usize,
    pub warnings: Vec<Warning>,
}

/// How often a healthy background sync runs (`sync.md` 10).
pub const INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Waits after consecutive failures: 5 s, 15 s, 60 s, then every 5 minutes.
pub const BACKOFF_SECS: [u64; 4] = [5, 15, 60, 300];

/// How long to wait before the next background sync after `failures` in a row (0 is the first failure).
pub fn backoff(failures: usize) -> std::time::Duration {
    std::time::Duration::from_secs(BACKOFF_SECS[failures.min(BACKOFF_SECS.len() - 1)])
}

#[derive(Debug, PartialEq, Eq)]
pub enum SyncError {
    /// This device has no remote.
    NotEnabled,
    /// The provider refused the credentials: sign in again.
    SignIn,
    /// Could not reach the provider; local still works.
    Offline,
    /// The remote holds a different vault than the local folder. Never merged.
    ForeignVault,
    /// The remote has no vault but this device synced to it before: its folder was deleted or the app was
    /// disconnected. Uploading everything again needs the person's confirmation.
    RemoteLost,
    /// A file the sync needs (`vault.json`) is listed but its bytes are not on this device yet, and the wait
    /// for the OS to download it ran out (iCloud Drive evicts files). Not a missing file: try again later.
    NotDownloaded,
    /// A `vault.json` that is not valid JSON with a `vault_id` and `revision`.
    Damaged,
    /// Another sync on this device is running; it does the work.
    Busy,
    /// Deleting the Drive copy was asked for without the typed confirmation.
    NotConfirmed,
    Other(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SyncError::NotEnabled => "sync is not turned on for this device",
            SyncError::SignIn => "Google sign-in is no longer valid; run `figlock drive login`",
            SyncError::Offline => "could not reach the remote; the local vault is unaffected",
            SyncError::ForeignVault => "the remote holds a different vault than this device's",
            SyncError::RemoteLost => {
                "the remote vault is gone; confirm to upload everything from this device again"
            }
            SyncError::NotDownloaded => {
                "waiting for the storage provider to download the vault; try again shortly"
            }
            SyncError::Damaged => "a vault.json is damaged, so nothing was synced",
            SyncError::Busy => "another sync is already running on this device",
            SyncError::NotConfirmed => {
                "the confirmation was not typed exactly, so nothing was deleted"
            }
            SyncError::Other(m) => m,
        })
    }
}

impl std::error::Error for SyncError {}

impl From<StoreError> for SyncError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Unauthorized => SyncError::SignIn,
            StoreError::Unavailable => SyncError::Offline,
            e => SyncError::Other(format!("storage: {e}")),
        }
    }
}

impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::Other(format!("i/o error: {}", e.kind()))
    }
}

pub struct Opts<'a> {
    /// This device's id in hex. Without `push_all` only the files it wrote are pushed, so a file another
    /// device compacted away is not put back from a stale local copy.
    pub own_device: Option<&'a str>,
    /// Push every local file: the first sync to a remote, or an approved re-upload after it was lost. It only
    /// takes effect while the remote holds no `vault.json`; a remote that has one is not new or lost, so a
    /// device that pulled files from it and then failed its first sync does not put back a file that
    /// another device compacted away in the meantime.
    pub push_all: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Winner {
    Local,
    Remote,
    /// Same `vault.json` on both sides, or neither side has one.
    Same,
}

fn read(store: &dyn BlobStore, path: &str) -> Result<Option<Vec<u8>>, SyncError> {
    match store.get(path) {
        Ok(b) => Ok(Some(b)),
        Err(StoreError::NotFound | StoreError::NotYetAvailable) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// How long a sync waits for the OS to download a file the storage lists but has not fetched (FIG-195).
#[derive(Clone, Copy)]
pub struct Wait {
    pub tries: u32,
    pub every: std::time::Duration,
}

impl Wait {
    /// About 15 seconds in total: long enough for a small file from iCloud, short enough to fail the sync
    /// and let the next one (with its back-off) try again.
    pub const DEFAULT: Wait = Wait {
        tries: 30,
        every: std::time::Duration::from_millis(500),
    };
}

/// Like [`read`] for a file that must be judged by its bytes. A listed file that is not downloaded yet is
/// waited for, then reported as [`SyncError::NotDownloaded`]; it is never treated as missing, which would make
/// a vault look lost or empty.
fn read_waiting(
    store: &dyn BlobStore,
    path: &str,
    wait: Wait,
) -> Result<Option<Vec<u8>>, SyncError> {
    for attempt in 0..=wait.tries {
        match store.get(path) {
            Ok(b) => return Ok(Some(b)),
            Err(StoreError::NotFound) => return Ok(None),
            Err(StoreError::NotYetAvailable) if attempt < wait.tries => {
                std::thread::sleep(wait.every);
            }
            Err(StoreError::NotYetAvailable) => return Err(SyncError::NotDownloaded),
            Err(e) => return Err(e.into()),
        }
    }
    Err(SyncError::NotDownloaded)
}

/// `(vault_id, revision)` from `vault.json`, or `Damaged` when it is not JSON with both. Both are plain fields;
/// the signature is checked on unlock, so this only says whether the file is shaped like a vault's.
pub fn vault_identity(bytes: &[u8]) -> Result<(String, u64), SyncError> {
    identity(bytes)
}

fn identity(bytes: &[u8]) -> Result<(String, u64), SyncError> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| SyncError::Damaged)?;
    match (v["vault_id"].as_str(), v["revision"].as_u64()) {
        (Some(id), Some(rev)) => Ok((id.to_owned(), rev)),
        _ => Err(SyncError::Damaged),
    }
}

/// Every path in the store to "is it available here yet" (a listed file whose bytes are not on the device is
/// not missing). One listing, not one per directory: Google Drive's flat namespace costs a full listing request
/// for each call, and a sync on a slow connection is dominated by them.
fn listing(store: &dyn BlobStore) -> Result<BTreeMap<String, bool>, SyncError> {
    Ok(store
        .list("")?
        .into_iter()
        .map(|e| (e.path, e.available))
        .collect())
}

fn under(all: &BTreeMap<String, bool>, prefix: &str) -> BTreeMap<String, bool> {
    all.iter()
        .filter(|(p, _)| p.starts_with(prefix))
        .map(|(p, a)| (p.clone(), *a))
        .collect()
}

/// What copying one file did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Copied {
    Written,
    /// The destination had it, or the source no longer does.
    Skipped,
    /// The source lists it but its bytes are not on this device yet. Reading it asked the OS to download it.
    NotDownloaded,
}

/// Copy `path` from one store to the other unless the destination already has it. A source file that is not
/// downloaded is read anyway: that read is what asks iCloud to fetch it (FIG-226).
fn copy_new(from: &dyn BlobStore, to: &dyn BlobStore, path: &str) -> Result<Copied, SyncError> {
    let bytes = match from.get(path) {
        Ok(b) => b,
        Err(StoreError::NotFound) => return Ok(Copied::Skipped),
        Err(StoreError::NotYetAvailable) => return Ok(Copied::NotDownloaded),
        Err(e) => return Err(e.into()),
    };
    match to.put_new(path, &bytes) {
        Ok(()) => Ok(Copied::Written),
        Err(StoreError::Exists) => Ok(Copied::Skipped),
        Err(e) => Err(e.into()),
    }
}

struct Mirror<'a> {
    local: &'a dyn BlobStore,
    remote: &'a dyn BlobStore,
    /// What each side held when the sync started.
    local_all: BTreeMap<String, bool>,
    remote_all: BTreeMap<String, bool>,
    report: Report,
}

impl Mirror<'_> {
    fn count(&mut self, into_local: bool) {
        if into_local {
            self.report.pulled += 1;
        } else {
            self.report.pushed += 1;
        }
    }

    /// `(from, to)` for a copy into the local folder or into the remote.
    fn sides(&self, into_local: bool) -> (&dyn BlobStore, &dyn BlobStore) {
        if into_local {
            (self.remote, self.local)
        } else {
            (self.local, self.remote)
        }
    }

    /// Copy files that must not exist at the destination yet, a few at a time (`jobs` is `(into_local, path)`).
    /// Someone else having written one first is fine. Each file is its own request chain and Drive answers in
    /// about a second, so a first upload of many files is dominated by waiting; the files have distinct
    /// names, so they cannot interfere. Stops at the first error (the lowest-numbered job's, for a stable
    /// report); files already copied stay, and the next sync finishes the rest.
    fn create_all(&mut self, jobs: Vec<(bool, String)>) -> Result<(), SyncError> {
        let next = AtomicUsize::new(0);
        let failed = AtomicBool::new(false);
        let done: Mutex<Vec<(usize, Result<Copied, SyncError>)>> = Mutex::new(Vec::new());
        let (local, remote) = (self.local, self.remote);
        std::thread::scope(|scope| {
            for _ in 0..PARALLEL_COPIES.min(jobs.len()) {
                scope.spawn(|| {
                    while !failed.load(Ordering::Relaxed) {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some((into_local, path)) = jobs.get(i) else {
                            break;
                        };
                        let (from, to) = if *into_local {
                            (remote, local)
                        } else {
                            (local, remote)
                        };
                        let r = copy_new(from, to, path);
                        failed.fetch_or(r.is_err(), Ordering::Relaxed);
                        done.lock().unwrap_or_else(|e| e.into_inner()).push((i, r));
                    }
                });
            }
        });
        let mut done = done.into_inner().unwrap_or_else(|e| e.into_inner());
        done.sort_by_key(|(i, _)| *i);
        let mut first_err = None;
        for (i, r) in done {
            match r {
                Ok(Copied::Written) => self.count(jobs[i].0),
                Ok(Copied::NotDownloaded) => self.report.waiting += 1,
                Ok(Copied::Skipped) => {}
                Err(e) => first_err = first_err.or(Some(e)),
            }
        }
        first_err.map_or(Ok(()), Err)
    }

    fn replace(&mut self, into_local: bool, path: &str) -> Result<(), SyncError> {
        let (from, to) = self.sides(into_local);
        let bytes = match from.get(path) {
            Ok(b) => b,
            Err(StoreError::NotFound) => return Ok(()),
            Err(StoreError::NotYetAvailable) => {
                self.report.waiting += 1;
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        if read(to, path)?.as_deref() != Some(bytes.as_slice()) {
            to.put_replace(path, &bytes)?;
            self.count(into_local);
        }
        Ok(())
    }

    fn create_only(&mut self, prefix: &str, opts: &Opts) -> Result<(), SyncError> {
        let (l, r) = (
            under(&self.local_all, prefix),
            under(&self.remote_all, prefix),
        );
        let mut jobs = Vec::new();
        for path in r.keys() {
            if !l.contains_key(path) {
                jobs.push((true, path.clone()));
            }
        }
        for path in l.keys() {
            if !r.contains_key(path) && pushable(path, prefix, opts) {
                jobs.push((false, path.clone()));
            }
        }
        self.create_all(jobs)
    }

    /// The authority's `devices/` records win outright: a rotation re-signs them and a revocation removes one.
    fn devices(&mut self, winner: Winner) -> Result<(), SyncError> {
        let (l, r) = (
            under(&self.local_all, "devices/"),
            under(&self.remote_all, "devices/"),
        );
        match winner {
            Winner::Same => {
                let mut jobs = Vec::new();
                for path in r.keys() {
                    if !l.contains_key(path) {
                        jobs.push((true, path.clone()));
                    }
                }
                for path in l.keys() {
                    if !r.contains_key(path) {
                        jobs.push((false, path.clone()));
                    }
                }
                self.create_all(jobs)?;
            }
            w => {
                let into_local = w == Winner::Remote;
                let (src, dst) = if into_local { (&r, &l) } else { (&l, &r) };
                for path in src.keys() {
                    self.replace(into_local, path)?;
                }
                let gone: Vec<&String> = dst.keys().filter(|p| !src.contains_key(*p)).collect();
                for path in gone {
                    self.sides(into_local).1.delete(path)?;
                    self.count(into_local);
                }
            }
        }
        Ok(())
    }
}

fn pushable(path: &str, prefix: &str, opts: &Opts) -> bool {
    if opts.push_all || prefix == "inbox/" {
        return true;
    }
    opts.own_device.is_some_and(|own| {
        path.starts_with(&format!("log/{own}/")) || path.starts_with(&format!("snap/{own}-"))
    })
}

fn pick_vault_json(
    local: &Option<Vec<u8>>,
    remote: &Option<Vec<u8>>,
    opts: &Opts,
    warnings: &mut Vec<Warning>,
) -> Result<Winner, SyncError> {
    match (local, remote) {
        (None, None) => Ok(Winner::Same),
        (Some(_), None) if opts.push_all => Ok(Winner::Local),
        (Some(_), None) => Err(SyncError::RemoteLost),
        (None, Some(_)) => Ok(Winner::Remote),
        (Some(l), Some(r)) => {
            let ((lid, lrev), (rid, rrev)) = (identity(l)?, identity(r)?);
            if lid != rid {
                return Err(SyncError::ForeignVault);
            }
            Ok(match lrev.cmp(&rrev) {
                std::cmp::Ordering::Greater => Winner::Local,
                std::cmp::Ordering::Less => Winner::Remote,
                std::cmp::Ordering::Equal if l == r => Winner::Same,
                std::cmp::Ordering::Equal => {
                    // Both devices wrote the next revision. Every device picks the same winner by the
                    // lowest hash, so the two sides converge with no coordination.
                    warnings.push(Warning::VaultJsonFork);
                    if Sha256::digest(l) <= Sha256::digest(r) {
                        Winner::Local
                    } else {
                        Winner::Remote
                    }
                }
            })
        }
    }
}

/// Pull then push once. Safe to repeat and safe to interrupt: a failure part way leaves each side with a
/// subset of what it should have, and the next run finishes it.
pub fn mirror(
    local: &dyn BlobStore,
    remote: &dyn BlobStore,
    opts: &Opts,
) -> Result<Report, SyncError> {
    mirror_waiting(local, remote, opts, Wait::DEFAULT)
}

fn mirror_waiting(
    local: &dyn BlobStore,
    remote: &dyn BlobStore,
    opts: &Opts,
    wait: Wait,
) -> Result<Report, SyncError> {
    let mut warnings = Vec::new();
    // List first: a Drive listing also fills the id cache that makes the reads below one request each.
    let (local_all, remote_all) = (listing(local)?, listing(remote)?);
    let (lv, rv) = (
        read_waiting(local, "vault.json", wait)?,
        read_waiting(remote, "vault.json", wait)?,
    );
    let opts = &Opts {
        own_device: opts.own_device,
        push_all: opts.push_all && rv.is_none(),
    };
    let winner = pick_vault_json(&lv, &rv, opts, &mut warnings)?;
    if lv.is_none() && rv.is_none() {
        return Ok(Report::default());
    }
    let mut m = Mirror {
        local,
        remote,
        local_all,
        remote_all,
        report: Report {
            warnings,
            ..Report::default()
        },
    };
    for prefix in CREATE_ONLY {
        m.create_only(prefix, opts)?;
    }
    let waiting_before = m.report.waiting;
    m.devices(winner)?;
    if m.report.waiting > waiting_before {
        return Ok(m.report);
    }
    match winner {
        Winner::Local => m.replace(false, "vault.json")?,
        Winner::Remote => m.replace(true, "vault.json")?,
        Winner::Same => {}
    }
    Ok(m.report)
}

// ---- running it for this device ----

// ---- what a device supplies ----

/// The time, in Unix milliseconds. Passed in so the engine reads no clock of its own.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// The small record a device keeps about its syncing. It lives wherever the platform keeps device state (a
/// file on desktop, the app group on iPhone); the engine only asks and tells.
pub trait SyncState: Send + Sync {
    /// Whether this device has synced to its remote before, so an empty remote means "lost", not "new".
    fn synced_before(&self) -> bool;
    /// When this device last finished a sync, for the status line.
    fn last_sync_ms(&self) -> Option<u64>;
    /// Whether the last sync was refused because the provider no longer honours this device's sign-in.
    fn sign_in_refused(&self) -> bool;
    fn mark_synced(&self, now_ms: u64);
    fn mark_sign_in_refused(&self);
    /// A new sign-in replaces the refused one.
    fn clear_sign_in_refused(&self);
    /// Forget that this device synced, as turning sync off does, so turning it on again is a fresh upload.
    fn forget(&self);
}

/// Sync this device's local folder with its remote. `confirm_reupload` is the person's yes to uploading
/// everything again after the remote was lost.
pub fn run(
    local: &dyn BlobStore,
    remote: &dyn BlobStore,
    state: &dyn SyncState,
    clock: &dyn Clock,
    own_device: Option<&str>,
    confirm_reupload: bool,
) -> Result<Report, SyncError> {
    let report = mirror(
        local,
        remote,
        &Opts {
            own_device,
            push_all: !state.synced_before() || confirm_reupload,
        },
    )
    .inspect_err(|e| {
        if *e == SyncError::SignIn {
            state.mark_sign_in_refused();
        }
    })?;
    // Neither side held a vault, so nothing was synced. Marking it would make an empty remote look "lost"
    // once the vault arrives (another remote of this device delivers it first, FIG-196).
    if local.get("vault.json").is_ok() {
        state.mark_synced(clock.now_ms());
    }
    Ok(report)
}

/// Whether the local folder already holds a vault, so an app can skip opening the remote (a vault that exists
/// opens offline). A vault.json that is listed but not downloaded yet is waited for (then `NotDownloaded`), never taken as missing.
pub fn has_vault(local: &dyn BlobStore) -> Result<bool, SyncError> {
    Ok(read_waiting(local, "vault.json", Wait::DEFAULT)?.is_some())
}

/// Makes sure the local folder holds a vault. Pulls only, and touches the remote only when the local folder has
/// no vault yet: a device with a local vault must open offline.
pub fn fill_local(local: &dyn BlobStore, remote: &dyn BlobStore) -> Result<(), SyncError> {
    if has_vault(local)? {
        return Ok(());
    }
    pull_into(local, remote)
}

/// Opening an existing vault from the remote during setup (open or join): always looks at the remote, and
/// refuses when the local folder already holds a different vault, so a stale copy is never unlocked by mistake.
pub fn open_from_remote(local: &dyn BlobStore, remote: &dyn BlobStore) -> Result<(), SyncError> {
    let Some(have) = read_waiting(local, "vault.json", Wait::DEFAULT)? else {
        return pull_into(local, remote);
    };
    if let Some(theirs) = read_waiting(remote, "vault.json", Wait::DEFAULT)?
        && identity(&have)?.0 != identity(&theirs)?.0
    {
        return Err(SyncError::ForeignVault);
    }
    Ok(())
}

fn pull_into(local: &dyn BlobStore, remote: &dyn BlobStore) -> Result<(), SyncError> {
    mirror(
        local,
        remote,
        &Opts {
            own_device: None,
            push_all: false,
        },
    )?;
    Ok(())
}

/// Push and pull for a join in progress, before this device has an id: it sends its request and collects the
/// answer. Only the `first` exchange pushes every local file; later ones push the inbox only.
pub fn exchange(
    local: &dyn BlobStore,
    remote: &dyn BlobStore,
    first: bool,
) -> Result<Report, SyncError> {
    mirror(
        local,
        remote,
        &Opts {
            own_device: None,
            push_all: first,
        },
    )
}

/// Take files out of the remote too. Files in `inbox/` are mirrored create-only, so one deleted only here
/// would be pulled straight back. Best effort: a file that stays is only clutter, and never an open invite
/// (the invite itself is gone from memory).
pub fn remove_remote(remote: &dyn BlobStore, paths: &[String]) {
    for path in paths {
        let _ = remote.delete(path);
    }
}

/// Delete this vault's copy in the remote: every file it holds for it, and nothing of this device (ADR-0035).
/// Needs the typed confirmation. The caller turns the remote off in its own config and then forgets the sync
/// state: a device left syncing would at once offer to upload the vault again.
pub fn delete_remote(
    local: &dyn BlobStore,
    remote: &dyn BlobStore,
    typed: &str,
) -> Result<Deleted, SyncError> {
    if !delete_confirmed(typed) {
        return Err(SyncError::NotConfirmed);
    }
    delete_all(local, remote)
}

/// What a person types to delete the vault's copy in Google Drive (FIG-122, ADR-0035). A phrase, not the vault
/// name: every vault here is called "Personal", so the name would be typed without thinking.
pub const DELETE_REMOTE_PHRASE: &str = "delete from Google Drive";

/// Whether `typed` is the confirmation phrase. Case and the space around it do not matter; nothing else does.
pub fn delete_confirmed(typed: &str) -> bool {
    typed.trim().eq_ignore_ascii_case(DELETE_REMOTE_PHRASE)
}

/// The files of this vault in the Drive app folder, gone. `files` is how many were deleted.
#[derive(Debug, PartialEq, Eq)]
pub struct Deleted {
    pub files: usize,
}

/// The deletion itself, between two stores. Refuses unless the remote's `vault.json` is this vault's (the local
/// copy names it), so another vault in the same Drive is never touched. `vault.json` goes last: a run that is
/// interrupted can be repeated, because the remote still says whose it is. A file another device uploads
/// meanwhile is swept up on the next pass, a few at most.
fn delete_all(local: &dyn BlobStore, remote: &dyn BlobStore) -> Result<Deleted, SyncError> {
    const PASSES: usize = 3;
    let ours = read_waiting(local, "vault.json", Wait::DEFAULT)?.ok_or_else(|| {
        SyncError::Other("this device has no vault to match the Drive copy against".into())
    })?;
    let id = identity(&ours)?.0;
    let mut files = 0;
    let mut checked = false;
    for _ in 0..PASSES {
        let all = remote.list("")?;
        if all.is_empty() {
            break;
        }
        match read_waiting(remote, "vault.json", Wait::DEFAULT)? {
            Some(theirs) if identity(&theirs)?.0 == id => checked = true,
            Some(_) => return Err(SyncError::ForeignVault),
            // After the first pass `vault.json` is already gone and what is left was checked then.
            None if checked => {}
            None => {
                return Err(SyncError::Other(
                    "the Drive copy has no vault.json, so FigLock cannot tell it is this vault's; nothing was deleted"
                        .into(),
                ));
            }
        }
        for e in all.iter().filter(|e| e.path != "vault.json") {
            remote.delete(&e.path)?;
            files += 1;
        }
        if all.iter().any(|e| e.path == "vault.json") {
            remote.delete("vault.json")?;
            files += 1;
        }
    }
    if !remote.list("")?.is_empty() {
        return Err(SyncError::Other(
            "some files are still in Google Drive; run the deletion again".into(),
        ));
    }
    Ok(Deleted { files })
}
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use figlock_store::{Entry, LocalFolder};

    use super::*;

    fn store() -> (tempfile::TempDir, LocalFolder) {
        let d = tempfile::tempdir().unwrap();
        let s = LocalFolder::open(d.path()).unwrap();
        (d, s)
    }

    fn vault_json(id: &str, revision: u64, tag: &str) -> Vec<u8> {
        serde_json::json!({ "vault_id": id, "revision": revision, "tag": tag })
            .to_string()
            .into_bytes()
    }

    fn put(s: &dyn BlobStore, path: &str, bytes: &[u8]) {
        s.put_replace(path, bytes).unwrap();
    }

    fn all(s: &dyn BlobStore) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        for prefix in ["devices/", "log/", "snap/", "inbox/"] {
            for e in s.list(prefix).unwrap() {
                out.insert(e.path.clone(), s.get(&e.path).unwrap());
            }
        }
        if let Ok(v) = s.get("vault.json") {
            out.insert("vault.json".into(), v);
        }
        out
    }

    fn opts(own: Option<&str>, push_all: bool) -> Opts<'_> {
        Opts {
            own_device: own,
            push_all,
        }
    }

    const A: &str = "aa";
    const B: &str = "bb";

    /// Device A's local folder with a vault, one device record and one op file.
    fn device_a() -> (tempfile::TempDir, LocalFolder) {
        let (d, s) = store();
        put(&s, "vault.json", &vault_json("v1", 1, "x"));
        put(&s, "devices/aa.json", b"rec-a");
        put(&s, "log/aa/00000001.op", b"op-a1");
        (d, s)
    }

    /// A store whose `vault.json` is evicted for its first `evicted_gets` reads, as iCloud Drive does.
    struct Evicting {
        inner: LocalFolder,
        evicted_gets: AtomicUsize,
    }

    impl BlobStore for Evicting {
        fn get(&self, path: &str) -> figlock_store::Result<Vec<u8>> {
            if path == "vault.json" && self.evicted_gets.load(Ordering::SeqCst) > 0 {
                self.evicted_gets.fetch_sub(1, Ordering::SeqCst);
                return Err(StoreError::NotYetAvailable);
            }
            self.inner.get(path)
        }
        fn put_new(&self, path: &str, bytes: &[u8]) -> figlock_store::Result<()> {
            self.inner.put_new(path, bytes)
        }
        fn put_replace(&self, path: &str, bytes: &[u8]) -> figlock_store::Result<()> {
            self.inner.put_replace(path, bytes)
        }
        fn list(&self, prefix: &str) -> figlock_store::Result<Vec<Entry>> {
            self.inner.list(prefix)
        }
        fn delete(&self, path: &str) -> figlock_store::Result<()> {
            self.inner.delete(path)
        }
    }

    const QUICK: Wait = Wait {
        tries: 3,
        every: std::time::Duration::from_millis(1),
    };

    #[test]
    fn an_evicted_vault_json_is_waited_for_and_then_syncs() {
        let (_a, la) = device_a();
        let (_r, inner) = store();
        put(&inner, "vault.json", &vault_json("v1", 1, "x"));
        let remote = Evicting {
            inner,
            evicted_gets: AtomicUsize::new(2),
        };
        mirror_waiting(&la, &remote, &opts(Some(A), false), QUICK).unwrap();
        assert!(remote.inner.get("log/aa/00000001.op").is_ok());
    }

    #[test]
    fn an_evicted_vault_json_is_never_taken_as_missing() {
        // Taken as missing it would look like a lost remote (`RemoteLost`) or let a first sync push everything.
        let (_a, la) = device_a();
        let (_r, inner) = store();
        put(&inner, "vault.json", &vault_json("v1", 1, "x"));
        let remote = Evicting {
            inner,
            evicted_gets: AtomicUsize::new(100),
        };
        for push_all in [false, true] {
            assert_eq!(
                mirror_waiting(&la, &remote, &opts(Some(A), push_all), QUICK),
                Err(SyncError::NotDownloaded)
            );
        }
        assert!(remote.inner.get("log/aa/00000001.op").is_err());
    }

    /// A store whose op files of other devices are cloud-only until `arrive` is called, as iCloud Drive lists
    /// them: they show up in the listing as not available, and reading one asks for the download.
    struct CloudOnly {
        inner: LocalFolder,
        cloud_only: std::sync::atomic::AtomicBool,
        download_requests: AtomicUsize,
    }

    impl CloudOnly {
        fn is_op(path: &str) -> bool {
            path.starts_with("log/") || path.starts_with("devices/")
        }
        fn arrive(&self) {
            self.cloud_only.store(false, Ordering::SeqCst);
        }
    }

    impl BlobStore for CloudOnly {
        fn get(&self, path: &str) -> figlock_store::Result<Vec<u8>> {
            if Self::is_op(path) && self.cloud_only.load(Ordering::SeqCst) {
                self.download_requests.fetch_add(1, Ordering::SeqCst);
                return Err(StoreError::NotYetAvailable);
            }
            self.inner.get(path)
        }
        fn put_new(&self, path: &str, bytes: &[u8]) -> figlock_store::Result<()> {
            self.inner.put_new(path, bytes)
        }
        fn put_replace(&self, path: &str, bytes: &[u8]) -> figlock_store::Result<()> {
            self.inner.put_replace(path, bytes)
        }
        fn list(&self, prefix: &str) -> figlock_store::Result<Vec<Entry>> {
            let cloud_only = self.cloud_only.load(Ordering::SeqCst);
            Ok(self
                .inner
                .list(prefix)?
                .into_iter()
                .map(|mut e| {
                    if cloud_only && Self::is_op(&e.path) {
                        e.available = false;
                    }
                    e
                })
                .collect())
        }
        fn delete(&self, path: &str) -> figlock_store::Result<()> {
            self.inner.delete(path)
        }
    }

    #[test]
    fn cloud_only_files_are_requested_and_counted_as_waiting_until_they_arrive() {
        let (_a, la) = device_a();
        let (_r, inner) = store();
        put(&inner, "vault.json", &vault_json("v1", 1, "x"));
        put(&inner, "log/bb/00000001.op", b"op-b1");
        let remote = CloudOnly {
            inner,
            cloud_only: std::sync::atomic::AtomicBool::new(true),
            download_requests: AtomicUsize::new(0),
        };

        let first = mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert_eq!(first.waiting, 1);
        assert!(remote.download_requests.load(Ordering::SeqCst) >= 1);
        assert!(la.get("log/bb/00000001.op").is_err());

        remote.arrive();
        let second = mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert_eq!((second.waiting, second.pulled), (0, 1));
        assert_eq!(la.get("log/bb/00000001.op").unwrap(), b"op-b1");
    }

    #[test]
    fn a_cloud_only_record_on_the_winning_side_is_requested_and_counted_as_waiting() {
        let (_a, la) = device_a();
        let (_r, inner) = store();
        put(&inner, "vault.json", &vault_json("v1", 2, "rotated"));
        put(&inner, "devices/aa.json", b"rec-a-resigned");
        let remote = CloudOnly {
            inner,
            cloud_only: std::sync::atomic::AtomicBool::new(true),
            download_requests: AtomicUsize::new(0),
        };

        let first = mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert_eq!(first.waiting, 1);
        assert!(remote.download_requests.load(Ordering::SeqCst) >= 1);
        assert_eq!(la.get("devices/aa.json").unwrap(), b"rec-a");

        remote.arrive();
        let second = mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert_eq!(second.waiting, 0);
        assert_eq!(la.get("devices/aa.json").unwrap(), b"rec-a-resigned");
    }

    #[test]
    fn two_devices_converge_through_the_remote() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        let (_b, lb) = store();

        let up = mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        assert_eq!((up.pulled, up.pushed), (0, 3));

        let down = mirror(&lb, &remote, &opts(Some(B), true)).unwrap();
        assert_eq!((down.pulled, down.pushed), (3, 0));
        assert_eq!(all(&lb), all(&la));

        put(&lb, "log/bb/00000001.op", b"op-b1");
        put(&la, "log/aa/00000002.op", b"op-a2");
        mirror(&lb, &remote, &opts(Some(B), false)).unwrap();
        mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        mirror(&lb, &remote, &opts(Some(B), false)).unwrap();
        assert_eq!(all(&la), all(&lb));
        assert_eq!(all(&la), all(&remote));
        assert_eq!(all(&la).len(), 5);
    }

    #[test]
    fn syncing_twice_changes_nothing() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        let again = mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert_eq!(again, Report::default());
    }

    #[test]
    fn without_push_all_only_own_files_go_up() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        put(&remote, "vault.json", &vault_json("v1", 1, "x"));
        // Another device's file that the remote no longer has (it was compacted there) stays local only.
        put(&la, "log/bb/00000001.op", b"op-b1");
        put(&la, "snap/aa-00000001-00000001.snap", b"snap-a");
        mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert!(remote.get("log/aa/00000001.op").is_ok());
        assert!(remote.get("snap/aa-00000001-00000001.snap").is_ok());
        assert_eq!(remote.get("log/bb/00000001.op"), Err(StoreError::NotFound));

        // Asking to push everything changes nothing while the remote holds a vault: it is neither new nor lost.
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        assert_eq!(remote.get("log/bb/00000001.op"), Err(StoreError::NotFound));

        // The first sync to a new remote, or an approved re-upload to a lost one, pushes everything.
        let (_new, empty) = store();
        mirror(&la, &empty, &opts(Some(A), true)).unwrap();
        assert!(empty.get("log/bb/00000001.op").is_ok());
    }

    #[test]
    fn a_failed_first_sync_does_not_put_back_a_compacted_file() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();

        // B pulls A's files, but its first sync never finishes, so B is still asking to push everything.
        let (_b, lb) = store();
        mirror(&lb, &remote, &opts(Some(B), true)).unwrap();
        assert!(lb.get("log/aa/00000001.op").is_ok());

        // A compacts the op away on its side and on the remote; B still holds a stale copy.
        la.delete("log/aa/00000001.op").unwrap();
        remote.delete("log/aa/00000001.op").unwrap();

        mirror(&lb, &remote, &opts(Some(B), true)).unwrap();
        assert_eq!(remote.get("log/aa/00000001.op"), Err(StoreError::NotFound));
    }

    #[test]
    fn the_higher_revision_wins_and_the_device_records_follow_it() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();

        // Another device revoked "cc" and rotated: revision 2, with cc's record gone.
        put(&la, "devices/cc.json", b"rec-c");
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        put(&remote, "vault.json", &vault_json("v1", 2, "rotated"));
        put(&remote, "devices/aa.json", b"rec-a-resigned");
        remote.delete("devices/cc.json").unwrap();

        let r = mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert!(r.warnings.is_empty());
        assert_eq!(
            la.get("vault.json").unwrap(),
            vault_json("v1", 2, "rotated")
        );
        assert_eq!(la.get("devices/aa.json").unwrap(), b"rec-a-resigned");
        assert_eq!(la.get("devices/cc.json"), Err(StoreError::NotFound));

        // And the other way: local is newer, so the remote takes it.
        put(&la, "vault.json", &vault_json("v1", 3, "local"));
        put(&la, "devices/dd.json", b"rec-d");
        mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert_eq!(
            remote.get("vault.json").unwrap(),
            vault_json("v1", 3, "local")
        );
        assert_eq!(remote.get("devices/dd.json").unwrap(), b"rec-d");
    }

    #[test]
    fn a_vault_json_fork_converges_on_the_same_winner_from_both_sides() {
        let (_a, la) = device_a();
        let (_b, lb) = store();
        let (_r, remote) = store();
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        mirror(&lb, &remote, &opts(Some(B), false)).unwrap();

        // Both devices wrote revision 2, differently.
        put(&la, "vault.json", &vault_json("v1", 2, "from-a"));
        put(&lb, "vault.json", &vault_json("v1", 2, "from-b"));

        let first = mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert!(first.warnings.is_empty(), "the remote still had revision 1");
        let second = mirror(&lb, &remote, &opts(Some(B), false)).unwrap();
        assert_eq!(second.warnings, vec![Warning::VaultJsonFork]);
        mirror(&la, &remote, &opts(Some(A), false)).unwrap();

        let winner = remote.get("vault.json").unwrap();
        assert_eq!(la.get("vault.json").unwrap(), winner);
        assert_eq!(lb.get("vault.json").unwrap(), winner);
        let lowest = [vault_json("v1", 2, "from-a"), vault_json("v1", 2, "from-b")]
            .into_iter()
            .min_by_key(|b| Sha256::digest(b))
            .unwrap();
        assert_eq!(winner, lowest);
    }

    #[test]
    fn a_different_vault_is_never_merged() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        put(&remote, "vault.json", &vault_json("other", 9, "x"));
        put(&remote, "log/zz/00000001.op", b"theirs");
        assert_eq!(
            mirror(&la, &remote, &opts(Some(A), true)),
            Err(SyncError::ForeignVault)
        );
        assert_eq!(remote.get("log/aa/00000001.op"), Err(StoreError::NotFound));
        assert_eq!(la.get("log/zz/00000001.op"), Err(StoreError::NotFound));
    }

    #[test]
    fn a_stale_local_vault_is_refused_and_a_matching_one_left_alone() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        assert_eq!(open_from_remote(&la, &remote), Ok(()));
        put(&remote, "vault.json", &vault_json("v1", 5, "x"));
        put(&remote, "log/zz/00000001.op", b"theirs");
        assert_eq!(open_from_remote(&la, &remote), Ok(()));
        assert_eq!(la.get("log/zz/00000001.op"), Err(StoreError::NotFound));
        put(&remote, "vault.json", &vault_json("other", 5, "x"));
        assert_eq!(open_from_remote(&la, &remote), Err(SyncError::ForeignVault));
    }

    #[test]
    fn later_join_polls_do_not_resurrect_compacted_files() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        put(&remote, "vault.json", &vault_json("v1", 1, "x"));
        put(&la, "inbox/req", b"req");
        mirror(&la, &remote, &opts(None, false)).unwrap();
        assert!(remote.get("inbox/req").is_ok());
        assert_eq!(remote.get("log/aa/00000001.op"), Err(StoreError::NotFound));
    }

    #[test]
    fn a_damaged_vault_json_stops_the_sync() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        put(&remote, "vault.json", b"{not json");
        assert_eq!(
            mirror(&la, &remote, &opts(Some(A), true)),
            Err(SyncError::Damaged)
        );
    }

    #[test]
    fn a_lost_remote_is_only_refilled_after_confirmation() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        // Synced before (push_all false), and the remote is now empty.
        assert_eq!(
            mirror(&la, &remote, &opts(Some(A), false)),
            Err(SyncError::RemoteLost)
        );
        assert!(all(&remote).is_empty());
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        assert_eq!(all(&remote), all(&la));
    }

    #[test]
    fn a_new_local_copy_is_filled_from_the_remote_and_pushes_nothing() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        let (_n, fresh) = store();
        pull_into(&fresh, &remote).unwrap();
        assert_eq!(all(&fresh), all(&remote));
        // An empty remote leaves an empty local copy, for the caller to report "no vault".
        let (_e, empty) = store();
        let (_n2, fresh2) = store();
        pull_into(&fresh2, &empty).unwrap();
        assert!(all(&fresh2).is_empty());
    }

    #[test]
    fn nothing_anywhere_is_a_quiet_no_op() {
        let (_a, la) = store();
        let (_r, remote) = store();
        assert_eq!(
            mirror(&la, &remote, &opts(None, false)).unwrap(),
            Report::default()
        );
    }

    /// A store that works for `budget` calls and then reports the provider unreachable or signed out.
    struct Dies {
        inner: LocalFolder,
        budget: AtomicUsize,
        error: StoreError,
    }

    impl Dies {
        fn spend(&self) -> Result<(), StoreError> {
            let left = self.budget.load(Ordering::SeqCst);
            if left == 0 {
                return Err(self.error);
            }
            self.budget.store(left - 1, Ordering::SeqCst);
            Ok(())
        }
    }

    impl BlobStore for Dies {
        fn get(&self, p: &str) -> Result<Vec<u8>, StoreError> {
            self.spend()?;
            self.inner.get(p)
        }
        fn put_new(&self, p: &str, b: &[u8]) -> Result<(), StoreError> {
            self.spend()?;
            self.inner.put_new(p, b)
        }
        fn put_replace(&self, p: &str, b: &[u8]) -> Result<(), StoreError> {
            self.spend()?;
            self.inner.put_replace(p, b)
        }
        fn list(&self, p: &str) -> Result<Vec<Entry>, StoreError> {
            self.spend()?;
            self.inner.list(p)
        }
        fn delete(&self, p: &str) -> Result<(), StoreError> {
            self.spend()?;
            self.inner.delete(p)
        }
    }

    #[test]
    fn an_interrupted_sync_finishes_on_the_next_run_and_local_is_untouched() {
        let (_a, la) = device_a();
        put(&la, "log/aa/00000002.op", b"op-a2");
        put(&la, "log/aa/00000003.op", b"op-a3");
        let before = all(&la);
        let (_r, inner) = store();
        for budget in 0..40 {
            let remote = Dies {
                inner: inner.clone(),
                budget: AtomicUsize::new(budget),
                error: StoreError::Unavailable,
            };
            match mirror(&la, &remote, &opts(Some(A), true)) {
                Ok(_) => {}
                Err(e) => assert_eq!(e, SyncError::Offline),
            }
            assert_eq!(all(&la), before, "a failing remote never changes local");
        }
        mirror(&la, &inner, &opts(Some(A), true)).unwrap();
        assert_eq!(all(&inner), all(&la));
    }

    #[test]
    fn a_refused_sign_in_is_reported_as_one() {
        let (_a, la) = device_a();
        let (_r, inner) = store();
        let remote = Dies {
            inner,
            budget: AtomicUsize::new(0),
            error: StoreError::Unauthorized,
        };
        assert_eq!(
            mirror(&la, &remote, &opts(Some(A), true)),
            Err(SyncError::SignIn)
        );
    }

    /// Counts listing requests, the expensive call on Google Drive.
    struct Lists<'a> {
        inner: &'a LocalFolder,
        lists: AtomicUsize,
    }

    impl BlobStore for Lists<'_> {
        fn get(&self, p: &str) -> Result<Vec<u8>, StoreError> {
            self.inner.get(p)
        }
        fn put_new(&self, p: &str, b: &[u8]) -> Result<(), StoreError> {
            self.inner.put_new(p, b)
        }
        fn put_replace(&self, p: &str, b: &[u8]) -> Result<(), StoreError> {
            self.inner.put_replace(p, b)
        }
        fn list(&self, p: &str) -> Result<Vec<Entry>, StoreError> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            self.inner.list(p)
        }
        fn delete(&self, p: &str) -> Result<(), StoreError> {
            self.inner.delete(p)
        }
    }

    #[test]
    fn a_sync_lists_each_side_once() {
        let (_a, la) = device_a();
        let (_r, inner) = store();
        let remote = Lists {
            inner: &inner,
            lists: AtomicUsize::new(0),
        };
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        assert_eq!(remote.lists.load(Ordering::SeqCst), 1);
        mirror(&la, &remote, &opts(Some(A), false)).unwrap();
        assert_eq!(remote.lists.load(Ordering::SeqCst), 2, "one per sync");
    }

    /// Blocks in `put_new` long enough for the others to overlap, and records how many were inside at once.
    struct Overlap<'a> {
        inner: &'a LocalFolder,
        inside: AtomicUsize,
        most: AtomicUsize,
    }

    impl BlobStore for Overlap<'_> {
        fn get(&self, p: &str) -> Result<Vec<u8>, StoreError> {
            self.inner.get(p)
        }
        fn put_new(&self, p: &str, b: &[u8]) -> Result<(), StoreError> {
            let now = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.inside.fetch_sub(1, Ordering::SeqCst);
            self.inner.put_new(p, b)
        }
        fn put_replace(&self, p: &str, b: &[u8]) -> Result<(), StoreError> {
            self.inner.put_replace(p, b)
        }
        fn list(&self, p: &str) -> Result<Vec<Entry>, StoreError> {
            self.inner.list(p)
        }
        fn delete(&self, p: &str) -> Result<(), StoreError> {
            self.inner.delete(p)
        }
    }

    #[test]
    fn files_are_copied_a_few_at_a_time_and_all_of_them_arrive() {
        let (_a, la) = device_a();
        for i in 2..=40 {
            put(
                &la,
                &format!("log/aa/{i:08}.op"),
                format!("op-{i}").as_bytes(),
            );
        }
        let (_r, inner) = store();
        let remote = Overlap {
            inner: &inner,
            inside: AtomicUsize::new(0),
            most: AtomicUsize::new(0),
        };
        let r = mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        assert_eq!(r.pushed, 42);
        assert_eq!(all(&inner), all(&la));
        let most = remote.most.load(Ordering::SeqCst);
        assert!(most > 1, "uploads overlapped");
        assert!(most <= PARALLEL_COPIES, "and stayed bounded, got {most}");
    }

    #[test]
    fn a_failure_among_parallel_copies_is_reported_and_the_rest_finish_next_time() {
        let (_a, la) = device_a();
        for i in 2..=30 {
            put(&la, &format!("log/aa/{i:08}.op"), b"op");
        }
        let (_r, inner) = store();
        for budget in [3, 11, 25] {
            let (_f, fresh) = store();
            let remote = Dies {
                inner: fresh,
                budget: AtomicUsize::new(budget),
                error: StoreError::Unauthorized,
            };
            assert_eq!(
                mirror(&la, &remote, &opts(Some(A), true)),
                Err(SyncError::SignIn)
            );
        }
        mirror(&la, &inner, &opts(Some(A), true)).unwrap();
        assert_eq!(all(&inner), all(&la));
    }

    // ---- deleting the Drive copy (FIG-122) ----

    fn remote_with_vault(id: &str) -> (tempfile::TempDir, LocalFolder) {
        let (d, s) = store();
        put(&s, "vault.json", &vault_json(id, 3, "r"));
        put(&s, "devices/aa.json", b"rec-a");
        put(&s, "log/aa/00000001.op", b"op-a1");
        put(&s, "snap/aa-1.snap", b"snap");
        (d, s)
    }

    fn names(s: &dyn BlobStore) -> Vec<String> {
        s.list("").unwrap().into_iter().map(|e| e.path).collect()
    }

    #[test]
    fn only_the_exact_phrase_confirms() {
        assert!(delete_confirmed("delete from Google Drive"));
        assert!(delete_confirmed("  Delete From Google DRIVE \n"));
        for no in [
            "",
            "delete",
            "yes",
            "Personal",
            "delete from drive",
            "delete from Google Drive now",
        ] {
            assert!(!delete_confirmed(no), "{no:?}");
        }
    }

    #[test]
    fn deleting_removes_every_remote_file_and_nothing_local() {
        let (_a, local) = device_a();
        let (_r, remote) = remote_with_vault("v1");
        put(&local, "vault.json", &vault_json("v1", 3, "r"));
        let before = all(&local);
        let done = delete_all(&local, &remote).unwrap();
        assert_eq!(done, Deleted { files: 4 });
        assert!(names(&remote).is_empty());
        assert_eq!(all(&local), before, "the local vault is untouched");
    }

    #[test]
    fn deleting_twice_is_fine() {
        let (_a, local) = device_a();
        let (_r, remote) = remote_with_vault("v1");
        delete_all(&local, &remote).unwrap();
        assert_eq!(delete_all(&local, &remote), Ok(Deleted { files: 0 }));
    }

    #[test]
    fn a_different_vault_in_the_same_drive_is_never_deleted() {
        let (_a, local) = device_a();
        let (_r, remote) = remote_with_vault("someone-else");
        let before = all(&remote);
        assert_eq!(delete_all(&local, &remote), Err(SyncError::ForeignVault));
        assert_eq!(all(&remote), before);
    }

    #[test]
    fn without_a_local_vault_or_a_remote_vault_json_nothing_is_deleted() {
        let (_r, remote) = remote_with_vault("v1");
        let before = all(&remote);
        let (_e, empty) = store();
        assert!(matches!(
            delete_all(&empty, &remote),
            Err(SyncError::Other(_))
        ));
        assert_eq!(all(&remote), before);

        let (_a, local) = device_a();
        remote.delete("vault.json").unwrap();
        let before = all(&remote);
        assert!(matches!(
            delete_all(&local, &remote),
            Err(SyncError::Other(_))
        ));
        assert_eq!(all(&remote), before);
    }

    /// A store that lets another device upload one more file while `vault.json` is being deleted.
    struct Racy<'a> {
        inner: &'a LocalFolder,
        once: std::sync::Once,
    }

    impl BlobStore for Racy<'_> {
        fn get(&self, p: &str) -> figlock_store::Result<Vec<u8>> {
            self.inner.get(p)
        }
        fn put_new(&self, p: &str, b: &[u8]) -> figlock_store::Result<()> {
            self.inner.put_new(p, b)
        }
        fn put_replace(&self, p: &str, b: &[u8]) -> figlock_store::Result<()> {
            self.inner.put_replace(p, b)
        }
        fn list(&self, prefix: &str) -> figlock_store::Result<Vec<figlock_store::Entry>> {
            self.inner.list(prefix)
        }
        fn delete(&self, p: &str) -> figlock_store::Result<()> {
            if p == "vault.json" {
                self.once.call_once(|| {
                    self.inner.put_new("log/bb/00000001.op", b"late").unwrap();
                });
            }
            self.inner.delete(p)
        }
    }

    #[test]
    fn a_file_uploaded_during_the_deletion_is_swept_up_too() {
        let (_a, local) = device_a();
        let (_r, inner) = remote_with_vault("v1");
        let racy = Racy {
            inner: &inner,
            once: std::sync::Once::new(),
        };
        let done = delete_all(&local, &racy).unwrap();
        assert_eq!(done.files, 5);
        assert!(names(&inner).is_empty());
    }

    #[test]
    fn a_second_device_finds_the_vault_gone_and_uploads_nothing_by_itself() {
        let (_a, la) = device_a();
        let (_r, remote) = store();
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        let (_b, lb) = store();
        mirror(&lb, &remote, &opts(Some(B), true)).unwrap();

        delete_all(&la, &remote).unwrap();
        assert_eq!(
            mirror(&lb, &remote, &opts(Some(B), false)),
            Err(SyncError::RemoteLost)
        );
        assert!(names(&remote).is_empty(), "nothing went back up unasked");
        // Only the person's yes uploads it again.
        mirror(&lb, &remote, &opts(Some(B), true)).unwrap();
        assert!(names(&remote).contains(&"vault.json".to_owned()));
    }

    /// A remote whose provider refuses every call.
    struct Refused;

    impl BlobStore for Refused {
        fn get(&self, _: &str) -> Result<Vec<u8>, StoreError> {
            Err(StoreError::Unauthorized)
        }
        fn put_new(&self, _: &str, _: &[u8]) -> Result<(), StoreError> {
            Err(StoreError::Unauthorized)
        }
        fn put_replace(&self, _: &str, _: &[u8]) -> Result<(), StoreError> {
            Err(StoreError::Unauthorized)
        }
        fn list(&self, _: &str) -> Result<Vec<Entry>, StoreError> {
            Err(StoreError::Unauthorized)
        }
        fn delete(&self, _: &str) -> Result<(), StoreError> {
            Err(StoreError::Unauthorized)
        }
    }

    #[derive(Default)]
    struct Memory {
        synced: std::sync::atomic::AtomicBool,
        last: Mutex<Option<u64>>,
        refused: std::sync::atomic::AtomicBool,
        forgot: AtomicUsize,
    }

    impl SyncState for Memory {
        fn synced_before(&self) -> bool {
            self.synced.load(Ordering::SeqCst)
        }
        fn last_sync_ms(&self) -> Option<u64> {
            *self.last.lock().unwrap()
        }
        fn sign_in_refused(&self) -> bool {
            self.refused.load(Ordering::SeqCst)
        }
        fn mark_synced(&self, now_ms: u64) {
            self.synced.store(true, Ordering::SeqCst);
            *self.last.lock().unwrap() = Some(now_ms);
        }
        fn mark_sign_in_refused(&self) {
            self.refused.store(true, Ordering::SeqCst);
        }
        fn clear_sign_in_refused(&self) {
            self.refused.store(false, Ordering::SeqCst);
        }
        fn forget(&self) {
            self.synced.store(false, Ordering::SeqCst);
            self.forgot.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Fixed(u64);

    impl Clock for Fixed {
        fn now_ms(&self) -> u64 {
            self.0
        }
    }

    #[test]
    fn the_first_run_pushes_everything_and_records_the_time_it_was_given() {
        let (_a, local) = device_a();
        let (_r, remote) = store();
        let state = Memory::default();
        let r = run(&local, &remote, &state, &Fixed(42), Some(A), false).unwrap();
        assert_eq!(r.pushed, 3);
        assert_eq!(state.last_sync_ms(), Some(42));
        assert!(state.synced_before());
        assert!(!state.sign_in_refused());
    }

    #[test]
    fn after_a_sync_an_empty_remote_is_lost_and_needs_a_yes() {
        let (_a, local) = device_a();
        let (_r, remote) = store();
        let state = Memory::default();
        run(&local, &remote, &state, &Fixed(1), Some(A), false).unwrap();
        for e in remote.list("").unwrap() {
            remote.delete(&e.path).unwrap();
        }
        assert_eq!(
            run(&local, &remote, &state, &Fixed(2), Some(A), false),
            Err(SyncError::RemoteLost)
        );
        assert_eq!(
            state.last_sync_ms(),
            Some(1),
            "a failed run records nothing"
        );
        assert!(run(&local, &remote, &state, &Fixed(3), Some(A), true).is_ok());
        assert_eq!(state.last_sync_ms(), Some(3));
    }

    #[test]
    fn a_run_with_no_vault_on_either_side_is_not_a_sync() {
        // A device that gets its vault from another remote must not find this empty one "lost" afterwards.
        let (_l, local) = store();
        let (_r, remote) = store();
        let state = Memory::default();
        run(&local, &remote, &state, &Fixed(1), Some(A), false).unwrap();
        assert!(!state.synced_before());
        assert_eq!(state.last_sync_ms(), None);
        put(&local, "vault.json", &vault_json("v1", 1, "x"));
        assert!(run(&local, &remote, &state, &Fixed(2), Some(A), false).is_ok());
        assert!(
            remote.get("vault.json").is_ok(),
            "the first upload, not a lost remote"
        );
    }

    #[test]
    fn a_refused_sign_in_is_recorded_in_the_state_and_not_a_sync() {
        let (_a, local) = device_a();
        let remote = Refused;
        let state = Memory::default();
        assert_eq!(
            run(&local, &remote, &state, &Fixed(1), Some(A), false),
            Err(SyncError::SignIn)
        );
        assert!(state.sign_in_refused());
        assert!(!state.synced_before());
    }

    #[test]
    fn deleting_the_remote_needs_the_confirmation() {
        let (_a, local) = device_a();
        let (_r, remote) = remote_with_vault("v1");
        for typed in ["", "yes"] {
            assert_eq!(
                delete_remote(&local, &remote, typed),
                Err(SyncError::NotConfirmed)
            );
        }
        assert!(!names(&remote).is_empty());

        let done = delete_remote(&local, &remote, DELETE_REMOTE_PHRASE).unwrap();
        assert_eq!(done.files, 4);
        assert!(names(&remote).is_empty());
    }

    #[test]
    fn a_vault_that_is_here_opens_without_looking_at_the_remote() {
        let (_a, local) = device_a();
        assert_eq!(has_vault(&local), Ok(true));
        assert_eq!(fill_local(&local, &Refused), Ok(()));
        let (_e, empty) = store();
        assert_eq!(has_vault(&empty), Ok(false));
        assert_eq!(
            fill_local(&empty, &Refused),
            Err(SyncError::SignIn),
            "an empty local folder does need the remote"
        );
    }
}
