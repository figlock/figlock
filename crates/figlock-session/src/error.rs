//! One error type for the whole binary: a message safe to show, never a secret, a path to a secret or file
//! contents.

use std::fmt;

#[derive(Debug)]
pub struct Fail(pub String);

pub type Res<T> = Result<T, Fail>;

impl Fail {
    pub fn new(msg: impl Into<String>) -> Self {
        Fail(msg.into())
    }
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        Fail(format!("i/o error: {}", e.kind()))
    }
}

impl From<figlock_store::StoreError> for Fail {
    fn from(e: figlock_store::StoreError) -> Self {
        Fail(format!("storage: {e}"))
    }
}

/// Core errors are deliberately uninformative (`crypto.md`); call sites that can say more map them
/// themselves.
impl From<figlock_core::Error> for Fail {
    fn from(_: figlock_core::Error) -> Self {
        Fail::new("vault operation failed")
    }
}
