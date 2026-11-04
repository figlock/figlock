use core::fmt;

/// The single error returned for any verification or decryption failure, so callers cannot tell a wrong
/// key from a tampered file (`crypto.md` 10). Details go to a local debug log that never holds key or
/// plaintext bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("operation failed")
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;
