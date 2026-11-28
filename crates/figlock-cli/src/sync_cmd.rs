//! `figlock sync`: keep this device's local vault matched to its remotes - Google Drive, iCloud Drive or another
//! synced folder, any number at once (ADR-0026, ADR-0043) - and the quiet,
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

use crate::RemoteArgs;
use crate::remotes::label;

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

/// One remote's line in the text output.
fn outcome_line(label: &str, report: &Report) -> String {
    let base = if report.pulled == 0 && report.pushed == 0 && report.waiting == 0 {
        format!("{label}: already up to date.")
    } else {
        format!(
            "{label}: {} downloaded, {} uploaded.",
            report.pulled, report.pushed
        )
    };
    format!("{base}{}", waiting_note(report))
}

/// The line that says files are still on their way, so "up to date" is never claimed while some are missing.
fn waiting_note(report: &Report) -> String {
    match report.waiting {
        0 => String::new(),
        n => format!(
            " Waiting for {n} file(s) to download from the cloud; run sync again in a minute."
        ),
    }
}

/// Why one remote did not sync, in words that name it.
fn failure_text(remote: &Remote, e: &SyncError) -> String {
    match e {
        SyncError::RemoteLost => format!(
            "the vault is no longer in {} (its folder was deleted, or FigLock was disconnected there); run \
             `figlock sync --reupload` to upload everything from this device again",
            label(remote)
        ),
        e => e.to_string(),
    }
}

/// `figlock sync`: one pull and push to every remote, now. Each remote has its own turn, so one that is down
/// never stops the others.
pub fn run(reupload: bool, json: bool) -> Res<()> {
    let cfg = config::load()?;
    if !cfg.place.syncs() {
        return Err(Fail::new(
            "sync is not on for this device; run `figlock sync on`",
        ));
    }
    let outcomes = sync::run_all(&cfg, reupload);
    let mut total = Report::default();
    for r in outcomes.iter().filter_map(|o| o.result.as_ref().ok()) {
        total.pulled += r.pulled;
        total.pushed += r.pushed;
        total.waiting += r.waiting;
        for w in &r.warnings {
            if !total.warnings.contains(w) {
                total.warnings.push(*w);
            }
        }
    }
    let failed = outcomes.iter().filter(|o| o.result.is_err()).count();
    if json {
        let warnings: Vec<_> = total.warnings.iter().map(|w| warning_code(*w)).collect();
        for w in &total.warnings {
            eprintln!("warning: {}", warning_text(*w));
        }
        let remotes: Vec<_> = outcomes
            .iter()
            .map(|o| match &o.result {
                Ok(r) => json!({"remote": o.remote.id(), "status": if r.waiting == 0 { "synced" } else { "waiting" }, "pulled": r.pulled, "pushed": r.pushed, "waiting": r.waiting}),
                Err(e) => json!({"remote": o.remote.id(), "status": "failed", "error": failure_text(&o.remote, e)}),
            })
            .collect();
        print_json(&json!({
            "status": if failed == 0 { "synced" } else if failed == outcomes.len() { "failed" } else { "partial" },
            "pulled": total.pulled,
            "pushed": total.pushed,
            "waiting": total.waiting,
            "warnings": warnings,
            "remotes": remotes,
        }))?;
    } else {
        for w in &total.warnings {
            eprintln!("warning: {}", warning_text(*w));
        }
        let several = outcomes.len() > 1;
        for o in &outcomes {
            match &o.result {
                Ok(r) if several => println!("{}", outcome_line(&label(&o.remote), r)),
                Ok(r) if r.pulled == 0 && r.pushed == 0 && r.waiting == 0 => {
                    println!("Already up to date.")
                }
                Ok(r) => println!(
                    "Synced with {}: {} downloaded, {} uploaded.{}",
                    label(&o.remote),
                    r.pulled,
                    r.pushed,
                    waiting_note(r)
                ),
                Err(e) => eprintln!("{}: {}", label(&o.remote), failure_text(&o.remote, e)),
            }
        }
    }
    match failed {
        0 => Ok(()),
        n if n == outcomes.len() && n == 1 => {
            let o = &outcomes[0];
            let Err(e) = &o.result else { unreachable!() };
            Err(Fail::new(failure_text(&o.remote, e)))
        }
        n => Err(Fail::new(format!(
            "{n} of {} places did not sync (see above); the others did",
            outcomes.len()
        ))),
    }
}

/// What `sync on` and `sync off` act on: what was asked for, or (for `on` only) Google Drive as always.
fn turn_on_targets(r: &RemoteArgs) -> Res<Vec<Remote>> {
    if r.is_empty() {
        Ok(vec![Remote::Drive])
    } else {
        // Not next to another device's vault that is still arriving through iCloud: uploading this device's
        // `vault.json` into it would fork the vault. Checked before anything is made.
        crate::remotes::refuse_partial_icloud_folder(&r.resolve(false)?)?;
        r.resolve(true)
    }
}

/// `figlock sync on`: turn sync to each place asked for on, and upload what is already local. A place stays on
/// only if its first sync works, so a different vault there or a dead sign-in leaves the device exactly as it
/// was. Another place's trouble is never this one's fault, so each is decided alone.
pub fn on(r: &RemoteArgs, json: bool) -> Res<()> {
    let targets = turn_on_targets(r)?;
    let mut results = Vec::new();
    let mut failed = false;
    for remote in targets {
        let cfg = config::load()?;
        let name = label(&remote);
        if cfg.place.remotes.contains(&remote) {
            results.push(json!({"remote": remote.id(), "status": "already-on"}));
            if !json {
                println!("Sync to {name} is already on.");
            }
            continue;
        }
        if remote == Remote::Drive && !drive::status()?.signed_in {
            failed = true;
            let why = "not signed in to Google Drive; run `figlock drive login` first";
            results.push(json!({"remote": remote.id(), "status": "failed", "error": why}));
            eprintln!("{name}: {why}");
            continue;
        }
        sync::forget_state_of(&remote);
        let cfg = match config::add_remote(remote.clone()) {
            Ok(c) => c,
            Err(e) => {
                failed = true;
                results.push(json!({"remote": remote.id(), "status": "failed", "error": e.0}));
                eprintln!("{name}: {e}");
                continue;
            }
        };
        match sync::run_remote(&cfg, &remote, false) {
            Ok(report) => {
                results.push(json!({"remote": remote.id(), "status": "on", "pulled": report.pulled, "pushed": report.pushed}));
                if !json {
                    println!(
                        "Sync to {name} is on. {}",
                        outcome_line("First sync", &report)
                    );
                    if matches!(remote, Remote::Folder(ref p) if figlock_session::icloud::is_icloud_path(p))
                    {
                        println!(
                            "iCloud Drive is Apple-only. With Optimize Mac Storage on, iCloud may keep files only in the cloud; FigLock downloads them when needed."
                        );
                    }
                }
            }
            Err(e) => {
                failed = true;
                config::remove_remote(&remote)?;
                sync::forget_state_of(&remote);
                let why = format!(
                    "could not turn on sync, so nothing was changed: {}",
                    failure_text(&remote, &e)
                );
                results.push(json!({"remote": remote.id(), "status": "failed", "error": why}));
                eprintln!("{name}: {why}");
            }
        }
    }
    if json {
        print_json(&json!({ "status": if failed { "failed" } else { "on" }, "remotes": results }))?;
    }
    if failed {
        return Err(Fail::new(
            "sync was not turned on everywhere asked for (see above)",
        ));
    }
    Ok(())
}

/// The configured remotes `r` names; all of them when it names none. `--icloud` means every folder that is in
/// iCloud Drive, so it still works when iCloud Drive is off and its folder cannot be found.
fn turn_off_targets(cfg: &Config, r: &RemoteArgs) -> Res<Vec<Remote>> {
    if r.is_empty() {
        return Ok(cfg.place.remotes.clone());
    }
    let mut out = Vec::new();
    for remote in &cfg.place.remotes {
        let named = match remote {
            Remote::Drive => r.drive,
            Remote::Folder(p) => {
                (r.icloud && figlock_session::icloud::is_icloud_path(p))
                    || r.sync_folder.iter().any(|f| {
                        std::path::absolute(f).is_ok_and(|f| &f == p)
                            || std::fs::canonicalize(f)
                                .is_ok_and(|f| std::fs::canonicalize(p).is_ok_and(|p| f == p))
                    })
            }
        };
        if named {
            out.push(remote.clone());
        }
    }
    Ok(out)
}

/// `figlock sync off`: stop syncing to the places given, or to all. The local vault, the copies already stored
/// there and the Google sign-in are left alone.
pub fn off(r: &RemoteArgs, json: bool) -> Res<()> {
    let cfg = config::load()?;
    let targets = turn_off_targets(&cfg, r)?;
    if targets.is_empty() {
        return say(
            json,
            "already-off",
            if r.is_empty() {
                "Sync is already off."
            } else {
                "Sync to that place is already off."
            },
        );
    }
    let mut names = Vec::new();
    for remote in &targets {
        config::remove_remote(remote)?;
        sync::forget_state_of(remote);
        names.push(label(remote));
    }
    if json {
        return print_json(&json!({
            "status": "off",
            "remotes": targets.iter().map(Remote::id).collect::<Vec<_>>(),
        }));
    }
    println!(
        "Sync to {} is off. The vault on this device is untouched, and so are the copies already stored there.",
        names.join(" and ")
    );
    if targets.contains(&Remote::Drive) {
        println!("Use `figlock drive logout` to sign out of Google.");
    }
    Ok(())
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

/// `figlock sync status`: every remote, with no network call.
pub fn status(json: bool) -> Res<()> {
    let cfg = config::load()?;
    let stored = drive::status()?.signed_in;
    // Google refused the last sync: the sign-in is still on disk but no longer works.
    let refused = stored && sync::sign_in_refused();
    let signed_in = stored && !refused;
    let last = sync::last_sync_ms().map(|t| now_ms().saturating_sub(t) / 1000);
    let icloud = figlock_session::icloud::status();
    let details: Vec<RemoteStatus> = cfg
        .place
        .remotes
        .iter()
        .map(|r| RemoteStatus::of(r, signed_in, refused, &icloud))
        .collect();
    // Another device made a new Secret Key (ADR-0034): the one kept here opens nothing until the new one is typed.
    let key_changed = figlock_session::secret_key_store::system().is_ok_and(|keys| {
        figlock_session::session::peek_vault_id().is_ok_and(|vault| keys.has(&vault))
    }) && figlock_session::session::peek_kept_key()
        .is_ok_and(|k| k == figlock_core::KeptKey::Stale);
    if json {
        return print_json(&json!({
            "secret_key_changed": key_changed,
            "local": cfg.place.local.to_string_lossy(),
            "remote": if cfg.place.has_drive() { json!("drive") } else { json!(null) },
            "remotes": cfg.place.remotes.iter().map(Remote::id).collect::<Vec<_>>(),
            "details": details.iter().map(RemoteStatus::json).collect::<Vec<_>>(),
            "signed_in": signed_in,
            "email": if signed_in { json!(drive::account_email()) } else { json!(null) },
            "sign_in_refused": refused,
            "last_sync_seconds_ago": last,
        }));
    }
    println!("Local vault: {}", cfg.place.local.display());
    if details.is_empty() {
        println!(
            "Sync: off (this vault is only on this device). Turn it on with `figlock sync on --icloud` or `--drive`."
        );
    }
    for d in &details {
        println!("{}: on", d.name);
        if let Some(path) = &d.path {
            println!("  Folder: {path}");
        }
        println!("  {}", d.state);
        match d.last_sync_secs {
            Some(s) => println!("  Last sync: {s} seconds ago"),
            None => println!("  Last sync: never"),
        }
        if let Some(e) = &d.error {
            println!("  Last problem: {e}");
        }
    }
    if key_changed {
        println!(
            "Your Secret Key was changed on another device. The next command that unlocks the vault asks for the new one; type it from that device's Emergency Kit or `figlock secret-key show`."
        );
    }
    Ok(())
}

/// What `sync status` says about one remote.
struct RemoteStatus {
    id: String,
    kind: &'static str,
    name: String,
    path: Option<String>,
    /// Whether it can be used right now, from what is on this device (no network call).
    ready: bool,
    /// The same, in words.
    state: String,
    last_sync_secs: Option<u64>,
    error: Option<String>,
}

impl RemoteStatus {
    fn of(
        r: &Remote,
        signed_in: bool,
        refused: bool,
        icloud: &figlock_session::icloud::Status,
    ) -> Self {
        let (kind, path, ready, state) = match r {
            Remote::Drive => (
                "drive",
                None,
                signed_in,
                if signed_in {
                    match drive::account_email() {
                        Some(email) => format!("Signed in to Google: yes ({email})"),
                        None => "Signed in to Google: yes".to_owned(),
                    }
                } else if refused {
                    "Signed in to Google: no (Google has stopped FigLock's access; run `figlock drive login`)".to_owned()
                } else {
                    "Signed in to Google: no (run `figlock drive login`)".to_owned()
                },
            ),
            Remote::Folder(p) if figlock_session::icloud::is_icloud_path(p) => {
                let here = p.is_dir();
                let ready = icloud.ready && here;
                (
                    "icloud",
                    Some(p.display().to_string()),
                    ready,
                    if !icloud.apple {
                        "iCloud Drive: not available on this computer".to_owned()
                    } else if !icloud.ready {
                        "iCloud Drive: off or signed out (System Settings > your name > iCloud > Drive)".to_owned()
                    } else if !here {
                        "iCloud Drive: the FigLock folder is not there yet (still syncing?)"
                            .to_owned()
                    } else {
                        "iCloud Drive: on".to_owned()
                    },
                )
            }
            Remote::Folder(p) => (
                "folder",
                Some(p.display().to_string()),
                p.is_dir(),
                if p.is_dir() {
                    "Folder: available".to_owned()
                } else {
                    "Folder: not available (is the drive connected?)".to_owned()
                },
            ),
        };
        RemoteStatus {
            id: r.id(),
            kind,
            name: label(r),
            path,
            ready,
            state,
            last_sync_secs: sync::last_sync_ms_of(r).map(|t| now_ms().saturating_sub(t) / 1000),
            error: sync::last_error_of(r),
        }
    }

    fn json(&self) -> serde_json::Value {
        json!({
            "remote": self.id,
            "kind": self.kind,
            "name": self.name,
            "path": self.path,
            "ready": self.ready,
            "state": self.state,
            "last_sync_seconds_ago": self.last_sync_secs,
            "last_error": self.error,
        })
    }
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

/// Set when the pull before the command failed and said why, so the line after a failed push need not.
static PULL_FAILED: AtomicBool = AtomicBool::new(false);

fn warn_once(text: &str) -> bool {
    let first = !WARNED.swap(true, Ordering::SeqCst);
    if first {
        eprintln!("warning: {text}");
    }
    first
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
            Pushed::Failed(_) if PULL_FAILED.load(Ordering::SeqCst) => {
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
    if !cfg.place.syncs() {
        return Pushed::Off;
    }
    match timeboxed(cfg, limit) {
        Ok(report) => {
            for w in &report.warnings {
                let _ = warn_once(warning_text(*w));
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
    if !fresh
        && let Pushed::Failed(why) = quiet(PULL_TIMEOUT)
        && warn_once(&format!(
            "not synced ({why}); this device keeps working. Run `figlock sync` when you can"
        ))
    {
        PULL_FAILED.store(true, Ordering::SeqCst);
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
