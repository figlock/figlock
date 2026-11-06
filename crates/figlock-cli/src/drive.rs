//! `figlock drive`: sign this device in to Google Drive, the storage a vault can live in (FIG-42).
//!
//! The refresh token is kept in `drive-token.json` (owner-only) next to the device keys. It only unlocks
//! ciphertext: the vault is still protected by the master password and Secret Key.

use std::time::Duration;

use figlock_store::GoogleOAuth;
use serde_json::json;

use crate::config;
use crate::error::{Fail, Res};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn client() -> Res<GoogleOAuth> {
    GoogleOAuth::from_env().ok_or_else(|| {
        Fail::new(
            "Google sign-in is not configured in this build; set FIGLOCK_GOOGLE_CLIENT_ID and FIGLOCK_GOOGLE_CLIENT_SECRET",
        )
    })
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
    let Some(token) = config::load_drive_token()? else {
        if json {
            println!("{}", json!({"status": "not-signed-in"}));
        } else {
            println!("Not signed in to Google Drive.");
        }
        return Ok(());
    };
    // Revoke first so the token is dead at Google even if someone copied the file. A failure (offline, or
    // already revoked) must not trap the user in a signed-in state, so the local copy goes either way.
    let revoked = client().and_then(|c| c.revoke(&token).map_err(Fail::from));
    config::forget_drive_token()?;
    match (&revoked, json) {
        (Ok(()), true) => println!("{}", json!({"status": "signed-out", "revoked": true})),
        (Ok(()), false) => println!("Signed out of Google Drive and revoked FigLock's access."),
        (Err(_), true) => println!("{}", json!({"status": "signed-out", "revoked": false})),
        (Err(_), false) => println!(
            "Signed out on this device, but Google could not be reached to revoke access. Remove FigLock under Google Account > Security > Third-party access to be sure."
        ),
    }
    Ok(())
}
