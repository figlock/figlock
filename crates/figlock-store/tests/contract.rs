//! BlobStore contract tests (`sync.md` 2). `run_contract` is generic: every adapter must pass it unchanged.
//! The local adapter also gets tests for the properties that only a real file system can break.

use figlock_store::{BlobStore, LocalFolder, StoreError, validate_path};
use std::sync::Arc;

#[cfg(feature = "gdrive")]
mod support;

fn run_contract<S: BlobStore + Send + Sync + 'static>(make: impl Fn() -> S) {
    get_missing(&make());
    put_new_is_create_only(&make());
    put_replace_replaces_and_creates(&make());
    list_prefixes(&make());
    delete_is_idempotent(&make());
    invalid_paths_are_rejected_everywhere(&make());
    concurrent_put_new_has_one_winner(Arc::new(make()));
    readers_never_see_partial_replace(Arc::new(make()));
}

fn get_missing(s: &impl BlobStore) {
    assert_eq!(s.get("vault.json"), Err(StoreError::NotFound));
    assert_eq!(s.get("log/abcd/00000001.op"), Err(StoreError::NotFound));
}

fn put_new_is_create_only(s: &impl BlobStore) {
    s.put_new("log/abcd/00000001.op", b"first").unwrap();
    assert_eq!(s.get("log/abcd/00000001.op").unwrap(), b"first");
    assert_eq!(
        s.put_new("log/abcd/00000001.op", b"second"),
        Err(StoreError::Exists)
    );
    assert_eq!(
        s.get("log/abcd/00000001.op").unwrap(),
        b"first",
        "the original is untouched"
    );
    s.put_new("log/abcd/empty.op", b"").unwrap();
    assert_eq!(s.get("log/abcd/empty.op").unwrap(), b"");
}

fn put_replace_replaces_and_creates(s: &impl BlobStore) {
    s.put_replace("vault.json", b"v1").unwrap();
    s.put_replace("vault.json", b"v2 longer").unwrap();
    assert_eq!(s.get("vault.json").unwrap(), b"v2 longer");
    s.put_replace("vault.json", b"v3").unwrap();
    assert_eq!(s.get("vault.json").unwrap(), b"v3", "shrinking works too");
}

fn list_prefixes(s: &impl BlobStore) {
    for p in [
        "vault.json",
        "devices/aa.json",
        "log/aa/00000001.op",
        "log/aa/00000002.op",
        "log/bb/00000001.op",
        "snap/aa-00000001.snap",
        "snap/ab-00000001.snap",
    ] {
        s.put_new(p, p.as_bytes()).unwrap();
    }
    let paths = |prefix: &str| {
        s.list(prefix)
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect::<Vec<_>>()
    };
    assert_eq!(paths("").len(), 7);
    assert_eq!(
        paths("log/"),
        [
            "log/aa/00000001.op",
            "log/aa/00000002.op",
            "log/bb/00000001.op"
        ]
    );
    assert_eq!(
        paths("log/aa/"),
        ["log/aa/00000001.op", "log/aa/00000002.op"]
    );
    assert_eq!(paths("snap/aa-"), ["snap/aa-00000001.snap"]);
    assert_eq!(paths("devices/"), ["devices/aa.json"]);
    assert!(paths("log/zz/").is_empty());
    assert!(paths("nothing/").is_empty());
    let e = &s.list("vault.json").unwrap()[0];
    assert_eq!((e.size, e.available), ("vault.json".len() as u64, true));
    let sorted = paths("");
    let mut copy = sorted.clone();
    copy.sort();
    assert_eq!(sorted, copy, "sorted by path");
}

fn delete_is_idempotent(s: &impl BlobStore) {
    s.put_new("inbox/r1.req", b"x").unwrap();
    s.delete("inbox/r1.req").unwrap();
    assert_eq!(s.get("inbox/r1.req"), Err(StoreError::NotFound));
    s.delete("inbox/r1.req").unwrap();
    s.delete("never/existed.op").unwrap();
    s.put_new("inbox/r1.req", b"again").unwrap();
}

fn invalid_paths_are_rejected_everywhere(s: &impl BlobStore) {
    let long_segment = "a".repeat(201);
    let long_path = format!("{}/x", "a/".repeat(300));
    let bad = [
        "",
        "/etc/passwd",
        "../outside",
        "a/../b",
        "a/./b",
        "a//b",
        "a/",
        "/",
        ".hidden",
        "a/.hidden",
        ".tmp-1",
        "UPPER/case.op",
        "a\\b",
        "a/b\0c",
        "with space",
        "uni/c\u{f6}de",
        "a/b/c/d/e",
        &long_segment,
        &long_path,
    ];
    for p in bad {
        assert_eq!(s.get(p), Err(StoreError::InvalidPath), "get {p:?}");
        assert_eq!(
            s.put_new(p, b"x"),
            Err(StoreError::InvalidPath),
            "put_new {p:?}"
        );
        assert_eq!(
            s.put_replace(p, b"x"),
            Err(StoreError::InvalidPath),
            "put_replace {p:?}"
        );
        assert_eq!(s.delete(p), Err(StoreError::InvalidPath), "delete {p:?}");
        assert!(validate_path(p).is_err(), "{p:?}");
    }
    for prefix in ["../", "a/../", "/abs", "a//", ".hidden/", "UP/"] {
        assert_eq!(
            s.list(prefix).unwrap_err(),
            StoreError::InvalidPath,
            "list {prefix:?}"
        );
    }
}

fn concurrent_put_new_has_one_winner<S: BlobStore + Send + Sync + 'static>(s: Arc<S>) {
    let handles: Vec<_> = (0..16u8)
        .map(|i| {
            let s = s.clone();
            std::thread::spawn(move || s.put_new("log/cc/00000001.op", &vec![i; 4096]))
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "exactly one create wins"
    );
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| *r == Err(StoreError::Exists))
    );
    let got = s.get("log/cc/00000001.op").unwrap();
    assert_eq!(got.len(), 4096);
    assert!(
        got.iter().all(|b| *b == got[0]),
        "the winner's bytes are whole, never mixed"
    );
}

fn readers_never_see_partial_replace<S: BlobStore + Send + Sync + 'static>(s: Arc<S>) {
    let (a, b) = (vec![1u8; 200_000], vec![2u8; 150_000]);
    s.put_replace("vault.json", &a).unwrap();
    let writer = {
        let (s, a, b) = (s.clone(), a.clone(), b.clone());
        std::thread::spawn(move || {
            for i in 0..100 {
                s.put_replace("vault.json", if i % 2 == 0 { &b } else { &a })
                    .unwrap();
            }
        })
    };
    let reader = {
        let (s, a, b) = (s.clone(), a.clone(), b.clone());
        std::thread::spawn(move || {
            for _ in 0..300 {
                let got = s.get("vault.json").unwrap();
                assert!(got == a || got == b, "a reader saw a partial or mixed blob");
            }
        })
    };
    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn local_folder_meets_the_contract() {
    // Keep every temp dir alive for the duration of the contract run.
    let dirs = std::sync::Mutex::new(Vec::new());
    run_contract(|| {
        let d = tempfile::tempdir().unwrap();
        let s = LocalFolder::open(d.path()).unwrap();
        dirs.lock().unwrap().push(d);
        s
    });
}

#[test]
fn no_temporary_files_are_left_behind() {
    let d = tempfile::tempdir().unwrap();
    let s = LocalFolder::open(d.path()).unwrap();
    s.put_new("log/aa/00000001.op", b"x").unwrap();
    let _ = s.put_new("log/aa/00000001.op", b"y");
    s.put_replace("vault.json", b"v").unwrap();
    s.put_replace("vault.json", b"w").unwrap();
    fn all_files(dir: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            if e.path().is_dir() {
                all_files(&e.path(), out);
            } else {
                out.push(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    let mut names = Vec::new();
    all_files(d.path(), &mut names);
    names.sort();
    assert_eq!(
        names,
        ["00000001.op", "vault.json"],
        "no .tmp- leftovers, even after a failed put_new"
    );
}

#[test]
fn oversized_blobs_are_refused() {
    let d = tempfile::tempdir().unwrap();
    let s = LocalFolder::open(d.path()).unwrap();
    let big = vec![0u8; (figlock_store::MAX_BLOB_BYTES + 1) as usize];
    assert_eq!(s.put_new("a/big.op", &big), Err(StoreError::TooLarge));
    assert_eq!(s.put_replace("big.json", &big), Err(StoreError::TooLarge));
    // A hostile folder can contain an oversized file written by someone else: reading it is refused, not buffered.
    std::fs::create_dir_all(d.path().join("log")).unwrap();
    let f = std::fs::File::create(d.path().join("log/huge.op")).unwrap();
    f.set_len(figlock_store::MAX_BLOB_BYTES + 1).unwrap();
    assert_eq!(s.get("log/huge.op"), Err(StoreError::TooLarge));
}

#[test]
fn icloud_placeholder_is_not_yet_available_not_missing() {
    let d = tempfile::tempdir().unwrap();
    let s = LocalFolder::open(d.path()).unwrap();
    std::fs::create_dir_all(d.path().join("log/aa")).unwrap();
    std::fs::write(d.path().join("log/aa/.00000002.op.icloud"), b"placeholder").unwrap();
    s.put_new("log/aa/00000001.op", b"here").unwrap();

    assert_eq!(
        s.get("log/aa/00000002.op"),
        Err(StoreError::NotYetAvailable)
    );
    assert_eq!(s.get("log/aa/00000003.op"), Err(StoreError::NotFound));
    let listed = s.list("log/aa/").unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed[0].available && listed[0].path == "log/aa/00000001.op");
    assert!(!listed[1].available && listed[1].path == "log/aa/00000002.op");
    // Creating over a blob that exists in the cloud must not silently succeed.
    assert_eq!(
        s.put_new("log/aa/00000002.op", b"clobber"),
        Err(StoreError::Exists)
    );
    // Deleting removes the placeholder too.
    s.delete("log/aa/00000002.op").unwrap();
    assert_eq!(s.get("log/aa/00000002.op"), Err(StoreError::NotFound));
}

#[test]
fn foreign_files_in_the_folder_are_ignored_not_trusted() {
    let d = tempfile::tempdir().unwrap();
    let s = LocalFolder::open(d.path()).unwrap();
    s.put_new("vault.json", b"{}").unwrap();
    std::fs::write(d.path().join(".DS_Store"), b"junk").unwrap();
    std::fs::write(d.path().join("Notes (conflict).txt"), b"junk").unwrap();
    std::fs::create_dir_all(d.path().join(".hidden-dir")).unwrap();
    std::fs::write(d.path().join(".hidden-dir/x.op"), b"junk").unwrap();
    let paths: Vec<_> = s.list("").unwrap().into_iter().map(|e| e.path).collect();
    std::fs::write(d.path().join(".Upper.icloud"), b"").unwrap();
    assert_eq!(paths, vec!["vault.json".to_string()]);
    // Every listed path is addressable.
    for p in &paths {
        s.get(p).unwrap();
    }
    assert!(
        !paths
            .iter()
            .any(|p| p.contains(".DS_Store") || p.contains(".hidden-dir"))
    );
}

#[test]
fn list_tolerates_concurrent_writes_and_deletes() {
    let d = tempfile::tempdir().unwrap();
    let s = Arc::new(LocalFolder::open(d.path()).unwrap());
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (s, stop) = (s.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut i = 0u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let p = format!("log/aa/{:08}.op", i % 8);
                let _ = s.put_new(&p, b"x");
                s.put_replace(&p, b"y").unwrap();
                s.delete(&p).unwrap();
                i += 1;
            }
        })
    };
    for _ in 0..2000 {
        s.list("").unwrap();
        s.list("log/aa/").unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    writer.join().unwrap();
}

#[cfg(unix)]
mod symlinks {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlink_escape_is_refused_for_every_operation() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.op"), b"outside").unwrap();
        let d = tempfile::tempdir().unwrap();
        let s = LocalFolder::open(d.path()).unwrap();
        // A directory symlink pointing out of the store, and a file symlink pointing at an outside file.
        symlink(outside.path(), d.path().join("log")).unwrap();
        symlink(
            outside.path().join("secret.op"),
            d.path().join("vault.json"),
        )
        .unwrap();

        assert_eq!(s.get("log/secret.op"), Err(StoreError::InvalidPath));
        assert_eq!(s.get("vault.json"), Err(StoreError::InvalidPath));
        assert_eq!(s.put_new("log/new.op", b"x"), Err(StoreError::InvalidPath));
        assert_eq!(
            s.put_replace("vault.json", b"x"),
            Err(StoreError::InvalidPath)
        );
        assert_eq!(s.delete("log/secret.op"), Err(StoreError::InvalidPath));
        assert_eq!(s.list("log/").unwrap_err(), StoreError::InvalidPath);
        assert!(s.list("").unwrap().is_empty(), "symlinks are never listed");
        assert_eq!(
            std::fs::read(outside.path().join("secret.op")).unwrap(),
            b"outside",
            "outside file untouched"
        );
        assert!(
            !outside.path().join("new.op").exists(),
            "nothing was written outside the store"
        );
    }
}

#[cfg(feature = "gdrive")]
#[test]
fn google_drive_meets_the_contract() {
    use figlock_store::{GoogleDrive, StaticToken};
    let servers = std::sync::Mutex::new(Vec::new());
    run_contract(|| {
        let fake = support::fake_drive::FakeDrive::start();
        let drive =
            GoogleDrive::with_endpoint(&fake.base, StaticToken("good-token".into())).unwrap();
        servers.lock().unwrap().push(fake);
        drive
    });
}
