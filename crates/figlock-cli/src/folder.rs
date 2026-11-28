//! A device that knows a vault whose local folder is gone (deleted, moved, or on a drive that is not there):
//! the ways forward the desktop app offers, as commands (FIG-296). Nothing here deletes vault data: the old
//! folder and the synced copies are never touched, and `forget` only removes this device's own config and keys.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use figlock_session::config::{self, Remote};
use figlock_session::error::{Fail, Res};
use figlock_session::{icloud, store, sync};
use figlock_store::StoreError;
use serde_json::json;

use crate::{RestoreVaultArgs, prompt};

/// Only while the folder is missing: these commands are the recovery from that state, not general settings.
/// Returns where this device expects the vault.
fn require_missing() -> Res<config::Place> {
    let place = config::load()?.place;
    if place.local.join("vault.json").is_file() {
        return Err(Fail::new(format!(
            "the vault folder is available ({}), so there is nothing to recover",
            place.local.display()
        )));
    }
    Ok(place)
}

fn icloud_remote(place: &config::Place) -> Option<Remote> {
    place
        .remotes
        .iter()
        .find(|r| matches!(r, Remote::Folder(p) if icloud::is_icloud_path(p)))
        .cloned()
}

fn absolute(folder: &Path) -> Res<PathBuf> {
    if folder.as_os_str().is_empty() || !folder.is_absolute() {
        return Err(Fail::new("give the full path of a folder"));
    }
    Ok(folder.to_path_buf())
}

/// The vault was moved: point this device at the folder that holds it. The folder must hold a vault; whether it
/// is this device's vault is settled by unlocking, which fails for any other.
pub fn locate(folder: &Path, json: bool) -> Res<()> {
    require_missing()?;
    let path = std::fs::canonicalize(absolute(folder)?)
        .map_err(|_| Fail::new(format!("{} is not a folder", folder.display())))?;
    match store::open_local(&path)?.get("vault.json") {
        Ok(_) => {}
        Err(StoreError::NotFound) => {
            return Err(Fail::new(format!(
                "there is no FigLock vault in {}",
                path.display()
            )));
        }
        Err(e) => return Err(e.into()),
    }
    config::set_local(path.clone())?;
    if json {
        println!("{}", json!({ "status": "located" }));
    } else {
        println!("This device now uses the vault in {}.", path.display());
    }
    Ok(())
}

/// Download the vault from Google Drive or iCloud Drive into a new local folder (the chosen one, else the
/// default) and point this device at it. A folder with nothing in it is filled in a sibling folder first and
/// renamed into place once the download is whole, so a restore that fails leaves nothing on disk.
pub fn restore(a: &RestoreVaultArgs, json: bool) -> Res<()> {
    let mut place = require_missing()?;
    let drive = place.has_drive();
    let icloud_copy = icloud_remote(&place);
    let (remote, name) = match (a.drive, a.icloud) {
        (true, _) if !drive => {
            return Err(Fail::new(
                "sync to Google Drive was not turned on for this vault, so there is no copy there",
            ));
        }
        (true, _) => (Remote::Drive, "Google Drive"),
        (_, true) => match icloud_copy {
            Some(r) => (r, "iCloud Drive"),
            None => {
                return Err(Fail::new(
                    "sync to iCloud Drive was not turned on for this vault, so there is no copy there",
                ));
            }
        },
        (false, false) => match (drive, icloud_copy) {
            (true, None) => (Remote::Drive, "Google Drive"),
            (false, Some(r)) => (r, "iCloud Drive"),
            (true, Some(_)) => {
                return Err(Fail::new(
                    "this vault has a copy in Google Drive and in iCloud Drive; choose one with --drive or --icloud",
                ));
            }
            (false, None) => {
                return Err(Fail::new(
                    "sync was not turned on for this vault, so there is no copy in Google Drive or iCloud Drive. \
                     If the folder was moved, point at it with `figlock locate <folder>`",
                ));
            }
        },
    };
    match &remote {
        Remote::Drive if config::load_drive_token()?.is_none() => {
            return Err(Fail::new(
                "not signed in to Google Drive; run `figlock drive login` first",
            ));
        }
        Remote::Folder(p) => icloud::check_arrived(p)?,
        Remote::Drive => {}
    }
    // Only the place the person chose is read: another one may hold an older copy or be unreachable.
    place.remotes = vec![remote];
    let target = match &a.folder {
        Some(f) => absolute(f)?,
        None => config::default_local_dir()?,
    };
    if std::fs::read_dir(&target).is_ok_and(|mut e| e.next().is_some()) {
        // Something is already there: fill it in place, which refuses a different vault.
        place.local = std::fs::canonicalize(&target)
            .map_err(|_| Fail::new("that folder cannot be used; choose another"))?;
        fetch(&place, name)?;
    } else {
        let staging = staging_for(&target);
        let _ = std::fs::remove_dir_all(&staging);
        place.local = staging.clone();
        let moved = fetch(&place, name).and_then(|()| move_into_place(&staging, &target));
        if let Err(e) = moved {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        place.local = std::fs::canonicalize(&target)
            .map_err(|_| Fail::new("that folder cannot be used; choose another"))?;
    }
    let shown = place.local.display().to_string();
    config::set_local(place.local)?;
    if json {
        println!("{}", json!({ "status": "restored", "from": name }));
    } else {
        println!("Restored the vault from {name} into {shown}.");
    }
    Ok(())
}

/// Next to `target`, so the final rename stays on one volume.
fn staging_for(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map_or_else(|| "vault".to_owned(), |n| n.to_string_lossy().into_owned());
    target.with_file_name(format!(".{name}.restoring"))
}

fn move_into_place(staging: &Path, target: &Path) -> Res<()> {
    let failed = |_| Fail::new("that folder cannot be used; choose another");
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    // An empty folder is in the way (made by the person, or left over): the vault takes its place.
    if target.is_dir() {
        std::fs::remove_dir(target).map_err(failed)?;
    }
    std::fs::rename(staging, target).map_err(failed)
}

fn fetch(place: &config::Place, name: &str) -> Res<()> {
    sync::open_from_remote(place).map_err(Fail::from)?;
    if store::open_local(&place.local)?.get("vault.json").is_err() {
        return Err(Fail::new(format!("there is no FigLock vault in {name}")));
    }
    Ok(())
}

/// Remove this vault from this device: its config and device keys. The folder, the synced copies and the Secret
/// Key entry are left alone, so the vault can be opened again with `figlock open` or `figlock join`.
pub fn forget(yes: bool, json: bool) -> Res<()> {
    let place = config::load()?.place;
    if !yes {
        if !std::io::stdin().is_terminal() {
            return Err(Fail::new(
                "this is not a terminal, so nothing was removed; pass --yes to remove the vault from this device",
            ));
        }
        let question = format!(
            "Remove the vault in {} from this device? The vault itself and its synced copies stay as they are.",
            place.local.display()
        );
        if !prompt::confirm_default_no(&question)? {
            return Err(Fail::new("nothing was removed"));
        }
    }
    config::forget();
    if json {
        println!("{}", json!({ "status": "forgotten" }));
    } else {
        println!(
            "Removed the vault from this device. Nothing was deleted from {} or any synced copy. \
             Open it again with `figlock open` or `figlock join`.",
            place.local.display()
        );
    }
    Ok(())
}
