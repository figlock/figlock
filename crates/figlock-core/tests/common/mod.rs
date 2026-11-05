//! Shared setup for the pairing and revocation tests: a vault, the invite flow, and a way to read items.
//! Every credential is generated per run; nothing here is a real secret.
#![allow(dead_code)]

use figlock_core::{
    Committed, DeviceKeys, Invite, ItemType, JoinAttempt, Joined, KdfParams, Key, SecretKey, Vault,
};

pub const T0: u64 = 1_700_000_000_000;

pub fn pw() -> &'static str {
    static PW: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PW.get_or_init(|| {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).unwrap();
        b.iter().map(|x| format!("{x:02x}")).collect()
    })
}

pub fn sk_copy(sk: &SecretKey) -> SecretKey {
    SecretKey::parse(&sk.format()).unwrap()
}

pub fn new_vault() -> (Vault, figlock_core::Created) {
    Vault::create(pw(), "Laptop", KdfParams::FLOOR, T0).unwrap()
}

/// Run the whole invite flow between `a` (trusted) and a fresh device called `name`. `records` are the
/// device records `a` already stores; the new device's own record is appended for its unlock.
pub fn join(
    a: &mut Vault,
    vault_json: &[u8],
    records: &[Vec<u8>],
    sk: &SecretKey,
    name: &str,
    now: u64,
) -> (Joined, Committed) {
    let (mut invite, offer) = Invite::create(a.vault_id(), now).unwrap();
    let (attempt, req) = JoinAttempt::begin(
        &offer.code,
        &offer.inv,
        DeviceKeys::generate().unwrap(),
        name,
        now + 1,
    )
    .unwrap();
    assert_eq!(req.path, invite.req_path());
    let opened = invite.open_request(&req.bytes, now + 2).unwrap();
    let approval = a.approve_join(&invite, &opened, sk, now + 3).unwrap();
    assert_eq!(approval.response.path, attempt.resp_path());
    assert_eq!(approval.record.path, attempt.record_path());
    let mut all = records.to_vec();
    all.push(approval.record.bytes.clone());
    let joined = attempt
        .finish(&approval.response.bytes, vault_json, &all, pw(), now + 4)
        .unwrap();
    (joined, approval.record)
}

/// A login item with a title and password.
pub fn add_login(v: &mut Vault, title: &str, now: u64) -> [u8; 16] {
    v.add_item(
        ItemType::Login,
        "personal",
        &[("title", title), ("password", "generated-for-test")],
        now,
    )
    .unwrap()
}

pub fn titles(v: &mut Vault) -> Vec<String> {
    let (items, corrupt) = v.items(false);
    assert!(corrupt.is_empty(), "no unreadable items expected");
    let mut t: Vec<String> = items
        .iter()
        .map(|i| i.fields["title"].to_string())
        .collect();
    t.sort();
    t
}

/// A device's keys again, as the platform keystore hands them back after a restart.
pub fn clone_keys(v: &Vault) -> DeviceKeys {
    let k = v.device_keys();
    DeviceKeys {
        device_id: k.device_id,
        sign_seed: Key::from_bytes(*k.sign_seed.expose()),
        agree_priv: Key::from_bytes(*k.agree_priv.expose()),
    }
}
