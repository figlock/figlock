//! Importing an export file (FIG-285), against a real folder.

use std::path::PathBuf;

use super::device_tests::unlocked;
use super::tests::{code_of, desk, scenario_in};

fn scenario(name: &str) {
    scenario_in("import_tests", name);
}

/// A file with `text` in a folder next to the scenario's vault.
fn file(name: &str, text: &str) -> PathBuf {
    let dir = PathBuf::from(format!("{}-files", super::tests::vault_dir()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn an_import_previews_first_and_writes_only_when_confirmed() {
    scenario("import_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn import_inner() {
    let d = desk();
    let chrome = file(
        "chrome.csv",
        "name,url,username,password,note\nGitHub,https://github.com,me,a-generated-one,\nBank,https://bank.example,me,another-one,\n,,,,\n",
    );
    // Locked: nothing is read or held.
    assert_eq!(
        code_of(d.import_prepare("chrome", &chrome).unwrap_err()),
        "locked"
    );
    unlocked(&d);

    // The preview has counts and titles and writes nothing.
    let p = d.import_prepare("chrome", &chrome).unwrap();
    assert_eq!((p.logins, p.notes, p.skipped), (2, 0, 1));
    assert_eq!(p.titles, ["GitHub", "Bank"]);
    assert!(d.list_items("").unwrap().is_empty());

    // Discarding forgets the file; confirming then has nothing to write.
    d.import_discard();
    assert_eq!(code_of(d.import_commit().unwrap_err()), "nothing_to_import");

    // Confirming writes the items.
    d.import_prepare("chrome", &chrome).unwrap();
    assert_eq!(d.import_commit().unwrap(), 2);
    assert_eq!(d.list_items("").unwrap().len(), 2);
    // The held file is gone: a second confirm adds nothing.
    assert!(d.import_commit().is_err());
    assert_eq!(d.list_items("").unwrap().len(), 2);

    // Apple Passwords and a notes dump go through the same path.
    let apple = file(
        "apple.csv",
        "Title,URL,Username,Password,Notes,OTPAuth\nMail,https://mail.example,me,pw-for-mail,,\n",
    );
    assert_eq!(d.import_prepare("apple", &apple).unwrap().logins, 1);
    d.import_discard();
    let notes = file("notes.txt", "First note\nbody\n---\nSecond note\nbody\n");
    assert_eq!(d.import_prepare("notes", &notes).unwrap().notes, 2);
    d.import_discard();

    // A file that is not that export, a wrong source and a missing file are refused in fixed words.
    let wrong = d.import_prepare("apple", &chrome).unwrap_err();
    assert_eq!(wrong.code, "bad_file");
    assert!(!wrong.message.contains("a-generated-one"));
    assert_eq!(
        code_of(d.import_prepare("bitwarden", &chrome).unwrap_err()),
        "bad_source"
    );
    assert_eq!(
        code_of(
            d.import_prepare("chrome", &chrome.with_extension("nope"))
                .unwrap_err()
        ),
        "unreadable"
    );
    let empty = file("empty.csv", "name,url,username,password,note\n");
    assert_eq!(
        code_of(d.import_prepare("chrome", &empty).unwrap_err()),
        "nothing_to_import"
    );
    // A failed attempt holds nothing.
    assert!(d.import_commit().is_err());

    // Locking drops a file that was waiting.
    d.import_prepare("chrome", &chrome).unwrap();
    d.lock();
    assert!(d.import_commit().is_err());
}
