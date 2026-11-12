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
        return Err(Fail::new("the vault folder is missing or not available"));
    }
    Ok(Box::new(LocalFolder::open(path)?))
}

pub fn open_remote(remote: Remote) -> Res<Box<dyn BlobStore>> {
    match remote {
        Remote::Drive => open_drive(),
    }
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
    if let Some(remote) = place.remote {
        stores.push(open_remote(remote)?);
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
