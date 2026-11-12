//! An unlocked vault plus its store: load every op file, resolve item references, write changes back.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use figlock_core::{DeviceId, Index, ItemId, ItemView, Key, Revocation, SecretKey, Vault};
use figlock_store::{BlobStore, StoreError};
use zeroize::Zeroizing;

use crate::config;
use crate::error::{Fail, Res};
use crate::quick;

/// The master password and Secret Key a full unlock was given.
type Credentials = (Zeroizing<String>, SecretKey);

/// How a front end asks for the credentials and reports what it noticed. Nothing here is a secret.
pub trait Ui: Send {
    fn password(&self) -> Res<Zeroizing<String>>;
    fn secret_key(&self) -> Res<SecretKey>;
    /// Something worth knowing that is not a problem.
    fn notice(&self, message: &str);
    /// Something went wrong that did not stop the work.
    fn warn(&self, message: &str);
}

pub struct Session {
    pub vault: Vault,
    store: Box<dyn BlobStore>,
    ui: Box<dyn Ui>,
    uncertain: bool,
}

/// The vault name every item is added under, until a device holds more than one vault.
pub const VAULT_NAME: &str = "personal";

/// Replace control characters (escape sequences, bells, carriage returns) except newline and tab.
pub fn clean(s: &str) -> Cow<'_, str> {
    if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        Cow::Owned(
            s.chars()
                .map(|c| {
                    if c.is_control() && c != '\n' && c != '\t' {
                        '?'
                    } else {
                        c
                    }
                })
                .collect(),
        )
    } else {
        Cow::Borrowed(s)
    }
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
    crate::sync::fill_local(&cfg.place)?;
    let store = crate::store::open(&cfg.place)?;
    vault_id_of(&read_vault_json(store.as_ref())?)
}

fn wrong_credentials() -> Fail {
    Fail::new("could not unlock: wrong master password or Secret Key, or the vault file is damaged")
}

/// `vault_id` from `vault.json`, which names this vault's quick-unlock entry.
pub fn vault_id_of(vault_json: &[u8]) -> Res<String> {
    serde_json::from_slice::<serde_json::Value>(vault_json)
        .ok()
        .and_then(|v| v["vault_id"].as_str().map(str::to_owned))
        .ok_or_else(|| Fail::new("vault.json is damaged"))
}

impl Session {
    /// Unlock with quick unlock when it is on and still valid, otherwise with the master password and
    /// Secret Key.
    pub fn open(ui: Box<dyn Ui>) -> Res<Session> {
        Self::open_with(ui, false).map(|(s, _, _)| s)
    }

    /// Always asks for the master password and Secret Key, and hands back the Account Unlock Key for
    /// `figlock quick-unlock enable`.
    pub fn open_full(ui: Box<dyn Ui>) -> Res<(Session, Key)> {
        Self::open_with(ui, true).map(|(s, auk, _)| (s, auk))
    }

    /// Always asks for the master password and Secret Key and hands them back, for the commands that change
    /// the credentials (`Vault::change_credentials` checks them again against the wrapped `VK`).
    pub fn open_for_change(ui: Box<dyn Ui>) -> Res<(Session, Zeroizing<String>, SecretKey)> {
        let (session, _, creds) = Self::open_with(ui, true)?;
        let (password, secret_key) =
            creds.ok_or_else(|| Fail::new("the unlock did not ask for credentials"))?;
        Ok((session, password, secret_key))
    }

    fn open_with(ui: Box<dyn Ui>, force_full: bool) -> Res<(Session, Key, Option<Credentials>)> {
        let cfg = config::load()?;
        crate::sync::fill_local(&cfg.place)?;
        let store = crate::store::open(&cfg.place)?;
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
            match quick.fetch_or_full(&vault_id, now / 1000, ui.as_ref()) {
                quick::Fetch::Key(auk) => match unlock(&auk)? {
                    Ok(done) => quick_unlocked = Some((done, auk)),
                    Err(_) => {
                        // The kept key no longer opens the vault: the password or Secret Key changed.
                        match quick.forget(&vault_id) {
                            Ok(()) => {
                                ui.notice("quick unlock is off because the vault's keys changed")
                            }
                            Err(e) => {
                                ui.warn(&format!("quick unlock key was not removed: {e}"));
                            }
                        }
                    }
                },
                quick::Fetch::Full(quick::Fallback::Expired(why)) => {
                    ui.notice(&format!("{why}; enter the master password"));
                }
                quick::Fetch::Full(_) => {}
            }
        }
        let mut creds = None;
        let ((vault, new_record), auk) = match quick_unlocked {
            Some(done) => done,
            None => {
                let password = ui.password()?;
                let secret_key = ui.secret_key()?;
                let auk = Vault::unlock_key(&vault_json, &password, &secret_key)
                    .map_err(|_| wrong_credentials())?;
                let done = unlock(&auk)?.map_err(|_| wrong_credentials())?;
                if let Err(e) = quick.remember(&vault_id, &auk, now / 1000) {
                    ui.warn(&format!("quick unlock was not refreshed: {e}"));
                }
                creds = Some((password, secret_key));
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
        let mut session = Session {
            vault,
            store,
            ui,
            uncertain: false,
        };
        session.load_log(now)?;
        Ok((session, auk, creds))
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
                    self.ui.warn(&format!(
                        "op {seq} from device {} is not available yet; later changes from it wait",
                        short(&device)
                    ));
                    break;
                };
                if self.vault.apply_file(&bytes, &device, seq, now).is_err() {
                    self.ui.warn(&format!(
                        "ignoring changes from device {} from op {seq} on: the file failed verification or is out of order",
                        short(&device)
                    ));
                    break;
                }
            }
        }
        Ok(())
    }

    /// Replace `vault.json` (a credential change).
    pub fn replace_vault_json(&self, bytes: &[u8]) -> Res<()> {
        self.store.put_replace("vault.json", bytes)?;
        Ok(())
    }

    /// Store what a revocation produced, `vault.json` last so a device that sees the new epoch finds the
    /// snapshot and its record already there.
    pub fn write_revocation(&self, r: &Revocation) -> Res<()> {
        match self.store.put_new(&r.snapshot.path, &r.snapshot.bytes) {
            Ok(()) | Err(StoreError::Exists) => {}
            Err(e) => return Err(e.into()),
        }
        for record in &r.records {
            self.store.put_replace(&record.path, &record.bytes)?;
        }
        for path in &r.remove {
            match self.store.delete(path) {
                Ok(()) | Err(StoreError::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.store.put_replace("vault.json", &r.vault_json)?;
        Ok(())
    }

    /// Write pending changes as one op file. If the file definitely was not written the commit is taken back,
    /// so the changes are pending again: call `commit` to retry or `discard_pending` to give them up. If a
    /// failed write turns out to have landed, the commit counts as done. If that cannot be told, the session
    /// is `uncertain` and refuses to commit again: unlock again to resync.
    pub fn commit(&mut self) -> Res<()> {
        if self.uncertain {
            return Err(Self::uncertain_error());
        }
        let Some(c) = self.vault.commit()? else {
            return Ok(());
        };
        let Err(e) = self.store.put_new(&c.path, &c.bytes) else {
            self.vault.confirm_commit();
            return Ok(());
        };
        match self.store.get(&c.path) {
            Ok(bytes) if bytes == c.bytes => {
                self.vault.confirm_commit();
                Ok(())
            }
            Ok(_) | Err(StoreError::NotFound) => {
                self.vault.rollback_commit();
                match e {
                    StoreError::Exists => Err(Fail::new(
                        "another change was written at the same moment; run the command again",
                    )),
                    e => Err(e.into()),
                }
            }
            Err(_) => {
                self.uncertain = true;
                Err(Self::uncertain_error())
            }
        }
    }

    /// True once a save failed in a way that leaves it unknown whether it was written. Drop the session.
    pub fn is_uncertain(&self) -> bool {
        self.uncertain
    }

    fn uncertain_error() -> Fail {
        Fail::new(
            "the last save may or may not have been written; lock and unlock again to find out",
        )
    }

    /// Give up every change that has not been written, as if it had not been made.
    pub fn discard_pending(&mut self) {
        self.vault.discard_pending();
    }

    pub fn items(&mut self, include_deleted: bool) -> Vec<ItemView> {
        let (items, corrupt) = self.vault.items(include_deleted);
        if !corrupt.is_empty() {
            self.ui.warn(&format!(
                "{} item(s) could not be read and are hidden",
                corrupt.len()
            ));
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
                            clean(title_of(i))
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use figlock_core::{ItemType, KdfParams};
    use figlock_store::Entry;

    use super::*;

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        WriteThenFail,
        FailWithoutWriting,
        FailEverything,
    }

    #[derive(Clone)]
    struct Flaky {
        files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
        mode: Arc<Mutex<Option<Mode>>>,
    }

    impl BlobStore for Flaky {
        fn get(&self, path: &str) -> Result<Vec<u8>, StoreError> {
            if *self.mode.lock().unwrap() == Some(Mode::FailEverything) {
                return Err(StoreError::Unavailable);
            }
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or(StoreError::NotFound)
        }
        fn put_new(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
            let mode = *self.mode.lock().unwrap();
            let mut files = self.files.lock().unwrap();
            if files.contains_key(path) {
                return Err(StoreError::Exists);
            }
            match mode {
                Some(Mode::FailWithoutWriting | Mode::FailEverything) => {
                    Err(StoreError::Unavailable)
                }
                Some(Mode::WriteThenFail) => {
                    files.insert(path.to_owned(), bytes.to_vec());
                    Err(StoreError::Unavailable)
                }
                None => {
                    files.insert(path.to_owned(), bytes.to_vec());
                    Ok(())
                }
            }
        }
        fn put_replace(&self, path: &str, bytes: &[u8]) -> Result<(), StoreError> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_owned(), bytes.to_vec());
            Ok(())
        }
        fn list(&self, _prefix: &str) -> Result<Vec<Entry>, StoreError> {
            Ok(Vec::new())
        }
        fn delete(&self, path: &str) -> Result<(), StoreError> {
            self.files.lock().unwrap().remove(path);
            Ok(())
        }
    }

    struct NoUi;
    impl Ui for NoUi {
        fn password(&self) -> Res<Zeroizing<String>> {
            Err(Fail::new("no"))
        }
        fn secret_key(&self) -> Res<SecretKey> {
            Err(Fail::new("no"))
        }
        fn notice(&self, _: &str) {}
        fn warn(&self, _: &str) {}
    }

    fn session(mode: Option<Mode>) -> (Session, Flaky) {
        // A password made for this run, never a literal.
        let pw = SecretKey::random().unwrap().format().to_string();
        let (vault, _) = Vault::create(&pw, "Laptop", KdfParams::FLOOR, 1).unwrap();
        let store = Flaky {
            files: Arc::default(),
            mode: Arc::new(Mutex::new(mode)),
        };
        let s = Session {
            vault,
            store: Box::new(store.clone()),
            ui: Box::new(NoUi),
            uncertain: false,
        };
        (s, store)
    }

    fn add(s: &mut Session, title: &str) {
        s.vault
            .add_item(ItemType::Login, VAULT_NAME, &[("title", title)], 2)
            .unwrap();
    }

    fn op_files(store: &Flaky) -> Vec<String> {
        store.files.lock().unwrap().keys().cloned().collect()
    }

    #[test]
    fn a_write_that_landed_despite_the_error_counts_as_saved() {
        let (mut s, store) = session(Some(Mode::WriteThenFail));
        add(&mut s, "One");
        s.commit().unwrap();
        assert!(!s.is_uncertain());
        *store.mode.lock().unwrap() = None;
        add(&mut s, "Two");
        s.commit().unwrap();
        let files = op_files(&store);
        assert!(files[0].ends_with("00000001.op"));
        assert!(files[1].ends_with("00000002.op"));
        s.vault.rollback_commit();
        assert!(!s.vault.has_pending());
    }

    #[test]
    fn a_write_that_did_not_land_is_taken_back_and_retried_with_the_same_sequence() {
        let (mut s, store) = session(Some(Mode::FailWithoutWriting));
        add(&mut s, "One");
        assert!(s.commit().is_err());
        assert!(!s.is_uncertain());
        assert!(s.vault.has_pending());
        *store.mode.lock().unwrap() = None;
        s.commit().unwrap();
        let files = op_files(&store);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("00000001.op"));
    }

    #[test]
    fn when_the_read_back_fails_too_the_session_refuses_further_commits() {
        let (mut s, store) = session(Some(Mode::FailEverything));
        add(&mut s, "One");
        assert!(s.commit().is_err());
        assert!(s.is_uncertain());
        *store.mode.lock().unwrap() = None;
        add(&mut s, "Two");
        assert!(s.commit().is_err());
        assert!(op_files(&store).len() <= 1);
    }
}
