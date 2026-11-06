//! Quick unlock (`docs/spec/crypto.md` 8): after one full unlock the Account Unlock Key is kept in the
//! platform keystore, and later commands ask for Touch ID or the device password instead of the master
//! password and Secret Key.
//!
//! The key lives only in the keystore; `quick-unlock.json` in the FigLock home holds the policy state (when
//! the last full unlock was, when the key was last used, failed attempts) and never a secret. Everything
//! that can lock the key again goes through [`Quick::forget`]: idle time, boot, age, failed attempts, the
//! user, and the password, key and device changes that call it.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use figlock_biometric::Outcome;
use figlock_core::Key;
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::config;
use crate::error::{Fail, Res};
use crate::session::{self, Session};

/// Without use for this long, the next command needs a full unlock. `FIGLOCK_QUICK_IDLE_SECS` can only
/// shorten it.
const IDLE_SECS: u64 = 4 * 60 * 60;
/// A full unlock is needed after 7 days whatever happened in between.
const MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;
/// Declined prompts in a row before quick unlock switches itself off.
const MAX_FAILURES: u64 = 5;

/// What the platform gives quick unlock. Injected so tests need no keychain and no screen.
pub trait Backend {
    fn available(&self) -> bool;
    /// Ask the person at the keyboard (Touch ID or the device password).
    fn confirm(&self, reason: &str) -> Outcome;
    fn put_key(&self, vault: &str, key: &[u8; 32]) -> Res<()>;
    fn get_key(&self, vault: &str) -> Res<Option<Zeroizing<[u8; 32]>>>;
    /// Whether a key is kept, without reading it.
    fn has_key(&self, vault: &str) -> Res<bool>;
    fn delete_key(&self, vault: &str) -> Res<()>;
    /// Changes on every reboot, so a restart asks for the master password again.
    fn boot_id(&self) -> String;
}

/// Why a command still needs the master password.
#[derive(Debug, PartialEq, Eq)]
pub enum Fallback {
    /// Quick unlock is not turned on for this vault, or was never filled.
    Off,
    /// It was switched off by policy; the message says why.
    Expired(&'static str),
    /// The prompt was declined, cancelled or could not be shown.
    NotConfirmed,
}

pub enum Fetch {
    Key(Key),
    Full(Fallback),
}

pub struct Quick<B: Backend> {
    home: PathBuf,
    backend: B,
    idle_secs: u64,
}

pub fn system() -> Res<Quick<System>> {
    let idle = std::env::var("FIGLOCK_QUICK_IDLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(IDLE_SECS, |v| v.min(IDLE_SECS));
    Ok(Quick::new(config::home()?, System, idle))
}

impl<B: Backend> Quick<B> {
    pub fn new(home: PathBuf, backend: B, idle_secs: u64) -> Self {
        Quick {
            home,
            backend,
            idle_secs,
        }
    }

    fn state_path(&self) -> PathBuf {
        self.home.join("quick-unlock.json")
    }

    fn read_state(&self) -> Option<Value> {
        serde_json::from_slice(&fs::read(self.state_path()).ok()?).ok()
    }

    fn write_state(&self, state: &Value) -> Res<()> {
        config::private_dir(&self.home)?;
        let tmp = self.home.join(".quick-unlock.json.tmp");
        let _ = fs::remove_file(&tmp);
        let written = config::create_private_file(&tmp).and_then(|mut f| {
            f.write_all(state.to_string().as_bytes())?;
            f.sync_all()
        });
        if let Err(e) = written.and_then(|()| fs::rename(&tmp, self.state_path())) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }

    fn state_for(&self, vault: &str) -> Option<Value> {
        self.read_state()
            .filter(|s| s["vault_id"] == vault && s["enabled"] == true)
    }

    fn fresh_state(&self, vault: &str, now: u64) -> Value {
        json!({
            "vault_id": vault,
            "enabled": true,
            "unlocked_at": now,
            "last_used": now,
            "failures": 0,
            "boot": self.backend.boot_id(),
        })
    }

    /// Turn quick unlock on and keep `auk`. The caller has just done a full unlock.
    pub fn enable(&self, vault: &str, auk: &Key, now: u64) -> Res<()> {
        if !self.backend.available() {
            return Err(Fail::new(
                "this machine cannot ask for Touch ID or the device password, so quick unlock is unavailable",
            ));
        }
        self.backend.put_key(vault, auk.expose())?;
        self.write_state(&self.fresh_state(vault, now))
    }

    /// After a full unlock: refresh the kept key when quick unlock is on.
    pub fn remember(&self, vault: &str, auk: &Key, now: u64) -> Res<()> {
        if self.state_for(vault).is_none() {
            return Ok(());
        }
        self.enable(vault, auk, now)
    }

    /// Whether quick unlock is turned on for `vault`, and whether a key is currently kept.
    pub fn status(&self, vault: &str) -> (bool, bool) {
        let on = self.state_for(vault).is_some();
        let kept = on && matches!(self.backend.has_key(vault), Ok(true));
        (on, kept)
    }

    /// Lock: drop the kept key. Quick unlock stays on, so the next full unlock refills it.
    pub fn lock(&self, vault: &str) -> Res<()> {
        self.backend.delete_key(vault)
    }

    /// Turn quick unlock off and drop the key and the policy state. Used on user request, and by password
    /// change, Secret Key rotation and device revocation.
    pub fn forget(&self, vault: &str) -> Res<()> {
        self.backend.delete_key(vault)?;
        match fs::remove_file(self.state_path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Like `fetch`, but a keystore failure is a warning and a fall back to the master password, since
    /// quick unlock is only a convenience.
    pub fn fetch_or_full(&self, vault: &str, now: u64) -> Fetch {
        self.fetch(vault, now).unwrap_or_else(|e| {
            eprintln!("warning: quick unlock is unavailable: {e}");
            Fetch::Full(Fallback::NotConfirmed)
        })
    }

    /// The kept key, after the policy checks and the prompt.
    pub fn fetch(&self, vault: &str, now: u64) -> Res<Fetch> {
        let Some(mut state) = self.state_for(vault) else {
            return Ok(Fetch::Full(Fallback::Off));
        };
        let num = |k: &str| state[k].as_u64().unwrap_or(0);
        let expired = if state["boot"] != json!(self.backend.boot_id()) {
            Some("this machine restarted since the last full unlock")
        } else if now.saturating_sub(num("last_used")) > self.idle_secs {
            Some("quick unlock timed out from inactivity")
        } else if now.saturating_sub(num("unlocked_at")) > MAX_AGE_SECS {
            Some("it has been 7 days since the last full unlock")
        } else if num("failures") >= MAX_FAILURES {
            Some("quick unlock was switched off after too many failed attempts")
        } else {
            None
        };
        if let Some(why) = expired {
            self.backend.delete_key(vault)?;
            return Ok(Fetch::Full(Fallback::Expired(why)));
        }
        if !self.backend.has_key(vault)? {
            return Ok(Fetch::Full(Fallback::Off));
        }
        match self.backend.confirm("unlock your FigLock vault") {
            Outcome::Confirmed => {}
            Outcome::Declined => {
                let failures = num("failures") + 1;
                state["failures"] = json!(failures);
                self.write_state(&state)?;
                if failures >= MAX_FAILURES {
                    self.backend.delete_key(vault)?;
                }
                return Ok(Fetch::Full(Fallback::NotConfirmed));
            }
            Outcome::TimedOut | Outcome::Unavailable => {
                return Ok(Fetch::Full(Fallback::NotConfirmed));
            }
        }
        let Some(bytes) = self.backend.get_key(vault)? else {
            return Ok(Fetch::Full(Fallback::Off));
        };
        state["failures"] = json!(0);
        state["last_used"] = json!(now);
        self.write_state(&state)?;
        Ok(Fetch::Key(Key::from_bytes(*bytes)))
    }
}

fn say(json: bool, message: &str, on: bool, kept: bool) -> Res<()> {
    if json {
        println!("{}", json!({ "enabled": on, "unlocked": kept }));
    } else {
        println!("{message}");
    }
    Ok(())
}

/// `figlock quick-unlock enable`: a full unlock, then keep the key.
pub fn enable(json: bool) -> Res<()> {
    let quick = system()?;
    if !quick.backend.available() {
        return Err(Fail::new(
            "this machine cannot ask for Touch ID or the device password, so quick unlock is unavailable",
        ));
    }
    let (_session, auk) = Session::open_full()?;
    let vault = session::peek_vault_id()?;
    quick.enable(&vault, &auk, session::now_ms() / 1000)?;
    say(
        json,
        "Quick unlock is on. Later commands ask for Touch ID or your device password.",
        true,
        true,
    )
}

/// `figlock quick-unlock disable`
pub fn disable(json: bool) -> Res<()> {
    system()?.forget(&session::peek_vault_id()?)?;
    say(json, "Quick unlock is off.", false, false)
}

pub fn status(json: bool) -> Res<()> {
    let (on, kept) = system()?.status(&session::peek_vault_id()?);
    let msg = match (on, kept) {
        (false, _) => "Quick unlock is off.",
        (true, true) => "Quick unlock is on and the vault can be opened with Touch ID.",
        (true, false) => {
            "Quick unlock is on, but the vault is locked: the next command asks for the master password."
        }
    };
    say(json, msg, on, kept)
}

/// `figlock lock`: drop the kept key. Quick unlock stays on and refills at the next full unlock.
pub fn lock(json: bool) -> Res<()> {
    system()?.lock(&session::peek_vault_id()?)?;
    if json {
        println!("{}", json!({ "locked": true }));
    } else {
        println!("Locked. The next command asks for the master password.");
    }
    Ok(())
}

/// This machine's keystore: the login keychain on macOS, behind a Touch ID or device-password prompt.
/// Elsewhere quick unlock is unavailable until that platform gets an adapter.
pub struct System;

#[cfg(target_os = "macos")]
impl Backend for System {
    fn available(&self) -> bool {
        figlock_biometric::available()
    }

    fn confirm(&self, reason: &str) -> Outcome {
        figlock_biometric::confirm(reason, std::time::Duration::from_secs(60))
    }

    fn put_key(&self, vault: &str, key: &[u8; 32]) -> Res<()> {
        security_framework::passwords::set_generic_password(SERVICE, vault, key)
            .map_err(|_| Fail::new("could not save the key in the macOS keychain"))
    }

    fn get_key(&self, vault: &str) -> Res<Option<Zeroizing<[u8; 32]>>> {
        match security_framework::passwords::get_generic_password(SERVICE, vault) {
            Ok(bytes) => Ok(<[u8; 32]>::try_from(bytes.as_slice())
                .ok()
                .map(Zeroizing::new)),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(_) => Err(Fail::new("could not read the key from the macOS keychain")),
        }
    }

    fn has_key(&self, vault: &str) -> Res<bool> {
        self.get_key(vault).map(|k| k.is_some())
    }

    fn delete_key(&self, vault: &str) -> Res<()> {
        match security_framework::passwords::delete_generic_password(SERVICE, vault) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(_) => Err(Fail::new(
                "could not remove the key from the macOS keychain",
            )),
        }
    }

    fn boot_id(&self) -> String {
        std::process::Command::new("sysctl")
            .args(["-n", "kern.boottime"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    }
}

#[cfg(target_os = "macos")]
const SERVICE: &str = "com.figlock.cli";
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(not(target_os = "macos"))]
impl Backend for System {
    fn available(&self) -> bool {
        false
    }
    fn confirm(&self, _reason: &str) -> Outcome {
        Outcome::Unavailable
    }
    fn put_key(&self, _vault: &str, _key: &[u8; 32]) -> Res<()> {
        Err(Fail::new(
            "quick unlock is not available on this platform yet",
        ))
    }
    fn get_key(&self, _vault: &str) -> Res<Option<Zeroizing<[u8; 32]>>> {
        Ok(None)
    }
    fn has_key(&self, _vault: &str) -> Res<bool> {
        Ok(false)
    }
    fn delete_key(&self, _vault: &str) -> Res<()> {
        Ok(())
    }
    fn boot_id(&self) -> String {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    const VAULT: &str = "0123456789abcdef0123456789abcdef";
    const T0: u64 = 1_700_000_000;

    #[derive(Default)]
    struct Shared {
        key: RefCell<Option<[u8; 32]>>,
        answer: Cell<Option<Outcome>>,
        asked: Cell<u32>,
        boot: RefCell<String>,
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<Shared>);

    impl Backend for Fake {
        fn available(&self) -> bool {
            true
        }
        fn confirm(&self, _reason: &str) -> Outcome {
            self.0.asked.set(self.0.asked.get() + 1);
            self.0.answer.get().unwrap_or(Outcome::Confirmed)
        }
        fn put_key(&self, _vault: &str, key: &[u8; 32]) -> Res<()> {
            *self.0.key.borrow_mut() = Some(*key);
            Ok(())
        }
        fn get_key(&self, _vault: &str) -> Res<Option<Zeroizing<[u8; 32]>>> {
            Ok((*self.0.key.borrow()).map(Zeroizing::new))
        }
        fn has_key(&self, _vault: &str) -> Res<bool> {
            Ok(self.0.key.borrow().is_some())
        }
        fn delete_key(&self, _vault: &str) -> Res<()> {
            *self.0.key.borrow_mut() = None;
            Ok(())
        }
        fn boot_id(&self) -> String {
            self.0.boot.borrow().clone()
        }
    }

    fn setup() -> (tempfile::TempDir, Fake, Quick<Fake>) {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let quick = Quick::new(dir.path().to_path_buf(), fake.clone(), 3600);
        (dir, fake, quick)
    }

    fn key() -> Key {
        Key::from_bytes([7; 32])
    }

    fn got_key(f: Fetch) -> bool {
        matches!(f, Fetch::Key(k) if k.expose() == &[7; 32])
    }

    struct Broken;

    impl Backend for Broken {
        fn available(&self) -> bool {
            true
        }
        fn confirm(&self, _reason: &str) -> Outcome {
            Outcome::Confirmed
        }
        fn put_key(&self, _vault: &str, _key: &[u8; 32]) -> Res<()> {
            Err(Fail::new("keychain locked"))
        }
        fn get_key(&self, _vault: &str) -> Res<Option<Zeroizing<[u8; 32]>>> {
            Err(Fail::new("keychain locked"))
        }
        fn has_key(&self, _vault: &str) -> Res<bool> {
            Err(Fail::new("keychain locked"))
        }
        fn delete_key(&self, _vault: &str) -> Res<()> {
            Err(Fail::new("keychain locked"))
        }
        fn boot_id(&self) -> String {
            String::new()
        }
    }

    #[test]
    fn a_keychain_failure_falls_back_to_the_full_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let (_d, _fake, good) = setup();
        good.enable(VAULT, &key(), T0).unwrap();
        let state = std::fs::read(good.state_path()).unwrap();
        std::fs::write(dir.path().join("quick-unlock.json"), state).unwrap();
        let q = Quick::new(dir.path().to_path_buf(), Broken, 3600);
        assert!(q.fetch(VAULT, T0 + 60).is_err());
        assert!(matches!(
            q.fetch_or_full(VAULT, T0 + 60),
            Fetch::Full(Fallback::NotConfirmed)
        ));
    }

    #[test]
    fn off_until_enabled_and_nothing_is_asked_or_kept() {
        let (_d, fake, q) = setup();
        assert!(matches!(
            q.fetch(VAULT, T0).unwrap(),
            Fetch::Full(Fallback::Off)
        ));
        // A full unlock without opting in must not leave a key behind.
        q.remember(VAULT, &key(), T0).unwrap();
        assert!(fake.0.key.borrow().is_none());
        assert_eq!(fake.0.asked.get(), 0);
    }

    #[test]
    fn enabled_returns_the_key_only_after_the_prompt_is_confirmed() {
        let (_d, fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        assert!(got_key(q.fetch(VAULT, T0 + 60).unwrap()));
        assert_eq!(fake.0.asked.get(), 1);
    }

    #[test]
    fn a_declined_prompt_gives_no_key() {
        let (_d, fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        for answer in [Outcome::Declined, Outcome::TimedOut, Outcome::Unavailable] {
            fake.0.answer.set(Some(answer));
            assert!(matches!(
                q.fetch(VAULT, T0 + 1).unwrap(),
                Fetch::Full(Fallback::NotConfirmed)
            ));
        }
    }

    #[test]
    fn five_declined_attempts_delete_the_key() {
        let (_d, fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        fake.0.answer.set(Some(Outcome::Declined));
        for _ in 0..4 {
            q.fetch(VAULT, T0 + 1).unwrap();
        }
        assert!(fake.0.key.borrow().is_some(), "four failures keep the key");
        q.fetch(VAULT, T0 + 1).unwrap();
        assert!(fake.0.key.borrow().is_none(), "the fifth deletes it");
        // Even a confirmed prompt cannot bring it back; only a full unlock can.
        fake.0.answer.set(Some(Outcome::Confirmed));
        assert!(matches!(q.fetch(VAULT, T0 + 2).unwrap(), Fetch::Full(_)));
    }

    #[test]
    fn a_confirmed_attempt_resets_the_failure_count() {
        let (_d, fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        fake.0.answer.set(Some(Outcome::Declined));
        for _ in 0..4 {
            q.fetch(VAULT, T0 + 1).unwrap();
        }
        fake.0.answer.set(Some(Outcome::Confirmed));
        assert!(got_key(q.fetch(VAULT, T0 + 2).unwrap()));
        fake.0.answer.set(Some(Outcome::Declined));
        for _ in 0..4 {
            q.fetch(VAULT, T0 + 3).unwrap();
        }
        assert!(fake.0.key.borrow().is_some());
    }

    #[test]
    fn idle_time_locks_and_use_keeps_it_alive() {
        let (_d, fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        // Each use pushes the idle deadline out.
        assert!(got_key(q.fetch(VAULT, T0 + 3000).unwrap()));
        assert!(got_key(q.fetch(VAULT, T0 + 6000).unwrap()));
        let r = q.fetch(VAULT, T0 + 6000 + 3601).unwrap();
        assert!(matches!(r, Fetch::Full(Fallback::Expired(_))));
        assert!(fake.0.key.borrow().is_none());
    }

    #[test]
    fn seven_days_after_the_full_unlock_it_locks_even_if_used() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        // An idle limit longer than the age limit, so only the age limit can fire.
        let q = Quick::new(dir.path().to_path_buf(), fake.clone(), 30 * 24 * 3600);
        q.enable(VAULT, &key(), T0).unwrap();
        let day = 24 * 3600;
        for d in 1..=7 {
            assert!(got_key(q.fetch(VAULT, T0 + d * day).unwrap()));
        }
        let r = q.fetch(VAULT, T0 + 7 * day + 1800).unwrap();
        assert!(matches!(r, Fetch::Full(Fallback::Expired(_))));
        assert!(fake.0.key.borrow().is_none());
    }

    #[test]
    fn a_reboot_locks() {
        let (_d, fake, q) = setup();
        *fake.0.boot.borrow_mut() = "boot-1".into();
        q.enable(VAULT, &key(), T0).unwrap();
        *fake.0.boot.borrow_mut() = "boot-2".into();
        let r = q.fetch(VAULT, T0 + 1).unwrap();
        assert!(matches!(r, Fetch::Full(Fallback::Expired(_))));
        assert!(fake.0.key.borrow().is_none());
    }

    #[test]
    fn a_full_unlock_refills_a_locked_but_enabled_entry() {
        let (_d, fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        q.lock(VAULT).unwrap();
        assert!(fake.0.key.borrow().is_none());
        assert!(matches!(q.fetch(VAULT, T0 + 1).unwrap(), Fetch::Full(_)));
        q.remember(VAULT, &key(), T0 + 2).unwrap();
        assert!(got_key(q.fetch(VAULT, T0 + 3).unwrap()));
    }

    #[test]
    fn forget_removes_the_key_and_the_state() {
        let (d, fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        q.forget(VAULT).unwrap();
        assert!(fake.0.key.borrow().is_none());
        assert!(!d.path().join("quick-unlock.json").exists());
        assert!(matches!(
            q.fetch(VAULT, T0 + 1).unwrap(),
            Fetch::Full(Fallback::Off)
        ));
        q.forget(VAULT).unwrap();
    }

    #[test]
    fn another_vaults_state_is_ignored() {
        let (_d, _fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        assert!(matches!(
            q.fetch("ffff", T0 + 1).unwrap(),
            Fetch::Full(Fallback::Off)
        ));
    }

    #[test]
    fn the_state_file_holds_no_key_material() {
        let (d, _fake, q) = setup();
        q.enable(VAULT, &key(), T0).unwrap();
        let text = fs::read_to_string(d.path().join("quick-unlock.json")).unwrap();
        assert!(!text.contains(&hex::encode([7u8; 32])));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(d.path().join("quick-unlock.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
    }
}
