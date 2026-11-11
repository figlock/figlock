//! Items on an unlocked vault, against a real folder. Each scenario runs as a child with its own device
//! config and a clipboard that is a file (see `tests.rs`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::items::ItemDraft;
use super::tests::{code_of, created_key, desk, password, scenario_in};
use super::*;

fn scenario(name: &str) {
    scenario_in("item_tests", name);
}

/// A desk with a new vault, unlocked, and the credentials it was made with.
fn unlocked(d: &Desk) -> (String, String) {
    let pw = password();
    let key = created_key(d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    assert_eq!(d.finish_create(false).unwrap().phase, "unlocked");
    (pw, key)
}

fn draft(fields: &[(&str, &str)]) -> ItemDraft {
    ItemDraft {
        fields: fields
            .iter()
            .map(|(f, v)| ((*f).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        add_files: Vec::new(),
        remove_files: Vec::new(),
    }
}

fn test_dir() -> PathBuf {
    PathBuf::from(std::env::var("FIGLOCK_TEST_DIR").unwrap())
}

fn clipboard_now() -> String {
    std::fs::read_to_string(test_dir().join("clipboard")).unwrap()
}

fn titles(d: &Desk, query: &str) -> Vec<String> {
    d.list_items(query)
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect()
}

#[test]
fn a_login_is_added_found_read_edited_deleted_and_restored() {
    scenario("login_lifecycle_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn login_lifecycle_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    let secret = password();
    let id = d
        .add_item(
            "login",
            draft(&[
                ("title", "Cloudflare"),
                ("username", "ops@example.test"),
                ("password", &secret),
                ("url/a1", "dash.cloudflare.com"),
                ("tags/dns", ""),
                ("notes", "Owner account"),
            ]),
        )
        .unwrap();

    let list = d.list_items("").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].kind, "login");
    assert_eq!(list[0].subtitle, "ops@example.test");

    // Search finds titles, hosts and typos, and never a secret value.
    assert_eq!(titles(&d, "cloudflare"), ["Cloudflare"]);
    assert_eq!(titles(&d, "dash.cloudflare.com"), ["Cloudflare"]);
    assert_eq!(titles(&d, "cloudflair"), ["Cloudflare"]);
    assert!(titles(&d, "zzzzqq").is_empty());
    assert!(titles(&d, &secret).is_empty());
    assert!(titles(&d, &secret[..12]).is_empty());

    // The page never carries the secret; a reveal does, on request.
    let detail = d.item_detail(&id).unwrap();
    assert_eq!(detail.kind, "login");
    assert_eq!(detail.secrets, ["password"]);
    assert!(!detail.fields.contains_key("password"));
    let json = serde_json::to_string(&detail).unwrap();
    assert!(!json.contains(&secret), "the item page leaked the password");
    assert_eq!(d.reveal_field(&id, "password").unwrap(), secret);
    assert_eq!(
        code_of(d.reveal_field(&id, "nope").unwrap_err()),
        "not_found"
    );

    // Edit: a new title and password, and the username removed.
    let edit = d.item_for_edit(&id).unwrap();
    assert_eq!(edit.fields["password"], secret);
    let fresh = password();
    d.update_item(
        &id,
        draft(&[
            ("title", "Cloudflare DNS"),
            ("password", &fresh),
            ("url/a1", "dash.cloudflare.com"),
            ("notes", "Owner account"),
        ]),
    )
    .unwrap();
    assert_eq!(titles(&d, ""), ["Cloudflare DNS"]);
    assert_eq!(d.reveal_field(&id, "password").unwrap(), fresh);
    assert!(!d.item_detail(&id).unwrap().fields.contains_key("username"));

    // A draft the schema refuses changes nothing.
    assert_eq!(
        code_of(
            d.update_item(&id, draft(&[("title", ""), ("password", &fresh)]))
                .unwrap_err()
        ),
        "title_required"
    );
    assert_eq!(
        code_of(
            d.update_item(&id, draft(&[("title", "x"), ("private_key", "y")]))
                .unwrap_err()
        ),
        "failed"
    );
    assert_eq!(titles(&d, ""), ["Cloudflare DNS"]);

    // Delete and restore.
    d.delete_item(&id).unwrap();
    assert!(d.list_items("").unwrap().is_empty());
    assert_eq!(d.list_deleted().unwrap().len(), 1);
    assert!(d.item_detail(&id).unwrap().deleted);
    assert_eq!(code_of(d.item_for_edit(&id).unwrap_err()), "failed");
    d.restore_item(&id).unwrap();
    assert_eq!(titles(&d, ""), ["Cloudflare DNS"]);
    assert!(d.list_deleted().unwrap().is_empty());

    // It was written to the vault: a fresh unlock still has it.
    d.lock();
    assert_eq!(d.unlock(&pw, &key).unwrap().phase, "unlocked");
    assert_eq!(titles(&d, ""), ["Cloudflare DNS"]);
    assert_eq!(d.reveal_field(&id, "password").unwrap(), fresh);
}

#[test]
fn every_item_type_can_be_added_and_is_described_in_the_list() {
    scenario("every_type_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn every_type_inner() {
    let d = desk();
    unlocked(&d);
    let token = password();
    type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a str);
    let cases: &[Case] = &[
        (
            "token",
            &[
                ("title", "GitHub token"),
                ("provider", "GitHub"),
                ("token", &token),
                ("allowed_hosts/h1", "api.github.com"),
                ("env_var", "GITHUB_TOKEN"),
                ("expires_at", "2031-01-01"),
            ],
            "GitHub - api.github.com",
        ),
        (
            "note",
            &[
                ("title", "Office Wi-Fi"),
                ("body", "The password is on the router."),
            ],
            "Secure note",
        ),
        (
            "env",
            &[
                ("title", "Staging"),
                ("env/DATABASE_URL", "postgres://db.example.test/app"),
                ("env/REDIS_URL", "redis://cache.example.test"),
            ],
            "2 variables",
        ),
        (
            "ssh",
            &[
                ("title", "Deploy key"),
                ("private_key", "-----BEGIN EXAMPLE-----"),
                ("public_key", "ssh-ed25519 AAAAEXAMPLE"),
            ],
            "SSH key",
        ),
        (
            "login",
            &[
                ("title", "Linear"),
                ("username", "mira@example.test"),
                ("custom/c1/name", "Account"),
                ("custom/c1/value", &token),
            ],
            "mira@example.test",
        ),
    ];
    for (kind, fields, subtitle) in cases {
        let id = d.add_item(kind, draft(fields)).unwrap();
        let row = d
            .list_items("")
            .unwrap()
            .into_iter()
            .find(|i| i.id == id)
            .unwrap();
        assert_eq!(&row.subtitle, subtitle, "{kind}");
        assert_eq!(row.kind, *kind);
    }
    // Custom field names are shown; their values and every env value are secrets.
    let login = d.list_items("linear").unwrap().into_iter().next().unwrap();
    let detail = d.item_detail(&login.id).unwrap();
    assert_eq!(detail.fields["custom/c1/name"], "Account");
    assert_eq!(detail.secrets, ["custom/c1/value"]);
    let env = d.list_items("staging").unwrap().remove(0);
    assert_eq!(
        d.item_detail(&env.id).unwrap().secrets,
        ["env/DATABASE_URL", "env/REDIS_URL"]
    );
    // Tags and a missing type are refused.
    assert_eq!(
        code_of(d.add_item("nope", draft(&[("title", "x")])).unwrap_err()),
        "failed"
    );
    assert_eq!(
        code_of(
            d.add_item("login", draft(&[("username", "x")]))
                .unwrap_err()
        ),
        "title_required"
    );
    // A field longer than its limit is refused with its own code.
    assert_eq!(
        code_of(
            d.add_item(
                "login",
                draft(&[("title", "x"), ("username", &"u".repeat(2000))])
            )
            .unwrap_err()
        ),
        "field_too_long"
    );
}

#[test]
fn documents_hold_up_to_eight_files_that_are_read_in_the_app_core() {
    scenario("documents_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn documents_inner() {
    let d = desk();
    unlocked(&d);
    let dir = test_dir();
    let write = |name: &str, bytes: &[u8]| {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p.display().to_string()
    };
    let a = write("lease.pdf", b"lease contents");
    let b = write("key.json", b"{\"k\":1}");
    let empty = write("empty.txt", b"");
    let big = write("big.bin", &vec![7u8; figlock_core::MAX_FILE_BYTES + 1]);

    // One bad file does not stop the others, and says why.
    let staged = d
        .stage_files(&[a.clone(), empty, b.clone(), big, "/no/such/file".into()])
        .unwrap();
    assert_eq!(staged.files.len(), 2);
    assert_eq!(staged.refused.len(), 3);
    assert_eq!(staged.refused[0].name, "empty.txt");
    assert!(staged.refused[1].message.contains("768 KiB"));
    assert_eq!(staged.files[0].name, "lease.pdf");
    assert_eq!(staged.files[0].size, 14);

    let tokens: Vec<String> = staged.files.iter().map(|f| f.token.clone()).collect();
    // A document needs a file.
    assert_eq!(
        code_of(d.add_item("doc", draft(&[("title", "Lease")])).unwrap_err()),
        "no_files"
    );
    let mut with_files = draft(&[("title", "Studio lease"), ("password", "")]);
    with_files.add_files = tokens.clone();
    let id = d.add_item("doc", with_files).unwrap();

    let detail = d.item_detail(&id).unwrap();
    assert_eq!(detail.files.len(), 2);
    assert!(detail.secrets.is_empty());
    assert!(detail.fields.keys().all(|f| !f.starts_with("file/")));
    let lease = detail.files.iter().find(|f| f.name == "lease.pdf").unwrap();
    assert_eq!(lease.size, 14);
    assert_eq!(lease.mime, "application/pdf");
    let (name, bytes) = d.file_bytes(&id, &lease.id).unwrap();
    assert_eq!(name, "lease.pdf");
    assert_eq!(&**bytes, b"lease contents");
    assert_eq!(d.list_items("").unwrap()[0].subtitle, "2 files");

    // Saving used the staged files up: they cannot be attached twice.
    let mut again = draft(&[("title", "Again")]);
    again.add_files = tokens;
    assert_eq!(code_of(d.add_item("doc", again).unwrap_err()), "failed");

    // Edit: drop one file and attach another.
    let extra = d.stage_files(&[write("extra.txt", b"more")]).unwrap();
    let mut change = draft(&[("title", "Studio lease")]);
    change.remove_files = vec![lease.id.clone()];
    change.add_files = vec![extra.files[0].token.clone()];
    d.update_item(&id, change).unwrap();
    let names: Vec<String> = d
        .item_detail(&id)
        .unwrap()
        .files
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"extra.txt".to_owned()) && names.contains(&"key.json".to_owned()));
    assert_eq!(
        code_of(d.file_bytes(&id, &lease.id).unwrap_err()),
        "not_found"
    );

    // Removing the last files is refused; nine files are refused.
    let all: Vec<String> = d
        .item_detail(&id)
        .unwrap()
        .files
        .into_iter()
        .map(|f| f.id)
        .collect();
    let mut none = draft(&[("title", "Studio lease")]);
    none.remove_files = all;
    assert_eq!(code_of(d.update_item(&id, none).unwrap_err()), "no_files");
    let nine: Vec<String> = (0..9).map(|i| write(&format!("f{i}.txt"), b"x")).collect();
    let staged = d.stage_files(&nine).unwrap();
    let mut too_many = draft(&[("title", "Nine")]);
    too_many.add_files = staged.files.iter().map(|f| f.token.clone()).collect();
    assert_eq!(
        code_of(d.add_item("doc", too_many).unwrap_err()),
        "too_many_files"
    );
    // Files only go on documents, and a chosen file can be dropped before saving.
    let mut on_login = draft(&[("title", "Login")]);
    on_login.add_files = vec![staged.files[0].token.clone()];
    assert_eq!(
        code_of(d.add_item("login", on_login).unwrap_err()),
        "failed"
    );
    d.unstage_file(&staged.files[1].token);
    d.clear_staged();
}

#[test]
fn a_copy_clears_the_clipboard_when_due_unless_something_else_was_copied() {
    scenario("clipboard_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn clipboard_inner() {
    // The clear delay is zero, so a copy is due at once; the lock scenario covers a delay that is not.
    let d = Desk::with(KdfParams::FLOOR, Duration::from_secs(300), Duration::ZERO);
    unlocked(&d);
    let secret = password();
    let id = d
        .add_item("login", draft(&[("title", "Site"), ("password", &secret)]))
        .unwrap();

    assert_eq!(d.copy_field(&id, "password").unwrap(), 0);
    assert_eq!(clipboard_now(), secret);
    d.clear_clipboard(false);
    assert_eq!(clipboard_now(), "");

    // Something copied after ours is never wiped.
    d.copy_field(&id, "password").unwrap();
    std::fs::write(test_dir().join("clipboard"), "something else").unwrap();
    d.clear_clipboard(false);
    assert_eq!(clipboard_now(), "something else");

    assert_eq!(code_of(d.copy_field(&id, "nope").unwrap_err()), "not_found");
    assert_eq!(
        code_of(d.copy_field("zz", "password").unwrap_err()),
        "not_found"
    );
}

#[test]
fn locking_clears_a_copied_secret_and_closes_every_item_command() {
    scenario("lock_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn lock_inner() {
    let d = desk();
    unlocked(&d);
    let secret = password();
    let id = d
        .add_item("login", draft(&[("title", "Site"), ("password", &secret)]))
        .unwrap();
    assert_eq!(d.copy_field(&id, "password").unwrap(), 30);
    // Not due yet.
    d.clear_clipboard(false);
    assert_eq!(clipboard_now(), secret);

    d.lock();
    assert_eq!(
        clipboard_now(),
        "",
        "locking left a secret on the clipboard"
    );
    for e in [
        d.list_items("").unwrap_err(),
        d.list_deleted().unwrap_err(),
        d.item_detail(&id).unwrap_err(),
        d.reveal_field(&id, "password").unwrap_err(),
        d.copy_field(&id, "password").unwrap_err(),
        d.item_for_edit(&id).unwrap_err(),
        d.delete_item(&id).unwrap_err(),
        d.add_item("login", draft(&[("title", "x")])).unwrap_err(),
        d.stage_files(&[]).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code_of(e), "locked");
    }
}
