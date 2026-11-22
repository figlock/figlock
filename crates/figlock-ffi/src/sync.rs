//! Sync for the apps (ADR-0041 decision 3): the one Rust engine of `figlock-sync`, with the parts that need
//! platform APIs supplied by the app through foreign traits.
//!
//! The app implements [`BlobStoreFfi`] for a folder (Swift: `NSFileCoordinator`), [`SyncClock`] and
//! [`SyncStateStore`]; for Google Drive the Rust adapter stays here and is fed an access token by the app's
//! [`AccessTokens`]. A [`SyncEngine`] binds one local store and one remote store and runs the sync.
//!
//! What crosses the boundary is ciphertext paths and bytes, never a key. Errors carry no text: a failure is
//! one of a few fixed cases, so no path, token or file content can end up in a log line built from one.
//!
//! The engine copies a few files at once, so the app's store (and the Rust ones here) is called from several
//! threads at the same time. Implement every method so concurrent calls on different paths are safe.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use figlock_store::{BlobStore, Entry, LocalFolder, StoreError};
use figlock_sync::{Clock, SyncError, SyncState};

/// Why a storage call failed. The same cases as `figlock_store::StoreError`, with no payload. A foreign
/// implementation returns the one that fits; anything it throws that is not one of these becomes `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Error)]
pub enum StoreFailure {
    NotFound,
    /// `put_new` on a path that already exists.
    Exists,
    /// Listed, but the bytes are not on this device yet (an iCloud Drive placeholder).
    NotYetAvailable,
    InvalidPath,
    TooLarge,
    /// The provider refused the sign-in.
    Unauthorized,
    /// Offline, rate limited or the provider is failing. Retry later.
    Unavailable,
    /// Any other storage error.
    Other,
}

impl std::fmt::Display for StoreFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("storage error")
    }
}

impl std::error::Error for StoreFailure {}

impl From<uniffi::UnexpectedUniFFICallbackError> for StoreFailure {
    fn from(_: uniffi::UnexpectedUniFFICallbackError) -> Self {
        StoreFailure::Other
    }
}

impl From<StoreError> for StoreFailure {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => StoreFailure::NotFound,
            StoreError::Exists => StoreFailure::Exists,
            StoreError::NotYetAvailable => StoreFailure::NotYetAvailable,
            StoreError::InvalidPath => StoreFailure::InvalidPath,
            StoreError::TooLarge => StoreFailure::TooLarge,
            StoreError::Unauthorized => StoreFailure::Unauthorized,
            StoreError::Unavailable => StoreFailure::Unavailable,
            StoreError::Io(_) => StoreFailure::Other,
        }
    }
}

impl From<StoreFailure> for StoreError {
    fn from(e: StoreFailure) -> Self {
        match e {
            StoreFailure::NotFound => StoreError::NotFound,
            StoreFailure::Exists => StoreError::Exists,
            StoreFailure::NotYetAvailable => StoreError::NotYetAvailable,
            StoreFailure::InvalidPath => StoreError::InvalidPath,
            StoreFailure::TooLarge => StoreError::TooLarge,
            StoreFailure::Unauthorized => StoreError::Unauthorized,
            StoreFailure::Unavailable => StoreError::Unavailable,
            StoreFailure::Other => StoreError::Io(std::io::ErrorKind::Other),
        }
    }
}

/// One stored blob as listed by [`BlobStoreFfi::list`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BlobEntry {
    /// Path relative to the store root, using `/`.
    pub path: String,
    pub size: u64,
    /// Informational only; never used for ordering. Unix milliseconds.
    pub mtime_ms: Option<u64>,
    /// False for a file that is listed but not downloaded yet. Such a file is "not available yet", not
    /// "missing", and is never treated as a gap.
    pub available: bool,
}

/// The storage contract of `sync.md` 2, for the app to implement over its own storage. Every
/// implementation must behave as `tests/contract.rs` of `figlock-store` demands.
#[uniffi::export(with_foreign)]
pub trait BlobStoreFfi: Send + Sync {
    /// The full blob, or `NotFound`.
    fn get(&self, path: String) -> Result<Vec<u8>, StoreFailure>;
    /// Create the blob if it does not exist; `Exists` otherwise. Readers see nothing or the whole blob.
    fn put_new(&self, path: String, bytes: Vec<u8>) -> Result<(), StoreFailure>;
    /// Replace a blob atomically, creating it if absent.
    fn put_replace(&self, path: String, bytes: Vec<u8>) -> Result<(), StoreFailure>;
    /// Blobs whose path starts with `prefix`, sorted by path.
    fn list(&self, prefix: String) -> Result<Vec<BlobEntry>, StoreFailure>;
    /// Remove a blob. Removing a missing path succeeds.
    fn delete(&self, path: String) -> Result<(), StoreFailure>;
}

/// The time in Unix milliseconds, from the app.
#[uniffi::export(with_foreign)]
pub trait SyncClock: Send + Sync {
    fn now_ms(&self) -> u64;
}

/// The small record a device keeps about its syncing, kept by the app wherever it keeps device state.
#[uniffi::export(with_foreign)]
pub trait SyncStateStore: Send + Sync {
    /// Whether this device has synced to its remote before, so an empty remote means "lost", not "new".
    fn synced_before(&self) -> bool;
    fn last_sync_ms(&self) -> Option<u64>;
    fn sign_in_refused(&self) -> bool;
    fn mark_synced(&self, now_ms: u64);
    fn mark_sign_in_refused(&self);
    fn clear_sign_in_refused(&self);
    /// Forget that this device synced, so turning sync on again is a fresh upload.
    fn forget(&self);
}

/// A Google OAuth access token, from the app (sign-in and the refresh token live in Swift).
#[uniffi::export(with_foreign)]
pub trait AccessTokens: Send + Sync {
    /// A currently valid access token.
    fn access_token(&self) -> Result<String, StoreFailure>;
    /// The last token was refused with 401: drop any cached copy so the next call refreshes.
    fn invalidate(&self);
}

/// Why a sync did not finish. No text: the app words each case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Error)]
pub enum SyncFailure {
    /// This device has no remote.
    NotEnabled,
    /// The provider refused the credentials: sign in again.
    SignIn,
    /// Could not reach the provider; the local vault still works.
    Offline,
    /// The remote holds a different vault than the local folder. Never merged.
    ForeignVault,
    /// The remote has no vault but this device synced to it before. Uploading again needs a yes.
    RemoteLost,
    /// A `vault.json` that is not valid.
    Damaged,
    /// Another sync with this engine is running.
    Busy,
    /// Deleting the remote copy was asked for without the typed confirmation.
    NotConfirmed,
    Other,
}

impl std::fmt::Display for SyncFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("sync failed")
    }
}

impl std::error::Error for SyncFailure {}

impl From<SyncError> for SyncFailure {
    fn from(e: SyncError) -> Self {
        match e {
            SyncError::NotEnabled => SyncFailure::NotEnabled,
            SyncError::SignIn => SyncFailure::SignIn,
            SyncError::Offline => SyncFailure::Offline,
            SyncError::ForeignVault => SyncFailure::ForeignVault,
            SyncError::RemoteLost => SyncFailure::RemoteLost,
            SyncError::Damaged => SyncFailure::Damaged,
            SyncError::Busy => SyncFailure::Busy,
            SyncError::NotConfirmed => SyncFailure::NotConfirmed,
            SyncError::Other(_) => SyncFailure::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SyncWarning {
    /// Two devices changed `vault.json` at once. One copy was chosen by a fixed rule; the device whose change
    /// lost re-applies it on its next unlock.
    VaultJsonFork,
}

/// What one sync did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncReport {
    /// Files written to the local store.
    pub pulled: u32,
    /// Files written to the remote.
    pub pushed: u32,
    pub warnings: Vec<SyncWarning>,
}

impl From<figlock_sync::Report> for SyncReport {
    fn from(r: figlock_sync::Report) -> Self {
        SyncReport {
            pulled: u32::try_from(r.pulled).unwrap_or(u32::MAX),
            pushed: u32::try_from(r.pushed).unwrap_or(u32::MAX),
            warnings: r
                .warnings
                .into_iter()
                .map(|w| match w {
                    figlock_sync::Warning::VaultJsonFork => SyncWarning::VaultJsonFork,
                })
                .collect(),
        }
    }
}

/// A foreign (or Rust-provided) store seen as the engine's `BlobStore`.
struct Adapter(Arc<dyn BlobStoreFfi>);

impl BlobStore for Adapter {
    fn get(&self, path: &str) -> figlock_store::Result<Vec<u8>> {
        Ok(self.0.get(path.to_owned())?)
    }

    fn put_new(&self, path: &str, bytes: &[u8]) -> figlock_store::Result<()> {
        Ok(self.0.put_new(path.to_owned(), bytes.to_vec())?)
    }

    fn put_replace(&self, path: &str, bytes: &[u8]) -> figlock_store::Result<()> {
        Ok(self.0.put_replace(path.to_owned(), bytes.to_vec())?)
    }

    fn list(&self, prefix: &str) -> figlock_store::Result<Vec<Entry>> {
        Ok(self
            .0
            .list(prefix.to_owned())?
            .into_iter()
            .map(|e| Entry {
                path: e.path,
                size: e.size,
                mtime: e
                    .mtime_ms
                    .map(|ms| UNIX_EPOCH + std::time::Duration::from_millis(ms)),
                available: e.available,
            })
            .collect())
    }

    fn delete(&self, path: &str) -> figlock_store::Result<()> {
        Ok(self.0.delete(path.to_owned())?)
    }
}

/// A Rust store offered to the app as a [`BlobStoreFfi`].
fn entry_out(e: Entry) -> BlobEntry {
    BlobEntry {
        path: e.path,
        size: e.size,
        mtime_ms: e.mtime.and_then(|t| {
            t.duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
        }),
        available: e.available,
    }
}

macro_rules! rust_store {
    ($ty:ty) => {
        #[uniffi::export]
        impl BlobStoreFfi for $ty {
            fn get(&self, path: String) -> Result<Vec<u8>, StoreFailure> {
                Ok(self.0.get(&path)?)
            }

            fn put_new(&self, path: String, bytes: Vec<u8>) -> Result<(), StoreFailure> {
                Ok(self.0.put_new(&path, &bytes)?)
            }

            fn put_replace(&self, path: String, bytes: Vec<u8>) -> Result<(), StoreFailure> {
                Ok(self.0.put_replace(&path, &bytes)?)
            }

            fn list(&self, prefix: String) -> Result<Vec<BlobEntry>, StoreFailure> {
                Ok(self.0.list(&prefix)?.into_iter().map(entry_out).collect())
            }

            fn delete(&self, path: String) -> Result<(), StoreFailure> {
                Ok(self.0.delete(&path)?)
            }
        }
    };
}

/// A plain folder handled in Rust, for a local vault that needs no file coordination and for tests. The
/// iCloud Drive folder is the app's own [`BlobStoreFfi`], which coordinates its access.
#[derive(uniffi::Object)]
pub struct FolderStore(LocalFolder);

#[uniffi::export]
impl FolderStore {
    /// The folder must exist.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, StoreFailure> {
        Ok(Arc::new(FolderStore(LocalFolder::open(path)?)))
    }
}

rust_store!(FolderStore);

/// The vault in the signed-in person's Google Drive app folder, fed its token by the app.
#[cfg(feature = "gdrive")]
#[derive(uniffi::Object)]
pub struct DriveStore(figlock_store::GoogleDrive);

#[cfg(feature = "gdrive")]
struct Tokens(Arc<dyn AccessTokens>);

#[cfg(feature = "gdrive")]
impl figlock_store::TokenSource for Tokens {
    fn access_token(&self) -> figlock_store::Result<String> {
        Ok(self.0.access_token()?)
    }

    fn invalidate(&self) {
        self.0.invalidate();
    }
}

#[cfg(feature = "gdrive")]
#[uniffi::export]
impl DriveStore {
    #[uniffi::constructor]
    pub fn new(tokens: Arc<dyn AccessTokens>) -> Arc<Self> {
        Arc::new(DriveStore(figlock_store::GoogleDrive::new(Tokens(tokens))))
    }
}

#[cfg(feature = "gdrive")]
rust_store!(DriveStore);

struct ForeignClock(Arc<dyn SyncClock>);

impl Clock for ForeignClock {
    fn now_ms(&self) -> u64 {
        self.0.now_ms()
    }
}

struct ForeignState(Arc<dyn SyncStateStore>);

impl SyncState for ForeignState {
    fn synced_before(&self) -> bool {
        self.0.synced_before()
    }

    fn last_sync_ms(&self) -> Option<u64> {
        self.0.last_sync_ms()
    }

    fn sign_in_refused(&self) -> bool {
        self.0.sign_in_refused()
    }

    fn mark_synced(&self, now_ms: u64) {
        self.0.mark_synced(now_ms);
    }

    fn mark_sign_in_refused(&self) {
        self.0.mark_sign_in_refused();
    }

    fn clear_sign_in_refused(&self) {
        self.0.clear_sign_in_refused();
    }

    fn forget(&self) {
        self.0.forget();
    }
}

/// Releases the busy flag when a sync ends, however it ends.
struct Running<'a>(&'a AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// One local store and its remote, with this device's state and clock. Calls take turns: a second one made
/// while a sync runs fails with `Busy` instead of waiting. Call from a background task (a sync waits on the
/// network).
#[derive(uniffi::Object)]
pub struct SyncEngine {
    local: Adapter,
    remote: Adapter,
    state: ForeignState,
    clock: ForeignClock,
    own_device: Option<String>,
    busy: AtomicBool,
}

impl SyncEngine {
    fn guard(&self) -> Result<Running<'_>, SyncFailure> {
        if self.busy.swap(true, Ordering::Acquire) {
            return Err(SyncFailure::Busy);
        }
        Ok(Running(&self.busy))
    }
}

#[uniffi::export]
impl SyncEngine {
    /// `own_device` is this device's id in hex, once it has one; without it nothing but the inbox is pushed
    /// after the first sync.
    #[uniffi::constructor]
    pub fn new(
        local: Arc<dyn BlobStoreFfi>,
        remote: Arc<dyn BlobStoreFfi>,
        state: Arc<dyn SyncStateStore>,
        clock: Arc<dyn SyncClock>,
        own_device: Option<String>,
    ) -> Arc<Self> {
        Arc::new(SyncEngine {
            local: Adapter(local),
            remote: Adapter(remote),
            state: ForeignState(state),
            clock: ForeignClock(clock),
            own_device,
            busy: AtomicBool::new(false),
        })
    }

    /// Mirror the local store and the remote once. `confirm_reupload` is the person's yes to uploading
    /// everything again after the remote was lost (`RemoteLost`).
    pub fn run(&self, confirm_reupload: bool) -> Result<SyncReport, SyncFailure> {
        let _running = self.guard()?;
        Ok(figlock_sync::run(
            &self.local,
            &self.remote,
            &self.state,
            &self.clock,
            self.own_device.as_deref(),
            confirm_reupload,
        )?
        .into())
    }

    /// Make sure the local store holds a vault, looking at the remote only when it does not.
    pub fn fill_local(&self) -> Result<(), SyncFailure> {
        let _running = self.guard()?;
        Ok(figlock_sync::fill_local(&self.local, &self.remote)?)
    }

    /// Opening an existing vault from the remote during setup: always looks at the remote, and refuses
    /// (`ForeignVault`) when the local store holds a different vault.
    pub fn open_from_remote(&self) -> Result<(), SyncFailure> {
        let _running = self.guard()?;
        Ok(figlock_sync::open_from_remote(&self.local, &self.remote)?)
    }

    /// Push and pull for a join in progress. Only the `first` exchange pushes every local file.
    pub fn exchange(&self, first: bool) -> Result<SyncReport, SyncFailure> {
        let _running = self.guard()?;
        Ok(figlock_sync::exchange(&self.local, &self.remote, first)?.into())
    }

    /// Take files out of the remote too (an answered invite). Best effort.
    pub fn remove_remote(&self, paths: Vec<String>) {
        figlock_sync::remove_remote(&self.remote, &paths);
    }

    /// Delete this vault's copy in the remote. Needs `typed` to be the phrase of [`delete_remote_phrase`];
    /// refuses (`ForeignVault`) when the remote holds another vault. Forgets the sync state afterwards.
    pub fn delete_remote(&self, typed: String) -> Result<u32, SyncFailure> {
        let _running = self.guard()?;
        let done = figlock_sync::delete_remote(&self.local, &self.remote, &typed)?;
        self.state.forget();
        Ok(u32::try_from(done.files).unwrap_or(u32::MAX))
    }
}

/// What a person types to delete the remote copy of the vault.
#[uniffi::export]
pub fn delete_remote_phrase() -> String {
    figlock_sync::DELETE_REMOTE_PHRASE.to_owned()
}

/// Whether `typed` is the confirmation phrase.
#[uniffi::export]
pub fn delete_remote_confirmed(typed: String) -> bool {
    figlock_sync::delete_confirmed(&typed)
}

/// How often a healthy background sync runs, in seconds.
#[uniffi::export]
pub fn sync_interval_secs() -> u64 {
    figlock_sync::INTERVAL.as_secs()
}

/// How long to wait before the next sync after `failures` in a row (0 is the first failure), in seconds.
#[uniffi::export]
pub fn sync_backoff_secs(failures: u32) -> u64 {
    figlock_sync::backoff(usize::try_from(failures).unwrap_or(usize::MAX)).as_secs()
}

/// Which vault a `vault.json` belongs to and how far it has got.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct VaultIdentity {
    pub vault_id: String,
    pub revision: u64,
}

/// The vault id and revision a `vault.json` names, for the app to tell a vault from a damaged file without
/// unlocking. `Damaged` when the bytes are not JSON with both fields. Not a signature check: unlocking does that.
#[uniffi::export]
pub fn vault_identity(vault_json: Vec<u8>) -> Result<VaultIdentity, SyncFailure> {
    let (vault_id, revision) = figlock_sync::vault_identity(&vault_json)?;
    Ok(VaultIdentity { vault_id, revision })
}
