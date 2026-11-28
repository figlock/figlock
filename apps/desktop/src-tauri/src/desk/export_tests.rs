//! Exporting the vault (FIG-287), against a real folder.

use std::collections::BTreeMap;

use serde_json::Value;

use super::device_tests::unlocked;
use super::items::ItemDraft;
use super::tests::{code_of, desk, scenario_in};
use super::*;

fn scenario(name: &str) {
    scenario_in("export_tests", name);
}

fn login(title: &str, password: &str) -> ItemDraft {
    ItemDraft {
        fields: BTreeMap::from([
            ("title".to_owned(), title.to_owned()),
            ("username".to_owned(), "me".to_owned()),
            ("password".to_owned(), password.to_owned()),
        ]),
        clear: Vec::new(),
        add_files: Vec::new(),
        remove_files: Vec::new(),
    }
}

#[test]
fn the_export_leaves_secrets_out_unless_the_password_is_typed_again() {
    scenario("export_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn export_inner() {
    let d = desk();
    // Locked: nothing is made.
    assert_eq!(
        code_of(d.export_vault(false, false, None, None).unwrap_err()),
        "locked"
    );
    let (pw, key) = unlocked(&d);
    let kept_secret = format!("kept-{}", SecretKey::random().unwrap().format().as_str());
    let gone_secret = format!("gone-{}", SecretKey::random().unwrap().format().as_str());
    d.add_item("login", login("GitHub", &kept_secret)).unwrap();
    let gone = d.add_item("login", login("Old", &gone_secret)).unwrap();
    d.delete_item(&gone).unwrap();

    // Without secrets: no proof, no secret value, and the secret fields are only named.
    let text = d.export_vault(false, false, None, None).unwrap();
    assert!(!text.contains(&kept_secret) && !text.contains(&gone_secret));
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["format"], "figlock-export-1");
    assert_eq!(doc["secrets_included"], false);
    assert_eq!(doc["items"].as_array().unwrap().len(), 1);
    assert_eq!(doc["items"][0]["secret_fields"][0], "password");

    // The recently deleted item comes only when asked for.
    let with_deleted: Value =
        serde_json::from_str(&d.export_vault(false, true, None, None).unwrap()).unwrap();
    assert_eq!(with_deleted["items"].as_array().unwrap().len(), 2);

    // With secrets and no proof: refused, and nothing is made.
    assert_eq!(
        code_of(d.export_vault(true, false, None, None).unwrap_err()),
        "needs_password"
    );
    assert_eq!(
        code_of(d.export_vault(true, false, Some(""), None).unwrap_err()),
        "needs_password"
    );
    // A wrong password, or a wrong key, is refused in the same words as everywhere else.
    assert_eq!(
        code_of(
            d.export_vault(true, false, Some(&format!("{pw}-not")), Some(&key))
                .unwrap_err()
        ),
        "wrong_credentials"
    );
    let other = SecretKey::random().unwrap().format().to_string();
    assert_eq!(
        code_of(
            d.export_vault(true, false, Some(&pw), Some(&other))
                .unwrap_err()
        ),
        "wrong_credentials"
    );

    // The right pair gives the secrets, the same document the CLI writes.
    let full: Value =
        serde_json::from_str(&d.export_vault(true, false, Some(&pw), Some(&key)).unwrap()).unwrap();
    assert_eq!(full["secrets_included"], true);
    assert_eq!(full["items"][0]["fields"]["password"], kept_secret.as_str());
}
