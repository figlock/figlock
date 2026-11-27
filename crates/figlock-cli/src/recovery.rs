//! `figlock recovery-key ...` and `figlock forgot`: the optional Recovery Key and the password reset it makes
//! possible (`docs/spec/pairing-recovery.md` 7, ADR-0029, ADR-0045). The same safety net the Mac and iPhone apps have.
//!
//! The key is made in memory, shown once, checked by typing its last two groups back (or `--saved` for a script
//! that has stored it), and only then written into `vault.json`. Credentials are proved first, so a wrong
//! password or Secret Key changes nothing. FigLock never stores the key.

use std::io::{IsTerminal, Write};

use figlock_core::{
    DeviceKeys, KitInfo, Recover, RecoveryKey, SecretKey, Vault, render_recovery_kit,
};
use figlock_session::config::{self, Config};
use figlock_session::error::{Fail, Res};
use figlock_session::session::{Session, civil_date, now_ms, peek_vault_json};
use figlock_session::{docfile, kit, store};
use figlock_store::StoreError;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::{ForgotArgs, RecoveryArgs, prompt, remotes};

/// Characters that read the same on paper, folded together the way a typed answer is checked.
fn squash_typed(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            u => u,
        })
        .collect()
}

/// The last two dash-separated groups of a code.
fn tail_of(code: &str) -> String {
    let groups: Vec<&str> = code.split('-').collect();
    groups[groups.len().saturating_sub(2)..].concat()
}

/// Whether `answer` is the last two groups of `code`, however the person wrote the confusable characters.
fn tail_matches(code: &str, answer: &str) -> bool {
    squash_typed(answer) == squash_typed(&tail_of(code))
}

/// The forced check that a code that was shown can be read back. `saved` is the script's word that it has been
/// stored; without it a terminal is needed to type the check.
pub(crate) fn confirm_saved(code: &str, what: &str, saved: bool) -> Res<()> {
    if saved {
        return Ok(());
    }
    let question = format!("Type the last two groups of the {what} to prove you saved it: ");
    match prompt::typed_line(&question)? {
        Some(answer) if tail_matches(code, &answer) => Ok(()),
        Some(_) => Err(Fail::new(format!(
            "that is not the end of the {what}; nothing was changed"
        ))),
        None => Err(Fail::new(format!(
            "cannot check the {what} without a terminal; store it and run again with --saved"
        ))),
    }
}

/// Write the one page PDF for `key` to `path`, if asked.
fn write_recovery_kit(
    place: &config::Place,
    code: &str,
    path: Option<&str>,
    force: bool,
) -> Res<()> {
    let Some(path) = path else { return Ok(()) };
    let storage = kit::storage_label(place);
    let info = KitInfo {
        vault_name: "Personal",
        storage: &storage,
        created: &civil_date(now_ms()),
    };
    let pdf = render_recovery_kit(&info, code)
        .map_err(|_| Fail::new("could not make the Recovery Key page"))?;
    docfile::write_out(&pdf, path, force)
}

const KEEP_APART: &str = "Anyone with this code can open the vault without the password. Keep it apart from your \
Emergency Kit and your password.";

/// Show a new code: on stdout for a person, held back for the JSON that follows the write for a script.
fn show_code(code: &str, json: bool) {
    if !json {
        println!("Recovery Key: {code}");
    }
    eprintln!("{KEEP_APART}");
}

/// `figlock recovery-key status`: whether the vault has one. Reads `vault.json` only, so it needs no password.
pub fn status(json: bool) -> Res<()> {
    let on = Vault::recovery_in(&peek_vault_json()?)
        .map_err(|_| Fail::new("the vault file could not be read"))?;
    if json {
        println!("{}", json!({ "enabled": on }));
    } else if on {
        println!("on: a forgotten password can be reset with `figlock forgot`");
    } else {
        println!(
            "off: a forgotten password cannot be recovered. `figlock recovery-key enable` turns it on"
        );
    }
    Ok(())
}

/// `figlock recovery-key enable` and `replace`.
pub fn set(a: &RecoveryArgs, replace: bool, json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let (mut session, password, secret_key) = Session::open_for_change(prompt::terminal())?;
    session.ensure_writable()?;
    match (session.vault.has_recovery(), replace) {
        (true, false) => {
            return Err(Fail::new(
                "a Recovery Key is already on; `figlock recovery-key replace` makes a new one and ends the old",
            ));
        }
        (false, true) => {
            return Err(Fail::new(
                "there is no Recovery Key to replace; `figlock recovery-key enable` makes one",
            ));
        }
        _ => {}
    }
    let key = RecoveryKey::random().map_err(|_| Fail::new("could not get randomness"))?;
    let code = key.format();
    show_code(&code, json);
    confirm_saved(&code, "Recovery Key", a.saved)?;
    let place = config::load()?.place;
    write_recovery_kit(&place, &code, a.kit.as_deref(), a.force)?;
    let change = session
        .vault
        .set_recovery_with(&key, &password, &secret_key)?;
    session.replace_vault_json(&change.vault_json)?;
    let pushed = crate::sync_cmd::push_after_write();
    let what = if replace {
        "The Recovery Key is replaced; the old code opens nothing from now on."
    } else {
        "The Recovery Key is on."
    };
    if json {
        println!(
            "{}",
            json!({ "enabled": true, "replaced": replace, "recovery_key": code.as_str() })
        );
    } else {
        println!("{what}");
    }
    pushed.announce();
    Ok(())
}

/// `figlock recovery-key disable`
pub fn disable(yes: bool, json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let (mut session, _password, _secret_key) = Session::open_for_change(prompt::terminal())?;
    session.ensure_writable()?;
    if !session.vault.has_recovery() {
        return Err(Fail::new("the Recovery Key is already off"));
    }
    if !yes
        && !prompt::confirm_default_no(
            "Turn off the Recovery Key? A forgotten password will then be unrecoverable. [y/N] ",
        )?
    {
        return Err(Fail::new("not turned off"));
    }
    let change = session.vault.clear_recovery()?;
    session.replace_vault_json(&change.vault_json)?;
    let pushed = crate::sync_cmd::push_after_write();
    if json {
        println!("{}", json!({ "enabled": false }));
    } else {
        println!("The Recovery Key is off; its code opens nothing from now on.");
    }
    pushed.announce();
    Ok(())
}

/// A code typed or read from `FIGLOCK_RECOVERY_KEY`.
fn typed_recovery_key() -> Res<RecoveryKey> {
    let text = match std::env::var("FIGLOCK_RECOVERY_KEY") {
        Ok(t) if !t.is_empty() => Zeroizing::new(t),
        _ => {
            if !std::io::stdin().is_terminal() {
                return Err(Fail::new(
                    "no Recovery Key: type it at a terminal, or set FIGLOCK_RECOVERY_KEY",
                ));
            }
            let t = rpassword::prompt_password("Recovery Key (FLR1-...): ")
                .map_err(|_| Fail::new("could not read the Recovery Key"))?;
            Zeroizing::new(t)
        }
    };
    RecoveryKey::parse(text.trim())
        .map_err(|_| Fail::new("that is not a valid Recovery Key; check the code for typos"))
}

/// `figlock forgot`: reset the password with the Recovery Key. On a device that has the vault it keeps that
/// device; on a new one (`--store`, `--drive` ...) it sets the device up.
pub fn forgot(a: ForgotArgs, json: bool) -> Res<()> {
    let set_up = config::is_initialised()?;
    let place = if set_up {
        // The place this device is set up with, whatever else was typed.
        crate::sync_cmd::pull_first();
        config::load()?.place
    } else {
        let place = crate::pairing::place_for(&a.at)?;
        remotes::wait_for_icloud(&place.remotes, a.at.wait_for_sync)?;
        remotes::open_from_remote_waiting(&place, a.at.wait_for_sync)?;
        place
    };
    let storage = store::open(&place)?;
    let vault_json = storage.get("vault.json").map_err(|e| match e {
        StoreError::NotFound => Fail::new("there is no FigLock vault in that storage"),
        e => e.into(),
    })?;
    match Vault::recovery_in(&vault_json) {
        Ok(true) => {}
        Ok(false) => {
            return Err(Fail::new(
                "that vault has no Recovery Key, so it cannot be opened this way. Use your Emergency Kit and \
                 password (`figlock open`), or a device that already has the vault",
            ));
        }
        Err(_) => return Err(Fail::new("that vault could not be read")),
    }
    let mut records = Vec::new();
    for e in storage.list("devices/")? {
        if let Ok(bytes) = storage.get(&e.path) {
            records.push(bytes);
        }
    }
    let key = typed_recovery_key()?;
    let device_name = if set_up {
        config::load()?.device_name
    } else {
        a.at.device_name.clone()
    };
    let this_device = || -> Res<DeviceKeys> {
        if set_up {
            Ok(config::load_device()?)
        } else {
            Ok(DeviceKeys::generate()?)
        }
    };
    let opens_nothing = || {
        Fail::new(
            "that Recovery Key does not open this vault. Check the code, and that this is the right storage",
        )
    };

    // A trial run proves the key and shows what it found. Nothing is stored.
    let throwaway = SecretKey::random()?.format();
    let (found, _) = Vault::recover(
        Recover {
            vault_json: &vault_json,
            recovery_key: &key,
            new_password: throwaway.as_str(),
            new_recovery_key: None,
        },
        this_device()?,
        &device_name,
        &records,
        now_ms(),
    )
    .map_err(|_| opens_nothing())?;
    let preview = Session::preview(found, &place)?;
    eprintln!(
        "The Recovery Key opens a vault with {} item(s) and {} other device(s).",
        preview.items, preview.devices
    );

    let new_password = prompt::changed_password()?;
    if new_password.chars().count() < 8 {
        return Err(Fail::new("use at least 8 characters for the password"));
    }
    let new_key = if a.keep_recovery_key {
        None
    } else {
        Some(RecoveryKey::random().map_err(|_| Fail::new("could not get randomness"))?)
    };
    let (vault, out) = Vault::recover(
        Recover {
            vault_json: &vault_json,
            recovery_key: &key,
            new_password: &new_password,
            new_recovery_key: new_key.as_ref(),
        },
        this_device()?,
        &device_name,
        &records,
        now_ms(),
    )
    .map_err(|_| opens_nothing())?;

    // The new details are shown and checked before anything is written, so a vault is never left behind a Secret
    // Key nobody saved.
    let secret_key = out.secret_key.format();
    let new_code = new_key.as_ref().map(RecoveryKey::format);
    if !json {
        println!("Secret Key: {}", secret_key.as_str());
        if let Some(code) = &new_code {
            println!("Recovery Key: {}", code.as_str());
        }
    }
    eprintln!(
        "Write the new Secret Key down now and replace your Emergency Kit. The old one no longer works."
    );
    if new_code.is_some() {
        eprintln!("The Recovery Key you just used is replaced by the new one above. {KEEP_APART}");
    }
    confirm_saved(&secret_key, "Secret Key", a.saved)?;
    if let Some(code) = &new_code {
        confirm_saved(code, "new Recovery Key", a.saved)?;
        write_recovery_kit(&place, code, a.recovery_kit.as_deref(), a.force)?;
    }
    if let Some(path) = &a.kit {
        let pdf = kit::kit_pdf(&place, &out.secret_key, now_ms())?;
        docfile::write_out(&pdf, path, a.force)?;
    }

    let cfg = Config {
        place: place.clone(),
        device_name: device_name.clone(),
        autolock_secs: None,
    };
    if !set_up {
        config::save(&cfg, vault.device_keys())?;
    }
    let written = (|| -> Result<(), StoreError> {
        if let Some(record) = &out.new_record {
            let path = format!("devices/{}.json", hex::encode(vault.device_id()));
            match storage.put_new(&path, record) {
                Ok(()) | Err(StoreError::Exists) => {}
                Err(e) => return Err(e),
            }
        }
        storage.put_replace("vault.json", &out.vault_json)
    })();
    if let Err(e) = written {
        // The vault is as it was; a fresh device is not left half set up.
        if !set_up {
            config::forget();
        }
        return Err(e.into());
    }
    crate::credentials::keep_secret_key(&out.secret_key, &out.vault_json);
    crate::credentials::forget_quick_unlock();
    let pushed = crate::sync_cmd::push_after_write();
    if json {
        let mut o: Value = json!({
            "reset": true,
            "secret_key": secret_key.as_str(),
        });
        if let Some(code) = &new_code {
            o["recovery_key"] = json!(code.as_str());
        }
        println!("{o}");
    } else {
        println!(
            "The password is reset. Unlock with the new password; the Secret Key above replaces the old one."
        );
    }
    eprintln!(
        "Other devices will ask for a full unlock with the new details. Whoever held the old password or Secret \
         Key can still open an old copy of the vault: revoke devices you do not trust with `figlock device revoke`."
    );
    pushed.announce();
    let _ = std::io::stdout().flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "FLR1-AB12-CD34-EF56-GH78-JK90-MN12-PQ34-RS56-TV78-WX90-AB12-CD34-EF56-GH78";

    #[test]
    fn the_check_is_the_last_two_groups_however_they_are_typed() {
        assert!(tail_matches(CODE, "EF56-GH78"));
        assert!(tail_matches(CODE, "ef56 gh78"));
        assert!(tail_matches(CODE, "EF56GH78"));
        // Characters that look alike on paper are the same.
        assert!(tail_matches("FLR1-0O1I-LL11", "0o1i-ll11"));
        assert!(!tail_matches(CODE, "AB12-CD34"));
        assert!(!tail_matches(CODE, ""));
    }
}
