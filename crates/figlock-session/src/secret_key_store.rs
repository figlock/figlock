//! The Secret Key kept on this device (`docs/adr/0027-device-stores-the-secret-key.md`), so a restart needs
//! the master password only and a trusted device can hand the key to a new one.
//!
//! It reuses the quick unlock backends (`quick_se.rs`: a Secure Enclave key with the login keychain
//! underneath) with its own slot, so it has its own chip key and files and is never opened with the unlock
//! key's. Unlike the unlock key it has no idle or age limit: it is meant to survive restarts. Every read still
//! asks for Touch ID or the device password. Where the platform has no backend yet (FIG-105) the store is
//! unavailable and the key is typed, as before.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use figlock_biometric::Outcome;
use figlock_core::SecretKey;
use zeroize::Zeroizing;

use crate::config;
use crate::error::{Fail, Res};
use crate::quick::{Backend, System};
use crate::quick_se::{Chip, Hardware, SECRET_KEY_SLOT};

/// The backends keep 32 bytes; the Secret Key is 16, so the rest is zero and checked on the way out.
fn pack(key: &SecretKey) -> Zeroizing<[u8; 32]> {
    let mut packed = Zeroizing::new([0u8; 32]);
    packed[..16].copy_from_slice(key.expose());
    packed
}

fn unpack(packed: &[u8; 32]) -> Res<SecretKey> {
    if packed[16..].iter().any(|b| *b != 0) {
        return Err(Fail::new("the Secret Key kept on this device is damaged"));
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&packed[..16]);
    Ok(SecretKey::from_bytes(bytes))
}

/// Where the `sk_gen` kept with the key is written: this device's config folder, or memory for tests.
enum Gens {
    Disk(PathBuf),
    Memory(RefCell<BTreeMap<String, u32>>),
}

/// Whether quick unlock is on for `vault` on this device, which decides how the Secret Key is kept and read.
pub fn quick_on(vault: &str) -> bool {
    crate::quick::system().is_ok_and(|q| q.status(vault).0)
}

pub struct SecretKeyStore<B: Backend> {
    backend: B,
    /// Off when `FIGLOCK_DEVICE_KEYS=file` (headless machines, CI, tests): those keep nothing in a keystore.
    enabled: bool,
    gens: Gens,
}

/// This machine's store: the Secure Enclave on a Mac that has one, else the login keychain.
pub fn system() -> Res<SecretKeyStore<Hardware<Chip, System>>> {
    let backend = Hardware::with_slot(
        config::home()?,
        config::device_id(),
        Chip,
        System::SECRET_KEY,
        SECRET_KEY_SLOT,
    );
    Ok(SecretKeyStore {
        backend,
        enabled: !std::env::var("FIGLOCK_DEVICE_KEYS").is_ok_and(|v| v == "file"),
        gens: Gens::Disk(config::home()?),
    })
}

impl<B: Backend> SecretKeyStore<B> {
    /// A store whose generations are kept in memory, for tests.
    pub fn new(backend: B, enabled: bool) -> Self {
        SecretKeyStore {
            backend,
            enabled,
            gens: Gens::Memory(RefCell::new(BTreeMap::new())),
        }
    }

    fn set_gen(&self, vault: &str, sk_gen: Option<u32>) -> Res<()> {
        match &self.gens {
            Gens::Disk(home) => config::set_kept_sk_gen_in(home, vault, sk_gen),
            Gens::Memory(m) => {
                match sk_gen {
                    Some(n) => m.borrow_mut().insert(vault.to_owned(), n),
                    None => m.borrow_mut().remove(vault),
                };
                Ok(())
            }
        }
    }

    /// Whether this device can keep the key at all.
    pub fn available(&self) -> bool {
        self.enabled && self.backend.available()
    }

    /// Keep `key` for `vault`, replacing what was kept, together with the `sk_gen` of the vault it opens
    /// (`crypto.md` 9.4). With quick unlock on it goes to the strongest tier the Mac has, read after a prompt;
    /// with it off to the login keychain, read with none (ADR-0034).
    pub fn put(&self, vault: &str, key: &SecretKey, sk_gen: u32, quick_on: bool) -> Res<()> {
        if !self.available() {
            return Err(Fail::new(
                "this device cannot keep the Secret Key; it will be asked for when needed",
            ));
        }
        let packed = pack(key);
        if quick_on {
            self.backend.put_key(vault, &packed)?;
        } else {
            self.backend.put_key_plain(vault, &packed)?;
        }
        self.set_gen(vault, Some(sk_gen))
    }

    /// Whether a key is kept for `vault`, without asking the person.
    pub fn has(&self, vault: &str) -> bool {
        self.available() && matches!(self.backend.has_key(vault), Ok(true))
    }

    /// The `sk_gen` of the kept key (`crypto.md` 9.4).
    pub fn kept_gen(&self, vault: &str) -> u32 {
        match &self.gens {
            Gens::Disk(home) => config::kept_sk_gen_in(home, vault),
            Gens::Memory(m) => m.borrow().get(vault).copied().unwrap_or(1),
        }
    }

    /// The kept key. A key in the login keychain is read with no prompt when quick unlock is off; one on the
    /// chip, or any key while quick unlock is on, is read after Touch ID or the device password. `None` when
    /// nothing is kept or the person did not confirm; the caller then asks for the key.
    pub fn get(&self, vault: &str, reason: &str, quick_on: bool) -> Res<Option<SecretKey>> {
        if !self.has(vault) {
            return Ok(None);
        }
        if !quick_on && let Some(packed) = self.backend.get_key_quiet(vault)? {
            return unpack(&packed).map(Some);
        }
        if self.backend.confirm(vault, reason) != Outcome::Confirmed {
            return Ok(None);
        }
        match self.backend.get_key(vault)? {
            Some(packed) => {
                if !quick_on {
                    let _ = self.backend.put_key_plain(vault, &packed);
                }
                unpack(&packed).map(Some)
            }
            None => Ok(None),
        }
    }

    /// Move the kept key to the tier that matches quick unlock, keeping the same key and `sk_gen` (turning
    /// quick unlock on or off never deletes the key or makes a new one). Reading a key off the chip asks the
    /// person; when they decline it stays where it was.
    pub fn retier(&self, vault: &str, quick_on: bool, reason: &str) -> Res<()> {
        // Quietly when it sits in the keychain (moving up to the chip), after the prompt when it is on the chip.
        let Some(key) = self.get(vault, reason, false)? else {
            return Ok(());
        };
        let packed = pack(&key);
        if quick_on {
            self.backend.put_key(vault, &packed)
        } else {
            self.backend.put_key_plain(vault, &packed)
        }
    }

    /// Drop the kept key and the generation kept with it.
    pub fn forget(&self, vault: &str) -> Res<()> {
        if !self.enabled {
            return Ok(());
        }
        self.backend.delete_key(vault)?;
        self.set_gen(vault, None)
    }

    /// How the kept key is protected, for status lines: `secure-enclave` or `keychain`.
    pub fn protection(&self, vault: &str) -> Option<&'static str> {
        if self.has(vault) {
            self.backend.protection(vault)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    const VAULT: &str = "0123456789abcdef0123456789abcdef";

    #[derive(Default)]
    struct Shared {
        key: RefCell<Option<[u8; 32]>>,
        answer: Cell<Option<Outcome>>,
        asked: Cell<u32>,
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<Shared>);

    impl Backend for Fake {
        fn available(&self) -> bool {
            true
        }
        fn confirm(&self, _: &str, _: &str) -> Outcome {
            self.0.asked.set(self.0.asked.get() + 1);
            self.0.answer.get().unwrap_or(Outcome::Confirmed)
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
            "boot".to_owned()
        }
    }

    fn store() -> (SecretKeyStore<Fake>, Fake) {
        let fake = Fake::default();
        (SecretKeyStore::new(fake.clone(), true), fake)
    }

    #[test]
    fn a_kept_key_comes_back_after_the_prompt() {
        let (s, fake) = store();
        let key = SecretKey::random().unwrap();
        s.put(VAULT, &key, 1, true).unwrap();
        assert!(s.has(VAULT));
        let got = s.get(VAULT, "test", true).unwrap().unwrap();
        assert_eq!(got.expose(), key.expose());
        assert_eq!(fake.0.asked.get(), 1);
    }

    #[test]
    fn nothing_kept_means_no_prompt_and_no_key() {
        let (s, fake) = store();
        assert!(s.get(VAULT, "test", true).unwrap().is_none());
        assert_eq!(fake.0.asked.get(), 0);
    }

    #[test]
    fn a_declined_prompt_hands_out_nothing() {
        let (s, fake) = store();
        s.put(VAULT, &SecretKey::random().unwrap(), 1, true)
            .unwrap();
        for answer in [Outcome::Declined, Outcome::TimedOut, Outcome::Unavailable] {
            fake.0.answer.set(Some(answer));
            assert!(s.get(VAULT, "test", true).unwrap().is_none());
        }
        // Declining does not delete it: the person can try again.
        fake.0.answer.set(None);
        assert!(s.get(VAULT, "test", true).unwrap().is_some());
    }

    #[test]
    fn putting_again_replaces_the_key() {
        let (s, _) = store();
        let (old, new) = (SecretKey::random().unwrap(), SecretKey::random().unwrap());
        s.put(VAULT, &old, 1, true).unwrap();
        s.put(VAULT, &new, 1, true).unwrap();
        assert_eq!(
            s.get(VAULT, "test", true).unwrap().unwrap().expose(),
            new.expose()
        );
    }

    #[test]
    fn forgetting_removes_it() {
        let (s, _) = store();
        s.put(VAULT, &SecretKey::random().unwrap(), 1, true)
            .unwrap();
        s.forget(VAULT).unwrap();
        assert!(!s.has(VAULT));
        assert!(s.get(VAULT, "test", true).unwrap().is_none());
    }

    #[test]
    fn a_damaged_value_is_an_error_not_a_wrong_key() {
        let (s, fake) = store();
        let mut bytes = [0u8; 32];
        bytes[31] = 1;
        *fake.0.key.borrow_mut() = Some(bytes);
        assert!(s.get(VAULT, "test", true).is_err());
    }

    #[test]
    fn with_quick_unlock_off_a_login_keychain_key_is_read_without_a_prompt() {
        let (s, fake) = store();
        let key = SecretKey::random().unwrap();
        s.put(VAULT, &key, 1, false).unwrap();
        let got = s.get(VAULT, "test", false).unwrap().unwrap();
        assert_eq!(got.expose(), key.expose());
        assert_eq!(fake.0.asked.get(), 0, "no Touch ID or device password");
    }

    #[test]
    fn with_quick_unlock_on_the_same_key_is_read_after_the_prompt() {
        let (s, fake) = store();
        s.put(VAULT, &SecretKey::random().unwrap(), 1, true)
            .unwrap();
        assert!(s.get(VAULT, "test", true).unwrap().is_some());
        assert_eq!(fake.0.asked.get(), 1);
    }

    #[test]
    fn the_generation_is_kept_with_the_key_and_goes_with_it() {
        let (s, _) = store();
        assert_eq!(
            s.kept_gen(VAULT),
            1,
            "a key kept before the counter existed"
        );
        s.put(VAULT, &SecretKey::random().unwrap(), 3, false)
            .unwrap();
        assert_eq!(s.kept_gen(VAULT), 3);
        s.put(VAULT, &SecretKey::random().unwrap(), 4, true)
            .unwrap();
        assert_eq!(s.kept_gen(VAULT), 4);
        s.forget(VAULT).unwrap();
        assert_eq!(s.kept_gen(VAULT), 1);
    }

    #[test]
    fn turning_quick_unlock_on_or_off_moves_the_key_and_never_replaces_it() {
        let (s, _) = store();
        let key = SecretKey::random().unwrap();
        s.put(VAULT, &key, 2, false).unwrap();
        s.retier(VAULT, true, "test").unwrap();
        s.retier(VAULT, false, "test").unwrap();
        assert!(s.has(VAULT));
        assert_eq!(s.kept_gen(VAULT), 2);
        assert_eq!(
            s.get(VAULT, "test", false).unwrap().unwrap().expose(),
            key.expose()
        );
    }

    #[test]
    fn a_generation_on_disk_survives_a_new_store_and_is_per_vault() {
        let dir = tempfile::tempdir().unwrap();
        let on_disk = |fake: Fake| SecretKeyStore {
            backend: fake,
            enabled: true,
            gens: Gens::Disk(dir.path().to_path_buf()),
        };
        let key = SecretKey::random().unwrap();
        on_disk(Fake::default()).put(VAULT, &key, 5, false).unwrap();
        let again = on_disk(Fake::default());
        assert_eq!(again.kept_gen(VAULT), 5);
        assert_eq!(again.kept_gen("ffffffffffffffffffffffffffffffff"), 1);
        again.forget(VAULT).unwrap();
        assert_eq!(again.kept_gen(VAULT), 1);
    }

    #[test]
    fn a_disabled_store_keeps_nothing_and_says_so() {
        let fake = Fake::default();
        let s = SecretKeyStore::new(fake.clone(), false);
        assert!(!s.available());
        assert!(
            s.put(VAULT, &SecretKey::random().unwrap(), 1, true)
                .is_err()
        );
        assert!(fake.0.key.borrow().is_none());
        assert!(s.get(VAULT, "test", true).unwrap().is_none());
        assert!(s.forget(VAULT).is_ok());
    }
}
