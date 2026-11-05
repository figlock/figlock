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
fn private_dir(path: &Path) -> Res<()> {
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
fn private_dir(path: &Path) -> Res<()> {
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
