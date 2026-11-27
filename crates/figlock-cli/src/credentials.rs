//! Password change, Secret Key rotation and device revocation (`docs/adr/0022`, amendment FIG-72). Each one
//! turns quick unlock off, because the key it kept no longer opens the vault, and tells the person that other
//! devices need a full unlock.

use figlock_core::{CredentialChange, DeviceId, DeviceInfo, SecretKey};
use serde_json::json;

use crate::{prompt, quick};
use figlock_session::error::{Fail, Res};
use figlock_session::secret_key_store;
use figlock_session::session::{Session, peek_vault_id};
use figlock_session::{docfile, now_ms};

const OTHER_DEVICES: &str = "Other devices will ask for a full unlock with the new details.";

fn forget_quick_unlock() {
    let result = peek_vault_id().and_then(|vault| quick::system()?.forget(&vault));
    if let Err(e) = result {
        eprintln!("warning: quick unlock key was not removed: {e}");
    }
}

/// Keep the new Secret Key on this device, in place of the old one (ADR-0027). The vault is already changed, so a
/// failure only warns: the key is still shown and the next full unlock asks for it.
fn keep_secret_key(sk: &SecretKey, vault_json: &[u8]) {
    let kept = secret_key_store::system().and_then(|keys| {
        if !keys.available() {
            return Ok(());
        }
        let vault = peek_vault_id()?;
        let sk_gen = figlock_core::Vault::sk_gen_in(vault_json)?;
        keys.put(&vault, sk, sk_gen)
    });
    if let Err(e) = kept {
        eprintln!("warning: the new Secret Key was not kept on this device: {e}");
    }
}

fn show_secret_key(sk: &SecretKey, json: bool) -> Res<()> {
    if json {
        println!("{}", json!({ "secret_key": sk.format().as_str() }));
    } else {
        println!("Secret Key: {}", sk.format().as_str());
    }
    eprintln!(
        "Write the new Secret Key down now and replace your Emergency Kit. The old one no longer works."
    );
    Ok(())
}

fn done(json: bool, what: &str) {
    if json {
        eprintln!("{what} {OTHER_DEVICES}");
    } else {
        println!("{what} {OTHER_DEVICES}");
    }
    eprintln!(
        "Quick unlock is off on this device; run `figlock quick-unlock enable` to turn it back on."
    );
}

/// `figlock password change`
pub fn password_change(json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let (session, password, secret_key) = Session::open_for_change(prompt::terminal())?;
    let mut session = session;
    session.ensure_writable()?;
    let new_password = prompt::changed_password()?;
    let r = session.vault.change_credentials(
        &password,
        &secret_key,
        CredentialChange {
            new_password: Some(&new_password),
            ..CredentialChange::default()
        },
    )?;
    session.replace_vault_json(&r.vault_json)?;
    let pushed = crate::sync_cmd::push_after_write();
    forget_quick_unlock();
    done(json, "The password is changed.");
    pushed.announce();
    Ok(())
}

/// `figlock secret-key rotate`
pub fn secret_key_rotate(json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let (mut session, password, secret_key) = Session::open_for_change(prompt::terminal())?;
    session.ensure_writable()?;
    let r = session.vault.change_credentials(
        &password,
        &secret_key,
        CredentialChange {
            rotate_secret_key: true,
            ..CredentialChange::default()
        },
    )?;
    session.replace_vault_json(&r.vault_json)?;
    let pushed = crate::sync_cmd::push_after_write();
    forget_quick_unlock();
    let new = r
        .secret_key
        .ok_or_else(|| Fail::new("no new Secret Key was made"))?;
    keep_secret_key(&new, &r.vault_json);
    show_secret_key(&new, json)?;
    done(json, "The Secret Key is rotated.");
    pushed.announce();
    Ok(())
}

/// `figlock secret-key show`: print the Secret Key kept on this device, after Touch ID or the device password.
pub fn secret_key_show(json: bool) -> Res<()> {
    let keys = secret_key_store::system()?;
    let vault = peek_vault_id()?;
    if !keys.has(&vault) {
        return Err(Fail::new(
            "no Secret Key is kept on this device yet. It is kept the next time you unlock with the master \
             password and Secret Key, or find it on your Emergency Kit",
        ));
    }
    let key = keys
        .get(
            &vault,
            "show your Secret Key",
            secret_key_store::quick_on(&vault),
        )?
        .ok_or_else(|| {
            Fail::new(
                "the Secret Key was not shown: Touch ID or the device password was not confirmed",
            )
        })?;
    if json {
        println!("{}", json!({ "secret_key": key.format().as_str() }));
    } else {
        println!("Secret Key: {}", key.format().as_str());
    }
    Ok(())
}

/// `figlock secret-key kit <path>`: write the Emergency Kit PDF, made now from the Secret Key kept on this
/// device. It asks for Touch ID or the device password like `show`, writes only to `path`, and never uploads.
pub fn secret_key_kit(path: &str, force: bool, json: bool) -> Res<()> {
    let keys = secret_key_store::system()?;
    let vault = peek_vault_id()?;
    if !keys.has(&vault) {
        return Err(Fail::new(
            "no Secret Key is kept on this device, so there is nothing to make the kit from. It is kept the \
             next time you unlock with the password and Secret Key",
        ));
    }
    let place = figlock_session::config::load()?.place;
    let pdf = figlock_session::kit::kit_from_kept_key(
        &keys,
        &vault,
        secret_key_store::quick_on(&vault),
        &place,
        now_ms(),
    )?
    .ok_or_else(|| {
        Fail::new(
            "the Emergency Kit was not saved: Touch ID or the device password was not confirmed",
        )
    })?;
    docfile::write_out(&pdf, path, force)?;
    if json {
        println!("{}", json!({ "saved": path }));
    } else if path != "-" {
        println!(
            "Saved the Emergency Kit to {path} (owner-only). Print it or keep it safe, apart from your password."
        );
    }
    Ok(())
}

/// `figlock secret-key forget`: stop keeping the Secret Key on this device. The choice is remembered, so every
/// full unlock asks for the key until `figlock secret-key keep` (FIG-142).
pub fn secret_key_forget(json: bool) -> Res<()> {
    secret_key_store::system()?.opt_out(&peek_vault_id()?)?;
    if json {
        println!("{}", json!({ "kept": false }));
    } else {
        println!(
            "The Secret Key is no longer kept on this device. Every full unlock asks for it until you run `figlock secret-key keep`."
        );
    }
    Ok(())
}

/// `figlock secret-key keep`: keep the Secret Key on this device again. It asks for the password and
/// Secret Key once, checks them against the vault, and from then on the password is all that is typed.
pub fn secret_key_keep(json: bool) -> Res<()> {
    let keys = secret_key_store::system()?;
    if !keys.available() {
        return Err(Fail::new("this device cannot keep the Secret Key"));
    }
    let (_session, _password, secret_key) = Session::open_for_change(prompt::terminal())?;
    let vault = peek_vault_id()?;
    let sk_gen = figlock_core::Vault::sk_gen_in(&figlock_session::session::peek_vault_json()?)?;
    keys.keep_again(&vault, &secret_key, sk_gen)?;
    if json {
        println!("{}", json!({ "kept": true }));
    } else {
        println!("The Secret Key is kept on this device again.");
    }
    Ok(())
}

fn label(d: &DeviceInfo) -> String {
    d.name.as_deref().map_or_else(
        || "(name unknown)".to_owned(),
        |n| crate::render::clean(n).into_owned(),
    )
}

/// `figlock device list`
pub fn device_list(json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let session = Session::open(prompt::terminal())?;
    let this = *session.vault.device_id();
    let devices = session.vault.devices();
    if json {
        let rows: Vec<_> = devices
            .iter()
            .map(|d| {
                json!({
                    "device_id": hex::encode(d.device_id),
                    "name": d.name,
                    "fingerprint": d.fingerprint,
                    "revoked": d.revoked_at.is_some(),
                    "ignored_changes": session.ignored_changes(&d.device_id),
                    "this_device": d.device_id == this,
                })
            })
            .collect();
        println!("{}", json!(rows));
        return Ok(());
    }
    if let Some(at) = session.removed_at_ms() {
        eprintln!("{}", figlock_session::removed_message(at));
    }
    for d in &devices {
        let mark = if d.device_id == this {
            " (this device)"
        } else {
            ""
        };
        let ignored = session.ignored_changes(&d.device_id);
        let state = match (d.revoked_at.is_some(), ignored) {
            (false, _) => String::new(),
            (true, 0) => "  revoked".to_owned(),
            (true, 1) => "  (revoked, 1 later change ignored)".to_owned(),
            (true, n) => format!("  (revoked, {n} later changes ignored)"),
        };
        println!(
            "{}  {}  {}{mark}{state}",
            &hex::encode(d.device_id)[..8],
            d.fingerprint,
            label(d)
        );
    }
    Ok(())
}

/// A live device by id prefix (4+ hex digits).
fn pick(devices: &[DeviceInfo], this: &DeviceId, reference: &str) -> Res<DeviceId> {
    let r = reference.trim().to_ascii_lowercase();
    if r.len() < 4 || !r.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Fail::new(
            "give at least 4 hex digits of a device id from `figlock device list`",
        ));
    }
    let mut hits = devices
        .iter()
        .filter(|d| d.revoked_at.is_none() && hex::encode(d.device_id).starts_with(&r));
    let first = hits
        .next()
        .ok_or_else(|| Fail::new("no device matches that id"))?;
    if hits.next().is_some() {
        return Err(Fail::new(
            "that id matches several devices; type more of it",
        ));
    }
    if first.device_id == *this {
        return Err(Fail::new(
            "this is the device you are using; revoke it from another device",
        ));
    }
    Ok(first.device_id)
}

/// `figlock device revoke <id> [--change-password] [--rotate-secret-key]`
pub fn device_revoke(
    reference: &str,
    change_password: bool,
    rotate_secret_key: bool,
    json: bool,
) -> Res<()> {
    crate::sync_cmd::pull_first();
    let (mut session, password, secret_key) = Session::open_for_change(prompt::terminal())?;
    session.ensure_writable()?;
    let target = pick(
        &session.vault.devices(),
        session.vault.device_id(),
        reference,
    )?;
    let now = figlock_session::session::now_ms();
    let new_password = if change_password {
        Some(prompt::changed_password()?)
    } else {
        None
    };
    let changing = change_password || rotate_secret_key;
    let r = if changing {
        session.vault.revoke_device_and_change_credentials(
            &password,
            &secret_key,
            &target,
            CredentialChange {
                new_password: new_password.as_deref().map(String::as_str),
                rotate_secret_key,
                ..CredentialChange::default()
            },
            now,
        )?
    } else {
        session
            .vault
            .revoke_device(&password, &secret_key, &target, now)?
    };
    session.write_revocation(&r)?;
    let pushed = crate::sync_cmd::push_after_write();
    if changing {
        forget_quick_unlock();
    }
    if let Some(new) = &r.secret_key {
        keep_secret_key(new, &r.vault_json);
        show_secret_key(new, json)?;
    }
    let what = "The device is revoked.";
    if changing {
        done(json, what);
    } else {
        println!("{what}");
    }
    eprintln!(
        "It can no longer open anything written from now on. A copy of the vault it already has stays \
         readable, and so does anything it copied out."
    );
    if !changing {
        eprintln!(
            "It may still know your password and Secret Key. If it might have been compromised, \
             run `figlock device revoke` again with --change-password and --rotate-secret-key instead \
             of leaving the old ones in place."
        );
    }
    pushed.announce();
    Ok(())
}
