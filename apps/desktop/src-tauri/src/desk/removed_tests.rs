//! A device the vault removed, and the Emergency Kit saved again (FIG-135, FIG-134), against a real folder.

use std::collections::BTreeMap;
use std::path::Path;

use figlock_core::DeviceKeys;

use super::device_tests::unlocked;
use super::items::ItemDraft;
use super::tests::{code_of, desk, scenario_in, vault_dir};
use super::*;

fn scenario(name: &str) {
    scenario_in("removed_tests", name);
}

fn draft(title: &str) -> ItemDraft {
    ItemDraft {
        fields: BTreeMap::from([("title".to_owned(), title.to_owned())]),
        clear: Vec::new(),
        add_files: Vec::new(),
        remove_files: Vec::new(),
    }
}

/// Another device of the same vault, opened from the folder, removes this desk's device and writes the result
/// where a sync would bring it.
fn another_device_removes_this_one(d: &Desk, pw: &str, key: &str) {
    let secret_key = SecretKey::parse(key).unwrap();
    let this = *d.inner().session.as_ref().unwrap().vault.device_id();
    let root = vault_dir();
    let dir = Path::new(&root);
    let vault_json = std::fs::read(dir.join("vault.json")).unwrap();
    let records: Vec<Vec<u8>> = std::fs::read_dir(dir.join("devices"))
        .unwrap()
        .map(|e| std::fs::read(e.unwrap().path()).unwrap())
        .collect();
    let (mut other, record) = Vault::unlock(
        &vault_json,
        pw,
        &secret_key,
        DeviceKeys::generate().unwrap(),
        "Other Mac",
        &records,
        now_ms(),
    )
    .unwrap();
    let record = record.unwrap();
    std::fs::write(
        dir.join(format!("devices/{}.json", hex::encode(other.device_id()))),
        &record,
    )
    .unwrap();
    let r = other
        .revoke_device(pw, &secret_key, &this, now_ms() + 1_000)
        .unwrap();
    std::fs::create_dir_all(dir.join(Path::new(&r.snapshot.path).parent().unwrap())).unwrap();
    std::fs::write(dir.join(&r.snapshot.path), &r.snapshot.bytes).unwrap();
    for rec in &r.records {
        std::fs::write(dir.join(&rec.path), &rec.bytes).unwrap();
    }
    for gone in &r.remove {
        let _ = std::fs::remove_file(dir.join(gone));
    }
    std::fs::write(dir.join("vault.json"), &r.vault_json).unwrap();
}

#[test]
fn a_removed_mac_is_told_refused_every_write_and_can_leave_the_vault() {
    scenario("removed_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn removed_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    d.add_item("note", draft("Before")).unwrap();
    // Before anything is synced it cannot know.
    assert_eq!(d.state().unwrap().removed_at_ms, None);

    another_device_removes_this_one(&d, &pw, &key);
    d.lock();
    let state = d.unlock(&pw, Some(&key)).unwrap();
    assert_eq!(state.phase, "unlocked", "the copy stays readable");
    assert!(state.removed_at_ms.is_some());
    assert_eq!(d.list_items("").unwrap().len(), 1);

    // Every write is refused with the words the window shows, and nothing reaches the log.
    let log = Path::new(&vault_dir()).join("log");
    let files = || walk(&log);
    let before = files();
    for err in [
        d.add_item("note", draft("Lost")).unwrap_err(),
        d.update_item(&d.list_items("").unwrap()[0].id, draft("Changed"))
            .unwrap_err(),
        d.delete_item(&d.list_items("").unwrap()[0].id).unwrap_err(),
    ] {
        assert_eq!(err.code, "device_revoked");
        assert!(err.message.contains("removed from the vault on"));
        assert!(err.message.contains("join again with a new invite"));
    }
    assert_eq!(files(), before);
    assert_eq!(d.list_items("").unwrap().len(), 1);

    // Leaving stops using the vault here; the folder is left alone.
    d.leave_vault().unwrap();
    assert_eq!(d.state().unwrap().phase, "none");
    assert!(Path::new(&vault_dir()).join("vault.json").exists());
}

#[test]
fn a_removal_while_the_window_is_unlocked_is_noticed_and_refuses_writes() {
    scenario("removed_open_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn removed_open_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    d.add_item("note", draft("Before")).unwrap();
    assert_eq!(d.state().unwrap().removed_at_ms, None);

    // Removed by another device while this window stays unlocked: the next refresh tells the window.
    another_device_removes_this_one(&d, &pw, &key);
    assert!(d.refresh_local(), "the window must be told to reload");
    assert!(d.state().unwrap().removed_at_ms.is_some());

    let log = Path::new(&vault_dir()).join("log");
    let before = walk(&log);
    let err = d.add_item("note", draft("Lost")).unwrap_err();
    assert_eq!(err.code, "device_revoked");
    assert_eq!(walk(&log), before);
    assert_eq!(d.list_items("").unwrap().len(), 1);
}

fn walk(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |rd| {
        rd.map(|e| {
            let p = e.unwrap().path();
            if p.is_dir() { walk(&p) } else { 1 }
        })
        .sum()
    })
}

#[test]
fn a_device_that_was_not_removed_cannot_use_the_leave_step() {
    scenario("not_removed_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn not_removed_inner() {
    let d = desk();
    assert_eq!(code_of(d.leave_vault().unwrap_err()), "locked");
    let _ = unlocked(&d);
    assert_eq!(code_of(d.leave_vault().unwrap_err()), "not_removed");
    assert_eq!(d.state().unwrap().phase, "unlocked");
}

#[test]
fn the_kit_is_made_again_only_for_a_key_that_is_proved() {
    scenario("kit_again_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn kit_again_inner() {
    let d = desk();
    // Locked: nothing is made.
    assert_eq!(code_of(d.kit_again(None).unwrap_err()), "locked");
    let (pw, key) = unlocked(&d);

    // Nothing is kept on a headless run and no key is typed: no kit, and no key invented.
    assert_eq!(code_of(d.kit_again(None).unwrap_err()), "no_secret_key");

    // A typed pair that does not open the vault makes nothing.
    let other = SecretKey::random().unwrap().format().to_string();
    assert_eq!(
        code_of(d.kit_again(Some((&pw, &other))).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.kit_again(Some((&format!("{pw}-not"), &key))).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(
        code_of(d.kit_again(Some((&pw, "FL1-nope"))).unwrap_err()),
        "bad_secret_key"
    );

    // The proved key is the one on the kit, the same one Show Secret Key and the original kit carry.
    let pdf = d.kit_again(Some((&pw, &key))).unwrap();
    assert!(pdf.windows(key.len()).any(|w| w == key.as_bytes()));
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn deleting_the_drive_copy_needs_the_phrase_and_a_sign_in_and_touches_nothing_here() {
    scenario("delete_remote_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn delete_remote_inner() {
    let d = desk();
    let (_pw, _key) = unlocked(&d);
    let place = config::load().unwrap().place;
    let vault_json = std::fs::read(Path::new(&vault_dir()).join("vault.json")).unwrap();
    for typed in ["", "yes", "Personal", "delete"] {
        assert_eq!(
            code_of(d.sync_delete_remote(typed).unwrap_err()),
            "not_confirmed",
            "{typed:?}"
        );
    }
    // With the phrase but no Google sign-in, there is nothing to delete from, and nothing is changed.
    let phrase = figlock_session::sync::DELETE_REMOTE_PHRASE;
    assert_eq!(
        code_of(d.sync_delete_remote(phrase).unwrap_err()),
        "drive_sign_in"
    );
    assert_eq!(config::load().unwrap().place, place);
    assert_eq!(
        std::fs::read(Path::new(&vault_dir()).join("vault.json")).unwrap(),
        vault_json
    );
    assert_eq!(d.state().unwrap().phase, "unlocked");
}
