//! What a device keeps to open its vault, and the unlocked session built on it. No terminal or window code:
//! the front end (the CLI, the desktop app) supplies the questions and the warnings through [`Ui`].

pub mod clipboard;
pub mod config;
pub mod docfile;
pub mod drive;
pub mod error;
pub mod join;
#[cfg(target_os = "macos")]
mod keychain;
pub mod keystore;
pub mod quick;
pub mod quick_se;
pub mod secret_key_store;
pub mod session;
pub mod store;
pub mod sync;

pub use error::{Fail, FailKind, Res};
pub use session::{Session, Ui, clean, id_hex, now_ms, peek_vault_id, title_of};
