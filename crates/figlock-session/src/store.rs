//! Opening the vault's storage from this device's config: the local folder, and the remote it syncs to.

use figlock_store::{BlobStore, GoogleDrive, GoogleOAuth, LocalFolder, StoreError};

use crate::config::{self, Place, Remote};
use crate::drive::no_client;
use crate::error::{Fail, Res};

/// The local vault folder. Every read and write of the vault goes through it (ADR-0026).
pub fn open(place: &Place) -> Res<Box<dyn BlobStore>> {
    open_local(&place.local)
}

pub fn open_local(path: &std::path::Path) -> Res<Box<dyn BlobStore>> {
    if !path.is_dir() {
        return Err(Fail::new(format!(
            "the vault folder is missing or not available: {}\n\
             If it was moved or is on a drive that is not connected, put it back there. The FigLock desktop app \
             offers the other ways forward (locate the folder, restore from Google Drive, create a new vault, remove \
             this vault from this Mac). Without it, delete config.json and device.json in the FigLock config folder \
             to forget the vault on this machine (nothing in the vault folder or Drive is touched), then run \
             `figlock open` or `figlock init`.",
            path.display()
        )));
    }
    Ok(Box::new(LocalFolder::open(path)?))
}

/// Open `remote`. A folder remote is checked against `local` every time, not only when it is added: the
/// vault folder may have been moved onto it since.
pub fn open_remote(remote: &Remote, local: &std::path::Path) -> Res<Box<dyn BlobStore>> {
    match remote {
        Remote::Drive => open_drive(),
        Remote::Folder(path) => {
            check_remote_folder(local, path)?;
            // `LocalFolder::open` makes a missing folder. In iCloud Drive that must not happen on its own: a
            // `FigLock` folder made here before the other device's has arrived is synced as a second folder
            // ("FigLock 2"). A folder that was there and is gone means iCloud Drive was turned off.
            if crate::icloud::is_icloud_path(path) && !path.is_dir() {
                return Err(Fail::new(crate::icloud::missing_message(path)));
            }
            Ok(Box::new(LocalFolder::open(path)?))
        }
    }
}

/// A folder remote must be a different place from the local vault folder: syncing a folder with itself
/// would do nothing, and one inside the other would copy the vault into itself. Compared by real path, so a
/// symlink or a different spelling of the same directory does not get past it.
pub fn check_remote_folder(local: &std::path::Path, folder: &std::path::Path) -> Res<()> {
    if !folder.is_absolute() {
        return Err(Fail::new(format!(
            "the sync folder must be a full path: {}",
            folder.display()
        )));
    }
    let real = |p: &std::path::Path| {
        // A folder that does not exist yet is created later; compare it through its nearest existing parent.
        let mut tail = Vec::new();
        let mut cur = p.to_path_buf();
        loop {
            if let Ok(c) = std::fs::canonicalize(&cur) {
                return tail.iter().rev().fold(c, |acc, seg| acc.join(seg));
            }
            match (cur.file_name().map(ToOwned::to_owned), cur.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name);
                    cur = parent.to_path_buf();
                }
                _ => return p.to_path_buf(),
            }
        }
    };
    let (l, f) = (real(local), real(folder));
    if l == f {
        return Err(Fail::new(format!(
            "{} is this device's vault folder itself, so it cannot be a sync folder too",
            folder.display()
        )));
    }
    if l.starts_with(&f) || f.starts_with(&l) {
        return Err(Fail::new(format!(
            "{} and the vault folder {} contain one another, so it cannot be a sync folder",
            folder.display(),
            local.display()
        )));
    }
    Ok(())
}

/// Whether a vault is already stored at `place`, in the local folder or its remote. Only a definite "not
/// there" counts as no: an unreachable or signed-out store must not look empty, or a second vault could be
/// created next to the first.
pub fn vault_exists(place: &Place) -> Res<bool> {
    let mut stores = Vec::new();
    // A new vault's folder may not exist yet, which is an empty folder, not an error.
    if place.local.is_dir() {
        stores.push(open_local(&place.local)?);
    }
    for remote in &place.remotes {
        stores.push(open_remote(remote, &place.local)?);
    }
    for store in stores {
        match store.get("vault.json") {
            Ok(_) => return Ok(true),
            Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(false)
}

/// Google Drive as this device is signed in to it. Never prompts or opens a browser: a missing or dead
/// sign-in is an error that names the fix.
pub fn open_drive() -> Res<Box<dyn BlobStore>> {
    let token = config::load_drive_token()?
        .ok_or_else(|| Fail::new("not signed in to Google Drive; run `figlock drive login`"))?;
    let oauth = GoogleOAuth::from_env().ok_or_else(no_client)?;
    let source = oauth.token_source(figlock_store::Credentials {
        refresh_token: token.to_string(),
    });
    Ok(Box::new(GoogleDrive::new(source)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_names_itself_and_the_ways_forward() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("vault");
        let Err(Fail(message, _)) = open_local(&gone) else {
            panic!("a missing folder must not open");
        };
        assert!(message.contains(&gone.display().to_string()));
        assert!(message.contains("desktop app"));
        assert!(message.contains("figlock open"));
    }
}
