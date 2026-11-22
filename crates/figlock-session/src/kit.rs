//! The Emergency Kit made on demand from the Secret Key this device keeps (FIG-134, `pairing-recovery.md` 5).
//! It is the same pure `render_kit` as at creation and rotation; what is added here is the gate: the key is
//! read with the same Touch ID or device password prompt as `secret-key show`, and the bytes go only where the
//! caller writes them.

use figlock_core::{KitInfo, SecretKey, render_kit};
use zeroize::Zeroizing;

use crate::config::{Place, Remote};
use crate::error::{Fail, Res};
use crate::quick::Backend;
use crate::secret_key_store::SecretKeyStore;
use crate::session::civil_date;

/// What the kit says about the vault, not about this device's disk: a long path would not fit.
pub fn storage_label(place: &Place) -> String {
    match place.remotes.first() {
        Some(Remote::Drive) => "Google Drive".to_owned(),
        // The kit says where the vault is kept, and a sync folder is a copy of the local one.
        Some(Remote::Folder(_)) | None => {
            let name = place
                .local
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            format!("Folder {}", name.chars().take(60).collect::<String>())
        }
    }
}

/// The kit PDF for `key`. Its date line says when the kit was made.
pub fn kit_pdf(place: &Place, key: &SecretKey, now_ms: u64) -> Res<Zeroizing<Vec<u8>>> {
    let storage = storage_label(place);
    let info = KitInfo {
        vault_name: "Personal",
        storage: &storage,
        created: &civil_date(now_ms),
    };
    render_kit(&info, key).map_err(|_| Fail::new("Could not make the Emergency Kit."))
}

/// A kit from the key kept on this device. Reading the key asks for Touch ID or the device password exactly
/// as showing it does; when that is not confirmed, or no key is kept, there is no kit.
pub fn kit_from_kept_key<B: Backend>(
    keys: &SecretKeyStore<B>,
    vault: &str,
    quick_on: bool,
    place: &Place,
    now_ms: u64,
) -> Res<Option<Zeroizing<Vec<u8>>>> {
    match keys.get(vault, "save your FigLock Emergency Kit", quick_on)? {
        Some(key) => kit_pdf(place, &key, now_ms).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

    use figlock_biometric::Outcome;

    use super::*;

    const VAULT: &str = "0123456789abcdef0123456789abcdef";

    #[derive(Default)]
    struct Shared {
        key: RefCell<Option<[u8; 32]>>,
        answer: Cell<Option<Outcome>>,
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<Shared>);

    impl Backend for Fake {
        fn available(&self) -> bool {
            true
        }
        fn confirm(&self, _: &str, _: &str) -> Outcome {
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

    fn place() -> Place {
        Place {
            local: PathBuf::from("/somewhere/vault"),
            remotes: Vec::new(),
        }
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn the_kit_holds_the_key_that_show_would_print_and_says_when_it_was_made() {
        let fake = Fake::default();
        let keys = SecretKeyStore::new(fake.clone(), true);
        let key = SecretKey::random().unwrap();
        keys.put(VAULT, &key, 1).unwrap();
        let shown = keys.get(VAULT, "show", true).unwrap().unwrap().format();
        let pdf = kit_from_kept_key(&keys, VAULT, true, &place(), 1_794_000_000_000)
            .unwrap()
            .unwrap();
        assert!(contains(&pdf, shown.as_bytes()));
        assert!(contains(&pdf, b"Folder vault"));
        assert!(contains(&pdf, civil_date(1_794_000_000_000).as_bytes()));
    }

    #[test]
    fn without_the_prompt_confirmed_there_is_no_kit() {
        let fake = Fake::default();
        let keys = SecretKeyStore::new(fake.clone(), true);
        keys.put(VAULT, &SecretKey::random().unwrap(), 1).unwrap();
        for answer in [Outcome::Declined, Outcome::TimedOut, Outcome::Unavailable] {
            fake.0.answer.set(Some(answer));
            assert!(
                kit_from_kept_key(&keys, VAULT, true, &place(), 0)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn with_no_key_kept_there_is_no_kit() {
        let keys = SecretKeyStore::new(Fake::default(), true);
        assert!(
            kit_from_kept_key(&keys, VAULT, true, &place(), 0)
                .unwrap()
                .is_none()
        );
    }
}
