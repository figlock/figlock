//! The new device's side of pairing (`pairing-recovery.md` 3): read the invite, leave a request, wait for the
//! trusted device to answer, then unlock with the master password. Nothing is saved on this device until the
//! answer has opened and the vault has unlocked, so an abandoned join leaves no trace but the request file.

use figlock_core::{DeviceKeys, JoinAttempt, Joined, device_fingerprint, invite_path};
use figlock_store::{BlobStore, StoreError};

use crate::config::{Config, StoreSpec};
use crate::error::{Fail, Res};

/// A join that has asked and is waiting for the trusted device to approve it.
pub struct Join {
    attempt: JoinAttempt,
    store: Box<dyn BlobStore>,
    spec: StoreSpec,
    device_name: String,
    fingerprint: String,
}

fn bad_invite() -> Fail {
    Fail::new("that invite code is not valid, has expired, or belongs to a different vault")
}

/// Send the request for the invite `code` found in the storage `spec`.
pub fn begin(spec: StoreSpec, code: &str, device_name: &str, now_ms: u64) -> Res<Join> {
    let store = crate::store::open(&spec)?;
    let vault_json = store.get("vault.json").map_err(|e| match e {
        StoreError::NotFound => Fail::new("there is no FigLock vault in that storage"),
        e => e.into(),
    })?;
    let path = invite_path(code, &vault_json).map_err(|_| bad_invite())?;
    let inv = store.get(&path).map_err(|e| match e {
        StoreError::NotFound => bad_invite(),
        e => e.into(),
    })?;
    let device = DeviceKeys::generate()?;
    let fingerprint = device_fingerprint(&device.sign_public());
    let (attempt, request) =
        JoinAttempt::begin(code, &inv, device, device_name, now_ms).map_err(|_| bad_invite())?;
    match store.put_new(&request.path, &request.bytes) {
        Ok(()) => {}
        Err(StoreError::Exists) => {
            return Err(Fail::new(
                "someone already answered that invite; ask for a new code",
            ));
        }
        Err(e) => return Err(e.into()),
    }
    Ok(Join {
        attempt,
        store,
        spec,
        device_name: device_name.to_owned(),
        fingerprint,
    })
}

impl Join {
    /// Shown here and on the trusted device so the person can check they match.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Whether the trusted device has answered.
    pub fn answered(&self) -> Res<bool> {
        match self.store.get(&self.attempt.resp_path()) {
            Ok(_) => Ok(true),
            Err(StoreError::NotFound) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Unlock with the answer and the master password. Returns the unlocked vault and the config to save.
    pub fn finish(&self, password: &str, now_ms: u64) -> Res<(Joined, Config)> {
        let resp = self.store.get(&self.attempt.resp_path())?;
        let vault_json = self.store.get("vault.json")?;
        let mut records = Vec::new();
        for e in self.store.list("devices/")? {
            if let Ok(bytes) = self.store.get(&e.path) {
                records.push(bytes);
            }
        }
        let joined = self
            .attempt
            .finish(&resp, &vault_json, &records, password, now_ms)
            .map_err(|_| {
                Fail::new("could not join: wrong master password, or the answer is not valid")
            })?;
        Ok((
            joined,
            Config {
                store: self.spec.clone(),
                device_name: self.device_name.clone(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use figlock_core::{Invite, KdfParams, Vault};
    use figlock_store::LocalFolder;

    const T0: u64 = 1_700_000_000_000;

    /// A vault in a folder, and the trusted device's side of pairing played by the test.
    #[test]
    fn a_new_device_joins_after_the_trusted_one_approves() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFolder::open(dir.path()).unwrap();
        let (mut vault, created) =
            Vault::create("pw-for-test", "first", KdfParams::FLOOR, T0).unwrap();
        store.put_new("vault.json", &created.vault_json).unwrap();
        let own = format!("devices/{}.json", hex::encode(vault.device_id()));
        store.put_new(&own, &created.device_record).unwrap();

        let (mut invite, offer) = Invite::create(vault.vault_id(), T0).unwrap();
        store.put_new(&offer.inv_path, &offer.inv).unwrap();

        let join = begin(
            StoreSpec::Folder(dir.path().into()),
            &offer.code,
            "second",
            T0 + 1,
        )
        .unwrap();
        assert!(!join.answered().unwrap());

        let req = invite
            .open_request(&store.get(&invite.req_path()).unwrap(), T0 + 2)
            .unwrap();
        assert_eq!(req.fingerprint(), join.fingerprint());
        let approval = vault
            .approve_join(&invite, &req, &created.secret_key, T0 + 3)
            .unwrap();
        store
            .put_new(&approval.record.path, &approval.record.bytes)
            .unwrap();
        store
            .put_new(&approval.response.path, &approval.response.bytes)
            .unwrap();
        assert!(join.answered().unwrap());

        assert_eq!(join.fingerprint().len(), 19);
        let (joined, cfg) = join.finish("pw-for-test", T0 + 4).unwrap();
        assert_eq!(joined.vault.vault_id(), vault.vault_id());
        assert_eq!(cfg.device_name, "second");
    }

    #[test]
    fn a_wrong_master_password_does_not_join() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFolder::open(dir.path()).unwrap();
        let (mut vault, created) =
            Vault::create("right-pw", "first", KdfParams::FLOOR, T0).unwrap();
        store.put_new("vault.json", &created.vault_json).unwrap();
        let (mut invite, offer) = Invite::create(vault.vault_id(), T0).unwrap();
        store.put_new(&offer.inv_path, &offer.inv).unwrap();
        let join = begin(
            StoreSpec::Folder(dir.path().into()),
            &offer.code,
            "second",
            T0 + 1,
        )
        .unwrap();
        let req = invite
            .open_request(&store.get(&invite.req_path()).unwrap(), T0 + 2)
            .unwrap();
        let approval = vault
            .approve_join(&invite, &req, &created.secret_key, T0 + 3)
            .unwrap();
        store
            .put_new(&approval.record.path, &approval.record.bytes)
            .unwrap();
        store
            .put_new(&approval.response.path, &approval.response.bytes)
            .unwrap();
        assert!(join.finish("wrong-pw", T0 + 4).is_err());
        let (joined, _) = join.finish("right-pw", T0 + 5).unwrap();
        assert_eq!(joined.vault.vault_id(), vault.vault_id());
    }

    #[test]
    fn a_wrong_or_missing_invite_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFolder::open(dir.path()).unwrap();
        let (vault, created) = Vault::create("pw", "first", KdfParams::FLOOR, T0).unwrap();
        store.put_new("vault.json", &created.vault_json).unwrap();
        let (_, offer) = Invite::create(vault.vault_id(), T0).unwrap();
        // The invite was never stored.
        let spec = || StoreSpec::Folder(dir.path().into());
        assert!(begin(spec(), &offer.code, "second", T0 + 1).is_err());
        assert!(begin(spec(), "not a code", "second", T0 + 1).is_err());
        // Expired.
        store.put_new(&offer.inv_path, &offer.inv).unwrap();
        assert!(begin(spec(), &offer.code, "second", T0 + 11 * 60 * 1000).is_err());
    }
}
