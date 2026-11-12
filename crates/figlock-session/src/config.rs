//! This device's local state: where the vault lives and this device's private keys.
//!
//! `config.json` names the store folder and the device. `device.json` names the device and, where the platform
//! has a keystore (`keystore.rs`), holds no secret: the private keys are in the keystore under the device id.
//! Without one it also holds the keys (owner-only permissions). A `device.json` from before the keystore
//! existed is moved over the first time it is read. Quick unlock keeps its policy state next to them
//! (`quick.rs`). The master password and the Secret Key are never written here.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use figlock_core::{DeviceKeys, Key};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::error::{Fail, Res};
use crate::keystore::{self, SecretStore};

/// A second location the local vault is kept matched to by sync (ADR-0026).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remote {
    /// The hidden appDataFolder in the Google Drive this device is signed in to (`figlock drive login`).
    Drive,
}

/// Where this device's vault lives: always a local folder (possibly itself synced by iCloud Drive, Dropbox,
/// Syncthing), and optionally a remote that sync keeps matched to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub local: PathBuf,
    pub remote: Option<Remote>,
}

pub struct Config {
    pub place: Place,
    pub device_name: String,
}

/// Where the local copy goes when the person chooses Drive and does not pick a folder.
pub fn default_local_dir() -> Res<PathBuf> {
    Ok(home()?.join("vault"))
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
pub fn private_dir(path: &Path) -> Res<()> {
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
pub fn private_dir(path: &Path) -> Res<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

/// A new owner-only file; fails if it exists.
pub fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    create_private(path)
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
    save_in(&home()?, cfg, device, keystore::system().as_deref())
}

fn save_in(
    home: &Path,
    cfg: &Config,
    device: &DeviceKeys,
    store: Option<&dyn SecretStore>,
) -> Res<()> {
    private_dir(home)?;
    let body = config_body(cfg)?;
    let account = hex::encode(device.device_id);
    let keys = Zeroizing::new(match store {
        Some(_) => json!({ "device_id": account, "keystore": true }).to_string(),
        None => device_file(device).to_string(),
    });
    let mut f = create_private(&device_path(home))
        .map_err(|_| Fail::new("this device is already set up"))?;
    f.write_all(keys.as_bytes())?;
    if let Some(store) = store
        && let Err(e) = store.put(&account, &secret_blob(device))
    {
        let _ = fs::remove_file(device_path(home));
        return Err(e);
    }
    if let Err(e) = fs::write(config_path(home), body) {
        let _ = fs::remove_file(device_path(home));
        if let Some(store) = store {
            let _ = store.delete(&account);
        }
        return Err(e.into());
    }
    Ok(())
}

fn config_body(cfg: &Config) -> Res<String> {
    let path = cfg
        .place
        .local
        .to_str()
        .ok_or_else(|| Fail::new("the store path is not valid text"))?;
    let mut body = json!({ "store": path, "device_name": cfg.device_name });
    match cfg.place.remote {
        None => {}
        Some(Remote::Drive) => body["remote"] = json!("drive"),
    }
    Ok(body.to_string())
}

/// `device.json` with the keys in it, for platforms without a keystore.
fn device_file(device: &DeviceKeys) -> Value {
    json!({
        "device_id": hex::encode(device.device_id),
        "sign_seed": hex::encode(device.sign_seed.expose()),
        "agree_priv": hex::encode(device.agree_priv.expose()),
    })
}

/// What the keystore holds for a device: the sign seed, then the agreement private key.
fn secret_blob(device: &DeviceKeys) -> Zeroizing<Vec<u8>> {
    let mut b = Zeroizing::new(Vec::with_capacity(64));
    b.extend_from_slice(device.sign_seed.expose());
    b.extend_from_slice(device.agree_priv.expose());
    b
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

/// This device's id, read from `device.json` without touching any key.
pub fn device_id() -> Option<[u8; 16]> {
    let raw = fs::read(device_path(&home().ok()?)).ok()?;
    let id = serde_json::from_slice::<Value>(&raw)
        .ok()?
        .get("device_id")?
        .as_str()
        .map(str::to_owned)?;
    hex::decode(id).ok()?.try_into().ok()
}

/// Undo [`save`] after a later step of `init` failed.
pub fn forget() {
    if let Ok(home) = home() {
        forget_in(&home, keystore::system().as_deref());
    }
}

fn forget_in(home: &Path, store: Option<&dyn SecretStore>) {
    if let (Some(store), Ok(raw)) = (store, fs::read(device_path(home)))
        && let Some(id) = serde_json::from_slice::<Value>(&raw)
            .ok()
            .and_then(|v| v.get("device_id")?.as_str().map(str::to_owned))
    {
        let _ = store.delete(&id);
    }
    let _ = fs::remove_file(config_path(home));
    let _ = fs::remove_file(device_path(home));
}

fn not_set_up() -> Fail {
    Fail::new("no vault is set up on this device; run `figlock init --store <folder>`")
}

pub fn load() -> Res<Config> {
    load_in(&home()?)
}

fn load_in(home: &Path) -> Res<Config> {
    let raw = fs::read(config_path(home)).map_err(|_| not_set_up())?;
    let v: Value = serde_json::from_slice(&raw).map_err(|_| Fail::new("config.json is damaged"))?;
    let get = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| Fail::new("config.json is damaged"))
    };
    let unknown = || Fail::new("config.json names a storage kind this version does not know");
    let remote = match v.get("remote").and_then(Value::as_str) {
        None => None,
        Some("drive") => Some(Remote::Drive),
        Some(_) => return Err(unknown()),
    };
    let (local, legacy) = match v.get("store_kind").and_then(Value::as_str) {
        None => (get("store")?.into(), false),
        // A Drive-only device from before ADR-0026: it gets a local folder, filled from Drive on first use.
        Some("drive") => (home.join("vault"), true),
        Some(_) => return Err(unknown()),
    };
    let cfg = Config {
        place: Place {
            local,
            remote: if legacy { Some(Remote::Drive) } else { remote },
        },
        device_name: get("device_name")?,
    };
    if legacy {
        // Written before anything is downloaded, so a crash leaves a config whose empty local folder the
        // next open fills from Drive.
        fs::write(config_path(home), config_body(&cfg)?)?;
    }
    Ok(cfg)
}

/// Turn the remote on or off for this device, keeping everything else in `config.json`.
pub fn set_remote(remote: Option<Remote>) -> Res<Config> {
    set_remote_in(&home()?, remote)
}

fn set_remote_in(home: &Path, remote: Option<Remote>) -> Res<Config> {
    let mut cfg = load_in(home)?;
    cfg.place.remote = remote;
    fs::write(config_path(home), config_body(&cfg)?)?;
    Ok(cfg)
}

pub fn load_device() -> Res<DeviceKeys> {
    load_device_in(&home()?, keystore::system().as_deref())
}

fn load_device_in(home: &Path, store: Option<&dyn SecretStore>) -> Res<DeviceKeys> {
    let path = device_path(home);
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
    let device_id: [u8; 16] = field("device_id")?
        .as_slice()
        .try_into()
        .map_err(|_| damaged())?;

    if v.get("keystore") == Some(&Value::Bool(true)) {
        let store = store.ok_or_else(|| {
            Fail::new(
                "this device keeps its keys in the platform keystore, which is not in use here; unset FIGLOCK_DEVICE_KEYS",
            )
        })?;
        let blob = store.get(&hex::encode(device_id))?.ok_or_else(|| {
            Fail::new(
                "the device keys are missing from the keychain; add this device again with the pairing flow",
            )
        })?;
        return keys_from_blob(device_id, &blob).ok_or_else(|| {
            Fail::new("the device keys in the keychain are damaged; add this device again")
        });
    }

    let keys = DeviceKeys {
        device_id,
        sign_seed: Key::from_slice(&field("sign_seed")?).map_err(|_| damaged())?,
        agree_priv: Key::from_slice(&field("agree_priv")?).map_err(|_| damaged())?,
    };
    if let Some(store) = store {
        // A device set up before the keystore existed. A failed move leaves the file as it was, and the
        // keys still work from it.
        let _ = move_to_keystore(home, store, &keys);
    }
    Ok(keys)
}

fn keys_from_blob(device_id: [u8; 16], blob: &[u8]) -> Option<DeviceKeys> {
    if blob.len() != 64 {
        return None;
    }
    Some(DeviceKeys {
        device_id,
        sign_seed: Key::from_slice(&blob[..32]).ok()?,
        agree_priv: Key::from_slice(&blob[32..]).ok()?,
    })
}

/// Put the keys in the keystore, check they read back the same, then replace `device.json` with a copy that
/// names the device only. The file is replaced by rename, so a crash leaves either the old or the new one.
fn move_to_keystore(home: &Path, store: &dyn SecretStore, keys: &DeviceKeys) -> Res<()> {
    let account = hex::encode(keys.device_id);
    store.put(&account, &secret_blob(keys))?;
    let stored = store.get(&account)?;
    if stored.as_deref().map(Vec::as_slice) != Some(secret_blob(keys).as_slice()) {
        let _ = store.delete(&account);
        return Err(Fail::new("the keystore did not keep the device keys"));
    }
    let tmp = home.join(".device.json.tmp");
    let _ = fs::remove_file(&tmp);
    let body = json!({ "device_id": account, "keystore": true }).to_string();
    let written = create_private(&tmp).and_then(|mut f| {
        f.write_all(body.as_bytes())?;
        f.sync_all()
    });
    if let Err(e) = written.and_then(|()| fs::rename(&tmp, device_path(home))) {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
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

    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    /// An in-memory keystore, so no test touches the real keychain.
    #[derive(Default)]
    struct Fake {
        items: RefCell<HashMap<String, Vec<u8>>>,
        fail_put: Cell<bool>,
    }

    impl SecretStore for Fake {
        fn put(&self, account: &str, secret: &[u8]) -> Res<()> {
            if self.fail_put.get() {
                return Err(Fail::new("keystore refused"));
            }
            self.items
                .borrow_mut()
                .insert(account.into(), secret.to_vec());
            Ok(())
        }
        fn get(&self, account: &str) -> Res<Option<Zeroizing<Vec<u8>>>> {
            Ok(self
                .items
                .borrow()
                .get(account)
                .map(|v| Zeroizing::new(v.clone())))
        }
        fn delete(&self, account: &str) -> Res<()> {
            self.items.borrow_mut().remove(account);
            Ok(())
        }
    }

    fn cfg() -> Config {
        Config {
            place: Place {
                local: "/vault".into(),
                remote: Some(Remote::Drive),
            },
            device_name: "Laptop".into(),
        }
    }

    fn same(a: &DeviceKeys, b: &DeviceKeys) -> bool {
        a.device_id == b.device_id
            && a.sign_seed.expose() == b.sign_seed.expose()
            && a.agree_priv.expose() == b.agree_priv.expose()
    }

    fn file_text(home: &Path) -> String {
        String::from_utf8(fs::read(device_path(home)).unwrap()).unwrap()
    }

    #[test]
    fn with_a_keystore_device_json_holds_no_secret() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, Some(&store)).unwrap();

        let text = file_text(d.path());
        assert!(!text.contains(&hex::encode(keys.sign_seed.expose())));
        assert!(!text.contains(&hex::encode(keys.agree_priv.expose())));
        assert!(!text.contains("sign_seed") && !text.contains("agree_priv"));
        assert!(same(
            &load_device_in(d.path(), Some(&store)).unwrap(),
            &keys
        ));
    }

    #[test]
    fn without_a_keystore_the_keys_stay_in_the_owner_only_file() {
        let d = tempfile::tempdir().unwrap();
        let keys = DeviceKeys::generate().unwrap();
        save_in(d.path(), &cfg(), &keys, None).unwrap();
        assert!(same(&load_device_in(d.path(), None).unwrap(), &keys));
        assert!(file_text(d.path()).contains("sign_seed"));
    }

    #[test]
    fn an_existing_device_moves_to_the_keystore_on_first_read() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, None).unwrap();

        assert!(same(
            &load_device_in(d.path(), Some(&store)).unwrap(),
            &keys
        ));
        let text = file_text(d.path());
        assert!(!text.contains(&hex::encode(keys.sign_seed.expose())));
        assert!(!text.contains(&hex::encode(keys.agree_priv.expose())));
        assert_eq!(store.items.borrow().len(), 1);
        // The second read comes from the keystore alone.
        assert!(same(
            &load_device_in(d.path(), Some(&store)).unwrap(),
            &keys
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(device_path(d.path()))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let names: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(!names.iter().any(|n| n.contains("tmp")), "{names:?}");
    }

    #[test]
    fn a_keystore_that_refuses_leaves_the_file_and_the_keys_working() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, None).unwrap();
        store.fail_put.set(true);

        assert!(same(
            &load_device_in(d.path(), Some(&store)).unwrap(),
            &keys
        ));
        assert!(file_text(d.path()).contains("sign_seed"));
        assert!(store.items.borrow().is_empty());
    }

    #[test]
    fn a_removed_key_is_reported_not_replaced() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, Some(&store)).unwrap();
        store.items.borrow_mut().clear();

        let err = load_device_in(d.path(), Some(&store)).err().unwrap();
        assert!(err.0.contains("missing from the keychain"), "{}", err.0);
    }

    #[test]
    fn damaged_or_mismatched_keystore_contents_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, Some(&store)).unwrap();
        let account = hex::encode(keys.device_id);

        for bad in [vec![], vec![7u8; 63], vec![7u8; 65]] {
            store.items.borrow_mut().insert(account.clone(), bad);
            let err = load_device_in(d.path(), Some(&store)).err().unwrap();
            assert!(err.0.contains("damaged"), "{}", err.0);
        }
        // Another device's secret under this device's name opens nothing it should not: the id comes from
        // device.json, and the keys are whatever the keystore holds, so the unlock check fails later.
        let other = DeviceKeys::generate().unwrap();
        store
            .items
            .borrow_mut()
            .insert(account, secret_blob(&other).to_vec());
        let loaded = load_device_in(d.path(), Some(&store)).unwrap();
        assert_eq!(loaded.device_id, keys.device_id);
        assert!(!same(&loaded, &keys));
    }

    #[test]
    fn a_keystore_device_cannot_be_read_without_the_keystore() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, Some(&store)).unwrap();
        let err = load_device_in(d.path(), None).err().unwrap();
        assert!(err.0.contains("platform keystore"), "{}", err.0);
    }

    #[test]
    fn a_refusing_keystore_leaves_no_half_set_up_device() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        store.fail_put.set(true);
        assert!(save_in(d.path(), &cfg(), &keys, Some(&store)).is_err());
        assert!(!device_path(d.path()).exists() && !config_path(d.path()).exists());
    }

    #[test]
    fn forgetting_a_device_removes_its_keys_from_the_keystore() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, Some(&store)).unwrap();
        forget_in(d.path(), Some(&store));
        assert!(store.items.borrow().is_empty());
        assert!(!device_path(d.path()).exists() && !config_path(d.path()).exists());
    }

    #[test]
    fn a_second_save_does_not_replace_the_device() {
        let d = tempfile::tempdir().unwrap();
        let (store, keys) = (Fake::default(), DeviceKeys::generate().unwrap());
        save_in(d.path(), &cfg(), &keys, Some(&store)).unwrap();
        let again = DeviceKeys::generate().unwrap();
        assert!(save_in(d.path(), &cfg(), &again, Some(&store)).is_err());
        assert!(same(
            &load_device_in(d.path(), Some(&store)).unwrap(),
            &keys
        ));
        assert_eq!(store.items.borrow().len(), 1);
    }

    #[test]
    fn a_folder_with_and_without_drive_round_trips() {
        let d = tempfile::tempdir().unwrap();
        let keys = DeviceKeys::generate().unwrap();
        let mut c = cfg();
        c.place = Place {
            local: d.path().join("v"),
            remote: None,
        };
        save_in(d.path(), &c, &keys, None).unwrap();
        let back = load_in(d.path()).unwrap();
        assert_eq!(back.place, c.place);
        assert!(
            !fs::read_to_string(config_path(d.path()))
                .unwrap()
                .contains("remote")
        );

        c.place.remote = Some(Remote::Drive);
        fs::write(config_path(d.path()), config_body(&c).unwrap()).unwrap();
        assert_eq!(load_in(d.path()).unwrap().place, c.place);
    }

    #[test]
    fn a_drive_only_device_gets_a_local_folder_and_is_rewritten_once() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            config_path(d.path()),
            r#"{"store_kind":"drive","device_name":"Old"}"#,
        )
        .unwrap();
        let c = load_in(d.path()).unwrap();
        assert_eq!(c.place.local, d.path().join("vault"));
        assert_eq!(c.place.remote, Some(Remote::Drive));
        assert_eq!(c.device_name, "Old");
        let text = fs::read_to_string(config_path(d.path())).unwrap();
        assert!(!text.contains("store_kind"), "rewritten in the new shape");
        assert_eq!(
            load_in(d.path()).unwrap().place,
            c.place,
            "and stable after"
        );
    }

    #[test]
    fn an_unknown_remote_is_refused() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            config_path(d.path()),
            r#"{"store":"/v","remote":"s3","device_name":"x"}"#,
        )
        .unwrap();
        assert!(load_in(d.path()).is_err());
    }

    #[test]
    fn the_remote_can_be_turned_on_and_off() {
        let d = tempfile::tempdir().unwrap();
        let keys = DeviceKeys::generate().unwrap();
        let mut c = cfg();
        c.place.remote = None;
        save_in(d.path(), &c, &keys, None).unwrap();
        let on = set_remote_in(d.path(), Some(Remote::Drive)).unwrap();
        assert_eq!(on.place.remote, Some(Remote::Drive));
        assert_eq!(load_in(d.path()).unwrap().place.remote, Some(Remote::Drive));
        let off = set_remote_in(d.path(), None).unwrap();
        assert_eq!(off.place, c.place);
        assert_eq!(load_in(d.path()).unwrap().device_name, "Laptop");
    }
}
