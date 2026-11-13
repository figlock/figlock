//! Updating the app in place (FIG-112): look for a newer version, and install it only when the person says so.
//!
//! The updater downloads a signed archive of the whole app, so the command line tool inside it is replaced with
//! the app and their versions never diverge. It checks the archive against the update key in the app's config
//! (`plugins.updater.pubkey`) before it touches anything, refuses an older or equal version, and leaves the
//! installed app as it is when a download or a check fails. The window gets no updater permissions of its own:
//! it calls the three commands in `lib.rs`, which call this module.
//!
//! Only builds whose config has an `updater` entry can update (release builds); a development build says so.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

use super::{CmdError, CmdRes};

/// The newer version a check found, kept until the person installs it.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    /// `unavailable` (this build cannot update), `up_to_date`, `available` or `ready` (installed, restart to use).
    pub state: &'static str,
    pub current: String,
    pub version: Option<String>,
    pub notes: Option<String>,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Whether this build ships an update key and feed. Without the config entry the plugin is not registered.
pub fn configured(app: &AppHandle) -> bool {
    app.config().plugins.0.contains_key("updater")
}

pub fn view(
    state: &'static str,
    current: &str,
    version: Option<&str>,
    notes: Option<&str>,
) -> UpdateView {
    UpdateView {
        state,
        current: current.to_owned(),
        version: version.map(str::to_owned),
        notes: notes
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned),
    }
}

fn current(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The words for a failed check or install. A bad signature is said plainly, because it means the download was
/// not what we published; anything else is a connection or disk problem worth trying again.
pub fn failure(e: &UpdaterError, installing: bool) -> CmdError {
    // The window shows plain words; the real cause goes to the log for whoever has to find out why.
    eprintln!(
        "FigLock update {}: {e:?}",
        if installing {
            "install failed"
        } else {
            "check failed"
        }
    );
    let unverified = matches!(
        e,
        UpdaterError::Minisign(_)
            | UpdaterError::Base64(_)
            | UpdaterError::SignatureUtf8(_)
            | UpdaterError::SignedVersionMismatch { .. }
    );
    match (installing, unverified) {
        (true, true) => CmdError::new(
            "update_unverified",
            "The update could not be verified, so it was not installed. Your current version is unchanged.",
        ),
        (true, false) => CmdError::new(
            "update_failed",
            "The update could not be downloaded or installed. Your current version is unchanged. Try again in a moment.",
        ),
        (false, true) => CmdError::new(
            "update_unverified",
            "The update information could not be verified, so it was ignored.",
        ),
        (false, false) => CmdError::new(
            "update_check_failed",
            "FigLock could not check for updates. Check your internet connection and try again.",
        ),
    }
}

/// Ask the feed for a newer version.
pub async fn check(app: &AppHandle) -> CmdRes<UpdateView> {
    let now = current(app);
    if !configured(app) {
        return Ok(view("unavailable", &now, None, None));
    }
    let updater = app.updater().map_err(|e| failure(&e, false))?;
    let found = updater.check().await.map_err(|e| failure(&e, false))?;
    let pending = app.state::<Pending>();
    let mut slot = pending.0.lock().unwrap_or_else(|p| p.into_inner());
    Ok(match found {
        Some(update) => {
            let v = view(
                "available",
                &now,
                Some(&update.version),
                update.body.as_deref(),
            );
            *slot = Some(update);
            v
        }
        None => {
            *slot = None;
            view("up_to_date", &now, None, None)
        }
    })
}

/// Download, verify and install the update a check found. The app keeps running its current code until the
/// person restarts it.
pub async fn install(app: &AppHandle) -> CmdRes<UpdateView> {
    let now = current(app);
    let update = {
        let pending = app.state::<Pending>();
        let slot = pending.0.lock().unwrap_or_else(|p| p.into_inner());
        slot.clone()
    };
    let Some(update) = update else {
        return Err(CmdError::new(
            "no_update",
            "There is no update to install. Check for updates first.",
        ));
    };
    let mut downloaded = 0u64;
    let sink = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = sink.emit("update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| failure(&e, true))?;
    Ok(view(
        "ready",
        &now,
        Some(&update.version),
        update.body.as_deref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_signature_says_the_update_was_not_installed_and_the_version_is_unchanged() {
        for e in [
            UpdaterError::SignatureUtf8("x".into()),
            UpdaterError::SignedVersionMismatch {
                signed: "1.0.0".into(),
                announced: "9.9.9".into(),
            },
        ] {
            let f = failure(&e, true);
            assert_eq!(f.code, "update_unverified");
            assert!(f.message.contains("not installed"), "{}", f.message);
            assert!(f.message.contains("unchanged"), "{}", f.message);
        }
        assert_eq!(
            failure(&UpdaterError::SignatureUtf8("x".into()), false).code,
            "update_unverified"
        );
    }

    #[test]
    fn a_connection_problem_is_something_to_try_again_not_a_security_warning() {
        let install = failure(&UpdaterError::ReleaseNotFound, true);
        assert_eq!(install.code, "update_failed");
        assert!(install.message.contains("unchanged"), "{}", install.message);
        assert!(install.message.contains("Try again"), "{}", install.message);

        let check = failure(&UpdaterError::ReleaseNotFound, false);
        assert_eq!(check.code, "update_check_failed");
        assert!(
            check.message.contains("internet connection"),
            "{}",
            check.message
        );
    }

    #[test]
    fn no_message_shows_a_technical_detail() {
        for e in [
            UpdaterError::ReleaseNotFound,
            UpdaterError::EmptyEndpoints,
            UpdaterError::SignatureUtf8("c2VjcmV0".into()),
        ] {
            for installing in [true, false] {
                let m = failure(&e, installing).message;
                assert!(
                    !m.contains("c2VjcmV0") && !m.contains("endpoint") && !m.contains("JSON"),
                    "{m}"
                );
            }
        }
    }

    #[test]
    fn release_notes_are_trimmed_and_empty_ones_dropped() {
        assert_eq!(
            view(
                "available",
                "0.2.0",
                Some("0.2.1"),
                Some("  Faster sync.\n")
            )
            .notes
            .as_deref(),
            Some("Faster sync.")
        );
        assert_eq!(
            view("available", "0.2.0", Some("0.2.1"), Some(" \n ")).notes,
            None
        );
        assert_eq!(view("up_to_date", "0.2.0", None, None).version, None);
    }
}
