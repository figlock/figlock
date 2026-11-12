//! The desktop's vault state: whether it is set up, locked or unlocked, and the steps that get it there. No
//! window code, so the whole lifecycle is tested without one. Keys and plaintext stay inside [`Session`]; what
//! leaves this module is what a screen shows (ADR-0020).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use figlock_core::{
    Created, DeviceKeys, KdfParams, KitInfo, SecretKey, Vault, render_kit, verify_kit_groups,
};
use figlock_session::config::{self, Config, Place, Remote};
use figlock_session::error::{Fail, Res};
use figlock_session::session::{Session, Ui, now_ms};
use figlock_session::{join, quick};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The idle time before the vault locks, until Settings (a later task) lets the person change it.
pub const DEFAULT_AUTOLOCK: Duration = Duration::from_secs(5 * 60);
/// The shortest master password a new vault accepts; the Secret Key carries the rest of the strength.
pub const MIN_PASSWORD_CHARS: usize = 8;
/// How long a copied field stays on the clipboard before it is cleared.
pub const CLIPBOARD_CLEAR: Duration = Duration::from_secs(30);
/// A gap this long between two ticks of the lock timer means the Mac slept: lock as it wakes.
pub const SLEEP_GAP: Duration = Duration::from_secs(20);

/// Why a command failed, in a form the window can branch on.
#[derive(Serialize, Debug)]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
}

impl CmdError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        CmdError {
            code,
            message: message.into(),
        }
    }
}

impl From<Fail> for CmdError {
    fn from(f: Fail) -> Self {
        CmdError::new("failed", f.0)
    }
}

pub type CmdRes<T> = Result<T, CmdError>;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VaultState {
    /// `none` (nothing set up on this device), `locked` or `unlocked`.
    pub phase: &'static str,
    pub version: &'static str,
    /// Touch ID is turned on for this vault, whether or not a key is kept right now.
    pub quick_unlock: bool,
    /// This Mac can ask for Touch ID or the device password.
    pub touch_available: bool,
    pub autolock_secs: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CreatedVault {
    /// Shown once so the person can write it down; also what the Emergency Kit holds.
    pub secret_key: String,
}

/// A vault made in memory that is only written to storage once the person has saved the Secret Key.
struct Creating {
    vault: Vault,
    created: Created,
    password: Zeroizing<String>,
    place: Place,
    device_name: String,
    /// The person typed the end of the Secret Key back correctly, so it was seen and saved.
    key_confirmed: bool,
}

struct Inner {
    session: Option<Session>,
    /// List rows and the search index of the unlocked vault; rebuilt after any change.
    cache: Option<items::Cache>,
    /// Files chosen for a document item, until it is saved or the editor closes.
    staged: std::collections::HashMap<String, items::StagedData>,
    /// What the last copy left on the clipboard.
    copied: Option<items::Copied>,
    creating: Option<Creating>,
    joining: Option<join::Join>,
    /// A save happened since the last sync tick, so the next tick syncs soon instead of waiting its turn.
    dirty: bool,
    active_at: Instant,
    ticked_at: Instant,
}

pub struct Desk {
    inner: Mutex<Inner>,
    sync: Mutex<sync::SyncState>,
    kdf: KdfParams,
    autolock: Duration,
    clipboard_clear: Duration,
}

impl Inner {
    /// Drop the unlocked vault and everything read from it, a copied secret included. Keys and secret values
    /// are zeroed as they drop.
    fn drop_session(&mut self) {
        if let Some(c) = self.copied.take() {
            c.clear();
        }
        self.session = None;
        self.cache = None;
        self.staged.clear();
    }
}

/// Answers a full unlock with what the person typed.
struct Typed {
    password: Zeroizing<String>,
    secret_key: SecretKey,
}

impl Ui for Typed {
    fn password(&self) -> Res<Zeroizing<String>> {
        Ok(self.password.clone())
    }
    fn secret_key(&self) -> Res<SecretKey> {
        Ok(SecretKey::from_bytes(*self.secret_key.expose()))
    }
    fn notice(&self, _: &str) {}
    fn warn(&self, _: &str) {}
}

/// Answers nothing: a quick unlock that cannot finish on its own falls back to the unlock form.
struct NoCredentials {
    why: Arc<Mutex<Option<String>>>,
}

impl Ui for NoCredentials {
    fn password(&self) -> Res<Zeroizing<String>> {
        Err(Fail::new("needs credentials"))
    }
    fn secret_key(&self) -> Res<SecretKey> {
        Err(Fail::new("needs credentials"))
    }
    fn notice(&self, message: &str) {
        if let Ok(mut why) = self.why.lock() {
            *why = Some(message.to_owned());
        }
    }
    fn warn(&self, _: &str) {}
}

fn wrong_credentials() -> CmdError {
    CmdError::new(
        "wrong_credentials",
        "That master password and Secret Key do not match this vault. Check both and try again. Your Secret Key is in your Emergency Kit.",
    )
}

fn parse_secret_key(text: &str) -> CmdRes<SecretKey> {
    SecretKey::parse(text).map_err(|_| {
        CmdError::new(
            "bad_secret_key",
            "This Secret Key has a typo or is incomplete. It starts with FL1 and has eight groups of four letters and numbers.",
        )
    })
}

/// Today as `YYYY-MM-DD` (UTC), from the clock the app already reads.
pub fn civil_date(now_ms: u64) -> String {
    let z = i64::try_from(now_ms / 86_400_000).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Whether the lock timer should lock now: idle for the whole timeout, or the clock jumped (sleep).
pub fn should_lock(idle: Duration, since_last_tick: Duration, autolock: Duration) -> bool {
    idle >= autolock || since_last_tick >= SLEEP_GAP
}

impl Desk {
    pub fn new() -> Self {
        // `FIGLOCK_AUTOLOCK_SECS` can only shorten the timeout, for tests (like `FIGLOCK_QUICK_IDLE_SECS`).
        let autolock = std::env::var("FIGLOCK_AUTOLOCK_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(DEFAULT_AUTOLOCK, |s| {
                Duration::from_secs(s).min(DEFAULT_AUTOLOCK)
            });
        let clipboard_clear = std::env::var("FIGLOCK_CLIPBOARD_CLEAR_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(CLIPBOARD_CLEAR, |s| {
                Duration::from_secs(s).min(CLIPBOARD_CLEAR)
            });
        Self::with(KdfParams::DEFAULT, autolock, clipboard_clear)
    }

    pub fn with(kdf: KdfParams, autolock: Duration, clipboard_clear: Duration) -> Self {
        let now = Instant::now();
        Desk {
            inner: Mutex::new(Inner {
                session: None,
                cache: None,
                staged: std::collections::HashMap::new(),
                copied: None,
                creating: None,
                joining: None,
                dirty: false,
                active_at: now,
                ticked_at: now,
            }),
            sync: Mutex::new(sync::SyncState::new()),
            kdf,
            autolock,
            clipboard_clear,
        }
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn state(&self) -> CmdRes<VaultState> {
        let unlocked = self.inner().session.is_some();
        let initialised = config::is_initialised()?;
        let phase = match (initialised, unlocked) {
            (false, _) => "none",
            (true, false) => "locked",
            (true, true) => "unlocked",
        };
        let (quick_unlock, touch_available) = match quick::system() {
            Ok(q) => (
                initialised
                    && figlock_session::session::peek_vault_id()
                        .map(|v| q.status(&v).0)
                        .unwrap_or(false),
                q.available(),
            ),
            Err(_) => (false, false),
        };
        Ok(VaultState {
            phase,
            version: env!("CARGO_PKG_VERSION"),
            quick_unlock,
            touch_available,
            autolock_secs: self.autolock.as_secs(),
        })
    }

    /// Where a new vault goes unless the person picks a folder: iCloud Drive when it is there, so the vault
    /// syncs, otherwise a folder in Documents.
    pub fn suggested_folder() -> Option<PathBuf> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
        Some(if icloud.is_dir() {
            icloud.join("FigLock")
        } else {
            home.join("Documents/FigLock")
        })
    }

    /// The Mac's own name for the device list.
    pub fn device_name() -> String {
        std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().chars().take(40).collect::<String>())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "This computer".to_owned())
    }

    // ---- create ----

    /// Make a vault in memory. Nothing is written until [`Desk::finish_create`].
    pub fn create(&self, storage: &Storage, password: &str) -> CmdRes<CreatedVault> {
        if config::is_initialised()? {
            return Err(CmdError::new(
                "failed",
                "This device already has a vault set up.",
            ));
        }
        if password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(CmdError::new(
                "weak_password",
                "Use at least 8 characters for the master password.",
            ));
        }
        let place = storage_place(storage, true)?;
        // Only a definite "not there" lets this continue: an unreachable folder must not look empty.
        if figlock_session::store::vault_exists(&place)? {
            return Err(CmdError::new(
                "vault_exists",
                format!(
                    "A vault already exists in {}. Open it instead.",
                    storage.there()
                ),
            ));
        }
        let device_name = Self::device_name();
        let (vault, created) =
            Vault::create(password, &device_name, self.kdf, now_ms()).map_err(Fail::from)?;
        let secret_key = created.secret_key.format().to_string();
        self.inner().creating = Some(Creating {
            vault,
            created,
            password: Zeroizing::new(password.to_owned()),
            place,
            device_name,
            key_confirmed: false,
        });
        Ok(CreatedVault { secret_key })
    }

    /// The Emergency Kit PDF for the vault being created, to be written where the person chose.
    pub fn emergency_kit(&self) -> CmdRes<Zeroizing<Vec<u8>>> {
        let inner = self.inner();
        let c = inner.creating.as_ref().ok_or_else(nothing_to_do)?;
        let storage = match c.place.remote {
            Some(Remote::Drive) => "Google Drive".to_owned(),
            // The kit says what the vault is stored in, not where on this disk: a long path would not fit.
            None => {
                let name = c
                    .place
                    .local
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                format!("Folder {}", name.chars().take(60).collect::<String>())
            }
        };
        let info = KitInfo {
            vault_name: "Personal",
            storage: &storage,
            created: &civil_date(now_ms()),
        };
        render_kit(&info, &c.created.secret_key)
            .map_err(|_| CmdError::new("failed", "Could not make the Emergency Kit."))
    }

    /// The forced check that the Secret Key was really saved: the last two groups, typed back.
    pub fn check_kit_key(&self, typed: &str) -> CmdRes<bool> {
        let mut inner = self.inner();
        let c = inner.creating.as_mut().ok_or_else(nothing_to_do)?;
        let ok = verify_kit_groups(&c.created.secret_key, typed);
        c.key_confirmed |= ok;
        Ok(ok)
    }

    pub fn cancel_create(&self) {
        self.inner().creating = None;
    }

    /// Write the vault to storage, remember this device and unlock.
    pub fn finish_create(&self, touch_id: bool) -> CmdRes<VaultState> {
        let mut inner = self.inner();
        // Held here, not only by the window: no vault is written until the Secret Key check has passed.
        if !inner
            .creating
            .as_ref()
            .ok_or_else(nothing_to_do)?
            .key_confirmed
        {
            return Err(CmdError::new(
                "key_not_confirmed",
                "Confirm your Secret Key before the vault is created.",
            ));
        }
        let c = inner.creating.take().ok_or_else(nothing_to_do)?;
        drop(inner);
        let store = figlock_session::store::open(&c.place)?;
        let cfg = Config {
            place: c.place.clone(),
            device_name: c.device_name.clone(),
        };
        config::save(&cfg, c.vault.device_keys())?;
        let device_path = format!("devices/{}.json", hex::encode(c.vault.device_id()));
        let mut written: Vec<&str> = Vec::new();
        let result = store
            .put_new("vault.json", &c.created.vault_json)
            .and_then(|()| {
                written.push("vault.json");
                store.put_new(&device_path, &c.created.device_record)
            });
        if let Err(e) = result {
            config::forget();
            for p in written {
                let _ = store.delete(p);
            }
            return Err(Fail::from(e).into());
        }
        // The vault is safe on this device either way; a failed first upload is retried by the next sync.
        if cfg.place.remote.is_some() {
            let _ = figlock_session::sync::run(&cfg, false);
        }
        let key = SecretKey::from_bytes(*c.created.secret_key.expose());
        self.enter(
            Typed {
                password: c.password,
                secret_key: key,
            },
            touch_id,
        )
    }

    // ---- open an existing vault on this device ----

    /// Open a vault that already exists in `storage` with the master password and Secret Key, on a device that
    /// has not been set up (the recovery path of `pairing-recovery.md` 3.5).
    pub fn open_existing(
        &self,
        storage: &Storage,
        password: &str,
        secret_key: &str,
        touch_id: bool,
    ) -> CmdRes<VaultState> {
        if config::is_initialised()? {
            return Err(CmdError::new(
                "failed",
                "This device already has a vault set up.",
            ));
        }
        let key = parse_secret_key(secret_key)?;
        let place = storage_place(storage, false)?;
        // Downloads the vault into a new local copy when it lives in Drive.
        figlock_session::sync::open_from_remote(&place).map_err(Fail::from)?;
        let store = figlock_session::store::open(&place)?;
        let vault_json = store.get("vault.json").map_err(|e| match e {
            figlock_store::StoreError::NotFound => CmdError::new(
                "no_vault",
                format!("There is no FigLock vault in {}.", storage.there()),
            ),
            e => Fail::from(e).into(),
        })?;
        Vault::unlock_key(&vault_json, password, &key).map_err(|_| wrong_credentials())?;
        let device = DeviceKeys::generate().map_err(Fail::from)?;
        let cfg = Config {
            place,
            device_name: Self::device_name(),
        };
        config::save(&cfg, &device)?;
        self.enter(
            Typed {
                password: Zeroizing::new(password.to_owned()),
                secret_key: key,
            },
            touch_id,
        )
        .inspect_err(|_| config::forget())
    }

    // ---- join through pairing ----

    pub fn join_start(&self, storage: &Storage, code: &str) -> CmdRes<String> {
        if config::is_initialised()? {
            return Err(CmdError::new(
                "failed",
                "This device already has a vault set up.",
            ));
        }
        let place = storage_place(storage, false)?;
        let j = join::begin(place, code, &Self::device_name(), now_ms())
            .map_err(|f| CmdError::new("invite", f.0))?;
        let fingerprint = j.fingerprint().to_owned();
        self.inner().joining = Some(j);
        Ok(fingerprint)
    }

    pub fn join_answered(&self) -> CmdRes<bool> {
        let inner = self.inner();
        let j = inner.joining.as_ref().ok_or_else(nothing_to_do)?;
        Ok(j.answered()?)
    }

    pub fn join_cancel(&self) {
        self.inner().joining = None;
    }

    pub fn join_finish(&self, password: &str, touch_id: bool) -> CmdRes<VaultState> {
        let (joined, cfg) = {
            let inner = self.inner();
            let j = inner.joining.as_ref().ok_or_else(nothing_to_do)?;
            j.finish(password, now_ms())
                .map_err(|f| CmdError::new("wrong_credentials", f.0))?
        };
        self.inner().joining = None;
        config::save(&cfg, joined.vault.device_keys())?;
        let typed = Typed {
            password: Zeroizing::new(password.to_owned()),
            secret_key: joined.secret_key,
        };
        self.enter(typed, touch_id)
            .inspect_err(|_| config::forget())
    }

    // ---- unlock and lock ----

    /// Unlock with the master password and Secret Key.
    pub fn unlock(&self, password: &str, secret_key: &str) -> CmdRes<VaultState> {
        if password.is_empty() {
            return Err(CmdError::new(
                "empty_password",
                "Enter your master password.",
            ));
        }
        let key = parse_secret_key(secret_key)?;
        let typed = Typed {
            password: Zeroizing::new(password.to_owned()),
            secret_key: key,
        };
        let (session, _) = Session::open_full(Box::new(typed)).map_err(|f| {
            if f.0.starts_with("could not unlock") {
                wrong_credentials()
            } else {
                f.into()
            }
        })?;
        self.install(session);
        self.state()
    }

    /// Unlock with Touch ID, when it is on and still valid.
    pub fn quick_unlock(&self) -> CmdRes<VaultState> {
        let why = Arc::new(Mutex::new(None));
        let ui = NoCredentials { why: why.clone() };
        match Session::open(Box::new(ui)) {
            Ok(session) => {
                self.install(session);
                self.state()
            }
            Err(f) if f.0 == "needs credentials" => {
                let reason = why.lock().ok().and_then(|w| w.clone());
                Err(CmdError::new(
                    "quick_unavailable",
                    reason.map_or_else(
                        || "Touch ID did not unlock the vault. Type your master password and Secret Key.".to_owned(),
                        |r| format!("{}. Type your master password and Secret Key.", capitalise(&r.replace("; enter the master password", ""))),
                    ),
                ))
            }
            Err(f) => Err(f.into()),
        }
    }

    /// A full unlock with the details just typed, then optionally turn on Touch ID.
    fn enter(&self, typed: Typed, touch_id: bool) -> CmdRes<VaultState> {
        let (session, auk) = Session::open_full(Box::new(typed)).map_err(|f| {
            if f.0.starts_with("could not unlock") {
                wrong_credentials()
            } else {
                f.into()
            }
        })?;
        if touch_id {
            // Touch ID is a convenience: failing to turn it on never undoes the setup.
            if let (Ok(vault), Ok(q)) = (figlock_session::session::peek_vault_id(), quick::system())
            {
                let _ = q.enable(&vault, &auk, now_ms() / 1000);
            }
        }
        self.install(session);
        self.state()
    }

    fn install(&self, session: Session) {
        let mut inner = self.inner();
        inner.drop_session();
        inner.session = Some(session);
        inner.active_at = Instant::now();
        inner.ticked_at = Instant::now();
        drop(inner);
        self.clear_needs_unlock();
    }

    /// Drop the unlocked vault. Its keys are zeroed as it drops.
    pub fn lock(&self) {
        self.inner().drop_session();
        self.clear_clipboard(true);
    }

    /// The window saw the person do something.
    pub fn activity(&self) {
        let mut inner = self.inner();
        if inner.session.is_some() {
            inner.active_at = Instant::now();
        }
    }

    /// Called every second by the lock timer. Returns true when it just locked the vault.
    pub fn tick(&self) -> bool {
        let locked = {
            let mut inner = self.inner();
            let now = Instant::now();
            let gap = now.duration_since(inner.ticked_at);
            inner.ticked_at = now;
            let lock = inner.session.is_some()
                && should_lock(now.duration_since(inner.active_at), gap, self.autolock);
            if lock {
                inner.drop_session();
            }
            lock
        };
        // A copied secret leaves the clipboard when its time is up, and at once when the vault locks.
        self.clear_clipboard(locked);
        locked
    }
}

fn nothing_to_do() -> CmdError {
    CmdError::new("failed", "That step is no longer in progress. Start again.")
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| {
        f.to_uppercase().collect::<String>() + c.as_str()
    })
}

/// Where the person put the vault: a folder on this Mac (the local copy), and whether it syncs to the hidden
/// app folder in their Google Drive. Drive without a folder means the default local folder (ADR-0026).
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub drive: bool,
}

impl Storage {
    /// "that folder" or "this Google Drive", for messages: what a vault is looked for in.
    fn there(&self) -> &'static str {
        if self.drive {
            "this Google Drive"
        } else {
            "that folder"
        }
    }
}

/// Where the local copy lives and what it syncs to.
fn storage_place(storage: &Storage, create: bool) -> CmdRes<Place> {
    let local = match storage.path.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => folder_path(path, create)?,
        _ if storage.drive => {
            let dir = config::default_local_dir()?;
            if create {
                std::fs::create_dir_all(&dir)
                    .map_err(|_| CmdError::new("folder", "Could not make the vault folder."))?;
            }
            dir
        }
        _ => return Err(CmdError::new("folder", "Choose a folder for the vault.")),
    };
    if storage.drive && config::load_drive_token()?.is_none() {
        return Err(CmdError::new("drive_sign_in", "Sign in to Google first."));
    }
    Ok(Place {
        local,
        remote: storage.drive.then_some(Remote::Drive),
    })
}

/// A vault folder the person chose; a new vault's folder is made if it does not exist yet.
fn folder_path(folder: &str, create: bool) -> CmdRes<PathBuf> {
    let path = Path::new(folder.trim());
    if folder.trim().is_empty() || !path.is_absolute() {
        return Err(CmdError::new("folder", "Choose a folder for the vault."));
    }
    let unusable = || CmdError::new("folder", "That folder cannot be used. Choose another.");
    if create {
        std::fs::create_dir_all(path).map_err(|_| unusable())?;
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| unusable())?;
    Ok(canonical)
}

pub mod drive;
pub mod generate;
pub mod items;
pub mod sync;

#[cfg(test)]
mod item_tests;
#[cfg(test)]
mod tests;
