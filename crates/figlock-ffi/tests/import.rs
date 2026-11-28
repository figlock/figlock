//! Importing an export file through the FFI (FIG-286): a preview without secrets, chunked adds, and the failure
//! cases. File contents are generated.

use figlock_ffi::{ImportFailure, create_vault, minimum_kdf_settings, prepare_import};

const T0: u64 = 1_800_000_000_000;

fn session() -> std::sync::Arc<figlock_ffi::VaultSession> {
    create_vault(
        "generated-test-password-1".into(),
        "test phone".into(),
        minimum_kdf_settings(),
        T0,
    )
    .unwrap()
    .take_session()
    .unwrap()
}

fn chrome(n: usize, secret: &str) -> Vec<u8> {
    let mut s = String::from("name,url,username,password,note\n");
    for i in 0..n {
        s.push_str(&format!("Site {i},https://s{i}.example,me,{secret},\n"));
    }
    s.into_bytes()
}

#[test]
fn the_preview_has_counts_and_titles_and_no_secret() {
    let secret = "generated-import-secret-9";
    let plan = prepare_import("chrome".into(), chrome(3, secret)).unwrap();
    let p = plan.preview();
    assert_eq!((p.logins, p.notes, p.tokens, p.skipped), (3, 0, 0, 0));
    assert_eq!(p.titles, ["Site 0", "Site 1", "Site 2"]);
    assert!(!format!("{p:?}").contains(secret));
}

#[test]
fn nothing_is_added_until_add_next_and_chunks_add_every_item_once() {
    let s = session();
    let plan = prepare_import("chrome".into(), chrome(5, "generated-import-secret-9")).unwrap();
    assert!(s.items(false).unwrap().items.is_empty());
    assert_eq!(plan.remaining().unwrap(), 5);

    let first = plan.add_next(&s, 2, T0 + 1).unwrap();
    assert_eq!(first.len(), 2);
    assert!(s.commit().unwrap().is_some());
    s.confirm_commit().unwrap();
    assert_eq!(plan.add_next(&s, 2, T0 + 2).unwrap().len(), 2);
    assert_eq!(plan.add_next(&s, 2, T0 + 3).unwrap().len(), 1);
    assert!(plan.add_next(&s, 2, T0 + 4).unwrap().is_empty());
    assert_eq!(plan.remaining().unwrap(), 0);
    assert_eq!(s.items(false).unwrap().items.len(), 5);
}

#[test]
fn apple_and_notes_files_prepare_too() {
    let apple =
        "Title,URL,Username,Password,Notes,OTPAuth\nMail,https://mail.example,me,generated-pw,,\n";
    assert_eq!(
        prepare_import("apple".into(), apple.into())
            .unwrap()
            .preview()
            .logins,
        1
    );
    let notes = "First\nbody\n---\nSecond\nbody\n";
    assert_eq!(
        prepare_import("notes".into(), notes.into())
            .unwrap()
            .preview()
            .notes,
        2
    );
}

#[test]
fn bad_files_fail_with_a_fixed_reason_that_never_quotes_the_file() {
    let secret = "generated-import-secret-9";
    let wrong = prepare_import("apple".into(), chrome(1, secret))
        .err()
        .unwrap();
    assert_eq!(wrong, ImportFailure::WrongFormat);
    assert!(!wrong.to_string().contains(secret));
    assert_eq!(
        prepare_import("bitwarden".into(), chrome(1, secret))
            .err()
            .unwrap(),
        ImportFailure::UnknownSource
    );
    assert_eq!(
        prepare_import("chrome".into(), vec![0xff, 0xfe, 0x00])
            .err()
            .unwrap(),
        ImportFailure::NotUtf8
    );
    assert_eq!(
        prepare_import(
            "chrome".into(),
            b"name,url,username,password,note\n".to_vec()
        )
        .err()
        .unwrap(),
        ImportFailure::Empty
    );
}
