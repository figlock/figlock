//! Hardware-bound quick unlock for the CLI (`docs/adr/0025-secure-enclave-quick-unlock.md`).
//!
//! Where this Mac has a Secure Enclave, the kept key is wrapped to a chip-bound key `QK` instead of sitting in the
//! login keychain behind an app prompt: opening it needs the chip, which asks for Touch ID, the device password or
//! a paired Apple Watch every time. Two files in the FigLock home hold what is kept:
//!
//! - `quick-unlock.qk`: the opaque blob of `QK` (useless without this chip and a successful check);
//! - `quick-unlock.blob`: the Account Unlock Key wrapped to `QK` (`figlock_core::seal_quick_unlock_se`).
//!
//! Where there is no Secure Enclave, or anything about it fails, the keychain backend underneath is used as before.
//! An older keychain item is moved to the chip on its next successful unlock.

use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use figlock_biometric::Outcome;
use figlock_biometric::enclave::{self, POINT_LEN, SHARED_LEN};
use figlock_core::{
    Key, open_quick_unlock_se, quick_unlock_se_ephemeral_public, seal_quick_unlock_se,
};
use zeroize::Zeroizing;

use crate::config;
use crate::error::{Fail, Res};
use crate::quick::Backend;

/// Where one kept secret lives: its chip key, its wrapped value and the temporary file a write goes through.
/// Each secret has its own chip key, so one cannot be opened with the other's.
#[derive(Clone, Copy)]
pub struct Slot {
    pub qk: &'static str,
    pub blob: &'static str,
    pub tmp: &'static str,
}

/// The Account Unlock Key kept for quick unlock.
pub const QUICK_SLOT: Slot = Slot {
    qk: "quick-unlock.qk",
    blob: "quick-unlock.blob",
    tmp: ".quick-unlock.tmp",
};

/// The Secret Key kept on the device (ADR-0027).
pub const SECRET_KEY_SLOT: Slot = Slot {
    qk: "secret-key.qk",
    blob: "secret-key.blob",
    tmp: ".secret-key.tmp",
};
/// How long the system sheet may stay open before the command gives up and asks for the master password.
const PROMPT_WAIT: Duration = Duration::from_secs(60);

/// The chip. Injected so the logic here is tested without hardware or a prompt.
pub trait Enclave {
    fn available(&self) -> bool;
    fn create(&self) -> Result<enclave::NewKey, enclave::Error>;
    fn public_key(&self, blob: &[u8]) -> Result<[u8; POINT_LEN], enclave::Error>;
    fn agree(
        &self,
        blob: &[u8],
        peer: &[u8; POINT_LEN],
        reason: &str,
    ) -> Result<Zeroizing<[u8; SHARED_LEN]>, enclave::Error>;
}

/// The real Secure Enclave, through the Swift shim.
pub struct Chip;

impl Enclave for Chip {
    fn available(&self) -> bool {
        enclave::available()
    }
    fn create(&self) -> Result<enclave::NewKey, enclave::Error> {
        enclave::create()
    }
    fn public_key(&self, blob: &[u8]) -> Result<[u8; POINT_LEN], enclave::Error> {
        enclave::public_key(blob)
    }
    fn agree(
        &self,
        blob: &[u8],
        peer: &[u8; POINT_LEN],
        reason: &str,
    ) -> Result<Zeroizing<[u8; SHARED_LEN]>, enclave::Error> {
        enclave::agree(blob, peer, reason, PROMPT_WAIT)
    }
}

pub struct Hardware<E: Enclave, K: Backend> {
    home: PathBuf,
    /// This device's id; the wrap is bound to it. Without one the chip is not used.
    device: Option<[u8; 16]>,
    enclave: E,
    keychain: K,
    slot: Slot,
    /// The unlock key opened by the last successful `confirm`, handed out once by `get_key`.
    opened: RefCell<Option<Zeroizing<[u8; 32]>>>,
}

impl<E: Enclave, K: Backend> Hardware<E, K> {
    pub fn new(home: PathBuf, device: Option<[u8; 16]>, enclave: E, keychain: K) -> Self {
        Self::with_slot(home, device, enclave, keychain, QUICK_SLOT)
    }

    pub fn with_slot(
        home: PathBuf,
        device: Option<[u8; 16]>,
        enclave: E,
        keychain: K,
        slot: Slot,
    ) -> Self {
        Hardware {
            home,
            device,
            enclave,
            keychain,
            slot,
            opened: RefCell::new(None),
        }
    }

    fn qk_path(&self) -> PathBuf {
        self.home.join(self.slot.qk)
    }

    fn blob_path(&self) -> PathBuf {
        self.home.join(self.slot.blob)
    }

    fn ids(&self, vault: &str) -> Option<([u8; 16], [u8; 16])> {
        let v: [u8; 16] = hex::decode(vault).ok()?.try_into().ok()?;
        Some((v, self.device?))
    }

    fn has_chip_key(&self) -> bool {
        self.qk_path().is_file() && self.blob_path().is_file()
    }

    fn write_private(&self, path: &PathBuf, bytes: &[u8]) -> Res<()> {
        config::private_dir(&self.home)?;
        let tmp = self.home.join(self.slot.tmp);
        let _ = fs::remove_file(&tmp);
        let written = config::create_private_file(&tmp).and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        });
        if let Err(e) = written.and_then(|()| fs::rename(&tmp, path)) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }

    /// Wrap `key` to a chip key, making one if there is none that still works.
    fn store_on_chip(&self, v: &[u8; 16], d: &[u8; 16], key: &[u8; 32]) -> Res<()> {
        let existing = fs::read(self.qk_path())
            .ok()
            .and_then(|blob| self.enclave.public_key(&blob).ok());
        let public = match existing {
            Some(public) => public,
            None => {
                let made = self
                    .enclave
                    .create()
                    .map_err(|_| Fail::new("could not create a Secure Enclave key"))?;
                self.write_private(&self.qk_path(), &made.blob)?;
                made.public
            }
        };
        let wrapped = seal_quick_unlock_se(&public, &Key::from_bytes(*key), v, d)?;
        self.write_private(&self.blob_path(), &wrapped)
    }

    /// The prompt: the chip asks the person, then the unlock key is unwrapped from what it computed.
    fn open_from_chip(&self, v: &[u8; 16], d: &[u8; 16], reason: &str) -> Outcome {
        let (Ok(qk), Ok(wrapped)) = (fs::read(self.qk_path()), fs::read(self.blob_path())) else {
            return Outcome::Unavailable;
        };
        let (Ok(public), Ok(eph)) = (
            self.enclave.public_key(&qk),
            quick_unlock_se_ephemeral_public(&wrapped),
        ) else {
            return Outcome::Unavailable;
        };
        match self.enclave.agree(&qk, &eph, reason) {
            Ok(shared) => match open_quick_unlock_se(&wrapped, &public, shared.as_slice(), v, d) {
                Ok(key) => {
                    *self.opened.borrow_mut() = Some(Zeroizing::new(*key.expose()));
                    Outcome::Confirmed
                }
                Err(_) => Outcome::Unavailable,
            },
            Err(enclave::Error::Declined) => Outcome::Declined,
            Err(enclave::Error::TimedOut) => Outcome::TimedOut,
            Err(_) => Outcome::Unavailable,
        }
    }
}

impl<E: Enclave, K: Backend> Backend for Hardware<E, K> {
    fn available(&self) -> bool {
        self.enclave.available() || self.keychain.available()
    }

    fn confirm(&self, vault: &str, reason: &str) -> Outcome {
        *self.opened.borrow_mut() = None;
        match self.ids(vault) {
            Some((v, d)) if self.has_chip_key() => self.open_from_chip(&v, &d, reason),
            _ => self.keychain.confirm(vault, reason),
        }
    }

    fn put_key(&self, vault: &str, key: &[u8; 32]) -> Res<()> {
        if self.enclave.available()
            && let Some((v, d)) = self.ids(vault)
            && self.store_on_chip(&v, &d, key).is_ok()
        {
            // Nothing is kept in the keychain any more.
            let _ = self.keychain.delete_key(vault);
            return Ok(());
        }
        for path in [self.qk_path(), self.blob_path()] {
            let _ = fs::remove_file(path);
        }
        self.keychain.put_key(vault, key)
    }

    fn get_key(&self, vault: &str) -> Res<Option<Zeroizing<[u8; 32]>>> {
        if let Some(key) = self.opened.borrow_mut().take() {
            return Ok(Some(key));
        }
        if self.has_chip_key() {
            // A chip key that was not opened by `confirm` is never read around the prompt.
            return Ok(None);
        }
        let key = self.keychain.get_key(vault)?;
        if let Some(key) = &key
            && self.enclave.available()
        {
            // Move an older keychain item onto the chip; on any failure it stays where it was.
            let _ = self.put_key(vault, key);
        }
        Ok(key)
    }

    fn has_key(&self, vault: &str) -> Res<bool> {
        if self.has_chip_key() {
            return Ok(true);
        }
        self.keychain.has_key(vault)
    }

    fn delete_key(&self, vault: &str) -> Res<()> {
        *self.opened.borrow_mut() = None;
        for path in [self.qk_path(), self.blob_path()] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.keychain.delete_key(vault)
    }

    fn boot_id(&self) -> String {
        self.keychain.boot_id()
    }

    fn protection(&self, vault: &str) -> Option<&'static str> {
        if self.has_chip_key() {
            Some("secure-enclave")
        } else {
            self.keychain.protection(vault)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::os::unix::fs::PermissionsExt;
    use std::rc::Rc;

    use p256::{PublicKey, SecretKey, ecdh::diffie_hellman, elliptic_curve::sec1::ToSec1Point};

    use super::*;
    use crate::quick::{Fallback, Fetch, Quick};

    const VAULT: &str = "0123456789abcdef0123456789abcdef";
    const DEVICE: [u8; 16] = [3; 16];
    const T0: u64 = 1_700_000_000;

    fn auk() -> Key {
        Key::random().unwrap()
    }

    /// A software P-256 key standing in for the chip. The blob is the secret scalar, which the real one never is.
    #[derive(Default)]
    struct ChipState {
        missing: Cell<bool>,
        answer: Cell<Option<enclave::Error>>,
        asked: Cell<u32>,
        created: Cell<u32>,
    }

    #[derive(Clone, Default)]
    struct FakeChip(Rc<ChipState>);

    impl Enclave for FakeChip {
        fn available(&self) -> bool {
            !self.0.missing.get()
        }
        fn create(&self) -> Result<enclave::NewKey, enclave::Error> {
            self.0.created.set(self.0.created.get() + 1);
            let mut b = [0u8; 32];
            getrandom::fill(&mut b).unwrap();
            let k = SecretKey::from_slice(&b).map_err(|_| enclave::Error::Failed)?;
            Ok(enclave::NewKey {
                blob: b.to_vec(),
                public: self.public_key(&b)?,
            })
            .inspect(|_| drop(k))
        }
        fn public_key(&self, blob: &[u8]) -> Result<[u8; POINT_LEN], enclave::Error> {
            let k = SecretKey::from_slice(blob).map_err(|_| enclave::Error::Failed)?;
            k.public_key()
                .to_sec1_point(false)
                .as_bytes()
                .try_into()
                .map_err(|_| enclave::Error::Failed)
        }
        fn agree(
            &self,
            blob: &[u8],
            peer: &[u8; POINT_LEN],
            _reason: &str,
        ) -> Result<Zeroizing<[u8; SHARED_LEN]>, enclave::Error> {
            self.0.asked.set(self.0.asked.get() + 1);
            if let Some(e) = self.0.answer.get() {
                return Err(e);
            }
            let k = SecretKey::from_slice(blob).map_err(|_| enclave::Error::Failed)?;
            let peer = PublicKey::from_sec1_bytes(peer).map_err(|_| enclave::Error::Failed)?;
            let shared = diffie_hellman(k.to_nonzero_scalar(), peer.as_affine());
            Ok(Zeroizing::new(
                shared.raw_secret_bytes().as_slice().try_into().unwrap(),
            ))
        }
    }

    #[derive(Default)]
    struct KeychainState {
        key: RefCell<Option<[u8; 32]>>,
        asked: Cell<u32>,
    }

    #[derive(Clone, Default)]
    struct MemKeychain(Rc<KeychainState>);

    impl Backend for MemKeychain {
        fn available(&self) -> bool {
            true
        }
        fn confirm(&self, _vault: &str, _reason: &str) -> Outcome {
            self.0.asked.set(self.0.asked.get() + 1);
            Outcome::Confirmed
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
            "boot".into()
        }
        fn protection(&self, _vault: &str) -> Option<&'static str> {
            self.0.key.borrow().is_some().then_some("keychain")
        }
    }

    struct Rig {
        dir: tempfile::TempDir,
        chip: FakeChip,
        keychain: MemKeychain,
    }

    impl Rig {
        fn new() -> Rig {
            Rig {
                dir: tempfile::tempdir().unwrap(),
                chip: FakeChip::default(),
                keychain: MemKeychain::default(),
            }
        }
        fn hw(&self) -> Hardware<FakeChip, MemKeychain> {
            Hardware::new(
                self.dir.path().to_path_buf(),
                Some(DEVICE),
                self.chip.clone(),
                self.keychain.clone(),
            )
        }
        fn quick(&self) -> Quick<Hardware<FakeChip, MemKeychain>> {
            Quick::new(self.dir.path().to_path_buf(), self.hw(), 3600)
        }
        fn qk(&self) -> PathBuf {
            self.dir.path().join(QUICK_SLOT.qk)
        }
        fn blob(&self) -> PathBuf {
            self.dir.path().join(QUICK_SLOT.blob)
        }
    }

    fn opened(f: Fetch) -> Option<[u8; 32]> {
        match f {
            Fetch::Key(k) => Some(*k.expose()),
            Fetch::Full(_) => None,
        }
    }

    #[test]
    fn the_unlock_key_goes_to_the_chip_and_comes_back_after_the_prompt() {
        let rig = Rig::new();
        let key = auk();
        rig.quick().enable(VAULT, &key, T0).unwrap();
        assert!(rig.qk().is_file() && rig.blob().is_file());
        assert!(
            rig.keychain.0.key.borrow().is_none(),
            "nothing in the keychain"
        );
        assert_eq!(rig.chip.0.asked.get(), 0, "wrapping asks nobody");

        let got = rig.quick().fetch(VAULT, T0 + 60).unwrap();
        assert_eq!(opened(got), Some(*key.expose()));
        assert_eq!(rig.chip.0.asked.get(), 1, "one prompt to open it");
        assert_eq!(rig.keychain.0.asked.get(), 0, "the app prompt is not used");
        assert_eq!(rig.hw().protection(VAULT), Some("secure-enclave"));
    }

    #[test]
    fn a_refill_without_the_chip_leaves_no_chip_files_and_the_keychain_key_opens() {
        let rig = Rig::new();
        let key = auk();
        rig.quick().enable(VAULT, &key, T0).unwrap();
        assert!(rig.qk().is_file() && rig.blob().is_file());

        rig.chip.0.missing.set(true);
        rig.hw().put_key(VAULT, key.expose()).unwrap();
        assert!(!rig.qk().exists() && !rig.blob().exists());
        assert!(!rig.hw().has_chip_key());
        let got = rig.hw().get_key(VAULT).unwrap();
        assert_eq!(got.as_deref(), Some(key.expose()));
        assert_eq!(rig.hw().protection(VAULT), Some("keychain"));
    }

    #[test]
    fn a_failed_blob_write_leaves_no_chip_key_behind() {
        let rig = Rig::new();
        let key = auk();
        fs::create_dir(rig.blob()).unwrap();

        rig.hw().put_key(VAULT, key.expose()).unwrap();
        assert!(!rig.qk().exists(), "no new chip key without its wrap");
        assert!(!rig.hw().has_chip_key());
        let got = rig.hw().get_key(VAULT).unwrap();
        assert_eq!(got.as_deref(), Some(key.expose()));
    }

    #[test]
    fn nothing_on_disk_holds_the_key_or_any_secret_in_the_clear() {
        let rig = Rig::new();
        let key = auk();
        rig.quick().enable(VAULT, &key, T0).unwrap();
        for path in [
            rig.qk(),
            rig.blob(),
            rig.dir.path().join("quick-unlock.json"),
        ] {
            let raw = fs::read(&path).unwrap();
            assert!(
                !raw.windows(32).any(|w| w == key.expose()),
                "{path:?} holds the key"
            );
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600,
                "{path:?}"
            );
        }
    }

    #[test]
    fn declining_the_chip_prompt_gives_no_key_and_five_declines_delete_it() {
        let rig = Rig::new();
        rig.quick().enable(VAULT, &auk(), T0).unwrap();
        rig.chip.0.answer.set(Some(enclave::Error::Declined));
        for n in 1..=5u64 {
            let got = rig.quick().fetch(VAULT, T0 + n).unwrap();
            assert!(
                matches!(got, Fetch::Full(Fallback::NotConfirmed)),
                "try {n}"
            );
        }
        assert!(
            !rig.qk().exists() && !rig.blob().exists(),
            "files removed after five"
        );
        assert!(matches!(
            rig.quick().fetch(VAULT, T0 + 6).unwrap(),
            Fetch::Full(_)
        ));
    }

    #[test]
    fn a_timeout_or_a_chip_failure_falls_back_to_the_master_password() {
        let rig = Rig::new();
        rig.quick().enable(VAULT, &auk(), T0).unwrap();
        for e in [
            enclave::Error::TimedOut,
            enclave::Error::Unavailable,
            enclave::Error::Failed,
        ] {
            rig.chip.0.answer.set(Some(e));
            assert!(matches!(
                rig.quick().fetch(VAULT, T0 + 1).unwrap(),
                Fetch::Full(Fallback::NotConfirmed)
            ));
        }
        // None of those counted as a failed attempt, so the key is still there.
        rig.chip.0.answer.set(None);
        assert!(opened(rig.quick().fetch(VAULT, T0 + 2).unwrap()).is_some());
    }

    #[test]
    fn the_wrong_chip_key_a_removed_key_and_a_tampered_blob_never_open() {
        let key = auk();

        // Another chip key in place of the real one: the shared secret differs, so the tag fails.
        let rig = Rig::new();
        rig.quick().enable(VAULT, &key, T0).unwrap();
        fs::write(rig.qk(), rig.chip.create().unwrap().blob).unwrap();
        assert!(
            opened(rig.quick().fetch(VAULT, T0 + 1).unwrap()).is_none(),
            "wrong key"
        );

        // The key file removed while the wrapped blob stays: nothing is kept any more.
        let rig = Rig::new();
        rig.quick().enable(VAULT, &key, T0).unwrap();
        fs::remove_file(rig.qk()).unwrap();
        assert!(!rig.hw().has_key(VAULT).unwrap());
        assert!(
            opened(rig.quick().fetch(VAULT, T0 + 1).unwrap()).is_none(),
            "removed key"
        );

        // Any flipped byte of the wrapped blob.
        let rig = Rig::new();
        rig.quick().enable(VAULT, &key, T0).unwrap();
        let blob = fs::read(rig.blob()).unwrap();
        for at in [0, 1, 40, 66, blob.len() - 1] {
            let mut bad = blob.clone();
            bad[at] ^= 1;
            fs::write(rig.blob(), &bad).unwrap();
            assert!(
                opened(rig.quick().fetch(VAULT, T0 + 1).unwrap()).is_none(),
                "byte {at}"
            );
        }
        // Another vault or another device cannot open it either.
        fs::write(rig.blob(), &blob).unwrap();
        let other_vault = "ffffffffffffffffffffffffffffffff";
        assert!(rig.hw().confirm(other_vault, "t") != Outcome::Confirmed);
        let other_device = Hardware::new(
            rig.dir.path().to_path_buf(),
            Some([9; 16]),
            rig.chip.clone(),
            rig.keychain.clone(),
        );
        assert!(other_device.confirm(VAULT, "t") != Outcome::Confirmed);
        assert_eq!(rig.hw().confirm(VAULT, "t"), Outcome::Confirmed);
    }

    #[test]
    fn an_older_keychain_item_moves_to_the_chip_on_its_next_unlock() {
        let rig = Rig::new();
        let key = auk();
        rig.keychain.put_key(VAULT, key.expose()).unwrap();
        // State as the previous version left it: quick unlock on, key in the keychain.
        let seed = Rig::new();
        seed.quick().enable(VAULT, &auk(), T0).unwrap();
        fs::copy(
            seed.dir.path().join("quick-unlock.json"),
            rig.dir.path().join("quick-unlock.json"),
        )
        .unwrap();

        let got = rig.quick().fetch(VAULT, T0 + 60).unwrap();
        assert_eq!(
            opened(got),
            Some(*key.expose()),
            "still opens, via the keychain prompt"
        );
        assert_eq!(rig.keychain.0.asked.get(), 1);
        assert!(
            rig.qk().is_file() && rig.blob().is_file(),
            "moved to the chip"
        );
        assert!(
            rig.keychain.0.key.borrow().is_none(),
            "keychain item removed"
        );

        let again = rig.quick().fetch(VAULT, T0 + 120).unwrap();
        assert_eq!(opened(again), Some(*key.expose()), "same key from the chip");
        assert_eq!(rig.chip.0.asked.get(), 1);
        assert_eq!(rig.keychain.0.asked.get(), 1, "no more keychain prompts");
    }

    #[test]
    fn without_a_secure_enclave_the_keychain_is_used_exactly_as_before() {
        let rig = Rig::new();
        rig.chip.0.missing.set(true);
        let key = auk();
        rig.quick().enable(VAULT, &key, T0).unwrap();
        assert!(!rig.qk().exists() && !rig.blob().exists());
        assert!(rig.keychain.0.key.borrow().is_some());
        assert_eq!(
            opened(rig.quick().fetch(VAULT, T0 + 1).unwrap()),
            Some(*key.expose())
        );
        assert_eq!(rig.keychain.0.asked.get(), 1);
        assert_eq!(rig.hw().protection(VAULT), Some("keychain"));
    }

    #[test]
    fn a_chip_that_fails_to_make_a_key_falls_back_to_the_keychain() {
        // No device id means the wrap cannot be bound, which behaves like no chip.
        let rig = Rig::new();
        let hw = Hardware::new(
            rig.dir.path().to_path_buf(),
            None,
            rig.chip.clone(),
            rig.keychain.clone(),
        );
        hw.put_key(VAULT, auk().expose()).unwrap();
        assert!(!rig.qk().exists());
        assert!(rig.keychain.0.key.borrow().is_some());
        assert_eq!(rig.chip.0.created.get(), 0);
    }

    #[test]
    fn disabling_removes_the_chip_key_the_blob_and_the_state() {
        let rig = Rig::new();
        rig.quick().enable(VAULT, &auk(), T0).unwrap();
        rig.quick().forget(VAULT).unwrap();
        assert!(!rig.qk().exists() && !rig.blob().exists());
        assert!(!rig.dir.path().join("quick-unlock.json").exists());
        assert!(!rig.hw().has_key(VAULT).unwrap());
    }

    #[test]
    fn a_new_full_unlock_reuses_the_chip_key_and_wraps_the_new_unlock_key() {
        let rig = Rig::new();
        rig.quick().enable(VAULT, &auk(), T0).unwrap();
        let qk_before = fs::read(rig.qk()).unwrap();
        let newer = auk();
        rig.quick().remember(VAULT, &newer, T0 + 10).unwrap();
        assert_eq!(fs::read(rig.qk()).unwrap(), qk_before, "same chip key");
        assert_eq!(rig.chip.0.created.get(), 1);
        assert_eq!(
            opened(rig.quick().fetch(VAULT, T0 + 20).unwrap()),
            Some(*newer.expose())
        );
    }
}
