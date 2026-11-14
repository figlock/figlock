//! A device that knows a vault whose local folder is gone (deleted, moved, or on a drive that is not there).
//! The window shows ways forward before it asks for any credentials (FIG-118, ADR-0026 failure table). Nothing
//! here deletes vault data: the old folder and the Drive copy are never touched, and forgetting the vault only
//! removes this Mac's own device config and keys, and only after the person confirmed.

use figlock_session::error::Fail;
use figlock_session::{config, store, sync};
use figlock_store::StoreError;
use serde::Serialize;

use super::{CmdError, CmdRes, folder_path};

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MissingFolder {
    /// Where this device expects the vault, to show the person what is missing.
    pub path: String,
    /// Sync was on, so a copy may be in Google Drive.
    pub can_restore: bool,
}

/// `Some` when this device is set up but its local vault folder is not there.
pub fn missing() -> Option<MissingFolder> {
    let place = config::load().ok()?.place;
    (!place.local.is_dir()).then(|| MissingFolder {
        path: place.local.display().to_string(),
        can_restore: place.remote.is_some(),
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
pub fn restore(folder: Option<&str>) -> CmdRes<()> {
    require_missing()?;
    let mut place = config::load()?.place;
    if place.remote.is_none() {
        return Err(CmdError::new(
            "failed",
            "Sync was not turned on for this vault, so there is no copy in Google Drive.",
        ));
    }
    if config::load_drive_token()?.is_none() {
        return Err(CmdError::new("drive_sign_in", "Sign in to Google first."));
    }
    place.local = match folder.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => folder_path(f, true)?,
        None => config::default_local_dir()?,
    };
    sync::open_from_remote(&place).map_err(Fail::from)?;
    if store::open_local(&place.local)?.get("vault.json").is_err() {
        return Err(CmdError::new(
            "no_vault",
            "There is no FigLock vault in this Google Drive.",
        ));
    }
    config::set_local(place.local)?;
    Ok(())
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
