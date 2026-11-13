//! An unlocked vault plus its store: load every op file, resolve item references, write changes back.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use figlock_core::{
    Approval, DeviceId, Index, Invite, InviteOffer, ItemId, ItemView, JoinRequest, Key, Revocation,
    SecretKey, Vault,
};
use figlock_store::{BlobStore, StoreError};
use zeroize::Zeroizing;

use crate::config;
use crate::error::{Fail, Res};
use crate::quick;
use crate::secret_key_store::{self, SecretKeyStore};

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

/// What [`Session::refresh`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refreshed {
    /// Op files applied.
    pub applied: usize,
    /// `vault.json` changed on another device: lock and unlock to take it in.
    pub settings_changed: bool,
    /// Some device's changes could not be applied yet (a gap or a file that failed verification).
    pub stopped: bool,
}

pub struct Session {
    pub vault: Vault,
    store: Box<dyn BlobStore>,
    ui: Box<dyn Ui>,
    uncertain: bool,
}

/// What [`Session::preview`] found.
pub struct Preview {
    /// Live items (not deleted).
    pub items: usize,
    /// The newest change to any item, in milliseconds.
    pub last_change_ms: Option<u64>,
    /// Other devices that can still open the vault, not counting the one doing the preview.
    pub devices: usize,
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

/// The Secret Key, the Account Unlock Key and the opened vault for a full unlock. The key kept on this device
/// is tried first, after Touch ID or the device password. A key only counts when `open` accepts the key
/// derived from it: deriving one never fails, so a stale key would otherwise look fine. If nothing is kept, or
/// the kept key does not open the vault (the Secret Key was rotated on another device, or the password is
/// wrong, which cannot be told apart), the person types it, and what opened the vault is kept for next time
/// (ADR-0027). A kept key that fails is never deleted here: the password may simply have been mistyped.
fn credentials<B: quick::Backend, T>(
    vault_json: &[u8],
    vault: &str,
    password: &str,
    keys: &SecretKeyStore<B>,
    ui: &dyn Ui,
    open: &dyn Fn(&Key) -> Res<Result<T, figlock_core::Error>>,
) -> Res<(SecretKey, Key, T)> {
    let kept = keys
        .get(vault, "unlock your FigLock vault")
        .unwrap_or_else(|e| {
            ui.warn(&format!(
                "the Secret Key kept on this device was not read: {e}"
            ));
            None
        });
    if let Some(secret_key) = kept {
        let auk = Vault::unlock_key(vault_json, password, &secret_key)
            .map_err(|_| wrong_credentials())?;
        if let Ok(opened) = open(&auk)? {
            return Ok((secret_key, auk, opened));
        }
        ui.notice(
            "the Secret Key kept on this device did not open the vault; if your master password is right, \
             the Secret Key was changed, so enter the new one from your Emergency Kit",
        );
    }
    let secret_key = ui.secret_key()?;
    let auk =
        Vault::unlock_key(vault_json, password, &secret_key).map_err(|_| wrong_credentials())?;
    let opened = open(&auk)?.map_err(|_| wrong_credentials())?;
    if keys.available()
        && let Err(e) = keys.put(vault, &secret_key)
    {
        ui.warn(&format!("the Secret Key was not kept on this device: {e}"));
    }
    Ok((secret_key, auk, opened))
}

impl Session {
    /// A session over a vault already unlocked and the store it was read from.
    pub fn from_parts(vault: Vault, store: Box<dyn BlobStore>, ui: Box<dyn Ui>) -> Session {
        Session {
            vault,
            store,
            ui,
            uncertain: false,
        }
    }

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
                let keys = secret_key_store::system()?;
                let (secret_key, auk, done) = credentials(
                    &vault_json,
                    &vault_id,
                    &password,
                    &keys,
                    ui.as_ref(),
                    &unlock,
                )?;
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

    /// Apply every op file not applied yet, per device in order. A file that does not verify, or a gap, stops
    /// that device (`sync.md` 8) and is reported; the other devices are unaffected. Returns how many files
    /// were applied, and whether any device was stopped.
    fn load_log(&mut self, now: u64) -> Res<(usize, bool)> {
        let mut by_device: BTreeMap<DeviceId, Vec<(u64, String, bool)>> = BTreeMap::new();
        for e in self.store.list("log/")? {
            if let Some((device, seq)) = parse_log_path(&e.path) {
                by_device
                    .entry(device)
                    .or_default()
                    .push((seq, e.path, e.available));
            }
        }
        let (mut applied, mut stopped) = (0, false);
        for (device, mut files) in by_device {
            files.sort();
            let done = self.vault.applied_seq(&device);
            for (seq, path, available) in files {
                if seq <= done {
                    continue;
                }
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
                    stopped = true;
                    break;
                };
                if self.vault.apply_file(&bytes, &device, seq, now).is_err() {
                    self.ui.warn(&format!(
                        "ignoring changes from device {} from op {seq} on: the file failed verification or is out of order",
                        short(&device)
                    ));
                    stopped = true;
                    break;
                }
                applied += 1;
            }
        }
        Ok((applied, stopped))
    }

    /// Take in what a sync brought into the local folder while the vault is open: new device records and new
    /// op files. Returns `settings_changed` when another device changed `vault.json` (a password change,
    /// rotation or revocation), which an open session cannot absorb: lock and unlock to pick it up.
    pub fn refresh(&mut self) -> Res<Refreshed> {
        let vault_json = read_vault_json(self.store.as_ref())?;
        let v: serde_json::Value =
            serde_json::from_slice(&vault_json).map_err(|_| Fail::new("vault.json is damaged"))?;
        let settings_changed = v["revision"].as_u64() != Some(self.vault.revision())
            || v["key_epoch"].as_u64() != Some(u64::from(self.vault.epoch()));
        if settings_changed {
            return Ok(Refreshed {
                applied: 0,
                settings_changed: true,
                stopped: false,
            });
        }
        for e in self.store.list("devices/")? {
            if let Ok(bytes) = self.store.get(&e.path) {
                // Records already known or not valid are not an error here.
                let _ = self.vault.add_device_record(&bytes);
            }
        }
        let (applied, stopped) = self.load_log(now_ms())?;
        Ok(Refreshed {
            applied,
            settings_changed: false,
            stopped,
        })
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

    /// What an opened vault holds, read with its op files applied and nothing changed: for showing a person
    /// what a Recovery Key found before they commit to anything. `vault` is not saved or kept anywhere.
    pub fn preview(vault: Vault, place: &config::Place) -> Res<Preview> {
        struct Silent;
        impl Ui for Silent {
            fn password(&self) -> Res<Zeroizing<String>> {
                Err(Fail::new("no password here"))
            }
            fn secret_key(&self) -> Res<SecretKey> {
                Err(Fail::new("no Secret Key here"))
            }
            fn notice(&self, _: &str) {}
            fn warn(&self, _: &str) {}
        }
        let mut session = Session {
            vault,
            store: crate::store::open(place)?,
            ui: Box::new(Silent),
            uncertain: false,
        };
        session.load_log(now_ms())?;
        let items = session.items(false);
        Ok(Preview {
            items: items.len(),
            last_change_ms: items.iter().map(|i| i.updated.wall).max(),
            // The device the preview made up for itself is not one the vault had.
            devices: {
                let me = *session.vault.device_id();
                session
                    .vault
                    .devices()
                    .iter()
                    .filter(|d| d.revoked_at.is_none() && d.device_id != me)
                    .count()
            },
        })
    }

    /// `vault.json` as stored, for checking typed credentials against it.
    pub fn vault_json(&self) -> Res<Vec<u8>> {
        read_vault_json(self.store.as_ref())
    }

    /// Put an invite's `.inv` where a new device can find it (`pairing-recovery.md` 3.2 step 1).
    pub fn publish_invite(&self, offer: &InviteOffer) -> Res<()> {
        match self.store.put_new(&offer.inv_path, &offer.inv) {
            Ok(()) => Ok(()),
            Err(StoreError::Exists) => Err(Fail::new("that invite already exists; try again")),
            Err(e) => Err(e.into()),
        }
    }

    /// The `.req` a new device left for `invite`, if one is there. A file that does not open under the invite
    /// is deleted, since anyone can write to the inbox and a stranger's file must not block the real request.
    /// A request that opens consumes the invite and is returned.
    pub fn take_request(&self, invite: &mut Invite, now_ms: u64) -> Res<Option<JoinRequest>> {
        let path = invite.req_path();
        let bytes = match self.store.get(&path) {
            Ok(b) => b,
            Err(StoreError::NotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        match invite.open_request(&bytes, now_ms) {
            Ok(req) => Ok(Some(req)),
            Err(_) => {
                self.remove_quietly(&path);
                Ok(None)
            }
        }
    }

    /// Approve `req` and store what that produces: the signed device record first, then the sealed answer, so
    /// the new device finds its record as soon as it can open the answer. The `.inv` and `.req` are cleared.
    pub fn approve_join(
        &mut self,
        invite: &Invite,
        req: &JoinRequest,
        secret_key: &SecretKey,
        now_ms: u64,
    ) -> Res<()> {
        let Approval { record, response } = self
            .vault
            .approve_join(invite, req, secret_key, now_ms)
            .map_err(|_| Fail::new("this invite expired or this device is already in the vault"))?;
        // The vault now lists the device in memory. If a write fails, drop the session (as for an uncertain
        // commit) so that view is not kept.
        let written = self
            .store
            .put_replace(&record.path, &record.bytes)
            .and_then(|()| self.store.put_new(&response.path, &response.bytes));
        if let Err(e) = written {
            self.uncertain = true;
            return Err(e.into());
        }
        self.remove_quietly(&invite.inv_path());
        self.remove_quietly(&invite.req_path());
        Ok(())
    }

    /// Take the `.inv` out of the inbox once a request has opened: nobody else may use the invite.
    pub fn retire_invite(&self, invite: &Invite) {
        self.remove_quietly(&invite.inv_path());
    }

    /// Take an invite's files out of the inbox (it expired, or the person cancelled).
    pub fn close_invite(&self, invite: &Invite) {
        self.remove_quietly(&invite.inv_path());
        self.remove_quietly(&invite.req_path());
    }

    fn remove_quietly(&self, path: &str) {
        let _ = self.store.delete(path);
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

    /// Two devices of one vault over one folder, the way a sync leaves them.
    fn two_devices() -> (tempfile::TempDir, Session, Session, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let folder = move || Box::new(figlock_store::LocalFolder::open(&root).unwrap());
        let pw = SecretKey::random().unwrap().format().to_string();
        let (a, created) = Vault::create(&pw, "A", KdfParams::FLOOR, 1).unwrap();
        folder().put_new("vault.json", &created.vault_json).unwrap();
        let rec_a = format!("devices/{}.json", hex::encode(a.device_id()));
        folder().put_new(&rec_a, &created.device_record).unwrap();
        let auk = Vault::unlock_key(&created.vault_json, &pw, &created.secret_key).unwrap();
        let (b, record) = Vault::unlock_with_key(
            &created.vault_json,
            &auk,
            figlock_core::DeviceKeys::generate().unwrap(),
            "B",
            std::slice::from_ref(&created.device_record),
            2,
        )
        .unwrap();
        let rec_b = format!("devices/{}.json", hex::encode(b.device_id()));
        folder().put_new(&rec_b, &record.unwrap()).unwrap();
        let mk = |vault| Session {
            vault,
            store: folder(),
            ui: Box::new(NoUi),
            uncertain: false,
        };
        (dir, mk(a), mk(b), created.vault_json)
    }

    #[test]
    fn refresh_takes_in_what_another_device_wrote_once() {
        let (_dir, mut a, mut b, _) = two_devices();
        assert_eq!(a.refresh().unwrap().applied, 0);
        add(&mut b, "From B");
        b.commit().unwrap();
        // A learned of B's record only through the folder, as after a sync.
        let r = a.refresh().unwrap();
        assert_eq!(
            (r.applied, r.settings_changed, r.stopped),
            (1, false, false)
        );
        assert!(a.items(false).iter().any(|i| title_of(i) == "From B"));
        assert_eq!(
            a.refresh().unwrap().applied,
            0,
            "applied files are not applied twice"
        );

        add(&mut b, "Second");
        b.commit().unwrap();
        assert_eq!(a.refresh().unwrap().applied, 1);
        assert_eq!(a.items(false).len(), 2);
    }

    #[test]
    fn refresh_says_when_the_vault_settings_changed_elsewhere() {
        let (dir, mut a, _b, vault_json) = two_devices();
        let mut v: serde_json::Value = serde_json::from_slice(&vault_json).unwrap();
        v["revision"] = serde_json::json!(v["revision"].as_u64().unwrap() + 1);
        std::fs::write(dir.path().join("vault.json"), v.to_string()).unwrap();
        let r = a.refresh().unwrap();
        assert!(r.settings_changed);
        assert_eq!(r.applied, 0);
    }

    mod credentials_tests {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        use figlock_biometric::Outcome;

        use super::*;
        use crate::quick::Backend;

        #[derive(Default)]
        struct Shared {
            key: RefCell<Option<[u8; 32]>>,
        }

        #[derive(Clone, Default)]
        struct Fake(Rc<Shared>);

        impl Backend for Fake {
            fn available(&self) -> bool {
                true
            }
            fn confirm(&self, _: &str, _: &str) -> Outcome {
                Outcome::Confirmed
            }
            fn put_key(&self, _: &str, key: &[u8; 32]) -> Res<()> {
                *self.0.key.borrow_mut() = Some(*key);
                Ok(())
            }
            fn get_key(&self, _: &str) -> Res<Option<Zeroizing<[u8; 32]>>> {
                Ok((*self.0.key.borrow()).map(Zeroizing::new))
            }
            fn has_key(&self, _: &str) -> Res<bool> {
                Ok(self.0.key.borrow().is_some())
            }
            fn delete_key(&self, _: &str) -> Res<()> {
                *self.0.key.borrow_mut() = None;
                Ok(())
            }
            fn boot_id(&self) -> String {
                String::new()
            }
        }

        /// Answers the Secret Key question with `typed`, and counts how often it was asked.
        struct Typed {
            typed: Option<SecretKey>,
            asked: Cell<u32>,
            notices: RefCell<Vec<String>>,
        }

        impl Ui for Typed {
            fn password(&self) -> Res<Zeroizing<String>> {
                Err(Fail::new("not used"))
            }
            fn secret_key(&self) -> Res<SecretKey> {
                self.asked.set(self.asked.get() + 1);
                self.typed
                    .as_ref()
                    .map(|k| SecretKey::from_bytes(*k.expose()))
                    .ok_or_else(|| Fail::new("no key typed"))
            }
            fn notice(&self, m: &str) {
                self.notices.borrow_mut().push(m.to_owned());
            }
            fn warn(&self, _: &str) {}
        }

        fn ui(typed: Option<&SecretKey>) -> Typed {
            Typed {
                typed: typed.map(|k| SecretKey::from_bytes(*k.expose())),
                asked: Cell::new(0),
                notices: RefCell::new(Vec::new()),
            }
        }

        struct Rig {
            vault_json: Vec<u8>,
            vault: String,
            pw: String,
            sk: SecretKey,
            device_record: Vec<u8>,
        }

        impl Rig {
            fn creds(
                &self,
                password: &str,
                keys: &SecretKeyStore<Fake>,
                ui: &Typed,
            ) -> Res<(SecretKey, Key, Vault)> {
                let records = [self.device_record.clone()];
                credentials(
                    &self.vault_json,
                    &self.vault,
                    password,
                    keys,
                    ui,
                    &|auk: &Key| {
                        Ok(Vault::unlock_with_key(
                            &self.vault_json,
                            auk,
                            figlock_core::DeviceKeys::generate().unwrap(),
                            "Laptop",
                            &records,
                            2,
                        )
                        .map(|(v, _)| v))
                    },
                )
            }
        }

        fn rig() -> Rig {
            let pw = SecretKey::random().unwrap().format().to_string();
            let (v, created) = Vault::create(&pw, "Laptop", KdfParams::FLOOR, 1).unwrap();
            Rig {
                vault_json: created.vault_json,
                vault: hex::encode(v.vault_id()),
                pw,
                sk: created.secret_key,
                device_record: created.device_record,
            }
        }

        fn kept(fake: &Fake) -> Option<[u8; 16]> {
            (*fake.0.key.borrow()).map(|b| b[..16].try_into().unwrap())
        }

        #[test]
        fn a_kept_key_unlocks_without_asking_for_it() {
            let r = rig();
            let keys = SecretKeyStore::new(Fake::default(), true);
            keys.put(&r.vault, &r.sk).unwrap();
            let ui = ui(None);
            let (sk, _, _) = r.creds(&r.pw, &keys, &ui).unwrap();
            assert_eq!(sk.expose(), r.sk.expose());
            assert_eq!(ui.asked.get(), 0);
        }

        #[test]
        fn a_typed_key_is_kept_for_next_time() {
            let r = rig();
            let fake = Fake::default();
            let keys = SecretKeyStore::new(fake.clone(), true);
            let ui = ui(Some(&r.sk));
            r.creds(&r.pw, &keys, &ui).unwrap();
            assert_eq!(ui.asked.get(), 1);
            assert_eq!(kept(&fake), Some(*r.sk.expose()));
        }

        #[test]
        fn a_stale_kept_key_asks_for_the_new_one_and_replaces_it() {
            let r = rig();
            let fake = Fake::default();
            let keys = SecretKeyStore::new(fake.clone(), true);
            let old = SecretKey::random().unwrap();
            keys.put(&r.vault, &old).unwrap();
            let ui = ui(Some(&r.sk));
            let (sk, _, _) = r.creds(&r.pw, &keys, &ui).unwrap();
            assert_eq!(sk.expose(), r.sk.expose());
            assert_eq!(ui.asked.get(), 1);
            assert_eq!(ui.notices.borrow().len(), 1);
            assert_eq!(kept(&fake), Some(*r.sk.expose()));
        }

        #[test]
        fn a_wrong_password_does_not_delete_or_replace_the_kept_key() {
            let r = rig();
            let fake = Fake::default();
            let keys = SecretKeyStore::new(fake.clone(), true);
            keys.put(&r.vault, &r.sk).unwrap();
            // The person mistyped the password and typed the right key again when asked.
            let ui = ui(Some(&r.sk));
            let wrong = SecretKey::random().unwrap().format().to_string();
            assert!(r.creds(&wrong, &keys, &ui).is_err());
            assert_eq!(kept(&fake), Some(*r.sk.expose()));
        }

        #[test]
        fn a_wrong_typed_key_keeps_nothing() {
            let r = rig();
            let fake = Fake::default();
            let keys = SecretKeyStore::new(fake.clone(), true);
            let ui = ui(Some(&SecretKey::random().unwrap()));
            assert!(r.creds(&r.pw, &keys, &ui).is_err());
            assert_eq!(kept(&fake), None);
        }

        #[test]
        fn a_device_that_cannot_keep_the_key_still_unlocks_by_typing() {
            let r = rig();
            let keys = SecretKeyStore::new(Fake::default(), false);
            let ui = ui(Some(&r.sk));
            let (sk, _, _) = r.creds(&r.pw, &keys, &ui).unwrap();
            assert_eq!(sk.expose(), r.sk.expose());
            assert_eq!(ui.asked.get(), 1);
        }
    }
}
