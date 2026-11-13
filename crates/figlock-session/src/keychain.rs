//! The login keychain items FigLock keeps (device keys, the Secret Key, the quick unlock key), written so the
//! command line tool and the desktop app read each other's items without a keychain prompt (FIG-106).
//!
//! Reading and deleting use the plain `security-framework` calls; only creating needs the shared access list
//! (`figlock_biometric::keychain`), so that is what [`put`] adds.

use figlock_biometric::keychain::{ERR_SEC_DUPLICATE_ITEM, add_shared_generic_password};
use security_framework::base::Error;
use security_framework::passwords::{delete_generic_password, set_generic_password};

/// Save `secret` under `service` and `account`, readable by any app of this developer team.
///
/// An item written before FIG-106 has a per-app access list that `set_generic_password` would keep, so it is
/// replaced to get the new one. When it cannot be removed (another app made it and the person declined the
/// prompt) the secret is still updated in place and the old list stays until the item is next recreated. If
/// the shared item cannot be added after the old one was removed, the secret is stored with the default list
/// instead of being lost.
pub fn put(service: &str, account: &str, secret: &[u8]) -> Result<(), Error> {
    match add_shared_generic_password(service, account, secret) {
        Ok(()) => Ok(()),
        Err(ERR_SEC_DUPLICATE_ITEM) => {
            if delete_generic_password(service, account).is_ok() {
                add_shared_generic_password(service, account, secret)
                    .or_else(|_| set_generic_password(service, account, secret))
            } else {
                set_generic_password(service, account, secret)
            }
        }
        Err(code) => Err(Error::from_code(code)),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use security_framework::passwords::get_generic_password;

    use super::*;

    /// Writes to the real login keychain, so it runs only on request:
    /// `cargo test -p figlock-session --lib -- --ignored keychain`
    #[test]
    #[ignore = "writes a throwaway item to the login keychain"]
    fn an_item_from_before_the_shared_list_is_replaced_and_keeps_its_secret_current() {
        let service = "com.figlock.test.session-keychain";
        let account = format!("{:032x}", std::process::id());
        let cleanup = || {
            let _ = delete_generic_password(service, &account);
        };
        cleanup();

        // The old way: the default list, which trusts only this binary.
        set_generic_password(service, &account, b"old").expect("legacy item");
        put(service, &account, b"new").expect("put over it");
        assert_eq!(get_generic_password(service, &account).unwrap(), b"new");

        let dump = Command::new("/usr/bin/security")
            .args(["dump-keychain", "-a"])
            .output()
            .expect("run security");
        let dump = String::from_utf8_lossy(&dump.stdout).into_owned();
        cleanup();
        let block = dump
            .split("keychain: ")
            .find(|b| b.contains(&account))
            .expect("the item is in the keychain");
        let read = block
            .split("entry ")
            .find(|e| e.contains("decrypt"))
            .expect("a read entry");
        assert!(read.contains("applications: <null>"), "{block}");
    }
}
