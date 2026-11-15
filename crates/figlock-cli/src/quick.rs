//! `figlock quick-unlock` and `figlock lock`: the commands over the quick-unlock policy in `figlock-session`.

pub use figlock_session::quick::{InitChoice, InitOutcome, system};
use figlock_session::session::Session;
use figlock_session::{Fail, Res};
use serde_json::json;

use crate::prompt;

fn say(json: bool, message: &str, on: bool, kept: bool, protection: Option<&str>) -> Res<()> {
    if json {
        println!(
            "{}",
            json!({ "enabled": on, "unlocked": kept, "protection": protection })
        );
    } else {
        println!("{message}");
        match protection {
            Some("secure-enclave") => println!(
                "Protection: Secure Enclave. The key is bound to this Mac's chip and every use asks for Touch ID, your device password or your Apple Watch."
            ),
            Some(_) => println!(
                "Protection: login keychain, behind a Touch ID or device password prompt. This Mac cannot use the Secure Enclave."
            ),
            None => {}
        }
    }
    Ok(())
}

/// `figlock quick-unlock enable`: a full unlock, then keep the key.
pub fn enable(json: bool) -> Res<()> {
    let quick = system()?;
    if !quick.available() {
        return Err(Fail::new(
            "this machine cannot ask for Touch ID or the device password, so quick unlock is unavailable",
        ));
    }
    crate::sync_cmd::pull_first();
    let (_session, auk) = Session::open_full(prompt::terminal())?;
    let vault = figlock_session::session::peek_vault_id()?;
    quick.enable(&vault, &auk, figlock_session::session::now_ms() / 1000)?;
    // The Secret Key moves up to the Secure Enclave with it; it stays the same key (ADR-0034).
    if let Ok(keys) = figlock_session::secret_key_store::system()
        && let Err(e) = keys.retier(&vault, true, "keep your Secret Key with Touch ID")
    {
        eprintln!("warning: the Secret Key was not moved to the Secure Enclave: {e}");
    }
    say(
        json,
        "Quick unlock is on. Later commands ask for Touch ID or your device password.",
        true,
        true,
        quick.protection(&vault),
    )
}

/// `figlock quick-unlock disable`
pub fn disable(json: bool) -> Res<()> {
    let vault = figlock_session::session::peek_vault_id()?;
    system()?.forget(&vault)?;
    // The Secret Key stays on this Mac, in the login keychain where no prompt is needed (ADR-0034).
    if let Ok(keys) = figlock_session::secret_key_store::system()
        && let Err(e) = keys.retier(&vault, false, "keep your Secret Key without Touch ID")
    {
        eprintln!("warning: the Secret Key was not moved out of the Secure Enclave: {e}");
    }
    say(json, "Quick unlock is off.", false, false, None)
}

pub fn status(json: bool) -> Res<()> {
    let quick = system()?;
    let vault = figlock_session::session::peek_vault_id()?;
    let (on, kept) = quick.status(&vault);
    let protection = if on { quick.protection(&vault) } else { None };
    let msg = match (on, kept) {
        (false, _) => "Quick unlock is off.",
        (true, true) => "Quick unlock is on and the vault can be opened with Touch ID.",
        (true, false) => {
            "Quick unlock is on, but the vault is locked: the next command asks for the master password."
        }
    };
    say(json, msg, on, kept, protection)
}

/// `figlock lock`: drop the kept key. Quick unlock stays on and refills at the next full unlock.
pub fn lock(json: bool) -> Res<()> {
    system()?.lock(&figlock_session::session::peek_vault_id()?)?;
    if json {
        println!("{}", json!({ "locked": true }));
    } else {
        println!("Locked. The next command asks for the master password.");
    }
    Ok(())
}
