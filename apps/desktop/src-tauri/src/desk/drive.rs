//! Google Drive sign-in as the window sees it (ADR-0021). The refresh token stays in the device config; the
//! window only learns whether it can sign in and whether this device is signed in.

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

/// Opens the browser and waits for the person to finish. Blocks, so run it off the async runtime.
pub fn sign_in() -> CmdRes<DriveView> {
    drive::sign_in(|_| {}).map_err(|f| CmdError::new("drive_sign_in", f.0))?;
    status()
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
