//! The sync engine through the exported API, with the app's parts (store, clock, sync state, token) played by
//! Rust doubles that implement the same foreign traits Swift implements. Every value is generated.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use figlock_core::{KdfParams, Vault};
use figlock_ffi::{
    AccessTokens, BlobEntry, BlobStoreFfi, DriveStore, FolderStore, StoreFailure, SyncClock,
    SyncEngine, SyncFailure, SyncStateStore, SyncWarning, delete_remote_confirmed,
    delete_remote_phrase, sync_backoff_secs, sync_interval_secs, unlock_vault,
};

const T0: u64 = 1_800_000_000_000;

/// A store held in memory: what a Swift `BlobStore` over a folder looks like to the engine.
#[derive(Default)]
struct Mem {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    /// Paths listed but not downloaded yet.
    pending: Mutex<Vec<String>>,
    /// When set, every call fails with it.
    fail: Mutex<Option<StoreFailure>>,
}

impl Mem {
    fn new() -> Arc<Self> {
        Arc::new(Mem::default())
    }

    fn put(&self, path: &str, bytes: &[u8]) {
        self.files.lock().unwrap().insert(path.into(), bytes.into());
    }

    fn get_raw(&self, path: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(path).cloned()
    }

    fn names(&self) -> Vec<String> {
        self.files.lock().unwrap().keys().cloned().collect()
    }

    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        self.files.lock().unwrap().clone()
    }

    fn check(&self) -> Result<(), StoreFailure> {
        self.fail.lock().unwrap().map_or(Ok(()), Err)
    }
}

impl BlobStoreFfi for Mem {
    fn get(&self, path: String) -> Result<Vec<u8>, StoreFailure> {
        self.check()?;
        if self.pending.lock().unwrap().contains(&path) {
            return Err(StoreFailure::NotYetAvailable);
        }
        self.get_raw(&path).ok_or(StoreFailure::NotFound)
    }

    fn put_new(&self, path: String, bytes: Vec<u8>) -> Result<(), StoreFailure> {
        self.check()?;
        let mut files = self.files.lock().unwrap();
        if files.contains_key(&path) {
            return Err(StoreFailure::Exists);
        }
        files.insert(path, bytes);
        Ok(())
    }

    fn put_replace(&self, path: String, bytes: Vec<u8>) -> Result<(), StoreFailure> {
        self.check()?;
        self.files.lock().unwrap().insert(path, bytes);
        Ok(())
    }

    fn list(&self, prefix: String) -> Result<Vec<BlobEntry>, StoreFailure> {
        self.check()?;
        let pending = self.pending.lock().unwrap().clone();
        Ok(self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.starts_with(&prefix))
            .map(|(p, b)| BlobEntry {
                path: p.clone(),
                size: b.len() as u64,
                mtime_ms: None,
                available: !pending.contains(p),
            })
            .collect())
    }

    fn delete(&self, path: String) -> Result<(), StoreFailure> {
        self.check()?;
        self.files.lock().unwrap().remove(&path);
        Ok(())
    }
}

struct Clock(AtomicU64);

impl SyncClock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct State {
    synced: AtomicBool,
    last: Mutex<Option<u64>>,
    refused: AtomicBool,
}

impl SyncStateStore for State {
    fn synced_before(&self) -> bool {
        self.synced.load(Ordering::SeqCst)
    }

    fn last_sync_ms(&self) -> Option<u64> {
        *self.last.lock().unwrap()
    }

    fn sign_in_refused(&self) -> bool {
        self.refused.load(Ordering::SeqCst)
    }

    fn mark_synced(&self, now_ms: u64) {
        self.synced.store(true, Ordering::SeqCst);
        *self.last.lock().unwrap() = Some(now_ms);
    }

    fn mark_sign_in_refused(&self) {
        self.refused.store(true, Ordering::SeqCst);
    }

    fn clear_sign_in_refused(&self) {
        self.refused.store(false, Ordering::SeqCst);
    }

    fn forget(&self) {
        self.synced.store(false, Ordering::SeqCst);
        *self.last.lock().unwrap() = None;
    }
}

struct Device {
    local: Arc<Mem>,
    state: Arc<State>,
    engine: Arc<SyncEngine>,
}

fn device(remote: &Arc<Mem>, id: Option<&str>) -> Device {
    let local = Mem::new();
    let state = Arc::new(State::default());
    let engine = SyncEngine::new(
        local.clone(),
        remote.clone(),
        state.clone(),
        Arc::new(Clock(AtomicU64::new(T0))),
        id.map(str::to_owned),
    );
    Device {
        local,
        state,
        engine,
    }
}

fn vault_json(id: &str, revision: u64, tag: &str) -> Vec<u8> {
    serde_json::json!({ "vault_id": id, "revision": revision, "tag": tag })
        .to_string()
        .into_bytes()
}

/// A device with a vault the way the first device of one looks: `vault.json`, its record and an op file.
fn with_vault(d: &Device, id: &str, own: &str) {
    d.local.put("vault.json", &vault_json(id, 1, "x"));
    d.local.put(&format!("devices/{own}.json"), b"record");
    d.local.put(&format!("log/{own}/00000001.op"), b"op");
}

#[test]
fn two_devices_converge_through_a_remote_the_app_supplies() {
    let remote = Mem::new();
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    let up = a.engine.run(false).unwrap();
    assert_eq!((up.pulled, up.pushed), (0, 3));
    assert_eq!(a.state.last_sync_ms(), Some(T0), "the app's clock is used");
    assert_eq!(remote.snapshot(), a.local.snapshot());

    let b = device(&remote, Some("bb"));
    let down = b.engine.run(false).unwrap();
    assert_eq!((down.pulled, down.pushed), (3, 0));
    assert_eq!(b.local.snapshot(), a.local.snapshot());

    let again = b.engine.run(false).unwrap();
    assert_eq!((again.pulled, again.pushed), (0, 0));
}

#[test]
fn a_listed_file_that_is_not_downloaded_yet_is_retried_not_treated_as_missing() {
    let remote = Mem::new();
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    a.engine.run(false).unwrap();

    let b = device(&remote, Some("bb"));
    remote
        .pending
        .lock()
        .unwrap()
        .push("log/aa/00000001.op".into());
    let first = b.engine.run(false).unwrap();
    assert_eq!(first.pulled, 2, "everything that is here comes");
    assert_eq!(b.local.get_raw("log/aa/00000001.op"), None);
    assert_eq!(
        remote.get_raw("log/aa/00000001.op").as_deref(),
        Some(&b"op"[..])
    );

    remote.pending.lock().unwrap().clear();
    assert_eq!(b.engine.run(false).unwrap().pulled, 1);
    assert_eq!(b.local.snapshot(), a.local.snapshot());
}

#[test]
fn a_lost_remote_asks_before_uploading_again() {
    let remote = Mem::new();
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    a.engine.run(false).unwrap();

    remote.files.lock().unwrap().clear();
    assert_eq!(a.engine.run(false).unwrap_err(), SyncFailure::RemoteLost);
    assert!(remote.names().is_empty(), "nothing is uploaded unasked");

    let up = a.engine.run(true).unwrap();
    assert_eq!(up.pushed, 3);
    assert_eq!(remote.snapshot(), a.local.snapshot());
}

#[test]
fn a_different_vault_is_never_merged() {
    let remote = Mem::new();
    remote.put("vault.json", &vault_json("someone-else", 9, "r"));
    remote.put("devices/zz.json", b"theirs");
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    let (local_before, remote_before) = (a.local.snapshot(), remote.snapshot());

    assert_eq!(a.engine.run(true).unwrap_err(), SyncFailure::ForeignVault);
    assert_eq!(
        a.engine.open_from_remote().unwrap_err(),
        SyncFailure::ForeignVault
    );
    assert_eq!(
        a.engine.delete_remote(delete_remote_phrase()).unwrap_err(),
        SyncFailure::ForeignVault
    );
    assert_eq!(a.local.snapshot(), local_before);
    assert_eq!(
        remote.snapshot(),
        remote_before,
        "the other vault is untouched"
    );
}

#[test]
fn a_damaged_vault_json_stops_the_sync() {
    let remote = Mem::new();
    remote.put("vault.json", b"{ not json");
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    assert_eq!(a.engine.run(true).unwrap_err(), SyncFailure::Damaged);
    assert_eq!(a.local.names().len(), 3, "nothing was pulled over it");
    assert_eq!(
        remote.names(),
        ["vault.json"],
        "nothing was pushed next to it"
    );
}

#[test]
fn a_tampered_vault_json_travels_but_is_refused_when_the_vault_opens() {
    let (_vault, created) =
        Vault::create("generated-sync-password-1", "phone", KdfParams::FLOOR, T0).unwrap();
    let mut tampered: serde_json::Value = serde_json::from_slice(&created.vault_json).unwrap();
    tampered["revision"] = 7.into();
    let tampered = tampered.to_string().into_bytes();

    let remote = Mem::new();
    remote.put("vault.json", &tampered);
    let b = device(&remote, Some("bb"));
    // Sync never interprets ciphertext (`sync.md` 8): the bytes are copied as they are.
    b.engine.open_from_remote().unwrap();
    assert_eq!(
        b.local.get_raw("vault.json").as_deref(),
        Some(&tampered[..])
    );

    let keys = Arc::new(figlock_ffi::DeviceKeysHandle::generate().unwrap());
    let opened = unlock_vault(
        b.local.get_raw("vault.json").unwrap(),
        "generated-sync-password-1".into(),
        created.secret_key.format().to_string(),
        keys,
        "phone".into(),
        vec![],
        T0 + 1,
    );
    assert!(matches!(opened, Err(figlock_ffi::FigLockError::Failed)));
}

#[test]
fn a_revocation_reaches_the_revoked_device_and_removes_its_record() {
    let remote = Mem::new();
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    a.local.put("devices/bb.json", b"record-b");
    a.engine.run(false).unwrap();
    let b = device(&remote, Some("bb"));
    b.engine.run(false).unwrap();
    assert!(b.local.get_raw("devices/bb.json").is_some());

    // The authority revokes B: a new revision of `vault.json` and B's record gone.
    a.local.put("vault.json", &vault_json("v1", 2, "revoked-b"));
    a.local.files.lock().unwrap().remove("devices/bb.json");
    let up = a.engine.run(false).unwrap();
    assert_eq!(up.pushed, 2, "the new vault.json and the removal");
    assert!(remote.get_raw("devices/bb.json").is_none());

    let down = b.engine.run(false).unwrap();
    assert_eq!(down.pulled, 2);
    assert!(b.local.get_raw("devices/bb.json").is_none());
    assert_eq!(b.local.get_raw("vault.json"), a.local.get_raw("vault.json"));
}

#[test]
fn a_vault_json_fork_is_reported() {
    let remote = Mem::new();
    remote.put("vault.json", &vault_json("v1", 2, "from-remote"));
    let a = device(&remote, Some("aa"));
    a.local
        .put("vault.json", &vault_json("v1", 2, "from-local"));
    let r = a.engine.run(false).unwrap();
    assert_eq!(r.warnings, [SyncWarning::VaultJsonFork]);
    assert_eq!(a.local.get_raw("vault.json"), remote.get_raw("vault.json"));
}

#[test]
fn a_refused_sign_in_is_reported_and_remembered_without_text() {
    let remote = Mem::new();
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    *remote.fail.lock().unwrap() = Some(StoreFailure::Unauthorized);
    let err = a.engine.run(false).unwrap_err();
    assert_eq!(err, SyncFailure::SignIn);
    assert!(a.state.sign_in_refused());
    assert!(!a.state.synced_before());

    *remote.fail.lock().unwrap() = Some(StoreFailure::Unavailable);
    assert_eq!(a.engine.run(false).unwrap_err(), SyncFailure::Offline);
}

#[test]
fn errors_carry_no_text_from_the_store_the_paths_or_the_secrets() {
    let secret = "generated-token-for-the-test";
    let remote = Mem::new();
    let a = device(&remote, Some("aa"));
    a.local.put("vault.json", &vault_json("v1", 1, secret));
    a.local
        .put(&format!("devices/{secret}.json"), secret.as_bytes());
    *remote.fail.lock().unwrap() = Some(StoreFailure::Other);
    let err = a.engine.run(false).unwrap_err();
    assert_eq!(err, SyncFailure::Other);
    let shown = format!("{err} {err:?}");
    assert!(!shown.contains(secret) && !shown.contains("devices/"));
}

#[test]
fn only_the_typed_phrase_deletes_the_remote_copy() {
    let remote = Mem::new();
    let a = device(&remote, Some("aa"));
    with_vault(&a, "v1", "aa");
    a.engine.run(false).unwrap();
    let before = remote.snapshot();

    for typed in ["", "yes", "Personal", "delete from drive"] {
        assert_eq!(
            a.engine.delete_remote(typed.into()).unwrap_err(),
            SyncFailure::NotConfirmed
        );
    }
    assert_eq!(remote.snapshot(), before);
    assert!(a.state.synced_before());

    assert!(delete_remote_confirmed(
        "  Delete From Google DRIVE ".into()
    ));
    let files = a.engine.delete_remote(delete_remote_phrase()).unwrap();
    assert_eq!(files, 3);
    assert!(remote.names().is_empty());
    assert!(!a.state.synced_before(), "the device forgets it synced");
    assert_eq!(a.local.names().len(), 3, "nothing local is touched");
}

#[test]
fn a_second_call_while_a_sync_runs_is_busy_not_queued() {
    struct Gate {
        inner: Arc<Mem>,
        entered: Mutex<Option<mpsc::Sender<()>>>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl BlobStoreFfi for Gate {
        fn get(&self, p: String) -> Result<Vec<u8>, StoreFailure> {
            self.inner.get(p)
        }
        fn put_new(&self, p: String, b: Vec<u8>) -> Result<(), StoreFailure> {
            self.inner.put_new(p, b)
        }
        fn put_replace(&self, p: String, b: Vec<u8>) -> Result<(), StoreFailure> {
            self.inner.put_replace(p, b)
        }
        fn list(&self, p: String) -> Result<Vec<BlobEntry>, StoreFailure> {
            if let Some(tx) = self.entered.lock().unwrap().take() {
                tx.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
            }
            self.inner.list(p)
        }
        fn delete(&self, p: String) -> Result<(), StoreFailure> {
            self.inner.delete(p)
        }
    }

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let remote = Arc::new(Gate {
        inner: Mem::new(),
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(release_rx),
    });
    let engine = SyncEngine::new(
        Mem::new(),
        remote,
        Arc::new(State::default()),
        Arc::new(Clock(AtomicU64::new(T0))),
        None,
    );
    let running = {
        let engine = engine.clone();
        std::thread::spawn(move || engine.run(false))
    };
    entered_rx.recv().unwrap();
    assert_eq!(engine.run(false).unwrap_err(), SyncFailure::Busy);
    assert_eq!(engine.fill_local().unwrap_err(), SyncFailure::Busy);
    release_tx.send(()).unwrap();
    running.join().unwrap().unwrap();
    assert!(engine.run(false).is_ok(), "the flag is released");
}

#[test]
fn a_folder_handled_in_rust_works_as_either_side() {
    let (local_dir, remote_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let local = FolderStore::open(local_dir.path().display().to_string()).unwrap();
    local
        .put_replace("vault.json".into(), vault_json("v1", 1, "x"))
        .unwrap();
    local
        .put_new("log/aa/00000001.op".into(), b"op".to_vec())
        .unwrap();
    let remote = FolderStore::open(remote_dir.path().display().to_string()).unwrap();

    let engine = SyncEngine::new(
        local.clone(),
        remote.clone(),
        Arc::new(State::default()),
        Arc::new(Clock(AtomicU64::new(T0))),
        Some("aa".into()),
    );
    assert_eq!(engine.run(false).unwrap().pushed, 2);
    assert_eq!(remote.get("log/aa/00000001.op".into()).unwrap(), b"op");
    assert_eq!(remote.get("nope".into()), Err(StoreFailure::NotFound));
    assert_eq!(
        remote.put_new("vault.json".into(), vec![]),
        Err(StoreFailure::Exists)
    );
    assert_eq!(
        remote.get("../outside".into()),
        Err(StoreFailure::InvalidPath)
    );
    assert!(FolderStore::open("/no/such/folder/for/the/test".into()).is_err());
}

#[test]
fn the_drive_store_asks_the_app_for_its_token_and_a_refusal_means_sign_in() {
    struct Refused(AtomicBool);
    impl AccessTokens for Refused {
        fn access_token(&self) -> Result<String, StoreFailure> {
            self.0.store(true, Ordering::SeqCst);
            Err(StoreFailure::Unauthorized)
        }
        fn invalidate(&self) {}
    }
    let tokens = Arc::new(Refused(AtomicBool::new(false)));
    let drive = DriveStore::new(tokens.clone());
    let a = device(&Mem::new(), Some("aa"));
    with_vault(&a, "v1", "aa");
    let engine = SyncEngine::new(
        a.local.clone(),
        drive,
        a.state.clone(),
        Arc::new(Clock(AtomicU64::new(T0))),
        Some("aa".into()),
    );
    assert_eq!(engine.run(false).unwrap_err(), SyncFailure::SignIn);
    assert!(
        tokens.0.load(Ordering::SeqCst),
        "the app was asked for a token"
    );
    assert!(a.state.sign_in_refused());
}

#[test]
fn the_timing_rules_match_the_desktop() {
    assert_eq!(sync_interval_secs(), 15);
    assert_eq!(
        [0, 1, 2, 3, 4, 50].map(sync_backoff_secs),
        [5, 15, 60, 300, 300, 300]
    );
}
