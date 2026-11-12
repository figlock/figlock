//! Keeping the local vault folder matched to its remote (ADR-0026).
//!
//! Sync copies ciphertext between two [`BlobStore`]s and never interprets it: everything it moves is verified
//! later, when the vault reads it (`sync.md` 8). Because every file except `vault.json` and the `devices/`
//! records is create-only and named by its owner, merging the two sides is a set union; the two exceptions
//! follow the signed `revision`. Deletions are only mirrored for `devices/` (a revocation removes a record).
//!
//! Order matters: create-only files first, then `devices/`, then `vault.json` last, so a device that sees a
//! new revision already finds the records and snapshot it names.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use figlock_store::{BlobStore, StoreError};
use sha2::{Digest, Sha256};

use crate::config::{self, Config, Place};
use crate::error::Fail;

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
    pub warnings: Vec<Warning>,
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
    /// A `vault.json` that is not valid JSON with a `vault_id` and `revision`.
    Damaged,
    /// Another sync on this device is running; it does the work.
    Busy,
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
            SyncError::Damaged => "a vault.json is damaged, so nothing was synced",
            SyncError::Busy => "another sync is already running on this device",
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

impl From<Fail> for SyncError {
    fn from(f: Fail) -> Self {
        SyncError::Other(f.0)
    }
}

impl From<std::io::Error> for SyncError {
    fn from(e: std::io::Error) -> Self {
        SyncError::Other(Fail::from(e).0)
    }
}

impl From<SyncError> for Fail {
    fn from(e: SyncError) -> Self {
        Fail(e.to_string())
    }
}

pub struct Opts<'a> {
    /// This device's id in hex. Without `push_all` only the files it wrote are pushed, so a file another
    /// device compacted away is not put back from a stale local copy.
    pub own_device: Option<&'a str>,
    /// Push every local file: the first sync to a remote, or an approved re-upload after it was lost.
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

/// `(vault_id, revision)` from `vault.json`. Both are plain fields; the signature is checked on unlock.
fn identity(bytes: &[u8]) -> Result<(String, u64), SyncError> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| SyncError::Damaged)?;
    match (v["vault_id"].as_str(), v["revision"].as_u64()) {
        (Some(id), Some(rev)) => Ok((id.to_owned(), rev)),
        _ => Err(SyncError::Damaged),
    }
}

/// Path to "is it available here yet". A listed file whose bytes are not on the device is not missing.
fn listing(store: &dyn BlobStore, prefix: &str) -> Result<BTreeMap<String, bool>, SyncError> {
    Ok(store
        .list(prefix)?
        .into_iter()
        .map(|e| (e.path, e.available))
        .collect())
}

struct Mirror<'a> {
    local: &'a dyn BlobStore,
    remote: &'a dyn BlobStore,
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

    /// Copy a file that must not exist at the destination yet. Someone else having written it first is fine.
    fn create(&mut self, into_local: bool, path: &str) -> Result<(), SyncError> {
        let (from, to) = self.sides(into_local);
        let Some(bytes) = read(from, path)? else {
            return Ok(());
        };
        match to.put_new(path, &bytes) {
            Ok(()) => self.count(into_local),
            Err(StoreError::Exists) => {}
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    fn replace(&mut self, into_local: bool, path: &str) -> Result<(), SyncError> {
        let (from, to) = self.sides(into_local);
        let Some(bytes) = read(from, path)? else {
            return Ok(());
        };
        if read(to, path)?.as_deref() != Some(bytes.as_slice()) {
            to.put_replace(path, &bytes)?;
            self.count(into_local);
        }
        Ok(())
    }

    fn create_only(&mut self, prefix: &str, opts: &Opts) -> Result<(), SyncError> {
        let (l, r) = (listing(self.local, prefix)?, listing(self.remote, prefix)?);
        for (path, available) in &r {
            if *available && !l.contains_key(path) {
                self.create(true, path)?;
            }
        }
        for (path, available) in &l {
            if *available && !r.contains_key(path) && pushable(path, prefix, opts) {
                self.create(false, path)?;
            }
        }
        Ok(())
    }

    /// The authority's `devices/` records win outright: a rotation re-signs them and a revocation removes one.
    fn devices(&mut self, winner: Winner) -> Result<(), SyncError> {
        let (l, r) = (
            listing(self.local, "devices/")?,
            listing(self.remote, "devices/")?,
        );
        match winner {
            Winner::Same => {
                for (path, available) in &r {
                    if *available && !l.contains_key(path) {
                        self.create(true, path)?;
                    }
                }
                for (path, available) in &l {
                    if *available && !r.contains_key(path) {
                        self.create(false, path)?;
                    }
                }
            }
            w => {
                let into_local = w == Winner::Remote;
                let (src, dst) = if into_local { (&r, &l) } else { (&l, &r) };
                for (path, available) in src {
                    if *available {
                        self.replace(into_local, path)?;
                    }
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
    let mut warnings = Vec::new();
    let (lv, rv) = (read(local, "vault.json")?, read(remote, "vault.json")?);
    let winner = pick_vault_json(&lv, &rv, opts, &mut warnings)?;
    if lv.is_none() && rv.is_none() {
        return Ok(Report::default());
    }
    let mut m = Mirror {
        local,
        remote,
        report: Report {
            warnings,
            ..Report::default()
        },
    };
    for prefix in CREATE_ONLY {
        m.create_only(prefix, opts)?;
    }
    m.devices(winner)?;
    match winner {
        Winner::Local => m.replace(false, "vault.json")?,
        Winner::Remote => m.replace(true, "vault.json")?,
        Winner::Same => {}
    }
    Ok(m.report)
}

// ---- running it for this device ----

fn state_path() -> Result<PathBuf, Fail> {
    Ok(config::home()?.join("sync-state.json"))
}

fn state() -> Option<serde_json::Value> {
    let bytes = fs::read(state_path().ok()?).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Whether this device has synced to its remote before, so an empty remote means "lost", not "new".
fn synced_before() -> bool {
    state().is_some_and(|v| v["synced"] == true)
}

fn mark_synced(now_ms: u64) {
    if let Ok(p) = state_path() {
        let _ = fs::write(
            p,
            serde_json::json!({ "synced": true, "last_sync_ms": now_ms }).to_string(),
        );
    }
}

/// When this device last finished a sync, for the status line.
pub fn last_sync_ms() -> Option<u64> {
    state().and_then(|v| v["last_sync_ms"].as_u64())
}

/// Forget that this device synced, as turning sync off does, so turning it on again is a fresh upload.
pub fn forget_state() {
    if let Ok(p) = state_path() {
        let _ = fs::remove_file(p);
    }
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
    let report = mirror(
        local.as_ref(),
        remote.as_ref(),
        &Opts {
            own_device: own.as_deref(),
            push_all: !synced_before() || confirm_reupload,
        },
    )?;
    mark_synced(crate::session::now_ms());
    Ok(report)
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
    if read(local.as_ref(), "vault.json")?.is_some() {
        return Ok(());
    }
    pull_into(
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
    fill_local_from(local.as_ref(), remote.as_ref())
}

fn fill_local_from(local: &dyn BlobStore, remote: &dyn BlobStore) -> Result<(), SyncError> {
    let Some(have) = read(local, "vault.json")? else {
        return pull_into(local, remote);
    };
    if let Some(theirs) = read(remote, "vault.json")?
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

/// Push and pull for a join in progress, before this device has a config or id: it sends its request and
/// collects the answer. Only the `first` exchange pushes every local file; later ones push the inbox only.
pub fn exchange(place: &Place, first: bool) -> Result<Report, SyncError> {
    let Some(remote_kind) = place.remote else {
        return Ok(Report::default());
    };
    let local = crate::store::open_local(&place.local)?;
    let remote = crate::store::open_remote(remote_kind)?;
    mirror(
        local.as_ref(),
        remote.as_ref(),
        &Opts {
            own_device: None,
            push_all: first,
        },
    )
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

        // The first sync, or an approved re-upload, pushes everything.
        mirror(&la, &remote, &opts(Some(A), true)).unwrap();
        assert!(remote.get("log/bb/00000001.op").is_ok());
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
        assert_eq!(fill_local_from(&la, &remote), Ok(()));
        put(&remote, "vault.json", &vault_json("v1", 5, "x"));
        put(&remote, "log/zz/00000001.op", b"theirs");
        assert_eq!(fill_local_from(&la, &remote), Ok(()));
        assert_eq!(la.get("log/zz/00000001.op"), Err(StoreError::NotFound));
        put(&remote, "vault.json", &vault_json("other", 5, "x"));
        assert_eq!(fill_local_from(&la, &remote), Err(SyncError::ForeignVault));
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
}
