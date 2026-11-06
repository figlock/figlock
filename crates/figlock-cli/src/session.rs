//! An unlocked vault plus its store: load every op file, resolve item references, write changes back.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use figlock_core::{DeviceId, Index, ItemId, ItemView, Key, Vault};
use figlock_store::{BlobStore, StoreError};

use crate::config;
use crate::error::{Fail, Res};
use crate::prompt;
use crate::quick;

pub struct Session {
    pub vault: Vault,
    store: Box<dyn BlobStore>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

pub fn id_hex(id: &ItemId) -> String {
    hex::encode(id)
}

/// `log/<32 hex>/<8 digits>.op` to the writing device and sequence number.
fn parse_log_path(path: &str) -> Option<(DeviceId, u64)> {
    let rest = path.strip_prefix("log/")?;
    let (dev, file) = rest.split_once('/')?;
    let seq = file.strip_suffix(".op")?;
    if seq.len() != 8 || !seq.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let device: DeviceId = hex::decode(dev).ok()?.try_into().ok()?;
    Some((device, seq.parse().ok()?))
}

fn short(device: &DeviceId) -> String {
    hex::encode(&device[..4])
}

fn read_vault_json(store: &dyn BlobStore) -> Res<Vec<u8>> {
    store.get("vault.json").map_err(|e| match e {
        StoreError::NotFound => Fail::new("vault.json not found in the vault folder"),
        StoreError::Unauthorized => {
            Fail::new("storage sign-in was refused; run `figlock drive login`")
        }
        StoreError::Unavailable => {
            Fail::new("storage could not be reached; check the connection and try again")
        }
        e => e.into(),
    })
}

/// The vault's id, read without unlocking anything.
pub fn peek_vault_id() -> Res<String> {
    let cfg = config::load()?;
    let store = crate::store::open(&cfg.store)?;
    vault_id_of(&read_vault_json(store.as_ref())?)
}

fn wrong_credentials() -> Fail {
    Fail::new("could not unlock: wrong master password or Secret Key, or the vault file is damaged")
}

/// `vault_id` from `vault.json`, which names this vault's quick-unlock entry.
fn vault_id_of(vault_json: &[u8]) -> Res<String> {
    serde_json::from_slice::<serde_json::Value>(vault_json)
        .ok()
        .and_then(|v| v["vault_id"].as_str().map(str::to_owned))
        .ok_or_else(|| Fail::new("vault.json is damaged"))
}

impl Session {
    /// Unlock with quick unlock when it is on and still valid, otherwise with the master password and
    /// Secret Key.
    pub fn open() -> Res<Session> {
        Self::open_with(false).map(|(s, _)| s)
    }

    /// Always asks for the master password and Secret Key, and hands back the Account Unlock Key for
    /// `figlock quick-unlock enable`.
    pub fn open_full() -> Res<(Session, Key)> {
        Self::open_with(true)
    }

    fn open_with(force_full: bool) -> Res<(Session, Key)> {
        let cfg = config::load()?;
        let store = crate::store::open(&cfg.store)?;
        let vault_json = read_vault_json(store.as_ref())?;
        let mut records = Vec::new();
        for e in store.list("devices/")? {
            if let Ok(bytes) = store.get(&e.path) {
                records.push(bytes);
            }
        }
        let vault_id = vault_id_of(&vault_json)?;
        let quick = quick::system()?;
        let now = now_ms();
        // The outer error is a problem with this device's files; the inner one is a key that does not open
        // the vault.
        let unlock = |auk: &Key| -> Res<Result<_, figlock_core::Error>> {
            let device = config::load_device()?;
            Ok(Vault::unlock_with_key(
                &vault_json,
                auk,
                device,
                &cfg.device_name,
                &records,
                now,
            ))
        };

        let mut quick_unlocked = None;
        if !force_full {
            match quick.fetch(&vault_id, now / 1000)? {
                quick::Fetch::Key(auk) => match unlock(&auk)? {
                    Ok(done) => quick_unlocked = Some((done, auk)),
                    Err(_) => {
                        // The kept key no longer opens the vault: the password or Secret Key changed.
                        quick.forget(&vault_id)?;
                        eprintln!("quick unlock is off because the vault's keys changed");
                    }
                },
                quick::Fetch::Full(quick::Fallback::Expired(why)) => {
                    eprintln!("{why}; enter the master password");
                }
                quick::Fetch::Full(_) => {}
            }
        }
        let ((vault, new_record), auk) = match quick_unlocked {
            Some(done) => done,
            None => {
                let password = prompt::password()?;
                let secret_key = prompt::secret_key()?;
                let auk = Vault::unlock_key(&vault_json, &password, &secret_key)
                    .map_err(|_| wrong_credentials())?;
                let done = unlock(&auk)?.map_err(|_| wrong_credentials())?;
                if let Err(e) = quick.remember(&vault_id, &auk, now / 1000) {
                    eprintln!("warning: quick unlock was not refreshed: {e}");
                }
                (done, auk)
            }
        };
        if let Some(record) = new_record {
            let path = format!("devices/{}.json", hex::encode(vault.device_id()));
            match store.put_new(&path, &record) {
                Ok(()) | Err(StoreError::Exists) => {}
                Err(e) => return Err(e.into()),
            }
        }
        let mut session = Session { vault, store };
        session.load_log(now)?;
        Ok((session, auk))
    }

    /// Apply every op file, per device in order. A file that does not verify, or a gap, stops that device
    /// (`sync.md` 8) and is reported; the other devices are unaffected.
    fn load_log(&mut self, now: u64) -> Res<()> {
        let mut by_device: BTreeMap<DeviceId, Vec<(u64, String, bool)>> = BTreeMap::new();
        for e in self.store.list("log/")? {
            if let Some((device, seq)) = parse_log_path(&e.path) {
                by_device
                    .entry(device)
                    .or_default()
                    .push((seq, e.path, e.available));
            }
        }
        for (device, mut files) in by_device {
            files.sort();
            for (seq, path, available) in files {
                let bytes = if available {
                    self.store.get(&path).ok()
                } else {
                    None
                };
                let Some(bytes) = bytes else {
                    eprintln!(
                        "warning: op {seq} from device {} is not available yet; later changes from it wait",
                        short(&device)
                    );
                    break;
                };
                if self.vault.apply_file(&bytes, &device, seq, now).is_err() {
                    eprintln!(
                        "warning: ignoring changes from device {} from op {seq} on: the file failed verification or is out of order",
                        short(&device)
                    );
                    break;
                }
            }
        }
        Ok(())
    }

    /// Write pending changes as one op file.
    pub fn commit(&mut self) -> Res<()> {
        let Some(c) = self.vault.commit()? else {
            return Ok(());
        };
        match self.store.put_new(&c.path, &c.bytes) {
            Ok(()) => Ok(()),
            Err(StoreError::Exists) => Err(Fail::new(
                "another change was written at the same moment; run the command again",
            )),
            Err(e) => Err(e.into()),
        }
    }

    pub fn items(&mut self, include_deleted: bool) -> Vec<ItemView> {
        let (items, corrupt) = self.vault.items(include_deleted);
        if !corrupt.is_empty() {
            eprintln!(
                "warning: {} item(s) could not be read and are hidden",
                corrupt.len()
            );
        }
        items
    }

    /// An item by full id, unique id prefix (4+ hex digits) or a search that finds exactly one item.
    pub fn resolve(&mut self, reference: &str, include_deleted: bool) -> Res<ItemView> {
        let items = self.items(include_deleted);
        let r = reference.trim().to_ascii_lowercase();
        if r.len() >= 4 && r.bytes().all(|b| b.is_ascii_hexdigit()) {
            let mut hits = items.iter().filter(|i| id_hex(&i.id).starts_with(&r));
            if let Some(first) = hits.next() {
                if hits.next().is_some() {
                    return Err(Fail::new(
                        "that id prefix matches several items; type more of it",
                    ));
                }
                return self.take(first.id);
            }
        }
        let index = Index::build(items.iter());
        let found = index.search(reference, now_ms(), 5);
        match found.as_slice() {
            [] => Err(Fail::new("no item matches")),
            [one] => self.take(one.id),
            many => {
                let mut msg = String::from("several items match; use an id:");
                for hit in many {
                    if let Some(i) = items.iter().find(|i| i.id == hit.id) {
                        msg.push_str(&format!(
                            "\n  {}  {}",
                            &id_hex(&i.id)[..8],
                            crate::render::clean(title_of(i))
                        ));
                    }
                }
                Err(Fail(msg))
            }
        }
    }

    fn take(&mut self, id: ItemId) -> Res<ItemView> {
        Ok(self.vault.item(&id)?)
    }
}

pub fn title_of(item: &ItemView) -> &str {
    item.fields
        .get("title")
        .map_or("(untitled)", |t| t.as_str())
}
