//! The Secret Key kept on this device (`docs/adr/0027-device-stores-the-secret-key.md`), so a restart needs
//! the master password only and a trusted device can hand the key to a new one.
//!
//! It reuses the quick unlock backends (`quick_se.rs`: a Secure Enclave key with the login keychain
//! underneath) with its own slot, so it has its own chip key and files and is never opened with the unlock
//! key's. Unlike the unlock key it has no idle or age limit: it is meant to survive restarts. Every read still
//! asks for Touch ID or the device password. Where the platform has no backend yet (FIG-105) the store is
//! unavailable and the key is typed, as before.

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

pub struct SecretKeyStore<B: Backend> {
    backend: B,
    /// Off when `FIGLOCK_DEVICE_KEYS=file` (headless machines, CI, tests): those keep nothing in a keystore.
    enabled: bool,
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
    Ok(SecretKeyStore::new(
        backend,
        !std::env::var("FIGLOCK_DEVICE_KEYS").is_ok_and(|v| v == "file"),
    ))
}

impl<B: Backend> SecretKeyStore<B> {
    pub fn new(backend: B, enabled: bool) -> Self {
        SecretKeyStore { backend, enabled }
    }

    /// Whether this device can keep the key at all.
    pub fn available(&self) -> bool {
        self.enabled && self.backend.available()
    }

    /// Keep `key` for `vault`, replacing what was kept.
    pub fn put(&self, vault: &str, key: &SecretKey) -> Res<()> {
        if !self.available() {
            return Err(Fail::new(
                "this device cannot keep the Secret Key; it will be asked for when needed",
            ));
        }
        self.backend.put_key(vault, &pack(key))
    }

    /// Whether a key is kept for `vault`, without asking the person.
    pub fn has(&self, vault: &str) -> bool {
        self.available() && matches!(self.backend.has_key(vault), Ok(true))
    }

    /// The kept key, after Touch ID or the device password. `None` when nothing is kept or the person did
    /// not confirm; the caller then asks for the key.
    pub fn get(&self, vault: &str, reason: &str) -> Res<Option<SecretKey>> {
        if !self.has(vault) {
            return Ok(None);
        }
        if self.backend.confirm(vault, reason) != Outcome::Confirmed {
            return Ok(None);
        }
        match self.backend.get_key(vault)? {
            Some(packed) => unpack(&packed).map(Some),
            None => Ok(None),
        }
    }

    /// Drop the kept key.
    pub fn forget(&self, vault: &str) -> Res<()> {
        if !self.enabled {
            return Ok(());
        }
        self.backend.delete_key(vault)
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
        s.put(VAULT, &key).unwrap();
        assert!(s.has(VAULT));
        let got = s.get(VAULT, "test").unwrap().unwrap();
        assert_eq!(got.expose(), key.expose());
        assert_eq!(fake.0.asked.get(), 1);
    }

    #[test]
    fn nothing_kept_means_no_prompt_and_no_key() {
        let (s, fake) = store();
        assert!(s.get(VAULT, "test").unwrap().is_none());
        assert_eq!(fake.0.asked.get(), 0);
    }

    #[test]
    fn a_declined_prompt_hands_out_nothing() {
        let (s, fake) = store();
        s.put(VAULT, &SecretKey::random().unwrap()).unwrap();
        for answer in [Outcome::Declined, Outcome::TimedOut, Outcome::Unavailable] {
            fake.0.answer.set(Some(answer));
            assert!(s.get(VAULT, "test").unwrap().is_none());
        }
        // Declining does not delete it: the person can try again.
        fake.0.answer.set(None);
        assert!(s.get(VAULT, "test").unwrap().is_some());
    }

    #[test]
    fn putting_again_replaces_the_key() {
        let (s, _) = store();
        let (old, new) = (SecretKey::random().unwrap(), SecretKey::random().unwrap());
        s.put(VAULT, &old).unwrap();
        s.put(VAULT, &new).unwrap();
        assert_eq!(
            s.get(VAULT, "test").unwrap().unwrap().expose(),
            new.expose()
        );
    }

    #[test]
    fn forgetting_removes_it() {
        let (s, _) = store();
        s.put(VAULT, &SecretKey::random().unwrap()).unwrap();
        s.forget(VAULT).unwrap();
        assert!(!s.has(VAULT));
        assert!(s.get(VAULT, "test").unwrap().is_none());
    }

    #[test]
    fn a_damaged_value_is_an_error_not_a_wrong_key() {
        let (s, fake) = store();
        let mut bytes = [0u8; 32];
        bytes[31] = 1;
        *fake.0.key.borrow_mut() = Some(bytes);
        assert!(s.get(VAULT, "test").is_err());
    }

    #[test]
    fn a_disabled_store_keeps_nothing_and_says_so() {
        let fake = Fake::default();
        let s = SecretKeyStore::new(fake.clone(), false);
        assert!(!s.available());
        assert!(s.put(VAULT, &SecretKey::random().unwrap()).is_err());
        assert!(fake.0.key.borrow().is_none());
        assert!(s.get(VAULT, "test").unwrap().is_none());
        assert!(s.forget(VAULT).is_ok());
    }
}
