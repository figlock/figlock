//! `figlock drive`: sign this device in to Google Drive, the storage a vault can live in (FIG-42).
//!
//! The refresh token is kept in `drive-token.json` (owner-only) next to the device keys. It only unlocks
//! ciphertext: the vault is still protected by the master password and Secret Key.

use std::time::Duration;

use figlock_store::GoogleOAuth;
use serde_json::json;

use figlock_session::config::{self, StoredDriveToken};
use figlock_session::error::{Fail, Res};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Source builds carry no Google client secret (ADR-0021); say how to get sign-in anyway.
pub fn no_client() -> Fail {
    Fail::new(
        "this build has no Google client secret (only official release builds do); install a release build, \
         or set FIGLOCK_GOOGLE_CLIENT_SECRET (and FIGLOCK_GOOGLE_CLIENT_ID for your own client)",
    )
}

fn client() -> Res<GoogleOAuth> {
    GoogleOAuth::from_env().ok_or_else(no_client)
}

/// Best effort: ask the OS to open the consent page. The URL is always printed too.
fn open_browser(url: &str) {
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

pub fn login(json: bool) -> Res<()> {
    let oauth = client()?;
    let creds = oauth.sign_in(
        |url| {
            eprintln!(
                "Opening your browser to sign in to Google. If nothing opens, visit:\n\n  {url}\n"
            );
            open_browser(url);
        },
        LOGIN_TIMEOUT,
    )?;
    config::save_drive_token(&creds.refresh_token)?;
    if json {
        println!("{}", json!({"status": "signed-in"}));
    } else {
        println!("Signed in to Google Drive. FigLock can only see its own hidden app folder.");
    }
    Ok(())
}

pub fn logout(json: bool) -> Res<()> {
    let token = match config::read_drive_token_for_logout()? {
        StoredDriveToken::Missing => {
            if json {
                println!("{}", json!({"status": "not-signed-in"}));
            } else {
                println!("Not signed in to Google Drive.");
            }
            return Ok(());
        }
        StoredDriveToken::Damaged => None,
        StoredDriveToken::Token(t) => Some(t),
    };
    // Revoke first so the token is dead at Google even if someone copied the file. A failure (offline, or
    // already revoked) must not trap the user in a signed-in state, so the local copy goes either way.
    let revoked = token.map(|t| GoogleOAuth::for_revoke().revoke(&t).is_ok());
    config::forget_drive_token()?;
    match (revoked, json) {
        (Some(true), true) => println!("{}", json!({"status": "signed-out", "revoked": true})),
        (Some(true), false) => {
            println!("Signed out of Google Drive and revoked FigLock's access.")
        }
        (Some(false), true) => println!("{}", json!({"status": "signed-out", "revoked": false})),
        (Some(false), false) => println!(
            "Signed out on this device, but Google could not be reached to revoke access. Remove FigLock under Google Account > Security > Third-party access to be sure."
        ),
        (None, true) => println!(
            "{}",
            json!({"status": "signed-out", "revoked": false, "reason": "damaged-token-file"})
        ),
        (None, false) => println!(
            "Removed a damaged drive-token.json, so there was nothing to revoke. Remove FigLock under Google Account > Security > Third-party access to be sure."
        ),
    }
    Ok(())
}
