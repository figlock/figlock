//! Opening the vault's storage from this device's config: a local folder, or the signed-in Google Drive.

use figlock_store::{BlobStore, GoogleDrive, GoogleOAuth, LocalFolder};

use crate::config::{self, StoreSpec};
use crate::error::{Fail, Res};

pub fn open(spec: &StoreSpec) -> Res<Box<dyn BlobStore>> {
    match spec {
        StoreSpec::Folder(path) => {
            if !path.is_dir() {
                return Err(Fail::new("the vault folder is missing or not available"));
            }
            Ok(Box::new(LocalFolder::open(path)?))
        }
        StoreSpec::Drive => open_drive(),
    }
}

/// Google Drive as this device is signed in to it. Never prompts or opens a browser: a missing or dead
/// sign-in is an error that names the fix.
pub fn open_drive() -> Res<Box<dyn BlobStore>> {
    let token = config::load_drive_token()?
        .ok_or_else(|| Fail::new("not signed in to Google Drive; run `figlock drive login`"))?;
    let oauth = GoogleOAuth::from_env().ok_or_else(crate::drive::no_client)?;
    let source = oauth.token_source(figlock_store::Credentials {
        refresh_token: token.to_string(),
    });
    Ok(Box::new(GoogleDrive::new(source)))
}
