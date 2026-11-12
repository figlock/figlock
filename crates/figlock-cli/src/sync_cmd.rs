//! `figlock sync`: keep this device's local vault matched to Google Drive (ADR-0026), and the quiet,
//! time-boxed sync that ordinary commands do around themselves.
//!
//! The vault is always read and written in the local folder. Commands pull before they read (when the last
//! sync is older than the polling interval) and push after they write, each with a short timeout, so a slow or
//! missing network never stops a command from working: it prints a warning and the next sync catches up.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::json;

use figlock_session::config::{self, Config, Remote};
use figlock_session::drive;
use figlock_session::error::{Fail, Res};
use figlock_session::session::now_ms;
use figlock_session::sync::{self, Report, SyncError, Warning};

/// Commands pull when the last sync is older than this. The desktop app and the broker poll every 15 seconds
/// (`sync.md` 10); a one-shot command that paid a network round trip each time would be slow for an agent
/// calling it in a loop, so it accepts a minute of staleness (`figlock sync` forces a fresh pull).
const PULL_AFTER_MS: u64 = 60_000;
/// How long a command waits for the pull before it carries on without it.
const PULL_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a command waits for its own write to reach Drive. Drive writes are several requests each, so this
/// is generous: giving up early would kill the upload when the process exits.
const PUSH_TIMEOUT: Duration = Duration::from_secs(40);

fn print_json(v: &serde_json::Value) -> Res<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).map_err(|_| Fail::new("cannot format output"))?
    );
    Ok(())
}

fn warning_text(w: Warning) -> &'static str {
    match w {
        Warning::VaultJsonFork => {
            "two devices changed the vault settings (password, devices or keys) at the same time; one copy was \
             kept. Check `figlock device list` and repeat any change that is missing"
        }
    }
}

fn warning_code(w: Warning) -> &'static str {
    match w {
        Warning::VaultJsonFork => "vault-json-fork",
    }
}

fn show(report: &Report, json: bool) -> Res<()> {
    for w in &report.warnings {
        eprintln!("warning: {}", warning_text(*w));
    }
    if json {
        let warnings: Vec<_> = report.warnings.iter().map(|w| warning_code(*w)).collect();
        print_json(&json!({
            "status": "synced",
            "pulled": report.pulled,
            "pushed": report.pushed,
            "warnings": warnings,
        }))
    } else {
        println!(
            "Synced with Google Drive: {} downloaded, {} uploaded.",
            report.pulled, report.pushed
        );
        Ok(())
    }
}

/// `figlock sync`: one pull and push, now.
pub fn run(reupload: bool, json: bool) -> Res<()> {
    let cfg = config::load()?;
    if cfg.place.remote.is_none() {
        return Err(Fail::new(
            "sync is not on for this device; run `figlock sync on`",
        ));
    }
    let report = sync::run(&cfg, reupload).map_err(|e| match e {
        SyncError::RemoteLost => Fail::new(
            "the vault is no longer in Google Drive (its folder was deleted, or FigLock was disconnected there); \
             run `figlock sync --reupload` to upload everything from this device again",
        ),
        e => e.into(),
    })?;
    show(&report, json)
}

/// `figlock sync on`: turn Drive sync on and upload what is already local. Only stays on if the first sync
/// works, so a different vault in Drive or a dead sign-in leaves the device exactly as it was.
pub fn on(json: bool) -> Res<()> {
    let cfg = config::load()?;
    if cfg.place.remote.is_some() {
        return say(json, "already-on", "Sync to Google Drive is already on.");
    }
    if !drive::status()?.signed_in {
        return Err(Fail::new(
            "not signed in to Google Drive; run `figlock drive login` first",
        ));
    }
    sync::forget_state();
    let cfg = config::set_remote(Some(Remote::Drive))?;
    match sync::run(&cfg, false) {
        Ok(report) => show(&report, json),
        Err(e) => {
            config::set_remote(None)?;
            sync::forget_state();
            Err(Fail::new(format!(
                "could not turn on sync, so nothing was changed: {e}"
            )))
        }
    }
}

/// `figlock sync off`: stop syncing. The local vault, the copy in Drive and the sign-in are left alone.
pub fn off(json: bool) -> Res<()> {
    let cfg = config::load()?;
    if cfg.place.remote.is_none() {
        return say(json, "already-off", "Sync to Google Drive is already off.");
    }
    config::set_remote(None)?;
    sync::forget_state();
    say(
        json,
        "off",
        "Sync is off. The vault on this device is untouched, and so is the copy in Google Drive; use \
         `figlock drive logout` to sign out of Google.",
    )
}

fn say(json: bool, status: &str, text: &str) -> Res<()> {
    if json {
        print_json(&json!({ "status": status }))
    } else {
        println!("{text}");
        Ok(())
    }
}

/// `figlock sync status`: what is configured, with no network call.
pub fn status(json: bool) -> Res<()> {
    let cfg = config::load()?;
    let on = cfg.place.remote.is_some();
    let signed_in = drive::status()?.signed_in;
    let last = sync::last_sync_ms().map(|t| now_ms().saturating_sub(t) / 1000);
    if json {
        return print_json(&json!({
            "local": cfg.place.local.to_string_lossy(),
            "remote": if on { json!("drive") } else { json!(null) },
            "signed_in": signed_in,
            "last_sync_seconds_ago": last,
        }));
    }
    println!("Local vault: {}", cfg.place.local.display());
    println!("Google Drive sync: {}", if on { "on" } else { "off" });
    if on {
        println!(
            "Signed in to Google: {}",
            if signed_in {
                "yes"
            } else {
                "no (run `figlock drive login`)"
            }
        );
        match last {
            Some(s) => println!("Last sync: {s} seconds ago"),
            None => println!("Last sync: never"),
        }
    }
    Ok(())
}

// ---- around ordinary commands ----

/// Sync with a time limit. A run that goes over is left to finish on its own thread. It holds the sync lock
/// until then, so a later sync on this device reports busy instead of mirroring alongside it.
fn timeboxed(cfg: Config, limit: Duration) -> Result<Report, SyncError> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(sync::run(&cfg, false));
    });
    rx.recv_timeout(limit).unwrap_or(Err(SyncError::Offline))
}

/// A command that pulls and pushes would otherwise print the same warning twice.
static WARNED: AtomicBool = AtomicBool::new(false);

fn warn_once(text: &str) {
    if !WARNED.swap(true, Ordering::SeqCst) {
        eprintln!("warning: {text}");
    }
}

fn quiet(warn_when_offline: bool, limit: Duration) {
    let Ok(cfg) = config::load() else {
        return;
    };
    if cfg.place.remote.is_none() {
        return;
    }
    match timeboxed(cfg, limit) {
        Ok(report) => {
            for w in &report.warnings {
                warn_once(warning_text(*w));
            }
        }
        Err(SyncError::Busy) => {}
        Err(SyncError::Offline) if !warn_when_offline => {}
        Err(e) => warn_once(&format!(
            "not synced with Google Drive ({e}); this device keeps working. Run `figlock sync` when you can"
        )),
    }
}

/// Before a command reads the vault: bring in what other devices wrote, unless that was done a moment ago.
/// Stays silent when the network is simply not there.
pub fn pull_first() {
    let fresh = sync::last_sync_ms().is_some_and(|t| now_ms().saturating_sub(t) < PULL_AFTER_MS);
    if !fresh {
        quiet(false, PULL_TIMEOUT);
    }
}

/// After a command wrote to the vault: send it up, and say so if that did not work.
pub fn push_after_write() {
    quiet(true, PUSH_TIMEOUT);
}
