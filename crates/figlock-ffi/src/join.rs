use std::sync::Arc;

use figlock_core::{JoinAttempt, device_fingerprint, invite_code_format, invite_code_parse};
use zeroize::Zeroizing;

use crate::device::DeviceKeysHandle;
use crate::error::{FigLockError, Result};
use crate::session::VaultSession;

/// A file the caller must store: `path` is relative to the storage root.
#[derive(uniffi::Record)]
pub struct RequestFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// The new device's side of pairing (`pairing-recovery.md` 3): the request to store, and the handle that
/// finishes the join once the trusted device has answered. Holds the invite keys in memory only.
#[derive(uniffi::Object)]
pub struct JoinHandle {
    attempt: JoinAttempt,
    fingerprint: String,
}

/// Everything `start_join` returns.
#[derive(uniffi::Record)]
pub struct JoinStart {
    pub handle: Arc<JoinHandle>,
    /// Store at `request.path` (`inbox/<invite id>.req`), then poll for `response_path`.
    pub request: RequestFile,
}

/// Everything `JoinHandle::finish` returns.
#[derive(uniffi::Record)]
pub struct JoinOutcome {
    pub session: Arc<VaultSession>,
    /// The Secret Key as `FL1-...`, for the Keychain. Wipe the copy after storing it.
    pub secret_key_text: String,
}

#[uniffi::export]
impl JoinHandle {
    /// Shown on this screen and on the trusted device so the person can compare them.
    pub fn fingerprint(&self) -> String {
        self.fingerprint.clone()
    }

    /// Where the trusted device's answer appears (`inbox/<invite id>.resp`).
    pub fn response_path(&self) -> String {
        self.attempt.resp_path()
    }

    /// Where this device's signed record appears (`devices/<device id>.json`).
    pub fn record_path(&self) -> String {
        self.attempt.record_path()
    }

    /// Open the answer, check it is for this device and unlock with the typed password. `records`
    /// are the files of `devices/`; this device's own signed record must be among them.
    pub fn finish(
        &self,
        response: Vec<u8>,
        vault_json: Vec<u8>,
        records: Vec<Vec<u8>>,
        password: String,
        now_ms: u64,
    ) -> Result<JoinOutcome> {
        let password = Zeroizing::new(password);
        let joined = self
            .attempt
            .finish(&response, &vault_json, &records, &password, now_ms)?;
        Ok(JoinOutcome {
            session: Arc::new(VaultSession::new(joined.vault)),
            secret_key_text: joined.secret_key.format().to_string(),
        })
    }
}

#[uniffi::export]
/// Start joining from the typed or scanned `code` and the `.inv` file read from storage (find it with
/// `invite_path`). Refuses an expired invite, a malformed file, or a code that does not match it. The
/// same `Failed` covers all three, so a stranger's file cannot be told apart from a wrong code.
pub fn start_join(
    code: String,
    invite_bytes: Vec<u8>,
    device: Arc<DeviceKeysHandle>,
    device_name: String,
    now_ms: u64,
) -> Result<JoinStart> {
    let code = Zeroizing::new(code);
    let core_device = device.to_core();
    let fingerprint = device_fingerprint(&core_device.sign_public());
    let (attempt, file) =
        JoinAttempt::begin(&code, &invite_bytes, core_device, &device_name, now_ms)?;
    Ok(JoinStart {
        handle: Arc::new(JoinHandle {
            attempt,
            fingerprint,
        }),
        request: RequestFile {
            path: file.path,
            bytes: file.bytes,
        },
    })
}

/// Where the `.inv` for a typed `code` lives, given the storage's `vault.json`. The vault id is read
/// without verifying it: it only locates the file, which `start_join` then checks against the code.
#[uniffi::export]
pub fn invite_path(code: String, vault_json: Vec<u8>) -> Result<String> {
    let code = Zeroizing::new(code);
    figlock_core::invite_path(&code, &vault_json).map_err(|_| FigLockError::InvalidInput)
}

/// Whether `code` is a well formed invite code (length and alphabet; case, spaces and confusable letters
/// are accepted), so the UI can flag a typo before any storage access.
#[uniffi::export]
pub fn invite_code_is_valid(code: String) -> bool {
    let code = Zeroizing::new(code);
    invite_code_parse(&code).is_ok()
}

/// The canonical `XXXX-XXXX-XXXX-XXXX-XXXX` spelling of a typed invite code.
#[uniffi::export]
pub fn invite_code_canonical(code: String) -> Result<String> {
    let code = Zeroizing::new(code);
    let s = Zeroizing::new(invite_code_parse(&code).map_err(|_| FigLockError::InvalidInput)?);
    invite_code_format(&s).map_err(|_| FigLockError::InvalidInput)
}

/// The fingerprint of a device's public signing key (64 hex characters), as both screens show it.
#[uniffi::export]
pub fn fingerprint_of(sign_pub_hex: String) -> Result<String> {
    let bytes = hex_32(&sign_pub_hex)?;
    Ok(device_fingerprint(&bytes))
}

fn hex_32(hex: &str) -> Result<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return Err(FigLockError::InvalidInput);
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| FigLockError::InvalidInput)?;
    }
    Ok(out)
}
