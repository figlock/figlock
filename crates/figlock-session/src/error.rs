//! One error type for the whole binary: a message safe to show, never a secret, a path to a secret or file
//! contents.

use std::fmt;

/// What went wrong, for a window that words it for people. The message stays the CLI's text; a window picks its
/// own wording from the kind, so the message is never parsed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FailKind {
    #[default]
    Other,
    /// The platform keychain refused to read, save or remove a key, usually because the person denied its prompt.
    KeychainDenied,
    /// The device keys this device needs are not in the keychain, or are damaged: the device must be added again.
    DeviceKeysLost,
    /// The vault storage could not be reached.
    StorageUnreachable,
    /// The vault storage refused the saved sign-in.
    StorageSignedOut,
    /// No vault where this device looks for one.
    VaultMissing,
}

#[derive(Debug)]
pub struct Fail(pub String, pub FailKind);

pub type Res<T> = Result<T, Fail>;

impl Fail {
    pub fn new(msg: impl Into<String>) -> Self {
        Fail(msg.into(), FailKind::Other)
    }

    pub fn of(kind: FailKind, msg: impl Into<String>) -> Self {
        Fail(msg.into(), kind)
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        Fail::new(format!("i/o error: {}", e.kind()))
    }
}

impl From<figlock_store::StoreError> for Fail {
    fn from(e: figlock_store::StoreError) -> Self {
        Fail::new(format!("storage: {e}"))
    }
}

/// Core errors are deliberately uninformative (`crypto.md`); call sites that can say more map them
/// themselves.
impl From<figlock_core::Error> for Fail {
    fn from(_: figlock_core::Error) -> Self {
        Fail::new("vault operation failed")
    }
}
