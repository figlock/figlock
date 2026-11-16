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
        if report.pulled == 0 && report.pushed == 0 {
            println!("Already up to date.");
        } else {
            println!(
                "Synced with Google Drive: {} downloaded, {} uploaded.",
                report.pulled, report.pushed
            );
        }
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

/// `figlock sync delete-remote`: permanently delete this vault's copy from Google Drive (FIG-122, ADR-0035).
/// Says exactly what goes and what stays, then asks for the phrase to be typed (or given with `--confirm`).
/// Nothing is deleted without it, when Google is not signed in, or when Drive holds a different vault.
pub fn delete_remote(confirm: Option<String>, json: bool) -> Res<()> {
    let cfg = config::load()?;
    if !drive::status()?.signed_in {
        return Err(Fail::new(
            "not signed in to Google Drive; run `figlock drive login` first",
        ));
    }
    let phrase = sync::DELETE_REMOTE_PHRASE;
    let typed = match confirm {
        Some(t) => t,
        None => {
            eprintln!(
                "This permanently deletes every FigLock file of this vault from your Google Drive: the vault \
                 settings, device records, change history and snapshots. It cannot be undone.\n\
                 The vault on this device stays as it is, and sync is turned off here. Other devices that sync \
                 to Drive will find the vault missing and ask before uploading it again.\n"
            );
            crate::prompt::typed_line(&format!("Type \"{phrase}\" to continue: "))?.ok_or_else(|| {
                Fail::new(format!(
                    "this is not a terminal, so nothing was deleted; pass --confirm \"{phrase}\" to confirm"
                ))
            })?
        }
    };
    let done = sync::delete_remote(&cfg, &typed).map_err(|e| match e {
        SyncError::NotConfirmed => Fail::new(format!(
            "that is not the confirmation phrase, so nothing was deleted; it is \"{phrase}\""
        )),
        SyncError::ForeignVault => Fail::new(
            "Google Drive holds a different vault than this device's, so nothing was deleted",
        ),
        e => e.into(),
    })?;
    if json {
        print_json(&json!({ "status": "deleted", "files": done.files }))
    } else {
        println!(
            "Deleted {} files from Google Drive. The vault on this device is untouched and sync is off. \
             Turn it on again with `figlock sync on` to upload the vault anew.",
            done.files
        );
        Ok(())
    }
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
    let stored = drive::status()?.signed_in;
    // Google refused the last sync: the sign-in is still on disk but no longer works.
    let refused = stored && sync::sign_in_refused();
    let signed_in = stored && !refused;
    let last = sync::last_sync_ms().map(|t| now_ms().saturating_sub(t) / 1000);
    // Another device made a new Secret Key (ADR-0034): the one kept here opens nothing until the new one is typed.
    let key_changed = figlock_session::secret_key_store::system().is_ok_and(|keys| {
        figlock_session::session::peek_vault_id().is_ok_and(|vault| keys.has(&vault))
    }) && figlock_session::session::peek_kept_key()
        .is_ok_and(|k| k == figlock_core::KeptKey::Stale);
    if json {
        return print_json(&json!({
            "secret_key_changed": key_changed,
            "local": cfg.place.local.to_string_lossy(),
            "remote": if on { json!("drive") } else { json!(null) },
            "signed_in": signed_in,
            "sign_in_refused": refused,
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
            } else if refused {
                "no (Google has stopped FigLock's access; run `figlock drive login`)"
            } else {
                "no (run `figlock drive login`)"
            }
        );
        match last {
            Some(s) => println!("Last sync: {s} seconds ago"),
            None => println!("Last sync: never"),
        }
    }
    if key_changed {
        println!(
            "Your Secret Key was changed on another device. The next command that unlocks the vault asks for the new one; type it from that device's Emergency Kit or `figlock secret-key show`."
        );
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

/// What the automatic sync after a write did, so the command can say so.
pub enum Pushed {
    /// Sync is off on this device: nothing to say.
    Off,
    Synced {
        uploaded: usize,
        downloaded: usize,
    },
    /// The remote could not be reached; the next command retries.
    Offline,
    /// Another sync on this device holds the lock.
    Busy,
    /// Anything else that went wrong, in words.
    Failed(String),
}

impl Pushed {
    /// The one short line for a text-mode command, or `None` when sync is off.
    pub fn line(&self) -> Option<String> {
        Some(match self {
            Pushed::Off => return None,
            Pushed::Synced {
                uploaded,
                downloaded,
            } => match (uploaded, downloaded) {
                (0, 0) => "Saved and synced.".to_owned(),
                (u, 0) => format!("Saved and synced ({u} uploaded)."),
                (0, d) => format!("Saved and synced ({d} downloaded)."),
                (u, d) => format!("Saved and synced ({u} uploaded, {d} downloaded)."),
            },
            Pushed::Offline => {
                "Saved. Not synced: offline, will retry on the next command.".to_owned()
            }
            Pushed::Busy => {
                "Saved. Not synced yet: another sync is running here; run `figlock sync` if it does not catch up."
                    .to_owned()
            }
            // The pull before the command may already have said why, in a warning just above.
            Pushed::Failed(_) if WARNED.load(Ordering::SeqCst) => {
                "Saved. Not synced (see the warning above); run `figlock sync` when you can."
                    .to_owned()
            }
            Pushed::Failed(why) => {
                format!("Saved. Not synced ({why}); run `figlock sync` when you can.")
            }
        })
    }

    /// The `sync` member of a `--json` result, or `None` when sync is off.
    pub fn json(&self) -> Option<serde_json::Value> {
        let (state, uploaded, downloaded) = match self {
            Pushed::Off => return None,
            Pushed::Synced {
                uploaded,
                downloaded,
            } => ("synced", *uploaded, *downloaded),
            Pushed::Offline => ("offline", 0, 0),
            Pushed::Busy => ("busy", 0, 0),
            Pushed::Failed(_) => ("failed", 0, 0),
        };
        Some(json!({ "state": state, "uploaded": uploaded, "downloaded": downloaded }))
    }

    /// Say it on stderr, so a script capturing the id on stdout is not disturbed.
    pub fn announce(&self) {
        if let Some(line) = self.line() {
            eprintln!("{line}");
        }
    }

    /// `result` with the `sync` member added when sync is on.
    pub fn attach(&self, mut result: serde_json::Value) -> serde_json::Value {
        if let (Some(sync), Some(map)) = (self.json(), result.as_object_mut()) {
            map.insert("sync".to_owned(), sync);
        }
        result
    }
}

fn quiet(limit: Duration) -> Pushed {
    let Ok(cfg) = config::load() else {
        return Pushed::Off;
    };
    if cfg.place.remote.is_none() {
        return Pushed::Off;
    }
    match timeboxed(cfg, limit) {
        Ok(report) => {
            for w in &report.warnings {
                warn_once(warning_text(*w));
            }
            Pushed::Synced {
                uploaded: report.pushed,
                downloaded: report.pulled,
            }
        }
        Err(SyncError::Busy) => Pushed::Busy,
        Err(SyncError::Offline) => Pushed::Offline,
        Err(e) => Pushed::Failed(e.to_string()),
    }
}

/// Before a command reads the vault: bring in what other devices wrote, unless that was done a moment ago.
/// Stays silent when the network is simply not there, and when anything else goes wrong: the command that
/// follows reads the local vault either way.
pub fn pull_first() {
    let fresh = sync::last_sync_ms().is_some_and(|t| now_ms().saturating_sub(t) < PULL_AFTER_MS);
    if !fresh && let Pushed::Failed(why) = quiet(PULL_TIMEOUT) {
        warn_once(&format!(
            "not synced with Google Drive ({why}); this device keeps working. Run `figlock sync` when you can"
        ));
    }
}

/// After a command wrote to the vault: send it up. The caller says what happened with [`Pushed::announce`]
/// or puts [`Pushed::json`] in its result.
pub fn push_after_write() -> Pushed {
    quiet(PUSH_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(p: &Pushed) -> String {
        p.line().unwrap()
    }

    #[test]
    fn the_line_after_a_write_says_what_happened() {
        let synced = Pushed::Synced {
            uploaded: 1,
            downloaded: 0,
        };
        assert_eq!(line(&synced), "Saved and synced (1 uploaded).");
        let both = Pushed::Synced {
            uploaded: 2,
            downloaded: 3,
        };
        assert_eq!(line(&both), "Saved and synced (2 uploaded, 3 downloaded).");
        let none = Pushed::Synced {
            uploaded: 0,
            downloaded: 0,
        };
        assert_eq!(line(&none), "Saved and synced.");
        assert_eq!(
            line(&Pushed::Offline),
            "Saved. Not synced: offline, will retry on the next command."
        );
        assert!(line(&Pushed::Busy).starts_with("Saved. Not synced yet: another sync"));
        assert_eq!(
            line(&Pushed::Failed("no sign-in".to_owned())),
            "Saved. Not synced (no sign-in); run `figlock sync` when you can."
        );
    }

    #[test]
    fn sync_off_says_nothing_and_adds_nothing_to_json() {
        assert!(Pushed::Off.line().is_none());
        assert!(Pushed::Off.json().is_none());
        assert_eq!(Pushed::Off.attach(json!({"id": "ab"})), json!({"id": "ab"}));
    }

    #[test]
    fn json_carries_state_and_counts() {
        let synced = Pushed::Synced {
            uploaded: 1,
            downloaded: 2,
        };
        assert_eq!(
            synced.attach(json!({"id": "ab"})),
            json!({"id": "ab", "sync": {"state": "synced", "uploaded": 1, "downloaded": 2}})
        );
        assert_eq!(
            Pushed::Offline.json().unwrap(),
            json!({"state": "offline", "uploaded": 0, "downloaded": 0})
        );
    }
}
