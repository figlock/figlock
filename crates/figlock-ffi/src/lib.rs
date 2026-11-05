//! UniFFI exports of `figlock-core` for the Swift (iOS) and Kotlin (Android) apps.
//!
//! The surface is built so secrets cross the boundary as little as possible:
//! - keys and decrypted items live behind opaque handles ([`VaultSession`], [`CreatedVault`],
//!   [`DeviceKeysHandle`]); the foreign side holds a reference, never the bytes;
//! - listings carry non-secret fields only; a secret value comes out through `reveal_field` alone;
//! - the only error type, [`FigLockError`], has no payload, so nothing secret can end up in an error string;
//! - there is no I/O here: the app reads and writes `vault.json`, `devices/` and `log/` through its own
//!   storage code and passes the bytes in, and the caller supplies the clock (`now_ms`).
//!
//! Limits worth knowing: inputs and outputs that are `String` or `Vec<u8>` are copied by the binding layer,
//! and the copy on the Swift or Kotlin side is outside Rust's control, so the app should keep such values
//! short-lived and overwrite byte arrays it owns.

mod created;
mod device;
mod error;
mod session;

pub use created::{
    CreatedVault, KdfSettings, create_vault, default_kdf_settings, minimum_kdf_settings,
    secret_key_is_valid,
};
pub use device::DeviceKeysHandle;
pub use error::FigLockError;
pub use session::{
    CommittedFile, FieldInput, ItemListing, ItemSummary, SearchHit, UnlockOutcome, VaultSession,
    unlock_vault,
};

uniffi::setup_scaffolding!();
