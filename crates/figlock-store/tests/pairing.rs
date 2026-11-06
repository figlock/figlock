//! Pairing and revocation over a real local folder (FIG-33): the files the core produces are written with
//! the store's create-only and replace operations, read back by other devices, and cleaned up the way
//! `pairing-recovery.md` 3 and 4 describe. `Dev::sync` is a minimal stand-in for the sync engine.

use std::collections::BTreeMap;

use figlock_core::{
    DeviceKeys, InboxKind, Invite, ItemType, JoinAttempt, KdfParams, SecretKey, Vault, invite_path,
    parse_inbox_path,
};
use figlock_store::{BlobStore, LocalFolder, StoreError};

#[cfg(feature = "gdrive")]
mod support;

const T0: u64 = 1_700_000_000_000;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn password() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).unwrap();
    hex(&b)
}

/// A device: its vault and how far it has read each device's log.
struct Dev {
    vault: Vault,
    applied: BTreeMap<[u8; 16], u64>,
    /// Files the vault refused, by path.
    refused: Vec<String>,
}

impl Dev {
    fn new(vault: Vault) -> Dev {
        Dev {
            vault,
            applied: BTreeMap::new(),
            refused: Vec::new(),
        }
    }

    /// Commit pending changes and store the op file.
    fn push(&mut self, store: &impl BlobStore) {
        let file = self.vault.commit().unwrap().expect("something to write");
        store.put_new(&file.path, &file.bytes).unwrap();
        let seq = self
            .applied
            .get(self.vault.device_id())
            .copied()
            .unwrap_or(0)
            + 1;
        self.applied.insert(*self.vault.device_id(), seq);
    }

    /// Read device records and any new op files, in order per device.
    fn sync(&mut self, store: &impl BlobStore, now: u64) {
        for e in store.list("devices/").unwrap() {
            let _ = self.vault.add_device_record(&store.get(&e.path).unwrap());
        }
        let mut files: Vec<(String, [u8; 16], u64)> = store
            .list("log/")
            .unwrap()
            .into_iter()
            .filter_map(|e| {
                let rest = e.path.strip_prefix("log/")?;
                let (dev, file) = rest.split_once('/')?;
                let seq: u64 = file.strip_suffix(".op")?.parse().ok()?;
                let id: [u8; 16] = (0..16)
                    .map(|i| u8::from_str_radix(&dev[i * 2..i * 2 + 2], 16).ok())
                    .collect::<Option<Vec<u8>>>()?
                    .try_into()
                    .ok()?;
                Some((e.path, id, seq))
            })
            .collect();
        files.sort_by_key(|(_, id, seq)| (*id, *seq));
        for (path, id, seq) in files {
            if seq != self.applied.get(&id).copied().unwrap_or(0) + 1 {
                continue;
            }
            match self
                .vault
                .apply_file(&store.get(&path).unwrap(), &id, seq, now)
            {
                Ok(()) => {
                    self.applied.insert(id, seq);
                }
                Err(_) => self.refused.push(path),
            }
        }
    }

    fn titles(&mut self) -> Vec<String> {
        let (items, corrupt) = self.vault.items(false);
        assert!(corrupt.is_empty());
        let mut t: Vec<String> = items
            .iter()
            .map(|i| i.fields["title"].to_string())
            .collect();
        t.sort();
        t
    }
}

/// Pair a new device through the folder, as `pairing-recovery.md` 3.2 draws it.
fn pair(
    store: &impl BlobStore,
    a: &mut Dev,
    sk: &SecretKey,
    pw: &str,
    name: &str,
    now: u64,
) -> Dev {
    // T: open an invite and show the code.
    let (mut invite, offer) = Invite::create(a.vault.vault_id(), now).unwrap();
    store.put_new(&offer.inv_path, &offer.inv).unwrap();

    // N: has the storage, so it can read vault.json, find the invite from the code and answer it.
    let vault_json = store.get("vault.json").unwrap();
    let path = invite_path(&offer.code, &vault_json).unwrap();
    assert_eq!(path, offer.inv_path);
    let (attempt, req) = JoinAttempt::begin(
        &offer.code,
        &store.get(&path).unwrap(),
        DeviceKeys::generate().unwrap(),
        name,
        now + 1,
    )
    .unwrap();
    store.put_new(&req.path, &req.bytes).unwrap();

    // T: find the request among whatever is in inbox/, ask the user, approve, clean up.
    let req_path = store
        .list("inbox/")
        .unwrap()
        .into_iter()
        .find(|e| {
            matches!(parse_inbox_path(&e.path), Some((InboxKind::Request, id)) if id == *invite.invite_id())
        })
        .expect("the request is in the inbox")
        .path;
    let opened = invite
        .open_request(&store.get(&req_path).unwrap(), now + 2)
        .unwrap();
    assert_eq!(opened.device_name, name);
    let ap = a.vault.approve_join(&invite, &opened, sk, now + 3).unwrap();
    store.put_new(&ap.record.path, &ap.record.bytes).unwrap();
    store
        .put_new(&ap.response.path, &ap.response.bytes)
        .unwrap();
    store.delete(&invite.inv_path()).unwrap();
    store.delete(&req_path).unwrap();

    // N: read the answer and the records, unlock with the typed password, delete the answer.
    let records: Vec<Vec<u8>> = store
        .list("devices/")
        .unwrap()
        .iter()
        .map(|e| store.get(&e.path).unwrap())
        .collect();
    let resp_path = attempt.resp_path();
    let joined = attempt
        .finish(
            &store.get(&resp_path).unwrap(),
            &vault_json,
            &records,
            pw,
            now + 4,
        )
        .unwrap();
    store.delete(&resp_path).unwrap();
    assert_eq!(&*joined.secret_key.format(), &*sk.format());
    Dev::new(joined.vault)
}

#[test]
fn pair_and_revoke_over_a_local_folder() {
    let dir = tempfile::tempdir().unwrap();
    pair_and_revoke(LocalFolder::open(dir.path()).unwrap());
}

/// The same flow through the Google Drive adapter, against the fake Drive.
#[cfg(feature = "gdrive")]
#[test]
fn pair_and_revoke_over_google_drive() {
    let fake = support::fake_drive::FakeDrive::start();
    let drive = figlock_store::GoogleDrive::with_endpoint(
        &fake.base,
        figlock_store::StaticToken("good-token".into()),
    )
    .unwrap();
    pair_and_revoke(drive);
    // Everything stored is a flat, validated name: nothing but FigLock's own paths.
    for name in fake.names() {
        figlock_store::validate_path(&name).unwrap();
    }
}

fn pair_and_revoke(store: impl BlobStore) {
    let pw = password();

    // Device A creates the vault in the folder and stores a login.
    let (vault, created) = Vault::create(&pw, "Laptop", KdfParams::FLOOR, T0).unwrap();
    let sk = SecretKey::parse(&created.secret_key.format()).unwrap();
    let mut a = Dev::new(vault);
    store
        .put_replace("vault.json", &created.vault_json)
        .unwrap();
    store
        .put_new(
            &format!("devices/{}.json", hex(a.vault.device_id())),
            &created.device_record,
        )
        .unwrap();
    a.vault
        .add_item(
            ItemType::Login,
            "personal",
            &[("title", "Cloudflare"), ("password", "generated-for-test")],
            T0 + 10,
        )
        .unwrap();
    a.push(&store);

    // B and C join; the inbox is empty again afterwards.
    let mut b = pair(&store, &mut a, &sk, &pw, "Phone", T0 + 100);
    let mut c = pair(&store, &mut a, &sk, &pw, "Work PC", T0 + 200);
    assert!(
        store.list("inbox/").unwrap().is_empty(),
        "invites are cleaned up"
    );
    for d in [&mut a, &mut b, &mut c] {
        d.sync(&store, T0 + 300);
    }
    assert_eq!(b.titles(), ["Cloudflare"]);
    assert_eq!(c.titles(), ["Cloudflare"]);

    // C writes something, then A revokes C. The files go out in the order the docs recommend.
    c.vault
        .add_item(
            ItemType::Note,
            "personal",
            &[("title", "From the work PC"), ("body", "written before")],
            T0 + 350,
        )
        .unwrap();
    c.push(&store);
    a.sync(&store, T0 + 360);
    let c_id = *c.vault.device_id();
    let r = a.vault.revoke_device(&pw, &sk, &c_id, T0 + 400).unwrap();
    store.put_new(&r.snapshot.path, &r.snapshot.bytes).unwrap();
    for rec in &r.records {
        store.put_replace(&rec.path, &rec.bytes).unwrap();
    }
    for path in &r.remove {
        store.delete(path).unwrap();
    }
    store.put_replace("vault.json", &r.vault_json).unwrap();

    // A keeps working in the new epoch.
    a.vault
        .add_item(
            ItemType::Login,
            "personal",
            &[
                ("title", "After revocation"),
                ("password", "generated-for-test"),
            ],
            T0 + 500,
        )
        .unwrap();
    a.push(&store);

    // C, still on epoch 1, cannot read what A wrote afterwards and cannot see itself in devices/.
    c.sync(&store, T0 + 600);
    assert!(
        c.refused
            .iter()
            .any(|p| p.contains(&hex(a.vault.device_id()))),
        "C refused A's new-epoch file"
    );
    assert!(!c.titles().contains(&"After revocation".to_owned()));
    assert!(matches!(
        store.get(&format!("devices/{}.json", hex(&c_id))),
        Err(StoreError::NotFound)
    ));

    // B unlocks the new epoch from the folder with the same credentials and reads everything,
    // including what C wrote before it was revoked.
    let vault_json = store.get("vault.json").unwrap();
    let records: Vec<Vec<u8>> = store
        .list("devices/")
        .unwrap()
        .iter()
        .map(|e| store.get(&e.path).unwrap())
        .collect();
    let keys = DeviceKeys {
        device_id: *b.vault.device_id(),
        sign_seed: figlock_core::Key::from_bytes(*b.vault.device_keys().sign_seed.expose()),
        agree_priv: figlock_core::Key::from_bytes(*b.vault.device_keys().agree_priv.expose()),
    };
    let (vault, fresh) =
        Vault::unlock(&vault_json, &pw, &sk, keys, "Phone", &records, T0 + 700).unwrap();
    assert!(fresh.is_none());
    let mut b2 = Dev::new(vault);
    b2.sync(&store, T0 + 710);
    assert!(b2.refused.is_empty(), "{:?}", b2.refused);
    assert_eq!(
        b2.titles(),
        ["After revocation", "Cloudflare", "From the work PC"]
    );

    // Everything in the folder is ciphertext or public metadata.
    for e in store.list("").unwrap() {
        let bytes = store.get(&e.path).unwrap();
        for secret in ["Cloudflare", "generated-for-test", "From the work PC", &pw] {
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "{} holds plaintext",
                e.path
            );
        }
    }
}
