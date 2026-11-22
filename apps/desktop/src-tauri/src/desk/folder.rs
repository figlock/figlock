//! A device that knows a vault whose local folder is gone (deleted, moved, or on a drive that is not there).
//! The window shows ways forward before it asks for any credentials (FIG-118, ADR-0026 failure table). Nothing
//! here deletes vault data: the old folder and the Drive copy are never touched, and forgetting the vault only
//! removes this Mac's own device config and keys, and only after the person confirmed.

use figlock_session::error::Fail;
use figlock_session::{config, store, sync};
use figlock_store::StoreError;
use serde::Serialize;

use super::{CmdError, CmdRes, Desk, folder_path};

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MissingFolder {
    /// Where this device expects the vault, to show the person what is missing.
    pub path: String,
    /// Sync was on, so a copy may be in Google Drive.
    pub can_restore: bool,
}

/// `Some` when this device is set up but its local vault folder is not there, or is there with no vault in it (an
/// empty folder, as a failed restore or a drive that has not filled in yet leaves behind). Unlocking against either
/// could only fail, so the window shows the ways forward instead.
pub fn missing() -> Option<MissingFolder> {
    let place = config::load().ok()?.place;
    (!place.local.join("vault.json").is_file()).then(|| MissingFolder {
        path: place.local.display().to_string(),
        can_restore: place.has_drive(),
    })
}

/// Only while the folder is missing: these commands are the recovery from that state, not general settings.
fn require_missing() -> CmdRes<()> {
    missing().map(|_| ()).ok_or_else(|| {
        CmdError::new(
            "failed",
            "The vault folder is available, so there is nothing to recover.",
        )
    })
}

/// The vault was moved: point this device at the folder that holds it. The folder must hold a vault; whether it
/// is this device's vault is settled by unlocking, which fails for any other.
pub fn locate(folder: &str) -> CmdRes<()> {
    require_missing()?;
    let path = folder_path(folder, false)?;
    match store::open_local(&path)?.get("vault.json") {
        Ok(_) => {}
        Err(StoreError::NotFound) => {
            return Err(CmdError::new(
                "no_vault",
                "There is no FigLock vault in that folder.",
            ));
        }
        Err(e) => return Err(Fail::from(e).into()),
    }
    config::set_local(path)?;
    Ok(())
}

/// Download the vault from Google Drive into a new local folder (the chosen one, else the default) and point this
/// device at it. Blocks on the network. Refuses a folder that holds a different vault.
///
/// A folder with nothing in it is filled in a sibling folder first and renamed into place once the download is
/// whole, so a restore that fails (Google refused the sign-in, the network dropped) leaves nothing on disk and a
/// quit halfway never leaves a vault folder that only looks set up.
pub fn restore(folder: Option<&str>) -> CmdRes<()> {
    require_missing()?;
    let mut place = config::load()?.place;
    if !place.has_drive() {
        return Err(CmdError::new(
            "failed",
            "Sync was not turned on for this vault, so there is no copy in Google Drive.",
        ));
    }
    if config::load_drive_token()?.is_none() {
        return Err(CmdError::new("drive_sign_in", "Sign in to Google first."));
    }
    let target = match folder.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => absolute(f)?,
        None => config::default_local_dir()?,
    };
    if holds_files(&target) {
        // Something is already there: fill it in place, which refuses a different vault.
        place.local = folder_path(&target.display().to_string(), false)?;
        fetch(&place)?;
    } else {
        let staging = staging_for(&target);
        let _ = std::fs::remove_dir_all(&staging);
        place.local = staging.clone();
        let moved = fetch(&place).and_then(|()| move_into_place(&staging, &target));
        if let Err(e) = moved {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        place.local = folder_path(&target.display().to_string(), false)?;
    }
    config::set_local(place.local)?;
    Ok(())
}

fn absolute(folder: &str) -> CmdRes<std::path::PathBuf> {
    let path = std::path::Path::new(folder);
    if !path.is_absolute() {
        return Err(CmdError::new("folder", "Choose a folder for the vault."));
    }
    Ok(path.to_path_buf())
}

/// The folder exists and has something in it.
fn holds_files(path: &std::path::Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}

/// Next to `target`, so the final rename stays on one volume.
fn staging_for(target: &std::path::Path) -> std::path::PathBuf {
    let name = target
        .file_name()
        .map_or_else(|| "vault".to_owned(), |n| n.to_string_lossy().into_owned());
    target.with_file_name(format!(".{name}.restoring"))
}

fn move_into_place(staging: &std::path::Path, target: &std::path::Path) -> CmdRes<()> {
    let failed = |_| CmdError::new("folder", "That folder cannot be used. Choose another.");
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    // An empty folder is in the way (the person made it, or it was left over): the vault takes its place.
    if target.is_dir() {
        std::fs::remove_dir(target).map_err(failed)?;
    }
    std::fs::rename(staging, target).map_err(failed)
}

fn fetch(place: &config::Place) -> CmdRes<()> {
    sync::open_from_remote(place).map_err(restore_failure)?;
    if store::open_local(&place.local)?.get("vault.json").is_err() {
        return Err(CmdError::new(
            "no_vault",
            "There is no FigLock vault in this Google Drive.",
        ));
    }
    Ok(())
}

/// Why a restore failed, in words the person can act on.
pub(super) fn restore_failure(e: sync::SyncError) -> CmdError {
    match e {
        sync::SyncError::SignIn => CmdError::new(
            "drive_sign_in",
            "Google has stopped FigLock's access. Sign in again to restore.",
        ),
        sync::SyncError::Offline | sync::SyncError::Busy => CmdError::new(
            "storage_unreachable",
            "FigLock could not reach Google Drive. Check your connection, then try again.",
        ),
        e => Fail::from(e).into(),
    }
}

/// Remove this vault from this Mac: its device config and device keys. The folder, the Drive copy and the
/// Secret Key and Touch ID entries are left alone. `confirmed` is the person's explicit yes, asked for in the
/// window.
pub fn forget(confirmed: bool) -> CmdRes<()> {
    if !confirmed {
        return Err(CmdError::new(
            "not_confirmed",
            "Confirm that this Mac should stop using this vault.",
        ));
    }
    require_missing()?;
    config::forget();
    Ok(())
}

impl Desk {
    /// The person cannot unlock and has no way back (no other device, no Recovery Key): this Mac stops using the
    /// vault so they can open or join one again with their Emergency Kit (`pairing-recovery.md` 6, R3). Only
    /// while locked. Like `forget`, it leaves the folder, the Drive copy and the kept Secret Key alone.
    pub fn forget_locked(&self, confirmed: bool) -> CmdRes<()> {
        if !confirmed {
            return Err(CmdError::new(
                "not_confirmed",
                "Confirm that this Mac should stop using this vault.",
            ));
        }
        if self.inner().session.is_some() {
            return Err(CmdError::new("failed", "Lock the vault first."));
        }
        config::forget();
        Ok(())
    }
}
