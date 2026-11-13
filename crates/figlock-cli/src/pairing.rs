//! `figlock join`, `figlock open` and `figlock device add`: the CLI's side of adding a device
//! (`docs/spec/pairing-recovery.md` 3, ADR-0027).
//!
//! A trusted device runs `device add`, which shows an invite code and waits for a request. The new device runs
//! `join <code>`, which shows a fingerprint and waits for the answer. The person checks that the two
//! fingerprints match, approves, and the new device unlocks with the master password: the Secret Key arrives
//! in the answer. `open` is the fallback when no trusted device is at hand: master password and Secret Key
//! typed from the Emergency Kit.

use std::thread::sleep;
use std::time::{Duration, Instant};

use figlock_core::{DeviceKeys, Invite, Key, SecretKey, Vault};
use figlock_session::config::{self, Config, Place, Remote};
use figlock_session::error::{Fail, Res};
use figlock_session::session::{Session, now_ms, peek_vault_id};
use figlock_session::{join, secret_key_store, store, sync};
use figlock_store::StoreError;
use serde_json::json;

use crate::commands::offer_quick_unlock;
use crate::{DeviceAddArgs, JoinArgs, OpenArgs, WhereArgs, prompt, quick};

/// How often to look for the other device's file.
const POLL: Duration = Duration::from_secs(2);

/// How many times `join` asks for the master password at a terminal before giving up.
const MAX_PASSWORD_TRIES: u32 = 3;

fn already_set_up() -> Fail {
    Fail::new("this device already has a vault set up")
}

fn place_for(a: &WhereArgs) -> Res<Place> {
    let local = match &a.store {
        Some(path) => path.clone(),
        None => config::default_local_dir()?,
    };
    let remote = a.drive.then_some(Remote::Drive);
    // A Drive copy is downloaded into this folder, so it may not exist yet.
    if remote.is_some() {
        std::fs::create_dir_all(&local)?;
    }
    let local = std::fs::canonicalize(&local).map_err(|_| {
        Fail::new(format!(
            "there is no vault folder at {}; give the folder with --store, or use --drive",
            local.display()
        ))
    })?;
    Ok(Place { local, remote })
}

fn choice(a: &WhereArgs) -> quick::InitChoice {
    match (a.quick_unlock, a.no_quick_unlock) {
        (true, _) => quick::InitChoice::Yes,
        (_, true) => quick::InitChoice::No,
        _ => quick::InitChoice::Ask,
    }
}

/// The last step of `join` and `open`: save this device's config, unlock once with what was typed (which also
/// keeps the Secret Key on this device), and offer quick unlock. Nothing stays behind if the unlock fails.
fn settle(
    cfg: &Config,
    device: &DeviceKeys,
    password: &str,
    secret_key: SecretKey,
    at: &WhereArgs,
    json: bool,
) -> Res<()> {
    config::save(cfg, device)?;
    let (session, auk) = Session::open_full(prompt::known(password, secret_key))
        .inspect_err(|_| config::forget())?;
    let vault_id = hex::encode(session.vault.vault_id());
    let device_id = hex::encode(session.vault.device_id());
    drop(session);
    if json {
        println!(
            "{}",
            json!({ "vault_id": vault_id, "device_id": device_id })
        );
    } else {
        println!("This device is set up. Try `figlock find`.");
    }
    if cfg.place.remote.is_some()
        && let Err(e) = sync::run(cfg, false)
    {
        eprintln!(
            "warning: the first sync with Google Drive failed: {e}. Run `figlock sync` to retry."
        );
    }
    offer_quick_unlock(choice(at), json, &vault_id, || -> Res<Key> { Ok(auk) });
    Ok(())
}

/// `figlock join <code>`
pub fn join(a: JoinArgs, json: bool) -> Res<()> {
    if config::is_initialised()? {
        return Err(already_set_up());
    }
    let place = place_for(&a.at)?;
    let attempt = join::begin(place, &a.code, &a.at.device_name, now_ms())?;
    eprintln!(
        "Asked to join. Check that your other device shows this fingerprint before you approve:\n\n  {}\n",
        attempt.fingerprint()
    );
    eprintln!("Waiting for approval (run `figlock device add` there). Press Ctrl-C to stop.");
    let deadline = Instant::now() + Duration::from_secs(a.timeout);
    while !attempt.answered()? {
        if Instant::now() >= deadline {
            return Err(Fail::new(
                "no answer in time; ask for a new invite code and run `figlock join` again",
            ));
        }
        sleep(POLL);
    }
    let mut tries = 1;
    let (password, joined, cfg) = loop {
        let password = prompt::password()?;
        match attempt.finish(&password, now_ms()) {
            Ok((joined, cfg)) => break (password, joined, cfg),
            Err(e) if tries < MAX_PASSWORD_TRIES && prompt::can_retry_password() => {
                eprintln!("{e}. Try again.");
                tries += 1;
            }
            Err(e) => return Err(e),
        }
    };
    settle(
        &cfg,
        joined.vault.device_keys(),
        &password,
        joined.secret_key,
        &a.at,
        json,
    )
}

/// `figlock open`
pub fn open(a: OpenArgs, json: bool) -> Res<()> {
    if config::is_initialised()? {
        return Err(already_set_up());
    }
    let place = place_for(&a.at)?;
    // Downloads the vault into the local folder when it lives in Drive.
    sync::open_from_remote(&place)?;
    let vault_json = store::open(&place)?
        .get("vault.json")
        .map_err(|e| match e {
            StoreError::NotFound => Fail::new("there is no FigLock vault in that storage"),
            e => e.into(),
        })?;
    let password = prompt::password()?;
    let secret_key = prompt::secret_key()?;
    // Deriving the key never fails, so the real check is the unlock inside `settle`.
    Vault::unlock_key(&vault_json, &password, &secret_key).map_err(|_| wrong_credentials())?;
    let cfg = Config {
        place,
        device_name: a.at.device_name.clone(),
    };
    settle(
        &cfg,
        &DeviceKeys::generate()?,
        &password,
        secret_key,
        &a.at,
        json,
    )
}

fn wrong_credentials() -> Fail {
    Fail::new("could not unlock: wrong master password or Secret Key, or the vault file is damaged")
}

/// The Secret Key to hand to the new device: the one this device keeps (after Touch ID or the device
/// password), or typed with the master password, which must open the vault since the key is about to leave.
fn key_to_share() -> Res<(Session, SecretKey)> {
    let vault = peek_vault_id()?;
    let keys = secret_key_store::system()?;
    if keys.has(&vault) {
        let session = Session::open(prompt::terminal())?;
        let key = keys.get(&vault, "add a device")?.ok_or_else(|| {
            Fail::new(
                "the Secret Key kept on this device was not released, so nothing was approved",
            )
        })?;
        return Ok((session, key));
    }
    let (session, _, key) = Session::open_for_change(prompt::terminal())?;
    Ok((session, key))
}

/// Take the invite's files out of the remote too: files in `inbox/` are mirrored create-only.
fn forget_remote(place: &Place, invite: &Invite) {
    sync::remove_remote(place, &[invite.inv_path(), invite.req_path()]);
}

/// `figlock device add`
pub fn device_add(a: &DeviceAddArgs, json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let cfg = config::load()?;
    let (mut session, key) = key_to_share()?;
    let (mut invite, offer) = Invite::create(session.vault.vault_id(), now_ms())?;
    session.publish_invite(&offer)?;
    // With Drive the new device finds the invite there, so it has to arrive before the code is shown.
    if cfg.place.remote.is_some()
        && let Err(e) = sync::run(&cfg, false)
    {
        session.close_invite(&invite);
        return Err(Fail::new(format!(
            "could not reach Google Drive, so the new device could not find the invite: {e}"
        )));
    }
    let code = offer.code.to_string();
    if json {
        println!(
            "{}",
            json!({ "invite_code": code, "expires_at_ms": invite.expires_at() })
        );
    } else {
        println!("{code}");
    }
    eprintln!("On the new device, run: figlock join <the code above>");
    eprintln!("The code works once and expires in 10 minutes. Waiting for the new device...");

    let request = loop {
        let now = now_ms();
        if invite.is_expired(now) {
            session.close_invite(&invite);
            forget_remote(&cfg.place, &invite);
            return Err(Fail::new(
                "the invite expired; run `figlock device add` again",
            ));
        }
        // A pull that fails now is tried again on the next turn.
        if cfg.place.remote.is_some() {
            let _ = sync::run(&cfg, false);
        }
        if let Some(req) = session.take_request(&mut invite, now)? {
            break req;
        }
        sleep(POLL);
    };
    session.retire_invite(&invite);
    sync::remove_remote(&cfg.place, &[invite.inv_path()]);

    eprintln!(
        "\"{}\" wants to join this vault. Check that it shows this fingerprint:\n\n  {}\n",
        request.device_name,
        request.fingerprint()
    );
    if !a.yes && !prompt::confirm_default_no("Approve this device?")? {
        session.close_invite(&invite);
        forget_remote(&cfg.place, &invite);
        return Err(Fail::new(
            "not approved; nothing was shared with that device",
        ));
    }
    if let Err(e) = session.approve_join(&invite, &request, &key, now_ms()) {
        return Err(if session.is_uncertain() {
            Fail::new(
                "the new device could not be saved; run a command again to unlock, then repeat `figlock device add`",
            )
        } else if invite.is_expired(now_ms()) {
            session.close_invite(&invite);
            forget_remote(&cfg.place, &invite);
            Fail::new("the invite expired; run `figlock device add` again")
        } else {
            e
        });
    }
    forget_remote(&cfg.place, &invite);
    // Share the new device's record and the answer; a failure is reported and `figlock sync` retries it.
    crate::sync_cmd::push_after_write();
    if json {
        eprintln!("Added \"{}\".", request.device_name);
    } else {
        println!(
            "Added \"{}\". It finishes setup with the master password.",
            request.device_name
        );
    }
    Ok(())
}
