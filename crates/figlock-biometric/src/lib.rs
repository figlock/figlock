//! Asking the person at the keyboard to confirm with Touch ID (or the device password when this Mac has no
//! biometrics). This crate is the one place in the workspace that needs `unsafe`, to call the operating
//! system; everything else stays `forbid(unsafe_code)`.

use std::time::Duration;

pub mod enclave;
#[cfg(target_os = "macos")]
pub mod keychain;

/// What the person did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Touch ID (or the device password) was accepted.
    Confirmed,
    /// The person cancelled, failed to authenticate or nobody could be asked.
    Declined,
    /// Nobody answered in time.
    TimedOut,
    /// This machine cannot show a biometric prompt (not macOS, no screen session, no password set).
    Unavailable,
}

/// Show the system prompt with `reason` as its text and wait at most `wait` for the answer.
pub fn confirm(reason: &str, wait: Duration) -> Outcome {
    imp::confirm(reason, wait)
}

/// Whether [`confirm`] could show a prompt right now.
pub fn available() -> bool {
    imp::available()
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code, reason = "calls into the LocalAuthentication framework")]
mod imp {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};

    use super::Outcome;

    const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthentication;

    pub fn available() -> bool {
        // SAFETY: a fresh context and a policy check that only reads system state.
        unsafe { LAContext::new().canEvaluatePolicy_error(POLICY).is_ok() }
    }

    pub fn confirm(reason: &str, wait: Duration) -> Outcome {
        // SAFETY: `LAContext` is used from this thread only; the reply block only sends on a channel, and
        // the context outlives the wait, so the framework never calls back into freed memory.
        unsafe {
            let ctx = LAContext::new();
            if ctx.canEvaluatePolicy_error(POLICY).is_err() {
                return Outcome::Unavailable;
            }
            let (tx, rx) = channel::<bool>();
            let reply = RcBlock::new(move |ok: Bool, _err: *mut NSError| {
                let _ = tx.send(ok.as_bool());
            });
            ctx.evaluatePolicy_localizedReason_reply(POLICY, &NSString::from_str(reason), &reply);
            match rx.recv_timeout(wait) {
                Ok(true) => Outcome::Confirmed,
                Ok(false) => Outcome::Declined,
                Err(RecvTimeoutError::Timeout) => {
                    ctx.invalidate();
                    Outcome::TimedOut
                }
                Err(RecvTimeoutError::Disconnected) => Outcome::Declined,
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::time::Duration;

    use super::Outcome;

    pub fn available() -> bool {
        false
    }

    pub fn confirm(_reason: &str, _wait: Duration) -> Outcome {
        Outcome::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_that_cannot_be_shown_is_reported_not_confirmed() {
        // CI has no screen session. Whatever this machine can do, it must never answer Confirmed on its own.
        if !available() {
            assert_eq!(
                confirm("test", Duration::from_millis(50)),
                Outcome::Unavailable
            );
        }
    }
}
