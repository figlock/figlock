//! Signing this device in to Google Drive and out again, shared by the CLI and the desktop app (ADR-0021).
//!
//! The refresh token is kept in `drive-token.json` (owner-only) next to the device keys. It only unlocks
//! ciphertext: the vault is still protected by the master password and Secret Key.

use std::time::Duration;

use figlock_store::GoogleOAuth;

use crate::config::{self, StoredDriveToken};
use crate::error::{Fail, Res};

/// How long the consent page may stay open before sign-in gives up.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Source builds carry no Google client secret (ADR-0021); say how to get sign-in anyway.
pub fn no_client() -> Fail {
    Fail::new(
        "this build has no Google client secret (only official release builds do); install a release build, \
         or set FIGLOCK_GOOGLE_CLIENT_SECRET (and FIGLOCK_GOOGLE_CLIENT_ID for your own client)",
    )
}

/// What the app can tell the person about Drive without making a network call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// This build, or the environment, has a Google client to sign in with.
    pub can_sign_in: bool,
    /// A sign-in is stored on this device. It may still have been revoked at Google.
    pub signed_in: bool,
}

pub fn status() -> Res<Status> {
    Ok(Status {
        can_sign_in: GoogleOAuth::from_env().is_some(),
        signed_in: config::load_drive_token()?.is_some(),
    })
}

/// Best effort: ask the OS to open `url` in the default browser.
pub fn open_browser(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        Some("open")
    } else if cfg!(all(unix, not(target_os = "macos"))) {
        Some("xdg-open")
    } else {
        None
    };
    if let Some(cmd) = opener {
        let _ = std::process::Command::new(cmd)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

/// Sign in through the browser and keep the refresh token. Blocks until the person finishes, or
/// `SIGN_IN_TIMEOUT`. `show_url` gets the consent URL, for a front end that wants to show it as well as open it.
pub fn sign_in(show_url: impl FnOnce(&str)) -> Res<()> {
    let oauth = GoogleOAuth::from_env().ok_or_else(no_client)?;
    let creds = oauth.sign_in(
        |url| {
            show_url(url);
            open_browser(url);
        },
        SIGN_IN_TIMEOUT,
    )?;
    config::save_drive_token(&creds.refresh_token)
}

/// What signing out did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignOut {
    /// There was no sign-in on this device.
    NotSignedIn,
    /// Signed out here and Google confirmed the access is revoked.
    Revoked,
    /// Signed out here, but Google could not be reached to revoke it (offline, or already revoked).
    NotRevoked,
    /// The token file was damaged, so there was nothing to revoke; it is removed.
    Damaged,
}

/// Revoke the token at Google, then forget it locally. Revoking comes first so the token is dead even if
/// someone copied the file. A failure (offline, or already revoked) must not trap the person in a signed-in
/// state, so the local copy goes either way.
pub fn sign_out() -> Res<SignOut> {
    let token = match config::read_drive_token_for_logout()? {
        StoredDriveToken::Missing => return Ok(SignOut::NotSignedIn),
        StoredDriveToken::Damaged => None,
        StoredDriveToken::Token(t) => Some(t),
    };
    let revoked = token.map(|t| GoogleOAuth::for_revoke().revoke(&t).is_ok());
    config::forget_drive_token()?;
    Ok(match revoked {
        Some(true) => SignOut::Revoked,
        Some(false) => SignOut::NotRevoked,
        None => SignOut::Damaged,
    })
}
