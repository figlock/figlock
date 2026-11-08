//! Document items (FIG-77, ADR-0024) through the public API: files are stored as base64 text fields, so they
//! ride the normal item encryption, merge and search paths. Includes the negative cases from `AGENTS.md`.
//! File contents are generated per run; nothing here is a real key or certificate.

mod common;

use common::{join, new_vault, sk_copy};
use figlock_core::{
    Index, ItemType, KdfParams, MAX_FIELDS_PER_ITEM, MAX_FILE_BYTES, MAX_FILES_PER_ITEM,
    MAX_VALUE_BYTES, Vault, decode_file, encode_file, is_secret_field, max_value_bytes,
    validate_field,
};

const T0: u64 = 1_700_000_000_000;

fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).unwrap();
    b
}

fn random_hex(n: usize) -> String {
    random_bytes(n).iter().map(|x| format!("{x:02x}")).collect()
}

fn vault() -> Vault {
    Vault::create(&random_hex(16), "Laptop", KdfParams::FLOOR, T0)
        .unwrap()
        .0
}

#[test]
fn a_p12_and_its_password_round_trip_through_one_item() {
    let mut v = vault();
    let file = random_bytes(3_000);
    let data = encode_file(&file).unwrap();
    let password = random_hex(12);
    let id = v
        .add_item(
            ItemType::Document,
            "work",
            &[
                ("title", "Apple signing"),
                ("url/a1", "https://developer.apple.com"),
                ("tags/ci", ""),
                ("password", &password),
                ("expires_at", "2027-12-08"),
                ("file/f1/name", "devid.p12"),
                ("file/f1/mime", "application/x-pkcs12"),
                ("file/f1/data", &data),
            ],
            T0,
        )
        .unwrap();
    let item = v.item(&id).unwrap();
    assert_eq!(item.item_type, ItemType::Document);
    assert_eq!(item.fields["file/f1/name"].as_str(), "devid.p12");
    assert_eq!(item.fields["password"].as_str(), password);
    assert_eq!(
        &**decode_file(&item.fields["file/f1/data"]).unwrap(),
        &file[..]
    );
}

#[test]
fn one_item_holds_several_files() {
    let mut v = vault();
    let a = random_bytes(100);
    let b = random_bytes(200);
    let id = v
        .add_item(
            ItemType::Document,
            "work",
            &[
                ("title", "Cert pair"),
                ("file/a/name", "cert.cer"),
                ("file/a/data", &encode_file(&a).unwrap()),
                ("file/b/name", "key.p8"),
                ("file/b/data", &encode_file(&b).unwrap()),
            ],
            T0,
        )
        .unwrap();
    // A file added later on another device is one more field, so it merges rather than colliding.
    v.set_field(&id, "file/c/name", Some("extra.json"), T0 + 1)
        .unwrap();
    let item = v.item(&id).unwrap();
    assert_eq!(&**decode_file(&item.fields["file/a/data"]).unwrap(), &a[..]);
    assert_eq!(&**decode_file(&item.fields["file/b/data"]).unwrap(), &b[..]);
    assert!(item.fields.contains_key("file/c/name"));
}

#[test]
fn the_largest_file_fits_and_one_byte_more_does_not() {
    let mut v = vault();
    let max = encode_file(&random_bytes(MAX_FILE_BYTES)).unwrap();
    assert_eq!(max.len(), MAX_VALUE_BYTES);
    v.add_item(
        ItemType::Document,
        "work",
        &[("title", "Big"), ("file/a/data", &max)],
        T0,
    )
    .unwrap();
    assert!(encode_file(&random_bytes(MAX_FILE_BYTES + 1)).is_err());
    // Even bypassing encode_file, the vault refuses an oversize field value.
    let too_big = "A".repeat(MAX_VALUE_BYTES + 4);
    assert!(
        v.add_item(
            ItemType::Document,
            "work",
            &[("title", "Too big"), ("file/a/data", &too_big)],
            T0
        )
        .is_err()
    );
}

#[test]
fn file_fields_belong_to_documents_only_and_names_are_strict() {
    for field in [
        "file/a/name",
        "file/a/mime",
        "file/a/data",
        "password",
        "expires_at",
        "url/a",
        "title",
        "notes",
    ] {
        assert!(validate_field(ItemType::Document, field).is_ok(), "{field}");
    }
    for field in ["file/a/name", "file/a/data"] {
        assert!(
            validate_field(ItemType::Login, field).is_err(),
            "login {field}"
        );
        assert!(
            validate_field(ItemType::Token, field).is_err(),
            "token {field}"
        );
    }
    for bad in [
        "file/a",
        "file//data",
        "file/A/data",
        "file/a/b/data",
        "file/a/other",
        "file/aaaaaaaaaaaaaaaaa/data",
        "file/a/data/",
        "_file/a/data",
        "username",
        "private_key",
    ] {
        assert!(validate_field(ItemType::Document, bad).is_err(), "{bad}");
    }
}

#[test]
fn file_contents_are_secret_but_names_are_not() {
    assert!(is_secret_field("file/a/data"));
    assert!(is_secret_field("password"));
    assert!(!is_secret_field("file/a/name"));
    assert!(!is_secret_field("file/a/mime"));
}

#[test]
fn search_finds_the_file_name_but_never_the_contents_or_password() {
    let mut v = vault();
    let file = random_bytes(600);
    let data = encode_file(&file).unwrap();
    let password = random_hex(12);
    let id = v
        .add_item(
            ItemType::Document,
            "work",
            &[
                ("title", "Notary key"),
                ("password", &password),
                ("file/a/name", "AuthKey_ABCDE12345.p8"),
                ("file/a/data", &data),
            ],
            T0,
        )
        .unwrap();
    let (items, corrupt) = v.items(false);
    assert!(corrupt.is_empty());
    let ix = Index::build(&items);
    let hits = |q: &str| {
        ix.search(q, T0, 10)
            .iter()
            .map(|h| h.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(hits("authkey"), vec![id], "file name");
    assert_eq!(hits("notary"), vec![id], "title");
    // A slice of the base64 and the password must find nothing.
    assert!(hits(&data[40..80]).is_empty(), "file contents were indexed");
    assert!(hits(&password).is_empty(), "password was indexed");
}

#[test]
fn removing_a_file_is_a_field_removal() {
    let mut v = vault();
    let id = v
        .add_item(
            ItemType::Document,
            "work",
            &[
                ("title", "Temp"),
                ("file/a/name", "a.bin"),
                ("file/a/data", &encode_file(&random_bytes(10)).unwrap()),
            ],
            T0,
        )
        .unwrap();
    v.set_field(&id, "file/a/data", None, T0 + 1).unwrap();
    v.set_field(&id, "file/a/name", None, T0 + 2).unwrap();
    let item = v.item(&id).unwrap();
    assert!(!item.fields.keys().any(|k| k.starts_with("file/")));
}

#[test]
fn a_document_syncs_to_another_device() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let file = random_bytes(5_000);
    let id = a
        .add_item(
            ItemType::Document,
            "work",
            &[
                ("title", "Synced"),
                ("file/a/name", "key.p8"),
                ("file/a/data", &encode_file(&file).unwrap()),
            ],
            T0 + 10,
        )
        .unwrap();
    let op = a.commit().unwrap().unwrap();
    let (mut joined, _) = join(
        &mut a,
        &c.vault_json,
        std::slice::from_ref(&c.device_record),
        &sk,
        "Phone",
        T0 + 100,
    );
    let b = &mut joined.vault;
    b.apply_file(&op.bytes, a.device_id(), 1, T0 + 200).unwrap();
    let item = b.item(&id).unwrap();
    assert_eq!(item.item_type, ItemType::Document);
    assert_eq!(
        &**decode_file(&item.fields["file/a/data"]).unwrap(),
        &file[..]
    );
}

#[test]
fn every_field_has_its_own_cap_and_nothing_exceeds_the_ceiling() {
    let samples = [
        "title",
        "username",
        "password",
        "totp",
        "token",
        "provider",
        "env_var",
        "expires_at",
        "scopes",
        "notes",
        "body",
        "private_key",
        "public_key",
        "passphrase",
        "url/a",
        "allowed_hosts/a",
        "tags/x",
        "env/NAME",
        "custom/a/name",
        "custom/a/value",
        "file/a/name",
        "file/a/mime",
        "file/a/data",
        "_type",
        "_vault",
    ];
    for f in samples {
        let cap = max_value_bytes(f);
        assert!(cap > 0 && cap <= MAX_VALUE_BYTES, "{f}: {cap}");
    }
    // Free text is the biggest ordinary field; identity-like fields are small.
    assert!(max_value_bytes("title") <= 1024);
    assert!(max_value_bytes("password") <= 4096);
    assert!(max_value_bytes("file/a/data") == MAX_VALUE_BYTES);
}

#[test]
fn a_value_at_its_cap_is_stored_and_one_byte_more_is_refused() {
    let cases: [(ItemType, &str); 9] = [
        (ItemType::Login, "username"),
        (ItemType::Login, "password"),
        (ItemType::Login, "url/a"),
        (ItemType::Token, "token"),
        (ItemType::Note, "body"),
        (ItemType::SshKey, "private_key"),
        (ItemType::EnvBundle, "env/NAME"),
        (ItemType::Document, "file/a/mime"),
        (ItemType::Login, "custom/a/value"),
    ];
    for (t, field) in cases {
        let mut v = vault();
        let cap = max_value_bytes(field);
        let at = "x".repeat(cap);
        let id = v
            .add_item(t, "work", &[("title", "T"), (field, &at)], T0)
            .unwrap();
        assert_eq!(v.item(&id).unwrap().fields[field].len(), cap, "{field}");
        let over = "x".repeat(cap + 1);
        assert!(
            v.set_field(&id, field, Some(&over), T0 + 1).is_err(),
            "{field} +1"
        );
        assert!(
            v.add_item(t, "work", &[("title", "T"), (field, &over)], T0)
                .is_err(),
            "{field} add"
        );
    }
    // The title is capped too: an imported or typed title cannot be a megabyte.
    let mut v = vault();
    let long = "t".repeat(max_value_bytes("title") + 1);
    assert!(
        v.add_item(ItemType::Note, "work", &[("title", &long)], T0)
            .is_err()
    );
}

#[test]
fn an_item_cannot_hold_more_than_the_field_and_file_limits() {
    let mut v = vault();
    let id = v
        .add_item(ItemType::Document, "work", &[("title", "Many")], T0)
        .unwrap();
    for i in 0..MAX_FILES_PER_ITEM {
        v.set_field(
            &id,
            &format!("file/f{i}/name"),
            Some("a.bin"),
            T0 + 1 + i as u64,
        )
        .unwrap();
    }
    assert!(
        v.set_field(&id, "file/zz/name", Some("one too many"), T0 + 100)
            .is_err(),
        "ninth file"
    );
    // Replacing a field of an existing file is fine, and so is removing one and adding another.
    v.set_field(&id, "file/f0/name", Some("renamed.bin"), T0 + 101)
        .unwrap();
    v.set_field(&id, "file/f1/name", None, T0 + 102).unwrap();
    v.set_field(&id, "file/zz/name", Some("fits now"), T0 + 103)
        .unwrap();

    let mut v = vault();
    let id = v
        .add_item(ItemType::Login, "work", &[("title", "Tags")], T0)
        .unwrap();
    for i in 0..(MAX_FIELDS_PER_ITEM - 1) {
        v.set_field(&id, &format!("tags/t{i}"), Some(""), T0 + 1 + i as u64)
            .unwrap();
    }
    assert!(
        v.set_field(&id, "tags/over", Some(""), T0 + 1000).is_err(),
        "field limit"
    );
}
