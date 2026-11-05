use std::fmt;

/// The only error that crosses the boundary. It never carries text from the input, a path or key bytes, so
/// a log line or crash report built from it cannot leak a secret.
///
/// - `Failed`: any verification, decryption or lookup failure. Deliberately one case, so a caller cannot
///   tell a wrong password from a wrong Secret Key or a tampered file (`crypto.md` 10), nor an unknown
///   item from one it may not read.
/// - `Locked`: the handle was locked or consumed; unlock again.
/// - `InvalidInput`: the argument is malformed (bad hex, wrong length, unknown item type, bad Secret Key
///   format). Safe to show as "check what you typed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Error)]
#[uniffi(flat_error)]
pub enum FigLockError {
    Failed,
    Locked,
    InvalidInput,
}

impl fmt::Display for FigLockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FigLockError::Failed => "operation failed",
            FigLockError::Locked => "vault is locked",
            FigLockError::InvalidInput => "invalid input",
        })
    }
}

impl std::error::Error for FigLockError {}

impl From<figlock_core::Error> for FigLockError {
    fn from(_: figlock_core::Error) -> Self {
        FigLockError::Failed
    }
}

pub type Result<T> = core::result::Result<T, FigLockError>;

/// Parse a 32 hex character id (item or device) into 16 bytes.
pub(crate) fn parse_id(hex: &str) -> Result<[u8; 16]> {
    if hex.len() != 32 || !hex.is_ascii() {
        return Err(FigLockError::InvalidInput);
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| FigLockError::InvalidInput)?;
    }
    Ok(out)
}

pub(crate) fn id_hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}
