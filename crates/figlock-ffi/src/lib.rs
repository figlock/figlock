//! UniFFI exports of `figlock-core` for the Swift (iOS) and Kotlin (Android) apps.
//!
//! The surface is built so secrets cross the boundary as little as possible:
//! - keys and decrypted items live behind opaque handles ([`VaultSession`], [`CreatedVault`],
//!   [`DeviceKeysHandle`]); the foreign side holds a reference, never the bytes;
//! - listings carry non-secret fields only; a secret value comes out through `reveal_field` alone;
//! - the only error type, [`FigLockError`], has no payload, so nothing secret can end up in an error string;
//! - there is no I/O of its own in the vault calls: the app reads and writes `vault.json`, `devices/` and
//!   `log/` through its own storage code and passes the bytes in, and the caller supplies the clock (`now_ms`);
//! - sync is the one Rust engine (`figlock-sync`) with the app's storage, clock and state passed in as foreign
//!   traits ([`SyncEngine`], [`BlobStoreFfi`]); the Google Drive adapter is Rust, fed a token by the app.
//!
//! Limits worth knowing: inputs and outputs that are `String` or `Vec<u8>` are copied by the binding layer,
//! and the copy on the Swift or Kotlin side is outside Rust's control, so the app should keep such values
//! short-lived and overwrite byte arrays it owns.

mod created;
mod device;
mod error;
mod join;
mod quick;
mod session;
mod sync;

pub use created::{
    CreatedVault, KdfSettings, create_vault, default_kdf_settings, minimum_kdf_settings,
    render_emergency_kit, secret_key_is_valid,
};
pub use device::DeviceKeysHandle;
pub use error::FigLockError;
pub use join::{
    JoinHandle, JoinOutcome, JoinStart, RequestFile, fingerprint_of, invite_code_canonical,
    invite_code_is_valid, invite_path, start_join,
};
pub use quick::{
    UnlockKey, derive_unlock_key, open_quick_unlock, quick_unlock_ephemeral_public,
    unlock_vault_with_key,
};
pub use session::{
    CommittedFile, DeviceEntry, FieldInput, ItemListing, ItemSummary, RevocationFiles, SearchHit,
    UnlockOutcome, VaultSession, unlock_vault,
};
#[cfg(feature = "gdrive")]
pub use sync::DriveStore;
pub use sync::{
    AccessTokens, BlobEntry, BlobStoreFfi, FolderStore, StoreFailure, SyncClock, SyncEngine,
    SyncFailure, SyncReport, SyncStateStore, SyncWarning, VaultIdentity, delete_remote_confirmed,
    delete_remote_phrase, sync_backoff_secs, sync_interval_secs, vault_identity,
};

uniffi::setup_scaffolding!();
