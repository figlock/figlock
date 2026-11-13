//! Adding a device from this one, against a real folder. The new device is played by the test with the core's
//! `JoinAttempt`, writing into the same folder, so these cover the whole invite, request, approval and answer
//! path. Each scenario runs as a child with its own device config (see `tests.rs`).

use figlock_core::{DeviceKeys, INVITE_TTL_MS, JoinAttempt};
use figlock_store::{BlobStore, LocalFolder};

use super::pairing::advance_clock_for_test;
use super::tests::{code_of, created_key, desk, password, scenario_in, vault_dir};
use super::*;

fn scenario(name: &str) {
    scenario_in("pairing_tests", name);
}

fn unlocked(d: &Desk) -> (String, String) {
    let pw = password();
    let key = created_key(d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    d.recovery_skip(true).unwrap();
    assert_eq!(d.finish_create(false).unwrap().phase, "unlocked");
    (pw, key)
}

fn folder_store() -> LocalFolder {
    LocalFolder::open(std::path::Path::new(&vault_dir())).unwrap()
}

/// The new device's request for `code`, written where the trusted device looks. Returns what it needs later.
fn request(code: &str, name: &str) -> JoinAttempt {
    let store = folder_store();
    let vault_json = store.get("vault.json").unwrap();
    let inv_path = figlock_core::invite_path(code, &vault_json).unwrap();
    let inv = store.get(&inv_path).unwrap();
    let (attempt, req) =
        JoinAttempt::begin(code, &inv, DeviceKeys::generate().unwrap(), name, now_ms()).unwrap();
    store.put_new(&req.path, &req.bytes).unwrap();
    attempt
}

fn inbox() -> Vec<String> {
    folder_store()
        .list("inbox/")
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect()
}

#[test]
fn a_device_is_invited_approved_and_joins() {
    scenario("join_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn join_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    assert_eq!(d.add_device_status().unwrap().state, "none");

    let invite = d.add_device_start().unwrap();
    assert_eq!(invite.code.len(), 24);
    assert!(invite.qr.len() >= 21 && invite.qr.iter().all(|r| r.len() == invite.qr.len()));
    assert_eq!(inbox().len(), 1, "the invite is in the inbox");
    let waiting = d.add_device_status().unwrap();
    assert_eq!(waiting.state, "waiting");
    assert_eq!(waiting.expires_at_ms, Some(invite.expires_at_ms));

    // The new device answers. The trusted one sees its name and the fingerprint the new one shows.
    let attempt = request(&invite.code, "Work MacBook");
    let status = d.add_device_status().unwrap();
    assert_eq!(status.state, "request");
    assert_eq!(status.device_name.as_deref(), Some("Work MacBook"));
    assert_eq!(status.fingerprint.as_ref().map(String::len), Some(19));
    // The invite file is gone as soon as a valid request opened; only the request remains.
    assert_eq!(inbox().len(), 1);
    assert!(inbox()[0].ends_with(".req"));
    // Asking again does not change the answer.
    assert_eq!(d.add_device_status().unwrap(), status);
    let before = d.devices().unwrap().len();

    // Approval needs the credentials on a device that keeps no key and whose session holds none (ADR-0030;
    // the create that unlocked it held one), and a wrong pair changes nothing.
    d.inner().held_key = None;
    assert_eq!(
        code_of(d.add_device_approve(None, None).unwrap_err()),
        "needs_secret_key"
    );
    let other = password();
    assert_eq!(
        code_of(d.add_device_approve(Some(&other), Some(&key)).unwrap_err()),
        "wrong_credentials"
    );
    assert_eq!(d.devices().unwrap().len(), before);
    assert_eq!(d.add_device_status().unwrap().state, "request");

    let rows = d.add_device_approve(Some(&pw), Some(&key)).unwrap();
    assert_eq!(rows.len(), before + 1);
    assert!(
        rows.iter()
            .any(|r| r.name.as_deref() == Some("Work MacBook") && !r.revoked)
    );
    assert_eq!(d.add_device_status().unwrap().state, "none");

    // The new device reads the answer and unlocks with the master password.
    let store = folder_store();
    let resp = store.get(&attempt.resp_path()).unwrap();
    let vault_json = store.get("vault.json").unwrap();
    let records: Vec<Vec<u8>> = store
        .list("devices/")
        .unwrap()
        .iter()
        .map(|e| store.get(&e.path).unwrap())
        .collect();
    let joined = attempt
        .finish(&resp, &vault_json, &records, &pw, now_ms())
        .unwrap();
    assert_eq!(joined.secret_key.format().to_string(), key);
    // The invite and the request are cleared; the answer stays for the new device.
    assert_eq!(inbox(), [attempt.resp_path()]);
}

#[test]
fn an_expired_invite_is_closed_and_its_request_refused() {
    scenario("expiry_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn expiry_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);

    // Never answered: after 10 minutes it is expired and its file is gone.
    d.add_device_start().unwrap();
    advance_clock_for_test(INVITE_TTL_MS + 1);
    assert_eq!(d.add_device_status().unwrap().state, "expired");
    assert!(inbox().is_empty());
    assert_eq!(d.add_device_status().unwrap().state, "none");
    assert_eq!(
        code_of(d.add_device_approve(Some(&pw), Some(&key)).unwrap_err()),
        "no_invite"
    );

    // Answered in time but approved too late: refused, and nothing is added.
    let invite = d.add_device_start().unwrap();
    request(&invite.code, "Late laptop");
    assert_eq!(d.add_device_status().unwrap().state, "request");
    let before = d.devices().unwrap().len();
    advance_clock_for_test(INVITE_TTL_MS + 1);
    assert_eq!(
        code_of(d.add_device_approve(Some(&pw), Some(&key)).unwrap_err()),
        "expired"
    );
    assert_eq!(d.devices().unwrap().len(), before);
    assert!(inbox().is_empty());
}

#[test]
fn an_invite_works_once_and_a_stranger_cannot_burn_it() {
    scenario("single_use_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn single_use_inner() {
    let d = desk();
    let (pw, key) = unlocked(&d);
    let invite = d.add_device_start().unwrap();

    // Garbage in the request slot is deleted and the invite stays open.
    let store = folder_store();
    let vault_json = store.get("vault.json").unwrap();
    let inv_path = figlock_core::invite_path(&invite.code, &vault_json).unwrap();
    let req_path = inv_path.replace(".inv", ".req");
    store.put_new(&req_path, b"not a request").unwrap();
    assert_eq!(d.add_device_status().unwrap().state, "waiting");
    assert_eq!(inbox(), std::slice::from_ref(&inv_path));

    // The real request opens, and the invite is used up even if the person declines.
    request(&invite.code, "Phone");
    assert_eq!(d.add_device_status().unwrap().state, "request");
    d.add_device_cancel().unwrap();
    assert_eq!(d.add_device_status().unwrap().state, "none");
    assert!(inbox().is_empty());
    assert_eq!(
        code_of(d.add_device_approve(Some(&pw), Some(&key)).unwrap_err()),
        "no_invite"
    );
    assert!(
        d.devices().unwrap().iter().all(|r| r.this_device),
        "a declined device was added"
    );

    // The used code finds no invite on the new device's side any more.
    assert!(figlock_core::invite_path(&invite.code, &vault_json).is_ok());
    assert!(store.get(&inv_path).is_err());
}

#[test]
fn a_new_invite_replaces_the_open_one_and_a_locked_vault_has_none() {
    scenario("replace_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn replace_inner() {
    let d = desk();
    assert_eq!(code_of(d.add_device_start().unwrap_err()), "locked");
    assert_eq!(d.add_device_status().unwrap().state, "none");
    unlocked(&d);
    let first = d.add_device_start().unwrap();
    let second = d.add_device_start().unwrap();
    assert_ne!(first.code, second.code);
    assert_eq!(inbox().len(), 1, "only one invite is open at a time");
    d.add_device_cancel().unwrap();
    assert!(inbox().is_empty());
    // Locking drops the open invite.
    d.add_device_start().unwrap();
    d.lock();
    assert_eq!(d.add_device_status().unwrap().state, "none");
}
