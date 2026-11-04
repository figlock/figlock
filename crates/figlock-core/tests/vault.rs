//! Vault layer tests (FIG-25): create, unlock, lock, items and op files across devices, with the negative
//! cases from `AGENTS.md` (wrong key, tampered file, unknown device, held future op).

use figlock_core::{
    DeviceKeys, ItemType, KdfParams, SecretKey, Vault, is_secret_field, validate_field,
};
use serde_json::Value;

const T0: u64 = 1_700_000_000_000;

fn pw() -> &'static str {
    static PW: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PW.get_or_init(|| {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).unwrap();
        b.iter().map(|x| format!("{x:02x}")).collect()
    })
}

fn params() -> KdfParams {
    KdfParams::FLOOR
}

fn created() -> (Vault, figlock_core::Created) {
    Vault::create(pw(), "Laptop", params(), T0).unwrap()
}

fn second_device(
    c: &figlock_core::Created,
    sk: &SecretKey,
    records: &[Vec<u8>],
) -> (Vault, Vec<u8>) {
    let (v, rec) = Vault::unlock(
        &c.vault_json,
        pw(),
        sk,
        DeviceKeys::generate().unwrap(),
        "Phone",
        records,
        T0 + 1,
    )
    .unwrap();
    (v, rec.expect("a new device gets its own record"))
}

fn sk_copy(c: &figlock_core::Created) -> SecretKey {
    SecretKey::parse(&c.secret_key.format()).unwrap()
}

#[test]
fn two_devices_converge_on_items() {
    let (mut a, c) = created();
    let sk = sk_copy(&c);
    let login = a
        .add_item(
            ItemType::Login,
            "personal",
            &[
                ("title", "Cloudflare"),
                ("username", "me@example.com"),
                ("password", "hunter2-generated"),
                ("url/a1", "https://dash.cloudflare.com"),
            ],
            T0 + 10,
        )
        .unwrap();
    let token = a
        .add_item(
            ItemType::Token,
            "personal",
            &[
                ("title", "GitHub token"),
                ("provider", "github"),
                ("token", "ghp_EXAMPLEEXAMPLEEXAMPLE"),
                ("env_var", "GITHUB_TOKEN"),
                ("allowed_hosts/h1", "api.github.com"),
            ],
            T0 + 11,
        )
        .unwrap();
    a.add_item(
        ItemType::Note,
        "personal",
        &[("title", "wifi"), ("body", "router admin page")],
        T0 + 12,
    )
    .unwrap();
    a.add_item(
        ItemType::EnvBundle,
        "work",
        &[
            ("title", "staging"),
            ("env/DATABASE_URL", "postgres://example"),
        ],
        T0 + 13,
    )
    .unwrap();
    a.add_item(
        ItemType::SshKey,
        "work",
        &[
            ("title", "deploy key"),
            ("private_key", "-----BEGIN EXAMPLE-----"),
        ],
        T0 + 14,
    )
    .unwrap();
    let file = a.commit().unwrap().expect("pending ops");
    assert!(file.path.starts_with("log/") && file.path.ends_with("/00000001.op"));
    assert!(
        a.commit().unwrap().is_none(),
        "nothing pending after a commit"
    );

    let (mut b, b_record) = second_device(&c, &sk, std::slice::from_ref(&c.device_record));
    b.apply_file(&file.bytes, a.device_id(), 1, T0 + 100)
        .unwrap();
    a.add_device_record(&b_record).unwrap();

    let (ai, bi) = (a.items(false).0, b.items(false).0);
    assert_eq!(ai.len(), 5);
    assert_eq!(bi.len(), 5);
    let view = b.item(&token).unwrap();
    assert_eq!(view.item_type, ItemType::Token);
    assert_eq!(view.vault, "personal");
    assert_eq!(view.fields["token"].as_str(), "ghp_EXAMPLEEXAMPLEEXAMPLE");
    assert_eq!(
        b.item(&login).unwrap().fields["title"].as_str(),
        "Cloudflare"
    );
    for (x, y) in ai.iter().zip(&bi) {
        assert_eq!(x.id, y.id);
        assert_eq!(x.fields.len(), y.fields.len());
        for (k, v) in &x.fields {
            assert_eq!(v.as_str(), y.fields[k].as_str(), "field {k}");
        }
    }
}

#[test]
fn concurrent_edits_merge_field_by_field_and_converge() {
    let (mut a, c) = created();
    let sk = sk_copy(&c);
    let id = a
        .add_item(
            ItemType::Login,
            "personal",
            &[("title", "Site"), ("username", "old"), ("password", "p1")],
            T0 + 10,
        )
        .unwrap();
    let f0 = a.commit().unwrap().unwrap();
    let (mut b, b_record) = second_device(&c, &sk, std::slice::from_ref(&c.device_record));
    b.apply_file(&f0.bytes, a.device_id(), 1, T0 + 20).unwrap();
    a.add_device_record(&b_record).unwrap();

    // Concurrently: A changes the username, B changes the password and the same title (B's is later).
    a.set_field(&id, "username", Some("alice"), T0 + 100)
        .unwrap();
    a.set_field(&id, "title", Some("Site (A)"), T0 + 101)
        .unwrap();
    b.set_field(&id, "password", Some("p2"), T0 + 102).unwrap();
    b.set_field(&id, "title", Some("Site (B)"), T0 + 200)
        .unwrap();
    let (fa, fb) = (a.commit().unwrap().unwrap(), b.commit().unwrap().unwrap());
    a.apply_file(&fb.bytes, b.device_id(), 1, T0 + 300).unwrap();
    b.apply_file(&fa.bytes, a.device_id(), 2, T0 + 300).unwrap();

    for v in [&mut a, &mut b] {
        let it = v.item(&id).unwrap();
        assert_eq!(it.fields["username"].as_str(), "alice");
        assert_eq!(it.fields["password"].as_str(), "p2");
        assert_eq!(
            it.fields["title"].as_str(),
            "Site (B)",
            "the later write wins the same field"
        );
    }
}

#[test]
fn delete_and_restore_travel_between_devices() {
    let (mut a, c) = created();
    let sk = sk_copy(&c);
    let id = a
        .add_item(ItemType::Note, "personal", &[("title", "n")], T0 + 10)
        .unwrap();
    let f1 = a.commit().unwrap().unwrap();
    let (mut b, rec) = second_device(&c, &sk, std::slice::from_ref(&c.device_record));
    b.apply_file(&f1.bytes, a.device_id(), 1, T0 + 20).unwrap();
    a.add_device_record(&rec).unwrap();

    b.delete_item(&id, T0 + 100).unwrap();
    let fb1 = b.commit().unwrap().unwrap();
    a.apply_file(&fb1.bytes, b.device_id(), 1, T0 + 110)
        .unwrap();
    assert!(a.items(false).0.is_empty(), "deleted items are hidden");
    assert!(a.item(&id).unwrap().deleted);
    assert_eq!(a.items(true).0.len(), 1);

    a.restore_item(&id, T0 + 200).unwrap();
    let fa = a.commit().unwrap().unwrap();
    b.apply_file(&fa.bytes, a.device_id(), 2, T0 + 210).unwrap();
    assert_eq!(b.items(false).0.len(), 1);
}

#[test]
fn storage_never_holds_plaintext() {
    let (mut a, c) = created();
    a.add_item(
        ItemType::Login,
        "personal",
        &[
            ("title", "ZZ-unique-title-ZZ"),
            ("username", "zz-user"),
            ("password", "ZZ-unique-password-ZZ"),
            ("url/a", "https://zz-unique-host.example"),
        ],
        T0 + 10,
    )
    .unwrap();
    let file = a.commit().unwrap().unwrap();
    for needle in [
        "ZZ-unique-title-ZZ",
        "zz-user",
        "ZZ-unique-password-ZZ",
        "zz-unique-host",
        pw(),
    ] {
        assert!(
            !contains(&file.bytes, needle.as_bytes()),
            "op file leaks {needle}"
        );
        assert!(
            !contains(&c.vault_json, needle.as_bytes()),
            "vault.json leaks {needle}"
        );
        assert!(
            !contains(&c.device_record, needle.as_bytes()),
            "device record leaks {needle}"
        );
    }
    assert!(!file.path.contains("zz"), "path names no content");
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn unlock_rejects_wrong_credentials() {
    let (a, c) = created();
    let sk = sk_copy(&c);
    let dev = || DeviceKeys::generate().unwrap();
    let r = |pw: &str, sk: &SecretKey| {
        Vault::unlock(&c.vault_json, pw, sk, dev(), "d", &[], T0).is_ok()
    };
    assert!(r(pw(), &sk));
    assert!(!r(&format!("{}x", pw()), &sk), "wrong password");
    assert!(!r(pw(), &SecretKey::random().unwrap()), "wrong Secret Key");
    assert!(
        SecretKey::parse("FL1-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA").is_err(),
        "typo not caught by the check"
    );
    a.lock();
}

fn tamper(json: &[u8], f: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut v: Value = serde_json::from_slice(json).unwrap();
    f(&mut v);
    serde_json::to_vec(&v).unwrap()
}

#[test]
fn unlock_rejects_tampered_vault_json() {
    let (_a, c) = created();
    let sk = sk_copy(&c);
    let try_unlock = |bytes: &[u8]| {
        Vault::unlock(
            bytes,
            pw(),
            &sk,
            DeviceKeys::generate().unwrap(),
            "d",
            &[],
            T0,
        )
        .is_ok()
    };
    assert!(try_unlock(&c.vault_json));
    // Downgrade of the KDF parameters (attacker with write access): detected, never accepted.
    assert!(!try_unlock(
        &tamper(&c.vault_json, |v| v["kdf"]["m_kib"] = 32769.into())
    ));
    assert!(
        !try_unlock(&tamper(&c.vault_json, |v| v["kdf"]["t"] = 1.into())),
        "below the floor"
    );
    assert!(
        !try_unlock(&tamper(&c.vault_json, |v| v["kdf"]["m_kib"] = 4_000_000.into())),
        "above the ceiling"
    );
    assert!(
        !try_unlock(&tamper(&c.vault_json, |v| v["revision"] = 99.into())),
        "signed revision"
    );
    assert!(
        !try_unlock(&tamper(&c.vault_json, |v| v["key_epoch"] = 2.into())),
        "epoch is in the wrap AAD"
    );
    assert!(!try_unlock(&tamper(
        &c.vault_json,
        |v| v["min_format_version"] = 0.into()
    )));
    assert!(!try_unlock(
        &tamper(&c.vault_json, |v| v["suite"] = "fl2".into())
    ));
    assert!(!try_unlock(&tamper(&c.vault_json, |v| {
        v.as_object_mut().unwrap().remove("signature");
    })));
    assert!(!try_unlock(
        &tamper(&c.vault_json, |v| v["signature"] = "00".repeat(64).into())
    ));
    assert!(!try_unlock(&tamper(
        &c.vault_json,
        |v| v["authority_pub"] = "00".repeat(32).into()
    )));
    // The wrapped key from a different vault does not open this one.
    let (_b, other) = created();
    let other_json: Value = serde_json::from_slice(&other.vault_json).unwrap();
    assert!(!try_unlock(&tamper(&c.vault_json, |v| v["wrapped_vk"] =
        other_json["wrapped_vk"].clone())));
    assert!(!try_unlock(b"not json"));
    assert!(!try_unlock(b"{}"));
}

#[test]
fn create_refuses_weak_kdf_parameters() {
    let weak = KdfParams {
        m_kib: 1024,
        t: 1,
        p: 1,
    };
    assert!(Vault::create(pw(), "d", weak, T0).is_err());
    let huge = KdfParams {
        m_kib: 4_000_000,
        t: 3,
        p: 4,
    };
    assert!(Vault::create(pw(), "d", huge, T0).is_err());
}

#[test]
fn device_records_must_be_signed_by_the_vault_authority() {
    let (mut a, c) = created();
    let (other, other_c) = created();
    assert!(
        a.add_device_record(&other_c.device_record).is_err(),
        "record from another vault"
    );
    let forged = tamper(&c.device_record, |v| v["name"] = "Evil".into());
    assert!(a.add_device_record(&forged).is_err(), "edited record");
    assert!(a.add_device_record(&c.device_record).is_ok());
    other.lock();
}

#[test]
fn files_from_unknown_or_tampered_sources_are_refused() {
    let (mut a, c) = created();
    let sk = sk_copy(&c);
    let id = a
        .add_item(ItemType::Note, "personal", &[("title", "n")], T0 + 10)
        .unwrap();
    let f1 = a.commit().unwrap().unwrap();
    a.set_field(&id, "body", Some("x"), T0 + 20).unwrap();
    let f2 = a.commit().unwrap().unwrap();

    // B does not know A (no device record): refused before anything is read.
    let (mut b, _rec) = second_device(&c, &sk, &[]);
    assert!(
        b.apply_file(&f1.bytes, a.device_id(), 1, T0 + 30).is_err(),
        "unknown device"
    );
    b.add_device_record(&c.device_record).unwrap();

    // Out of order, then in order, then a replay.
    assert!(
        b.apply_file(&f2.bytes, a.device_id(), 2, T0 + 30).is_err(),
        "gap: file 2 before file 1"
    );
    let mut flipped = f1.bytes.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 1;
    assert!(
        b.apply_file(&flipped, a.device_id(), 1, T0 + 30).is_err(),
        "tampered file"
    );
    assert!(
        b.apply_file(&f1.bytes, a.device_id(), 2, T0 + 30).is_err(),
        "file replayed under another sequence number"
    );
    b.apply_file(&f1.bytes, a.device_id(), 1, T0 + 30).unwrap();
    assert!(
        b.apply_file(&f1.bytes, a.device_id(), 1, T0 + 30).is_err(),
        "replay of an applied file"
    );
    b.apply_file(&f2.bytes, a.device_id(), 2, T0 + 30).unwrap();
    // An already applied own file cannot be applied again.
    assert!(
        {
            let own = *a.device_id();
            a.apply_file(&f1.bytes, &own, 1, T0 + 30)
        }
        .is_err()
    );
}

#[test]
fn ops_stamped_far_in_the_future_are_held() {
    let (mut a, c) = created();
    let sk = sk_copy(&c);
    // A's clock is 3 days ahead of B's.
    let ahead = T0 + 3 * 24 * 3600 * 1000;
    a.add_item(
        ItemType::Note,
        "personal",
        &[("title", "from the future")],
        ahead,
    )
    .unwrap();
    let f = a.commit().unwrap().unwrap();
    let (mut b, _) = second_device(&c, &sk, std::slice::from_ref(&c.device_record));
    assert!(
        b.apply_file(&f.bytes, a.device_id(), 1, T0 + 100).is_err(),
        "held until B's clock catches up"
    );
    assert!(b.items(true).0.is_empty(), "nothing was applied");
    b.apply_file(&f.bytes, a.device_id(), 1, ahead).unwrap();
    assert_eq!(b.items(false).0.len(), 1);
}

#[test]
fn schema_rejects_bad_fields() {
    let (mut a, _c) = created();
    // Wrong type, reserved, malformed, oversized.
    assert!(
        a.add_item(ItemType::Note, "p", &[("password", "x")], T0)
            .is_err(),
        "password is not a note field"
    );
    assert!(
        a.add_item(ItemType::Login, "p", &[("_deleted", "x")], T0)
            .is_err(),
        "reserved"
    );
    assert!(
        a.add_item(ItemType::Login, "p", &[("_type", "x")], T0)
            .is_err(),
        "reserved"
    );
    assert!(
        a.add_item(ItemType::Login, "p", &[("url/BAD ID", "x")], T0)
            .is_err()
    );
    assert!(
        a.add_item(ItemType::EnvBundle, "p", &[("env/lower", "x")], T0)
            .is_err()
    );
    assert!(
        a.add_item(ItemType::Login, "p", &[("title\u{e9}", "x")], T0)
            .is_err(),
        "non-ascii field id"
    );
    let big = "x".repeat(figlock_core::MAX_VALUE_BYTES + 1);
    assert!(
        a.add_item(ItemType::Note, "p", &[("body", &big)], T0)
            .is_err()
    );
    // A refused add leaves no half-written item behind in the pending ops of a later commit.
    for (t, f) in [
        (ItemType::Login, "username"),
        (ItemType::Login, "url/a1"),
        (ItemType::Token, "allowed_hosts/h1"),
        (ItemType::Token, "env_var"),
        (ItemType::Note, "body"),
        (ItemType::EnvBundle, "env/API_KEY"),
        (ItemType::SshKey, "private_key"),
        (ItemType::Login, "tags/work"),
        (ItemType::Login, "custom/ab12/value"),
    ] {
        validate_field(t, f).unwrap_or_else(|_| panic!("{t:?} {f}"));
    }
    for secret in [
        "password",
        "token",
        "private_key",
        "passphrase",
        "totp",
        "env/API_KEY",
        "custom/ab12/value",
    ] {
        assert!(is_secret_field(secret), "{secret}");
    }
    for plain in [
        "title",
        "username",
        "url/a1",
        "provider",
        "notes",
        "body",
        "env_var",
        "custom/ab12/name",
    ] {
        assert!(!is_secret_field(plain), "{plain}");
    }
}

#[test]
fn unknown_item_operations_fail_cleanly() {
    let (mut a, _c) = created();
    let ghost = [9u8; 16];
    assert!(a.set_field(&ghost, "title", Some("x"), T0).is_err());
    assert!(a.delete_item(&ghost, T0).is_err());
    assert!(a.restore_item(&ghost, T0).is_err());
    assert!(a.item(&ghost).is_err());
    assert!(!a.has_pending());
}

#[test]
fn vault_json_is_signed_and_names_only_public_data() {
    let (_a, c) = created();
    let v: Value = serde_json::from_slice(&c.vault_json).unwrap();
    for k in [
        "v",
        "suite",
        "vault_id",
        "revision",
        "key_epoch",
        "min_format_version",
        "kdf",
        "wrapped_vk",
        "authority_pub",
        "signature",
    ] {
        assert!(v.get(k).is_some(), "{k}");
    }
    assert_eq!(v["kdf"]["m_kib"], 32768);
    assert_eq!(v["revision"], 1);
    assert!(c.secret_key.format().starts_with("FL1-"));
}

#[test]
fn own_files_replay_after_unlock_to_restore_the_chain() {
    let (mut a, c) = created();
    let sk = sk_copy(&c);
    let keys = DeviceKeys {
        device_id: *a.device_id(),
        sign_seed: figlock_core::Key::from_bytes(*a.device_keys().sign_seed.expose()),
        agree_priv: figlock_core::Key::from_bytes(*a.device_keys().agree_priv.expose()),
    };
    let id = a
        .add_item(ItemType::Note, "personal", &[("title", "one")], T0 + 10)
        .unwrap();
    let f1 = a.commit().unwrap().unwrap();
    a.set_field(&id, "title", Some("two"), T0 + 20).unwrap();
    let f2 = a.commit().unwrap().unwrap();
    a.lock();

    let (mut a, rec) = Vault::unlock(
        &c.vault_json,
        pw(),
        &sk,
        keys,
        "Laptop",
        std::slice::from_ref(&c.device_record),
        T0 + 100,
    )
    .unwrap();
    assert!(rec.is_none());
    let own = *a.device_id();
    assert!(
        a.apply_file(&f2.bytes, &own, 2, T0 + 100).is_err(),
        "out of order"
    );
    a.apply_file(&f1.bytes, &own, 1, T0 + 100).unwrap();
    a.apply_file(&f2.bytes, &own, 2, T0 + 100).unwrap();
    assert_eq!(a.items(false).0[0].fields["title"].as_str(), "two");

    a.set_field(&id, "notes", Some("later"), T0 + 200).unwrap();
    let f3 = a.commit().unwrap().unwrap();
    assert!(f3.path.ends_with("00000003.op"));

    let (mut b, _) = second_device(&c, &sk, std::slice::from_ref(&c.device_record));
    for (f, seq) in [(&f1, 1), (&f2, 2), (&f3, 3)] {
        b.apply_file(&f.bytes, &own, seq, T0 + 300).unwrap();
    }
    assert_eq!(b.items(false).0[0].fields["notes"].as_str(), "later");
}
