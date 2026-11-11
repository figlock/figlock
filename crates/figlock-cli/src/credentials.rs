//! Password change, Secret Key rotation and device revocation (`docs/adr/0022`, amendment FIG-72). Each one
//! turns quick unlock off, because the key it kept no longer opens the vault, and tells the person that other
//! devices need a full unlock.

use figlock_core::{CredentialChange, DeviceId, DeviceInfo, SecretKey};
use serde_json::json;

use crate::error::{Fail, Res};
use crate::session::{Session, peek_vault_id};
use crate::{prompt, quick};

const OTHER_DEVICES: &str = "Other devices will ask for a full unlock with the new details.";

fn forget_quick_unlock() {
    let result = peek_vault_id().and_then(|vault| quick::system()?.forget(&vault));
    if let Err(e) = result {
        eprintln!("warning: quick unlock key was not removed: {e}");
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
    let (session, password, secret_key) = Session::open_for_change()?;
    let mut session = session;
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
    forget_quick_unlock();
    done(json, "The master password is changed.");
    Ok(())
}

/// `figlock secret-key rotate`
pub fn secret_key_rotate(json: bool) -> Res<()> {
    let (mut session, password, secret_key) = Session::open_for_change()?;
    let r = session.vault.change_credentials(
        &password,
        &secret_key,
        CredentialChange {
            rotate_secret_key: true,
            ..CredentialChange::default()
        },
    )?;
    session.replace_vault_json(&r.vault_json)?;
    forget_quick_unlock();
    let new = r
        .secret_key
        .ok_or_else(|| Fail::new("no new Secret Key was made"))?;
    show_secret_key(&new, json)?;
    done(json, "The Secret Key is rotated.");
    Ok(())
}

fn label(d: &DeviceInfo) -> String {
    d.name.as_deref().map_or_else(
        || "(revoked)".to_owned(),
        |n| crate::render::clean(n).into_owned(),
    )
}

/// `figlock device list`
pub fn device_list(json: bool) -> Res<()> {
    let session = Session::open()?;
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
                    "this_device": d.device_id == this,
                })
            })
            .collect();
        println!("{}", json!(rows));
        return Ok(());
    }
    for d in &devices {
        let mark = if d.device_id == this {
            " (this device)"
        } else {
            ""
        };
        let state = if d.revoked_at.is_some() {
            "  revoked"
        } else {
            ""
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
    let (mut session, password, secret_key) = Session::open_for_change()?;
    let target = pick(
        &session.vault.devices(),
        session.vault.device_id(),
        reference,
    )?;
    let now = crate::session::now_ms();
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
    if changing {
        forget_quick_unlock();
    }
    if let Some(new) = &r.secret_key {
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
            "It may still know your master password and Secret Key. If it might have been compromised, \
             run `figlock device revoke` again with --change-password and --rotate-secret-key instead \
             of leaving the old ones in place."
        );
    }
    Ok(())
}
