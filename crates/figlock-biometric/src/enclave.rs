//! A Secure Enclave key for quick unlock (`docs/adr/0025-secure-enclave-quick-unlock.md`).
//!
//! The key is made with CryptoKit by a small Swift shim (`swift/enclave.swift`, built by `build.rs`) and never
//! leaves the chip. The caller keeps the opaque blob the shim returns; using the key (an ECDH with a peer point)
//! makes the chip ask the person first. Where the shim was not built, or there is no Secure Enclave, every call
//! says so and the caller falls back to the keychain.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use zeroize::Zeroizing;

/// Length of an uncompressed P-256 point.
pub const POINT_LEN: usize = 65;
/// Length of the ECDH result.
pub const SHARED_LEN: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No Secure Enclave on this Mac, or this build has no Swift shim.
    Unavailable,
    /// The person cancelled or failed the check.
    Declined,
    /// Nobody answered in time.
    TimedOut,
    /// The key blob or the peer point was refused, or the system failed.
    Failed,
}

/// A freshly made key: the blob to store and its public point.
pub struct NewKey {
    pub blob: Vec<u8>,
    pub public: [u8; POINT_LEN],
}

/// Whether this build and this Mac can make Secure Enclave keys.
pub fn available() -> bool {
    imp::available()
}

/// Make a key that needs the person present every time it is used. Needs no prompt itself.
pub fn create() -> Result<NewKey, Error> {
    imp::create()
}

/// The public point of a stored key. Needs no prompt.
pub fn public_key(blob: &[u8]) -> Result<[u8; POINT_LEN], Error> {
    imp::public_key(blob)
}

/// ECDH between the stored key and `peer`. The chip asks the person first, with `reason` as the sheet's text, and
/// this gives up after `wait` (a sheet left open keeps its thread until dismissed).
pub fn agree(
    blob: &[u8],
    peer: &[u8; POINT_LEN],
    reason: &str,
    wait: Duration,
) -> Result<Zeroizing<[u8; SHARED_LEN]>, Error> {
    let (blob, peer, reason) = (blob.to_vec(), *peer, reason.to_owned());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(imp::agree(&blob, &peer, &reason));
    });
    match rx.recv_timeout(wait) {
        Ok(r) => r,
        Err(RecvTimeoutError::Timeout) => Err(Error::TimedOut),
        Err(RecvTimeoutError::Disconnected) => Err(Error::Failed),
    }
}

#[cfg(all(target_os = "macos", figlock_enclave))]
#[allow(unsafe_code, reason = "calls the Swift shim over a plain C interface")]
mod imp {
    use std::ffi::CString;

    use zeroize::Zeroizing;

    use super::{Error, NewKey, POINT_LEN, SHARED_LEN};

    const OK: i32 = 0;
    const UNAVAILABLE: i32 = 1;
    const DECLINED: i32 = 2;
    /// More than any real CryptoKit key blob (about 430 bytes).
    const BLOB_CAP: usize = 4096;

    unsafe extern "C" {
        fn figlock_se_available() -> i32;
        fn figlock_se_create(
            blob: *mut u8,
            blob_cap: usize,
            blob_len: *mut usize,
            public: *mut u8,
        ) -> i32;
        fn figlock_se_public(blob: *const u8, blob_len: usize, public: *mut u8) -> i32;
        fn figlock_se_agree(
            blob: *const u8,
            blob_len: usize,
            peer: *const u8,
            reason: *const std::ffi::c_char,
            shared: *mut u8,
        ) -> i32;
    }

    fn map(code: i32) -> Error {
        match code {
            UNAVAILABLE => Error::Unavailable,
            DECLINED => Error::Declined,
            _ => Error::Failed,
        }
    }

    pub fn available() -> bool {
        // SAFETY: a call with no arguments that only reads system state.
        unsafe { figlock_se_available() == 1 }
    }

    pub fn create() -> Result<NewKey, Error> {
        let mut blob = vec![0u8; BLOB_CAP];
        let mut len = 0usize;
        let mut public = [0u8; POINT_LEN];
        // SAFETY: the buffers are valid for the sizes given and the shim writes at most that much.
        let code = unsafe {
            figlock_se_create(blob.as_mut_ptr(), blob.len(), &mut len, public.as_mut_ptr())
        };
        if code != OK || len == 0 || len > blob.len() {
            return Err(map(code));
        }
        blob.truncate(len);
        Ok(NewKey { blob, public })
    }

    pub fn public_key(blob: &[u8]) -> Result<[u8; POINT_LEN], Error> {
        let mut public = [0u8; POINT_LEN];
        // SAFETY: `blob` is valid for its length and `public` for the 65 bytes the shim writes.
        let code = unsafe { figlock_se_public(blob.as_ptr(), blob.len(), public.as_mut_ptr()) };
        if code == OK {
            Ok(public)
        } else {
            Err(map(code))
        }
    }

    pub fn agree(
        blob: &[u8],
        peer: &[u8; POINT_LEN],
        reason: &str,
    ) -> Result<Zeroizing<[u8; SHARED_LEN]>, Error> {
        let reason = CString::new(reason.replace('\0', " ")).map_err(|_| Error::Failed)?;
        let mut shared = Zeroizing::new([0u8; SHARED_LEN]);
        // SAFETY: every pointer is valid for what the shim reads or writes; `reason` is NUL terminated.
        let code = unsafe {
            figlock_se_agree(
                blob.as_ptr(),
                blob.len(),
                peer.as_ptr(),
                reason.as_ptr(),
                shared.as_mut_ptr(),
            )
        };
        if code == OK {
            Ok(shared)
        } else {
            Err(map(code))
        }
    }
}

#[cfg(not(all(target_os = "macos", figlock_enclave)))]
mod imp {
    use zeroize::Zeroizing;

    use super::{Error, NewKey, POINT_LEN, SHARED_LEN};

    pub fn available() -> bool {
        false
    }

    pub fn create() -> Result<NewKey, Error> {
        Err(Error::Unavailable)
    }

    pub fn public_key(_blob: &[u8]) -> Result<[u8; POINT_LEN], Error> {
        Err(Error::Unavailable)
    }

    pub fn agree(
        _blob: &[u8],
        _peer: &[u8; POINT_LEN],
        _reason: &str,
    ) -> Result<Zeroizing<[u8; SHARED_LEN]>, Error> {
        Err(Error::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_works_without_a_secure_enclave_and_nothing_pretends_to() {
        if !available() {
            assert_eq!(create().err(), Some(Error::Unavailable));
            assert_eq!(public_key(&[1, 2, 3]).err(), Some(Error::Unavailable));
            assert_eq!(
                agree(&[1], &[0; POINT_LEN], "t", Duration::from_millis(50)).err(),
                Some(Error::Unavailable)
            );
        }
    }

    /// Needs a Mac with a Secure Enclave and a build with the shim; skipped elsewhere. Creating and reloading a
    /// key needs no prompt, so this runs unattended. Using the key (a prompt) is checked by hand.
    #[test]
    fn a_key_can_be_made_and_its_public_point_read_back() {
        if !available() {
            return;
        }
        let a = create().unwrap();
        assert_eq!(a.public[0], 4);
        assert_eq!(public_key(&a.blob).unwrap(), a.public);
        let b = create().unwrap();
        assert_ne!(a.public, b.public, "every key is new");
        assert_ne!(a.blob, b.blob);
        assert_eq!(
            public_key(&a.blob[..a.blob.len() - 1]).err(),
            Some(Error::Failed)
        );
        assert!(public_key(&[]).is_err());
    }
}
