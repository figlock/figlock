//! The exported API as the Swift and Kotlin apps call it: create, unlock, items, search, commit and apply,
//! kit, and the negative cases from `AGENTS.md` (wrong key, tampered file, locked use, bad handles).
//! Values are generated; nothing here is a real secret.

use std::sync::Arc;

use figlock_ffi::{
    CreatedVault, DeviceKeysHandle, FieldInput, FigLockError, VaultSession, create_vault,
    default_kdf_settings, minimum_kdf_settings, secret_key_is_valid, unlock_vault,
};

const T0: u64 = 1_800_000_000_000;
const PASSWORD: &str = "generated-test-password-1";
const SECRET: &str = "generated-test-secret-value-7";

fn created() -> Arc<CreatedVault> {
    create_vault(
        PASSWORD.into(),
        "test phone".into(),
        minimum_kdf_settings(),
        T0,
    )
    .unwrap()
}

fn field(name: &str, value: &str) -> FieldInput {
    FieldInput {
        name: name.into(),
        value: value.into(),
    }
}

fn add_login(s: &VaultSession) -> String {
    s.add_item(
        "login".into(),
        "personal".into(),
        vec![
            field("title", "Cloudflare"),
            field("url/a1", "https://dash.cloudflare.com"),
            field("password", SECRET),
        ],
        T0 + 1,
    )
    .unwrap()
}

/// Unlock `c`'s vault again as the same device, from the stored files.
fn unlock_same_device(
    c: &CreatedVault,
    password: &str,
    secret_key: &str,
) -> Result<Arc<VaultSession>, FigLockError> {
    let device = Arc::new(DeviceKeysHandle::from_bytes(c.device_keys().to_bytes()).unwrap());
    unlock_vault(
        c.vault_json().unwrap(),
        password.into(),
        secret_key.into(),
        device,
        "test phone".into(),
        vec![c.device_record().unwrap()],
        T0 + 10,
    )
    .map(|o| o.session)
}

#[test]
fn create_edit_lock_unlock_roundtrip() {
    let c = created();
    let secret_key = c.secret_key_text().unwrap();
    let s = c.take_session().unwrap();
    let id = add_login(&s);
    let op = s.commit().unwrap().expect("pending edits give an op file");
    assert!(op.path.starts_with("log/"), "op path is under log/");
    assert!(
        s.commit().unwrap().is_none(),
        "nothing pending after commit"
    );

    s.lock();
    let again = unlock_same_device(&c, PASSWORD, &secret_key).unwrap();
    assert!(
        again.items(false).unwrap().items.is_empty(),
        "items come from op files, not vault.json"
    );
    again
        .apply_file(op.bytes, again.device_id().unwrap(), 1, T0 + 11)
        .unwrap();

    let listing = again.items(false).unwrap();
    assert_eq!(listing.items.len(), 1);
    assert!(listing.corrupt_ids.is_empty());
    let item = &listing.items[0];
    assert_eq!(item.id, id);
    assert_eq!(item.item_type, "login");
    assert_eq!(item.fields["title"], "Cloudflare");
    assert_eq!(item.secret_fields, vec!["password".to_string()]);
    assert_eq!(again.reveal_field(id, "password".into()).unwrap(), SECRET);
}

#[test]
fn listing_and_search_never_expose_secret_values() {
    let c = created();
    let s = c.take_session().unwrap();
    let id = add_login(&s);

    let listing = s.items(false).unwrap();
    for item in &listing.items {
        assert!(
            !item.fields.contains_key("password"),
            "secret field absent from fields"
        );
        assert!(
            item.fields.values().all(|v| v != SECRET),
            "secret value absent from fields"
        );
    }

    let hit = s.search("cldflr".into(), T0 + 2, 5).unwrap();
    assert_eq!(
        hit.first().map(|h| h.id.as_str()),
        Some(id.as_str()),
        "typo search finds the item"
    );
    for needle in [SECRET, &SECRET[..12], "generated-test-secret"] {
        assert!(
            s.search(needle.into(), T0 + 2, 5).unwrap().is_empty(),
            "secret text is not searchable"
        );
    }

    // A change is visible to the next search (the index is rebuilt after edits).
    s.set_field(id.clone(), "title".into(), Some("Zebra".into()), T0 + 3)
        .unwrap();
    assert_eq!(s.search("zebra".into(), T0 + 4, 5).unwrap().len(), 1);
    s.delete_item(id.clone(), T0 + 5).unwrap();
    assert!(
        s.search("zebra".into(), T0 + 6, 5).unwrap().is_empty(),
        "deleted items leave search"
    );
    s.restore_item(id, T0 + 7).unwrap();
    assert_eq!(s.search("zebra".into(), T0 + 8, 5).unwrap().len(), 1);
}

#[test]
fn second_device_gets_a_record_and_the_data() {
    let c = created();
    let secret_key = c.secret_key_text().unwrap();
    let a = c.take_session().unwrap();
    let id = add_login(&a);
    let op = a.commit().unwrap().unwrap();
    let a_id = a.device_id().unwrap();

    let b_keys = Arc::new(DeviceKeysHandle::generate().unwrap());
    let outcome = unlock_vault(
        c.vault_json().unwrap(),
        PASSWORD.into(),
        secret_key,
        b_keys,
        "tablet".into(),
        vec![c.device_record().unwrap()],
        T0 + 20,
    )
    .unwrap();
    assert!(
        outcome.new_device_record.is_some(),
        "a new device gets a signed record to store"
    );
    outcome
        .session
        .apply_file(op.bytes, a_id, 1, T0 + 21)
        .unwrap();
    assert_eq!(
        outcome.session.reveal_field(id, "password".into()).unwrap(),
        SECRET
    );
}

#[test]
fn unlock_failures_are_one_generic_error() {
    let c = created();
    let secret_key = c.secret_key_text().unwrap();

    let other_key = created().secret_key_text().unwrap();
    assert!(
        secret_key_is_valid(other_key.clone()),
        "a different key is well formed"
    );

    for (name, password, key) in [
        ("wrong password", "not-the-password", secret_key.as_str()),
        ("wrong secret key", PASSWORD, other_key.as_str()),
    ] {
        let err = unlock_same_device(&c, password, key).err().expect(name);
        assert_eq!(err, FigLockError::Failed, "{name}");
        let shown = format!("{err} {err:?}");
        assert!(
            !shown.contains(password) && !shown.contains(key),
            "{name}: error text holds no input"
        );
    }

    // A malformed Secret Key is reported as a typo, before any slow key derivation.
    assert_eq!(
        unlock_same_device(&c, PASSWORD, "FL1-not-a-key").err(),
        Some(FigLockError::InvalidInput)
    );
    assert!(!secret_key_is_valid("FL1-not-a-key".into()));
    assert!(secret_key_is_valid(secret_key));
}

#[test]
fn tampered_files_are_refused_and_change_nothing() {
    let c = created();
    let secret_key = c.secret_key_text().unwrap();
    let s = c.take_session().unwrap();
    add_login(&s);
    let op = s.commit().unwrap().unwrap();
    let a_id = s.device_id().unwrap();
    s.lock();

    let again = unlock_same_device(&c, PASSWORD, &secret_key).unwrap();
    let mut bad = op.bytes.clone();
    let mid = bad.len() / 2;
    bad[mid] ^= 0x01;
    assert_eq!(
        again.apply_file(bad, a_id.clone(), 1, T0 + 11).err(),
        Some(FigLockError::Failed)
    );
    assert!(
        again.items(false).unwrap().items.is_empty(),
        "a refused file changes nothing"
    );
    assert_eq!(
        again.apply_file(vec![], a_id.clone(), 1, T0 + 12).err(),
        Some(FigLockError::Failed)
    );
    // The untouched file still applies afterwards.
    again.apply_file(op.bytes, a_id, 1, T0 + 13).unwrap();
    assert_eq!(again.items(false).unwrap().items.len(), 1);

    // A tampered vault.json does not unlock.
    let mut vj = c.vault_json().unwrap();
    let n = vj.len() / 2;
    vj[n] ^= 0x01;
    let device = Arc::new(DeviceKeysHandle::from_bytes(c.device_keys().to_bytes()).unwrap());
    let r = unlock_vault(
        vj,
        PASSWORD.into(),
        secret_key,
        device,
        "x".into(),
        vec![c.device_record().unwrap()],
        T0 + 14,
    );
    assert_eq!(r.err(), Some(FigLockError::Failed));
}

#[test]
fn a_locked_session_refuses_everything() {
    let c = created();
    let s = c.take_session().unwrap();
    let id = add_login(&s);
    assert!(!s.is_locked());
    s.lock();
    s.lock(); // idempotent
    assert!(s.is_locked());

    let locked = Some(FigLockError::Locked);
    assert_eq!(s.items(false).err(), locked);
    assert_eq!(s.reveal_field(id.clone(), "password".into()).err(), locked);
    assert_eq!(s.search("a".into(), T0, 5).err(), locked);
    assert_eq!(
        s.add_item("note".into(), "v".into(), vec![], T0).err(),
        locked
    );
    assert_eq!(
        s.set_field(id.clone(), "title".into(), None, T0).err(),
        locked
    );
    assert_eq!(s.delete_item(id.clone(), T0).err(), locked);
    assert_eq!(s.restore_item(id.clone(), T0).err(), locked);
    assert_eq!(s.commit().err(), locked);
    assert_eq!(s.apply_file(vec![1], id.clone(), 1, T0).err(), locked);
    assert_eq!(s.add_device_record(vec![1]).err(), locked);
    assert_eq!(s.vault_id().err(), locked);
    assert_eq!(s.device_id().err(), locked);
    assert_eq!(s.epoch().err(), locked);
    assert_eq!(s.revision().err(), locked);
    assert_eq!(s.has_pending().err(), locked);
}

#[test]
fn bad_handles_and_arguments() {
    let c = created();
    let s = c.take_session().unwrap();
    let id = add_login(&s);

    // Malformed ids are InvalidInput; well-formed but unknown ids are the generic Failed.
    for bad in [
        "",
        "nope",
        "zz".repeat(16).as_str(),
        &id[..30],
        &format!("{id}00"),
        "é".repeat(16).as_str(),
    ] {
        assert_eq!(
            s.reveal_field(bad.into(), "password".into()).err(),
            Some(FigLockError::InvalidInput),
            "id {bad:?}"
        );
        assert_eq!(
            s.delete_item(bad.into(), T0).err(),
            Some(FigLockError::InvalidInput)
        );
        assert_eq!(
            s.apply_file(vec![1], bad.into(), 1, T0).err(),
            Some(FigLockError::InvalidInput)
        );
    }
    let unknown = "00".repeat(16);
    assert_eq!(
        s.reveal_field(unknown.clone(), "password".into()).err(),
        Some(FigLockError::Failed)
    );
    assert_eq!(s.delete_item(unknown, T0).err(), Some(FigLockError::Failed));
    assert_eq!(
        s.reveal_field(id.clone(), "no_such_field".into()).err(),
        Some(FigLockError::Failed)
    );

    // Schema checks.
    assert_eq!(
        s.add_item("spaceship".into(), "v".into(), vec![], T0).err(),
        Some(FigLockError::InvalidInput)
    );
    assert_eq!(
        s.add_item(
            "login".into(),
            "v".into(),
            vec![field("not a field!", "x")],
            T0
        )
        .err(),
        Some(FigLockError::Failed)
    );
    assert_eq!(
        s.set_field(id, "private_key".into(), Some("x".into()), T0)
            .err(),
        Some(FigLockError::Failed),
        "a field outside the item type's schema is refused"
    );

    // Device keys: wrong lengths are refused, and a round trip keeps the id.
    for len in [0usize, 1, 79, 81, 200] {
        assert_eq!(
            DeviceKeysHandle::from_bytes(vec![7; len]).err(),
            Some(FigLockError::InvalidInput)
        );
    }
    let keys = DeviceKeysHandle::generate().unwrap();
    let back = DeviceKeysHandle::from_bytes(keys.to_bytes()).unwrap();
    assert_eq!(keys.device_id(), back.device_id());
    assert_eq!(keys.device_id().len(), 32);

    // KDF settings outside core's bounds are refused.
    let mut weak = minimum_kdf_settings();
    weak.m_kib = 1;
    assert_eq!(
        create_vault(PASSWORD.into(), "d".into(), weak, T0).err(),
        Some(FigLockError::Failed)
    );
    assert!(default_kdf_settings().m_kib > minimum_kdf_settings().m_kib);
}

#[test]
fn created_vault_hands_out_the_session_once() {
    let c = created();
    assert!(c.take_session().is_ok());
    assert_eq!(c.take_session().err(), Some(FigLockError::Locked));
    // The files and kit stay available after the session has been taken.
    assert!(!c.vault_json().unwrap().is_empty());
    assert!(!c.device_record().unwrap().is_empty());
}

#[test]
fn emergency_kit_and_its_verification() {
    let c = created();
    let key = c.secret_key_text().unwrap();
    assert!(key.starts_with("FL1-"));

    let pdf = c
        .render_kit(
            "Personal".into(),
            "Local folder".into(),
            "2026-11-05".into(),
        )
        .unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    // The password is never on the kit.
    assert!(
        !pdf.windows(PASSWORD.len())
            .any(|w| w == PASSWORD.as_bytes())
    );

    let groups: Vec<&str> = key.split('-').collect();
    let last_two = format!("{}-{}", groups[groups.len() - 2], groups[groups.len() - 1]);
    assert!(c.verify_kit_groups(last_two.clone()).unwrap());
    assert!(
        c.verify_kit_groups(last_two.to_lowercase()).unwrap(),
        "case does not matter"
    );
    assert!(!c.verify_kit_groups("AAAA-AAAA".into()).unwrap());
    assert!(!c.verify_kit_groups(String::new()).unwrap());

    for bad_date in ["2026-13", "yesterday", ""] {
        assert_eq!(
            c.render_kit("v".into(), "s".into(), bad_date.into()).err(),
            Some(FigLockError::InvalidInput)
        );
    }
}

#[test]
fn errors_never_contain_text() {
    for e in [
        FigLockError::Failed,
        FigLockError::Locked,
        FigLockError::InvalidInput,
    ] {
        let text = e.to_string();
        assert!(text.len() < 32 && !text.contains(PASSWORD) && !text.contains(SECRET));
    }
}
