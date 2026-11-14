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
use figlock_session::{join, quick, secret_key_store};
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
    /// This device keeps the Secret Key, so unlocking asks for the master password only (ADR-0027).
    pub secret_key_kept: bool,
    /// How the kept key is protected (`secure-enclave` or `keychain`), when one is kept.
    pub secret_key_protection: Option<&'static str>,
    /// This unlocked session holds the Secret Key it was unlocked with, so steps that rewrap the vault key
    /// need not ask for it again (ADR-0030). Gone when the vault locks; says nothing about a restart.
    pub secret_key_held: bool,
    pub autolock_secs: u64,
    /// The vault has a Recovery Key (`vault.json` carries `rk_pub`). Only while unlocked.
    pub recovery_enabled: bool,
    /// The new Secret Key of a rotation whose Emergency Kit step is not finished. Only while unlocked: the
    /// window shows that step again until the key is confirmed.
    pub pending_kit: Option<String>,
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
    /// The required choice about a Recovery Key (FIG-103).
    recovery: recovery::Choice,
}

/// A Secret Key made by a rotation, held until the person has saved the new Emergency Kit.
struct Reissued {
    key: SecretKey,
    /// The person typed the end of the new key back correctly.
    confirmed: bool,
}

struct Inner {
    session: Option<Session>,
    /// The Secret Key this session was unlocked with, kept in memory until it locks (ADR-0030). Set by a full
    /// unlock, create, open or join that had the key, replaced when the key rotates, dropped with the session.
    held_key: Option<SecretKey>,
    /// List rows and the search index of the unlocked vault; rebuilt after any change.
    cache: Option<items::Cache>,
    /// Files chosen for a document item, until it is saved or the editor closes.
    staged: std::collections::HashMap<String, items::StagedData>,
    /// What the last copy left on the clipboard.
    copied: Option<items::Copied>,
    creating: Option<Creating>,
    reissued: Option<Reissued>,
    joining: Option<join::Join>,
    /// A Recovery Key that was made and shown, until it is confirmed and written or dropped.
    recovery: Option<recovery::Draft>,
    /// A Recovery Key that opened a vault on this fresh install, until the new credentials are stored.
    recovering: Option<forgot::Recovering>,
    /// The invite this device has open for a new device to join with, until it is used, expires or is cancelled.
    inviting: Option<pairing::Inviting>,
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
        self.held_key = None;
        self.inviting = None;
        self.recovery = None;
        self.recovering = None;
        self.cache = None;
        self.staged.clear();
    }
}

/// Answers a full unlock with what the person typed.
struct Typed {
    password: Zeroizing<String>,
    /// Blank when this device keeps the Secret Key; asked for only if the kept one does not open the vault.
    secret_key: Option<SecretKey>,
}

impl Typed {
    /// A copy of the typed Secret Key for the session to hold, if one was given.
    fn copy_secret_key(&self) -> Option<SecretKey> {
        self.secret_key
            .as_ref()
            .map(|k| SecretKey::from_bytes(*k.expose()))
    }
}

impl Ui for Typed {
    fn password(&self) -> Res<Zeroizing<String>> {
        Ok(self.password.clone())
    }
    fn secret_key(&self) -> Res<SecretKey> {
        self.secret_key
            .as_ref()
            .map(|k| SecretKey::from_bytes(*k.expose()))
            .ok_or_else(|| Fail::new(NEEDS_SECRET_KEY))
    }
    fn notice(&self, _: &str) {}
    fn warn(&self, _: &str) {}
}

/// What a full unlock reports when it needs the Secret Key typed and none was given.
const NEEDS_SECRET_KEY: &str = "needs secret key";

/// The failure of a full unlock, for the window: a key that was needed and not given is its own code, so the
/// unlock form can ask for it.
fn unlock_error(f: Fail) -> CmdError {
    if f.0.starts_with("could not unlock") {
        wrong_credentials()
    } else if f.0 == NEEDS_SECRET_KEY {
        CmdError::new(
            "needs_secret_key",
            "That did not open the vault. If your master password is right, the Secret Key was changed on another device: type the new one from your Emergency Kit.",
        )
    } else {
        f.into()
    }
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

/// What a full unlock needs typed on this device: the Secret Key too only when none is kept.
fn what_to_type() -> &'static str {
    let kept = secret_key_store::system()
        .ok()
        .zip(figlock_session::session::peek_vault_id().ok())
        .is_some_and(|(keys, vault)| keys.has(&vault));
    if kept {
        "your master password"
    } else {
        "your master password and Secret Key"
    }
}

/// Where the Secret Key for a step that rewraps the vault key came from, which decides what a failed step
/// means and what the window should ask (ADR-0030).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyFrom {
    /// The person typed it: a failure is a mismatch of the pair they typed.
    Typed,
    /// This unlocked session holds it and the person was never asked for it: a failure is the master
    /// password, and the message must not blame a key nobody was asked for.
    Held,
    /// This device keeps it (Touch ID released it): a failure cannot be told from a stale key, so ask for it.
    Kept,
}

fn wrong_password() -> CmdError {
    CmdError::new(
        "wrong_credentials",
        "That master password does not match this vault. Check it and try again.",
    )
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

/// The Emergency Kit PDF for `key`, naming where the vault is stored.
/// What a printed kit says about where the vault is stored, not where on this disk: a long path would not fit.
fn kit_storage(place: &Place) -> String {
    match place.remote {
        Some(Remote::Drive) => "Google Drive".to_owned(),
        None => {
            let name = place
                .local
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            format!("Folder {}", name.chars().take(60).collect::<String>())
        }
    }
}

fn kit_pdf(place: &Place, key: &SecretKey) -> CmdRes<Zeroizing<Vec<u8>>> {
    let storage = kit_storage(place);
    let info = KitInfo {
        vault_name: "Personal",
        storage: &storage,
        created: &civil_date(now_ms()),
    };
    render_kit(&info, key).map_err(|_| CmdError::new("failed", "Could not make the Emergency Kit."))
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
                held_key: None,
                cache: None,
                staged: std::collections::HashMap::new(),
                copied: None,
                creating: None,
                reissued: None,
                joining: None,
                recovery: None,
                recovering: None,
                inviting: None,
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
        let initialised = config::is_initialised()?;
        let (unlocked, pending_kit, recovery_enabled, secret_key_held) = {
            let mut inner = self.inner();
            if !initialised {
                inner.reissued = None;
            }
            let unlocked = inner.session.is_some();
            let held = inner.held_key.is_some();
            let pending = inner
                .reissued
                .as_ref()
                .filter(|_| unlocked)
                .map(|r| r.key.format().to_string());
            let recovery = inner
                .session
                .as_ref()
                .is_some_and(|s| s.vault.has_recovery());
            (unlocked, pending, recovery, held)
        };
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
        let (secret_key_kept, secret_key_protection) = match initialised
            .then(secret_key_store::system)
            .and_then(Result::ok)
            .zip(figlock_session::session::peek_vault_id().ok())
        {
            Some((keys, vault)) => (keys.has(&vault), keys.protection(&vault)),
            None => (false, None),
        };
        Ok(VaultState {
            phase,
            version: env!("CARGO_PKG_VERSION"),
            quick_unlock,
            touch_available,
            secret_key_kept,
            secret_key_protection,
            secret_key_held,
            autolock_secs: self.autolock.as_secs(),
            recovery_enabled,
            pending_kit,
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
            recovery: recovery::Choice::Undecided,
        });
        Ok(CreatedVault { secret_key })
    }

    /// The Emergency Kit PDF for the vault being created, or for the Secret Key a rotation just made, to be
    /// written where the person chose.
    pub fn emergency_kit(&self) -> CmdRes<Zeroizing<Vec<u8>>> {
        let inner = self.inner();
        if let Some(c) = inner.creating.as_ref() {
            return kit_pdf(&c.place, &c.created.secret_key);
        }
        if let Some(r) = inner.recovering.as_ref() {
            let p = r.prepared().ok_or_else(nothing_to_do)?;
            return kit_pdf(r.place(), p.secret_key());
        }
        let r = inner.reissued.as_ref().ok_or_else(nothing_to_do)?;
        kit_pdf(&config::load()?.place, &r.key)
    }

    /// The forced check that the Secret Key was really saved: the last two groups, typed back.
    pub fn check_kit_key(&self, typed: &str) -> CmdRes<bool> {
        let mut inner = self.inner();
        if let Some(c) = inner.creating.as_mut() {
            let ok = verify_kit_groups(&c.created.secret_key, typed);
            c.key_confirmed |= ok;
            return Ok(ok);
        }
        if let Some(p) = inner.recovering.as_mut().and_then(|r| r.prepared_mut()) {
            let ok = verify_kit_groups(p.secret_key(), typed);
            p.key_confirmed |= ok;
            return Ok(ok);
        }
        let r = inner.reissued.as_mut().ok_or_else(nothing_to_do)?;
        let ok = verify_kit_groups(&r.key, typed);
        r.confirmed |= ok;
        Ok(ok)
    }

    pub fn cancel_create(&self) {
        let mut inner = self.inner();
        inner.creating = None;
        inner.recovery = None;
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
        let mut c = inner.creating.take().ok_or_else(nothing_to_do)?;
        let draft = inner.recovery.take();
        drop(inner);
        // The required choice about a Recovery Key (FIG-103): add the confirmed one to the first `vault.json`.
        let vault_json = match Self::apply_recovery_at_create(
            draft,
            c.recovery,
            &mut c.vault,
            &c.password,
            &c.created.secret_key,
        ) {
            Ok(json) => json,
            Err(e) => {
                // Nothing was written: give the setup back so the person can choose and try again.
                self.inner().creating = Some(c);
                return Err(e);
            }
        };
        if let Some(json) = vault_json {
            c.created.vault_json = json;
        }
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
                secret_key: Some(key),
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
                secret_key: Some(key),
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
            secret_key: Some(joined.secret_key),
        };
        self.enter(typed, touch_id)
            .inspect_err(|_| config::forget())
    }

    // ---- unlock and lock ----

    /// Unlock with the master password, and the Secret Key unless this device keeps it. `secret_key` blank
    /// means "use the one kept here"; if that does not open the vault the answer is `needs_secret_key`.
    pub fn unlock(&self, password: &str, secret_key: Option<&str>) -> CmdRes<VaultState> {
        if password.is_empty() {
            return Err(CmdError::new(
                "empty_password",
                "Enter your master password.",
            ));
        }
        let key = match secret_key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(text) => Some(parse_secret_key(text)?),
            None => None,
        };
        let typed = Typed {
            password: Zeroizing::new(password.to_owned()),
            secret_key: key,
        };
        let held = typed.copy_secret_key();
        let (session, _) = Session::open_full(Box::new(typed)).map_err(unlock_error)?;
        self.install_holding(session, held);
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
                        || {
                            format!(
                                "Touch ID did not unlock the vault. Type {}.",
                                what_to_type()
                            )
                        },
                        |r| {
                            format!(
                                "{}. Type {}.",
                                capitalise(&r.replace("; enter the master password", "")),
                                what_to_type()
                            )
                        },
                    ),
                ))
            }
            Err(f) => Err(f.into()),
        }
    }

    /// A full unlock with the details just typed, then optionally turn on Touch ID.
    fn enter(&self, typed: Typed, touch_id: bool) -> CmdRes<VaultState> {
        let held = typed.copy_secret_key();
        let (session, auk) = Session::open_full(Box::new(typed)).map_err(unlock_error)?;
        if touch_id {
            // Touch ID is a convenience: failing to turn it on never undoes the setup.
            if let (Ok(vault), Ok(q)) = (figlock_session::session::peek_vault_id(), quick::system())
            {
                let _ = q.enable(&vault, &auk, now_ms() / 1000);
            }
        }
        self.install_holding(session, held);
        self.state()
    }

    fn install(&self, session: Session) {
        self.install_holding(session, None);
    }

    /// Make `session` the unlocked one. `held` is the Secret Key it was unlocked with, when the person typed it
    /// or it came with the pairing answer (ADR-0030).
    fn install_holding(&self, session: Session, held: Option<SecretKey>) {
        let mut inner = self.inner();
        inner.drop_session();
        inner.session = Some(session);
        inner.held_key = held;
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

pub mod cli_tool;
pub mod credentials;
pub mod devices;
pub mod drive;
pub mod forgot;
pub mod generate;
pub mod health;
pub mod items;
pub mod pairing;
pub mod recovery;
pub mod secret_key;
pub mod sync;
pub mod update;

#[cfg(test)]
mod cli_tool_tests;
#[cfg(test)]
mod credential_tests;
#[cfg(test)]
mod device_tests;
#[cfg(test)]
mod forgot_tests;
#[cfg(test)]
mod health_tests;
#[cfg(test)]
mod item_tests;
#[cfg(test)]
mod pairing_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod tests;
