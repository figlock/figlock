use zeroize::{Zeroize, ZeroizeOnDrop};

/// A 256-bit secret key. Zeroizes on drop; deliberately not `Clone`, `Debug` or `Display`.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Key([u8; 32]);

impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Key(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> crate::Result<Self> {
        let arr: [u8; 32] = bytes.try_into().map_err(|_| crate::Error)?;
        Ok(Key(arr))
    }

    /// Fresh random key from the OS CSPRNG.
    pub fn random() -> crate::Result<Self> {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|_| crate::Error)?;
        Ok(Key(b))
    }

    /// Raw bytes. Callers must not log or persist them outside the documented key wrappers.
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}
