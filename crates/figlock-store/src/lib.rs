//! Storage adapters for FigLock (`docs/spec/sync.md` 2).
//!
//! A [`BlobStore`] is a flat namespace of paths and blobs with five operations. Adapters never interpret
//! contents (everything stored is ciphertext) and never trust paths: every path is validated before it
//! touches the underlying storage, so a hostile folder or provider cannot make FigLock read or write
//! outside its own area.

#[cfg(feature = "gdrive")]
mod gdrive;
mod local;
mod path;

#[cfg(feature = "gdrive")]
pub use gdrive::{GoogleDrive, StaticToken, TokenSource};
pub use local::LocalFolder;
pub use path::validate_path;

use std::fmt;
use std::time::SystemTime;

/// Largest blob an adapter will read into memory. A hostile or corrupted folder cannot exhaust memory with
/// one giant file; op files and snapshots are padded to at most a few MiB.
pub const MAX_BLOB_BYTES: u64 = 64 * 1024 * 1024;

/// One stored blob as listed by [`BlobStore::list`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the store root, using `/`.
    pub path: String,
    pub size: u64,
    /// Informational only; never used for ordering (`sync.md` 2).
    pub mtime: Option<SystemTime>,
    /// False for a file that is listed but not downloaded yet (for example an iCloud Drive placeholder).
    /// Such a file is "not available yet", not "missing", and is never treated as a gap.
    pub available: bool,
}

/// Why a storage operation failed. Deliberately carries no paths or contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    NotFound,
    /// `put_new` on a path that already exists.
    Exists,
    /// The file exists in the listing but its bytes are not on this device yet.
    NotYetAvailable,
    /// The path is not a valid FigLock storage path, or resolves outside the store.
    InvalidPath,
    TooLarge,
    /// The provider refused the credentials (expired or revoked sign-in, missing scope). Sign in again.
    Unauthorized,
    /// The provider could not be reached or kept failing (offline, rate limited, 5xx). Retry later.
    Unavailable,
    Io(std::io::ErrorKind),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound => f.write_str("not found"),
            StoreError::Exists => f.write_str("already exists"),
            StoreError::NotYetAvailable => f.write_str("not downloaded yet"),
            StoreError::InvalidPath => f.write_str("invalid path"),
            StoreError::TooLarge => f.write_str("blob too large"),
            StoreError::Unauthorized => f.write_str("sign-in required"),
            StoreError::Unavailable => f.write_str("storage unavailable"),
            StoreError::Io(kind) => write!(f, "storage error: {kind}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => StoreError::NotFound,
            k => StoreError::Io(k),
        }
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// The storage contract from `sync.md` 2. Every implementation must pass `tests/contract.rs`.
pub trait BlobStore {
    /// The full blob, or [`StoreError::NotFound`].
    fn get(&self, path: &str) -> Result<Vec<u8>>;

    /// Create the blob if it does not exist; [`StoreError::Exists`] otherwise. Readers see either nothing or
    /// the whole blob, never a partial write, and concurrent callers cannot both succeed.
    fn put_new(&self, path: &str, bytes: &[u8]) -> Result<()>;

    /// Replace a blob atomically (creating it if absent). Only for the files `sync.md` 3 names replaceable.
    fn put_replace(&self, path: &str, bytes: &[u8]) -> Result<()>;

    /// Blobs whose path starts with `prefix`, sorted by path. May be stale but eventually complete.
    fn list(&self, prefix: &str) -> Result<Vec<Entry>>;

    /// Remove a blob. Removing a missing path succeeds.
    fn delete(&self, path: &str) -> Result<()>;
}
