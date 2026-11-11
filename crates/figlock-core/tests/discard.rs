//! Taking back changes that were not saved (FIG-88): a failed save must leave the vault as it was, and a
//! failed upload must be retryable, so the app can keep the session open and let the person save again.

mod common;

use common::{T0, new_vault, pw, sk_copy};
use figlock_core::{DeviceKeys, ItemType, Vault};

fn add_login(v: &mut Vault, title: &str, now: u64) -> figlock_core::ItemId {
    v.add_item(
        ItemType::Login,
        "personal",
        &[("title", title), ("password", "generated-example-1")],
        now,
    )
    .unwrap()
}

fn title(v: &mut Vault, id: &figlock_core::ItemId) -> Option<String> {
    v.item(id)
        .ok()
        .map(|i| i.fields["title"].as_str().to_owned())
}

#[test]
fn discarding_removes_new_items_and_edits() {
    let (mut v, _) = new_vault();
    let kept = add_login(&mut v, "Kept", T0 + 1);
    v.commit().unwrap().unwrap();

    v.set_field(&kept, "title", Some("Edited"), T0 + 2).unwrap();
    let gone = add_login(&mut v, "Never saved", T0 + 3);
    v.delete_item(&kept, T0 + 4).unwrap();
    assert!(v.has_pending());

    v.discard_pending();
    assert!(!v.has_pending());
    assert_eq!(title(&mut v, &kept).as_deref(), Some("Kept"));
    assert!(v.item(&gone).is_err());
    assert_eq!(v.items(false).0.len(), 1, "the deleted flag is back too");
    assert!(v.commit().unwrap().is_none(), "nothing is left to write");
}

#[test]
fn a_change_after_a_discard_commits_cleanly() {
    let (mut v, _) = new_vault();
    let id = add_login(&mut v, "One", T0 + 1);
    v.commit().unwrap().unwrap();
    v.set_field(&id, "title", Some("Half applied"), T0 + 2)
        .unwrap();
    v.discard_pending();

    v.set_field(&id, "title", Some("Second try"), T0 + 3)
        .unwrap();
    let f = v.commit().unwrap().expect("pending");
    assert!(f.path.ends_with("/00000002.op"), "the sequence has no gap");
    assert_eq!(title(&mut v, &id).as_deref(), Some("Second try"));
}

#[test]
fn a_failed_upload_is_taken_back_and_can_be_retried_with_the_same_sequence() {
    let (mut v, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let id = add_login(&mut v, "Draft", T0 + 1);
    let first = v.commit().unwrap().unwrap();

    // The upload of this one fails.
    v.set_field(&id, "title", Some("Lost in transit"), T0 + 2)
        .unwrap();
    let lost = v.commit().unwrap().unwrap();
    assert!(lost.path.ends_with("/00000002.op"));
    v.rollback_commit();
    assert!(v.has_pending(), "the change is pending again");

    // Retrying writes the same sequence number, so other devices see no gap.
    let retry = v.commit().unwrap().unwrap();
    assert_eq!(retry.path, lost.path);

    // A second device that applies the first file and the retry converges.
    let (mut b, _) = Vault::unlock(
        &c.vault_json,
        pw(),
        &sk,
        DeviceKeys::generate().unwrap(),
        "Phone",
        std::slice::from_ref(&c.device_record),
        T0 + 10,
    )
    .unwrap();
    b.apply_file(&first.bytes, v.device_id(), 1, T0 + 11)
        .unwrap();
    b.apply_file(&retry.bytes, v.device_id(), 2, T0 + 12)
        .unwrap();
    assert_eq!(title(&mut b, &id).as_deref(), Some("Lost in transit"));
}

#[test]
fn a_failed_upload_can_be_discarded() {
    let (mut v, _) = new_vault();
    let id = add_login(&mut v, "Stable", T0 + 1);
    v.commit().unwrap().unwrap();

    v.set_field(&id, "title", Some("Not saved"), T0 + 2)
        .unwrap();
    let new = add_login(&mut v, "Also not saved", T0 + 3);
    v.commit().unwrap().unwrap();
    v.rollback_commit();
    v.discard_pending();

    assert_eq!(title(&mut v, &id).as_deref(), Some("Stable"));
    assert!(v.item(&new).is_err());
    assert!(!v.has_pending());
    v.set_field(&id, "title", Some("Next"), T0 + 4).unwrap();
    let f = v.commit().unwrap().unwrap();
    assert!(f.path.ends_with("/00000002.op"));
}

#[test]
fn rollback_does_nothing_once_something_changed_after_the_commit() {
    let (mut v, _) = new_vault();
    let id = add_login(&mut v, "One", T0 + 1);
    v.commit().unwrap().unwrap();
    v.set_field(&id, "title", Some("Two"), T0 + 2).unwrap();
    v.commit().unwrap().unwrap();
    v.set_field(&id, "title", Some("Three"), T0 + 3).unwrap();

    v.rollback_commit();
    let f = v.commit().unwrap().unwrap();
    assert!(
        f.path.ends_with("/00000003.op"),
        "the committed file stays committed"
    );
}

#[test]
fn discarding_keeps_a_value_another_device_wrote_in_the_meantime() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let id = add_login(&mut a, "Shared", T0 + 1);
    let first = a.commit().unwrap().unwrap();

    let (mut b, b_record) = Vault::unlock(
        &c.vault_json,
        pw(),
        &sk,
        DeviceKeys::generate().unwrap(),
        "Phone",
        std::slice::from_ref(&c.device_record),
        T0 + 10,
    )
    .unwrap();
    a.add_device_record(&b_record.expect("new device record"))
        .unwrap();
    b.apply_file(&first.bytes, a.device_id(), 1, T0 + 11)
        .unwrap();
    b.set_field(&id, "title", Some("From the phone"), T0 + 5_000)
        .unwrap();
    let from_b = b.commit().unwrap().unwrap();

    // `a` has an unsaved edit with an older stamp, then receives the phone's newer one, then discards.
    a.set_field(&id, "title", Some("Unsaved on laptop"), T0 + 2)
        .unwrap();
    a.apply_file(&from_b.bytes, b.device_id(), 1, T0 + 6_000)
        .unwrap();
    a.discard_pending();
    assert_eq!(title(&mut a, &id).as_deref(), Some("From the phone"));
}
