//! This device's local state: where the vault lives and this device's private keys.
//!
//! `config.json` names the store folder and the device. `device.json` holds the device keys (owner-only
//! permissions). Moving the keys into the platform keystore, and quick unlock, come with the broker daemon.
//! The master password and the Secret Key are never written here.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use figlock_core::{DeviceKeys, Key};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::error::{Fail, Res};

pub struct Config {
    pub store: PathBuf,
    pub device_name: String,
}

/// `FIGLOCK_HOME`, else the platform's per-user config directory.
pub fn home() -> Res<PathBuf> {
    use std::env::var_os;
    if let Some(h) = var_os("FIGLOCK_HOME").filter(|v| !v.is_empty()) {
        return Ok(h.into());
    }
    let base = if cfg!(windows) {
        var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("figlock"))
        .ok_or_else(|| Fail::new("cannot find a home folder; set FIGLOCK_HOME"))
}

fn config_path(home: &Path) -> PathBuf {
    home.join("config.json")
}

fn device_path(home: &Path) -> PathBuf {
    home.join("device.json")
}

pub fn is_initialised() -> Res<bool> {
    Ok(config_path(&home()?).exists())
}

#[cfg(unix)]
pub(crate) fn private_dir(path: &Path) -> Res<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    // The folder is figlock's own; tighten it even when it already existed.
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn private_dir(path: &Path) -> Res<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Save the config and device keys. Refuses to overwrite an existing device.
pub fn save(cfg: &Config, device: &DeviceKeys) -> Res<()> {
    let home = home()?;
    private_dir(&home)?;
    let store = cfg
        .store
        .to_str()
        .ok_or_else(|| Fail::new("the store path is not valid text"))?;
    let keys = Zeroizing::new(
        json!({
            "device_id": hex::encode(device.device_id),
            "sign_seed": hex::encode(device.sign_seed.expose()),
            "agree_priv": hex::encode(device.agree_priv.expose()),
        })
        .to_string(),
    );
    let mut f = create_private(&device_path(&home))
        .map_err(|_| Fail::new("this device is already set up"))?;
    f.write_all(keys.as_bytes())?;
    let body = json!({ "store": store, "device_name": cfg.device_name }).to_string();
    if let Err(e) = fs::write(config_path(&home), body) {
        let _ = fs::remove_file(device_path(&home));
        return Err(e.into());
    }
    Ok(())
}

fn drive_token_path(home: &Path) -> PathBuf {
    home.join("drive-token.json")
}

/// Keep the Google refresh token, owner-only. Written to a private temporary file and renamed into place, so
/// the token is never visible to other users and a crash cannot leave half a file.
pub fn save_drive_token(refresh_token: &str) -> Res<()> {
    save_drive_token_in(&home()?, refresh_token)
}

fn save_drive_token_in(home: &Path, refresh_token: &str) -> Res<()> {
    private_dir(home)?;
    let tmp = home.join(".drive-token.json.tmp");
    let _ = fs::remove_file(&tmp);
    let body = Zeroizing::new(json!({ "refresh_token": refresh_token }).to_string());
    let written = create_private(&tmp).and_then(|mut f| {
        f.write_all(body.as_bytes())?;
        f.sync_all()
    });
    if let Err(e) = written.and_then(|()| fs::rename(&tmp, drive_token_path(home))) {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// The stored Google refresh token, or `None` when this device is not signed in to Google.
#[allow(dead_code)]
pub fn load_drive_token() -> Res<Option<Zeroizing<String>>> {
    load_drive_token_in(&home()?)
}

#[allow(dead_code)]
fn load_drive_token_in(home: &Path) -> Res<Option<Zeroizing<String>>> {
    let path = drive_token_path(home);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(&path) {
            Ok(m) if m.permissions().mode() & 0o077 != 0 => {
                return Err(Fail::new(
                    "drive-token.json is readable by other users; run `chmod 600` on it or sign in again",
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    }
    let raw = match fs::read(&path) {
        Ok(r) => Zeroizing::new(r),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    parse_drive_token(&raw).map(Some).ok_or_else(|| {
        Fail::new("drive-token.json is damaged; sign in again with `figlock drive login`")
    })
}

fn parse_drive_token(raw: &[u8]) -> Option<Zeroizing<String>> {
    let v: Value = serde_json::from_slice(raw).ok()?;
    let token = v
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())?;
    Some(Zeroizing::new(token.to_owned()))
}

/// What sign-out finds on disk. Unlike [`load_drive_token`] it reads a token file with loose permissions too:
/// that file may have leaked, which is the best reason to revoke it.
pub enum StoredDriveToken {
    Missing,
    Damaged,
    Token(Zeroizing<String>),
}

pub fn read_drive_token_for_logout() -> Res<StoredDriveToken> {
    match fs::read(drive_token_path(&home()?)) {
        Ok(raw) => Ok(match parse_drive_token(&Zeroizing::new(raw)) {
            Some(t) => StoredDriveToken::Token(t),
            None => StoredDriveToken::Damaged,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StoredDriveToken::Missing),
        Err(e) => Err(e.into()),
    }
}

/// Delete the stored Google refresh token. Missing is fine.
pub fn forget_drive_token() -> Res<()> {
    match fs::remove_file(drive_token_path(&home()?)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Undo [`save`] after a later step of `init` failed.
pub fn forget() {
    if let Ok(home) = home() {
        let _ = fs::remove_file(config_path(&home));
        let _ = fs::remove_file(device_path(&home));
    }
}

fn not_set_up() -> Fail {
    Fail::new("no vault is set up on this device; run `figlock init --store <folder>`")
}

pub fn load() -> Res<Config> {
    let raw = fs::read(config_path(&home()?)).map_err(|_| not_set_up())?;
    let v: Value = serde_json::from_slice(&raw).map_err(|_| Fail::new("config.json is damaged"))?;
    let get = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Fail::new("config.json is damaged"))
    };
    Ok(Config {
        store: get("store")?.into(),
        device_name: get("device_name")?,
    })
}

pub fn load_device() -> Res<DeviceKeys> {
    let path = device_path(&home()?);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path)
            .map_err(|_| not_set_up())?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(Fail::new(
                "device.json is readable by other users; run `chmod 600` on it before continuing",
            ));
        }
    }
    let raw = Zeroizing::new(fs::read(&path).map_err(|_| not_set_up())?);
    let damaged = || Fail::new("device.json is damaged");
    let v: Value = serde_json::from_slice(&raw).map_err(|_| damaged())?;
    let field = |k: &str| -> Res<Zeroizing<Vec<u8>>> {
        let s = v.get(k).and_then(Value::as_str).ok_or_else(damaged)?;
        Ok(Zeroizing::new(hex::decode(s).map_err(|_| damaged())?))
    };
    Ok(DeviceKeys {
        device_id: field("device_id")?
            .as_slice()
            .try_into()
            .map_err(|_| damaged())?,
        sign_seed: Key::from_slice(&field("sign_seed")?).map_err(|_| damaged())?,
        agree_priv: Key::from_slice(&field("agree_priv")?).map_err(|_| damaged())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_token_round_trips_and_is_owner_only() {
        let d = tempfile::tempdir().unwrap();
        assert!(load_drive_token_in(d.path()).unwrap().is_none());
        save_drive_token_in(d.path(), "refresh-a").unwrap();
        save_drive_token_in(d.path(), "refresh-b").unwrap();
        assert_eq!(
            &**load_drive_token_in(d.path()).unwrap().unwrap(),
            "refresh-b"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(drive_token_path(d.path()))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            // A token anyone else can read is refused, not silently used.
            fs::set_permissions(
                drive_token_path(d.path()),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            assert!(load_drive_token_in(d.path()).is_err());
        }
        let names: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["drive-token.json"], "no temporary file left behind");
    }

    #[test]
    fn a_damaged_drive_token_asks_for_a_new_sign_in() {
        let d = tempfile::tempdir().unwrap();
        save_drive_token_in(d.path(), "x").unwrap();
        fs::write(drive_token_path(d.path()), b"{}").unwrap();
        assert!(
            load_drive_token_in(d.path())
                .unwrap_err()
                .0
                .contains("sign in again")
        );
    }
}
