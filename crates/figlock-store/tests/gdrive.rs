//! Google Drive adapter behaviour that only a Drive-shaped provider can break: duplicate names, throttling,
//! token refresh and revocation, ambiguous creates, big uploads and hostile responses. The shared contract
//! lives in `contract.rs`.
#![cfg(feature = "gdrive")]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use figlock_store::{BlobStore, GoogleDrive, MAX_BLOB_BYTES, StaticToken, StoreError, TokenSource};
use support::fake_drive::FakeDrive;

fn drive(fake: &FakeDrive) -> GoogleDrive {
    GoogleDrive::with_endpoint(&fake.base, StaticToken("good-token".into()))
        .unwrap()
        .with_retry_delays(vec![Duration::from_millis(5); 3])
}

fn count(fake: &FakeDrive, method: &str, path_prefix: &str) -> usize {
    fake.state
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|(m, p)| m == method && p.starts_with(path_prefix))
        .count()
}

#[test]
fn oldest_duplicate_wins_for_reads_and_list_and_delete_removes_all() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    fake.plant("log/aa/00000001.op", b"first");
    fake.plant("log/aa/00000001.op", b"second");
    fake.plant("log/aa/00000002.op", b"other");

    assert_eq!(d.get("log/aa/00000001.op").unwrap(), b"first");
    let listed = d.list("log/aa/").unwrap();
    assert_eq!(listed.len(), 2, "a duplicate is listed once");
    assert_eq!(listed[0].size, 5, "and with the oldest copy's size");
    assert_eq!(
        d.put_new("log/aa/00000001.op", b"x"),
        Err(StoreError::Exists)
    );

    d.delete("log/aa/00000001.op").unwrap();
    assert_eq!(d.get("log/aa/00000001.op"), Err(StoreError::NotFound));
    assert_eq!(
        fake.names(),
        ["log/aa/00000002.op"],
        "no duplicate resurfaces"
    );
}

#[test]
fn files_with_foreign_names_are_ignored() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    d.put_new("vault.json", b"{}").unwrap();
    for name in [
        "Notes (copy).txt",
        "UPPER.op",
        "../escape",
        ".hidden",
        "a b",
    ] {
        fake.plant(name, b"junk");
    }
    let paths: Vec<_> = d.list("").unwrap().into_iter().map(|e| e.path).collect();
    assert_eq!(paths, ["vault.json"]);
}

#[test]
fn listing_follows_pages() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    fake.state.lock().unwrap().page_size = 2;
    for i in 1..=7 {
        d.put_new(&format!("log/aa/{i:08}.op"), b"x").unwrap();
    }
    assert_eq!(d.list("log/aa/").unwrap().len(), 7);
}

struct Refreshing {
    calls: AtomicUsize,
    invalidations: Arc<AtomicUsize>,
}

impl TokenSource for Refreshing {
    fn access_token(&self) -> figlock_store::Result<String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        // The first token is stale; anything after an invalidation is good.
        Ok(
            if self.invalidations.load(Ordering::SeqCst) == 0 && n == 0 {
                "stale"
            } else {
                "good-token"
            }
            .into(),
        )
    }
    fn invalidate(&self) {
        self.invalidations.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn an_expired_token_is_refreshed_once_and_the_call_succeeds() {
    let fake = FakeDrive::start();
    let inv = Arc::new(AtomicUsize::new(0));
    let d = GoogleDrive::with_endpoint(
        &fake.base,
        Refreshing {
            calls: AtomicUsize::new(0),
            invalidations: inv.clone(),
        },
    )
    .unwrap();
    d.put_new("vault.json", b"{}").unwrap();
    assert_eq!(d.get("vault.json").unwrap(), b"{}");
    assert_eq!(inv.load(Ordering::SeqCst), 1, "refreshed exactly once");
}

#[test]
fn a_revoked_token_is_unauthorized_not_a_loop_and_not_a_missing_vault() {
    let fake = FakeDrive::start();
    let d = GoogleDrive::with_endpoint(&fake.base, StaticToken("revoked".into())).unwrap();
    assert_eq!(d.get("vault.json"), Err(StoreError::Unauthorized));
    assert_eq!(d.put_new("vault.json", b"x"), Err(StoreError::Unauthorized));
    assert_eq!(d.list("").unwrap_err(), StoreError::Unauthorized);
    assert_eq!(d.delete("vault.json"), Err(StoreError::Unauthorized));
    assert!(fake.names().is_empty());
    assert!(
        fake.hits() <= 8,
        "two attempts per call at most, got {}",
        fake.hits()
    );
}

#[test]
fn throttling_and_server_errors_are_retried() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    d.put_new("vault.json", b"{}").unwrap();
    // 429, 503 and a 403 rate-limit reason, then success.
    fake.state.lock().unwrap().fail_next = vec![429, 503, 403];
    assert_eq!(d.get("vault.json").unwrap(), b"{}");
}

#[test]
fn persistent_failure_is_unavailable_not_notfound() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    d.put_new("vault.json", b"{}").unwrap();
    fake.state.lock().unwrap().fail_next = vec![503; 20];
    assert_eq!(d.get("vault.json"), Err(StoreError::Unavailable));
    assert_eq!(d.list("").unwrap_err(), StoreError::Unavailable);
}

#[test]
fn a_create_whose_response_was_lost_is_not_repeated() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    fake.state.lock().unwrap().lose_next_create_response = true;
    d.put_new("log/aa/00000001.op", b"once").unwrap();
    assert_eq!(fake.names(), ["log/aa/00000001.op"], "exactly one copy");
    assert_eq!(
        count(&fake, "POST", "/upload"),
        1,
        "the landed create was found by its nonce"
    );
    assert_eq!(d.get("log/aa/00000001.op").unwrap(), b"once");
}

#[test]
fn losing_a_create_race_to_another_client_reports_exists_and_leaves_one_copy() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    fake.plant("log/aa/00000001.op", b"theirs");
    assert_eq!(
        d.put_new("log/aa/00000001.op", b"mine"),
        Err(StoreError::Exists)
    );
    assert_eq!(fake.names().len(), 1);
    assert_eq!(d.get("log/aa/00000001.op").unwrap(), b"theirs");
}

#[test]
fn big_blobs_use_a_resumable_session() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    let big: Vec<u8> = (0..6 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    d.put_new("snap/aa-00000001.snap", &big).unwrap();
    assert_eq!(d.get("snap/aa-00000001.snap").unwrap(), big);
    let bigger = vec![9u8; 7 * 1024 * 1024];
    d.put_replace("snap/aa-00000001.snap", &bigger).unwrap();
    assert_eq!(d.get("snap/aa-00000001.snap").unwrap(), bigger);
    assert_eq!(
        fake.state.lock().unwrap().max_multipart_seen,
        0,
        "nothing over 5 MiB went multipart"
    );
    assert_eq!(fake.names().len(), 1);
}

#[test]
fn an_upload_session_on_another_host_is_refused_and_never_sees_the_token() {
    let fake = FakeDrive::start();
    let evil = FakeDrive::start();
    fake.state.lock().unwrap().session_host = Some(evil.base.clone());
    let d = drive(&fake);
    let big = vec![1u8; 6 * 1024 * 1024];
    assert_eq!(
        d.put_new("snap/aa-00000001.snap", &big),
        Err(StoreError::Unavailable)
    );
    assert_eq!(
        evil.hits(),
        0,
        "the bearer token and the ciphertext stayed on the API host"
    );
}

#[test]
fn an_oversized_remote_file_is_refused_without_downloading_it() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    fake.plant(
        "log/aa/00000001.op",
        &vec![0u8; MAX_BLOB_BYTES as usize + 1],
    );
    assert_eq!(d.get("log/aa/00000001.op"), Err(StoreError::TooLarge));
    assert_eq!(
        count(&fake, "GET", "/drive/v3/files/"),
        0,
        "no download was attempted"
    );
}

#[test]
fn only_the_app_data_folder_is_touched() {
    let fake = FakeDrive::start();
    let d = drive(&fake);
    d.put_new("vault.json", b"{}").unwrap();
    d.put_replace("vault.json", b"{ }").unwrap();
    d.list("").unwrap();
    // The fake asserts `spaces=appDataFolder` on every listing and `parents=[appDataFolder]` on every
    // create; reaching here means neither was violated.
    assert!(count(&fake, "GET", "/drive/v3/files") >= 2);
}
