//! `figlock drive`: sign this device in to Google Drive, the storage a vault can live in (FIG-42).
//!
//! The sign-in itself is `figlock_session::drive`, shared with the desktop app; this file is the terminal
//! side: what to print.

use serde_json::json;

use figlock_session::drive::{self, SignOut};
use figlock_session::error::Res;

pub fn login(json: bool) -> Res<()> {
    drive::sign_in(|url| {
        eprintln!(
            "Opening your browser to sign in to Google. If nothing opens, visit:\n\n  {url}\n"
        );
    })?;
    if json {
        println!("{}", json!({"status": "signed-in"}));
    } else {
        println!("Signed in to Google Drive. FigLock can only see its own hidden app folder.");
    }
    Ok(())
}

pub fn logout(json: bool) -> Res<()> {
    match (drive::sign_out()?, json) {
        (SignOut::NotSignedIn, true) => println!("{}", json!({"status": "not-signed-in"})),
        (SignOut::NotSignedIn, false) => println!("Not signed in to Google Drive."),
        (SignOut::Revoked, true) => {
            println!("{}", json!({"status": "signed-out", "revoked": true}))
        }
        (SignOut::Revoked, false) => {
            println!("Signed out of Google Drive and revoked FigLock's access.")
        }
        (SignOut::NotRevoked, true) => {
            println!("{}", json!({"status": "signed-out", "revoked": false}))
        }
        (SignOut::NotRevoked, false) => println!(
            "Signed out on this device, but Google could not be reached to revoke access. Remove FigLock under Google Account > Security > Third-party access to be sure."
        ),
        (SignOut::Damaged, true) => println!(
            "{}",
            json!({"status": "signed-out", "revoked": false, "reason": "damaged-token-file"})
        ),
        (SignOut::Damaged, false) => println!(
            "Removed a damaged drive-token.json, so there was nothing to revoke. Remove FigLock under Google Account > Security > Third-party access to be sure."
        ),
    }
    Ok(())
}
