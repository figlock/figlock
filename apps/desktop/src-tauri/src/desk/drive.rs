//! Google Drive sign-in as the window sees it (ADR-0021). The refresh token stays in the device config; the
//! window only learns whether it can sign in and whether this device is signed in.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use figlock_session::drive::{self, SignOut};
use serde::Serialize;

use super::{CmdError, CmdRes};

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriveView {
    /// This build has a Google client to sign in with. Source builds do not (ADR-0021).
    pub can_sign_in: bool,
    pub signed_in: bool,
}

pub fn status() -> CmdRes<DriveView> {
    let s = drive::status()?;
    Ok(DriveView {
        can_sign_in: s.can_sign_in,
        signed_in: s.signed_in,
    })
}

/// The sign-in that is waiting for the browser, if any, so Cancel can end it.
static WAITING: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Opens the browser and waits for the person to finish, cancel or run out of time. Blocks, so run it off the
/// async runtime. A person who cancels gets the `sign_in_cancelled` code and nothing is changed.
pub fn sign_in() -> CmdRes<DriveView> {
    let cancel = Arc::new(AtomicBool::new(false));
    *WAITING.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
    let result = drive::sign_in_until(|_| {}, &cancel);
    let mut waiting = WAITING.lock().unwrap_or_else(|p| p.into_inner());
    // Only clear our own wait: a newer sign-in may have started since.
    if waiting.as_ref().is_some_and(|w| Arc::ptr_eq(w, &cancel)) {
        *waiting = None;
    }
    drop(waiting);
    result.map_err(|f| match f.1 {
        figlock_session::error::FailKind::SignInCancelled => {
            CmdError::new("sign_in_cancelled", f.0)
        }
        _ => CmdError::new("drive_sign_in", f.0),
    })?;
    status()
}

/// Stop the sign-in that is waiting for the browser. Nothing happens when none is.
pub fn cancel_sign_in() {
    if let Some(w) = WAITING.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        w.store(true, Ordering::Relaxed);
    }
}

/// What to tell the person after signing out.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SignedOut {
    /// Google confirmed FigLock's access is gone. When false, the person should remove it in their Google account.
    pub revoked: bool,
}

pub fn sign_out() -> CmdRes<SignedOut> {
    let r = drive::sign_out()?;
    Ok(SignedOut {
        revoked: matches!(r, SignOut::Revoked),
    })
}
