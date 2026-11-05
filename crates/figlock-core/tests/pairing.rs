//! Invite flow tests (FIG-33, `pairing-recovery.md` 3): the happy path across two devices, and the
//! negative cases from `AGENTS.md` (wrong code, swapped key, expired, reused, tampered, unknown device,
//! malformed inbox files).

mod common;

use common::{T0, add_login, join, new_vault, pw, sk_copy, titles};
use figlock_core::{
    DeviceKeys, INBOX_MAX_BYTES, INBOX_STALE_MS, INVITE_TTL_MS, InboxKind, Invite, InviteOffer,
    JoinAttempt, JoinRequestFile, Key, SecretKey, Vault, device_fingerprint, inbox_is_stale,
    parse_inbox_path, x25519_public,
};

fn flip(bytes: &[u8], at: usize) -> Vec<u8> {
    let mut b = bytes.to_vec();
    b[at] ^= 1;
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A new device's attempt on an open invite.
fn attempt(offer: &InviteOffer, name: &str) -> (JoinAttempt, JoinRequestFile) {
    JoinAttempt::begin(
        &offer.code,
        &offer.inv,
        DeviceKeys::generate().unwrap(),
        name,
        T0 + 1,
    )
    .unwrap()
}

/// One full approval on a fresh invite: the new device's attempt, and the response and record the trusted
/// device stored. `finish` consumes an attempt, so the negative cases each start from here.
fn approved(a: &mut Vault, sk: &SecretKey) -> (JoinAttempt, Vec<u8>, Vec<u8>) {
    let (mut invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let (attempt, req) = attempt(&offer, "Phone");
    let opened = invite.open_request(&req.bytes, T0 + 2).unwrap();
    let ap = a.approve_join(&invite, &opened, sk, T0 + 3).unwrap();
    (attempt, ap.response.bytes, ap.record.bytes)
}

#[test]
fn new_device_joins_and_reads_the_vault() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    add_login(&mut a, "Cloudflare", T0 + 10);
    let file = a.commit().unwrap().unwrap();

    let (mut joined, _) = join(
        &mut a,
        &c.vault_json,
        std::slice::from_ref(&c.device_record),
        &sk,
        "Phone",
        T0 + 100,
    );
    // The Secret Key arrived through the response and matches the one on the trusted device.
    assert_eq!(&*joined.secret_key.format(), &*sk.format());
    let b = &mut joined.vault;
    b.apply_file(&file.bytes, a.device_id(), 1, T0 + 200)
        .unwrap();
    assert_eq!(titles(b), ["Cloudflare"]);

    // Both sides know both devices, and the new one can write files the first accepts.
    assert_eq!(a.devices().len(), 2);
    assert_eq!(b.devices().len(), 2);
    add_login(b, "From phone", T0 + 300);
    let theirs = b.commit().unwrap().unwrap();
    a.apply_file(&theirs.bytes, b.device_id(), 1, T0 + 400)
        .unwrap();
    assert_eq!(titles(&mut a), ["Cloudflare", "From phone"]);
}

#[test]
fn both_screens_show_the_same_fingerprint() {
    let (a, _) = new_vault();
    let (mut invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let device = DeviceKeys::generate().unwrap();
    let shown_on_new_device = device_fingerprint(&device.sign_public());
    let (_, req) = JoinAttempt::begin(&offer.code, &offer.inv, device, "Phone", T0 + 1).unwrap();
    let opened = invite.open_request(&req.bytes, T0 + 2).unwrap();
    assert_eq!(opened.fingerprint(), shown_on_new_device);
    assert_eq!(opened.device_name, "Phone");
    let groups: Vec<&str> = shown_on_new_device.split('-').collect();
    assert!(groups.len() == 4 && groups.iter().all(|g| g.len() == 4));
}

#[test]
fn the_invite_file_is_public_and_the_code_is_not_in_it() {
    let (a, _) = new_vault();
    let (invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    assert_eq!(offer.inv_path, invite.inv_path());
    assert_eq!(invite.expires_at(), T0 + INVITE_TTL_MS);
    let text = String::from_utf8(offer.inv.clone()).unwrap().to_lowercase();
    assert!(
        !text.contains(&offer.code.replace('-', "").to_lowercase()),
        "the code must not appear in the .inv"
    );
    let (kind, id) = parse_inbox_path(&offer.inv_path).unwrap();
    assert_eq!((kind, &id), (InboxKind::Invite, invite.invite_id()));
}

#[test]
fn a_wrong_code_cannot_join() {
    let (a, _) = new_vault();
    let (_, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let (_, other) = Invite::create(a.vault_id(), T0).unwrap();
    let begin = |code: &str| {
        JoinAttempt::begin(
            code,
            &offer.inv,
            DeviceKeys::generate().unwrap(),
            "Phone",
            T0 + 1,
        )
        .is_ok()
    };
    assert!(begin(&offer.code));
    assert!(!begin(&other.code), "code of another invite");
    assert!(!begin("0000-0000-0000-0000-0000"));
    assert!(!begin("not a code"));
    assert!(!begin(""));
}

#[test]
fn a_request_sealed_for_the_wrong_invite_cannot_be_opened() {
    let (a, _) = new_vault();
    let (mut invite, _) = Invite::create(a.vault_id(), T0).unwrap();
    let (_, other) = Invite::create(a.vault_id(), T0).unwrap();
    let (_, req) = attempt(&other, "Phone");
    assert!(invite.open_request(&req.bytes, T0 + 2).is_err());
    assert!(!invite.is_consumed());
}

#[test]
fn a_swapped_invite_key_gets_a_stranger_nothing() {
    // The attacker rewrites the .inv with their own Et_pub. The new device seals to it, but the attacker
    // does not know the code, so they cannot derive the request key or the response key.
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let (mut invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let mut inv: serde_json::Value = serde_json::from_slice(&offer.inv).unwrap();
    inv["et_pub"] = hex(&x25519_public(&Key::random().unwrap())).into();
    let (victim, req) = JoinAttempt::begin(
        &offer.code,
        &serde_json::to_vec(&inv).unwrap(),
        DeviceKeys::generate().unwrap(),
        "Phone",
        T0 + 1,
    )
    .unwrap();
    // The real trusted device cannot open the request, so it never approves or sends the Secret Key.
    assert!(invite.open_request(&req.bytes, T0 + 2).is_err());
    // A genuine response that was meant for someone else does not open for the victim either.
    let (_, resp, rec) = approved(&mut a, &sk);
    let records = [c.device_record.clone(), rec];
    assert!(
        victim
            .finish(&resp, &c.vault_json, &records, pw(), T0 + 4)
            .is_err()
    );
}

#[test]
fn an_expired_invite_is_refused_at_every_step() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let (mut invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let expiry = T0 + INVITE_TTL_MS;
    let begin = |now| {
        JoinAttempt::begin(
            &offer.code,
            &offer.inv,
            DeviceKeys::generate().unwrap(),
            "Phone",
            now,
        )
    };
    assert!(begin(expiry).is_err(), "new device refuses an expired .inv");
    let (_, req) = begin(expiry - 1).unwrap();
    assert!(invite.is_expired(expiry));
    assert!(invite.open_request(&req.bytes, expiry).is_err());
    // Opened in time, approved too late.
    let opened = invite.open_request(&req.bytes, expiry - 1).unwrap();
    assert!(a.approve_join(&invite, &opened, &sk, expiry).is_err());
    assert!(a.approve_join(&invite, &opened, &sk, expiry - 1).is_ok());
}

#[test]
fn an_invite_is_single_use_but_garbage_does_not_burn_it() {
    let (a, _) = new_vault();
    let (mut invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    // Anyone with write access to inbox/ can drop junk; none of it uses up the invite.
    assert!(invite.open_request(&[7u8; 200], T0 + 2).is_err());
    assert!(invite.open_request(&[7u8; 10], T0 + 2).is_err());
    assert!(invite.open_request(&[], T0 + 2).is_err());
    assert!(!invite.is_consumed());

    let (_, first) = attempt(&offer, "First");
    let (_, second) = attempt(&offer, "Second");
    assert!(invite.open_request(&first.bytes, T0 + 3).is_ok());
    assert!(invite.is_consumed());
    assert!(invite.open_request(&second.bytes, T0 + 4).is_err());
    assert!(
        invite.open_request(&first.bytes, T0 + 4).is_err(),
        "no replay"
    );
}

#[test]
fn a_tampered_request_is_refused() {
    let (a, _) = new_vault();
    let (mut invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let (_, req) = attempt(&offer, "Phone");
    for at in [0, 31, 32, 40, req.bytes.len() - 1] {
        assert!(
            invite.open_request(&flip(&req.bytes, at), T0 + 2).is_err(),
            "flipped byte {at} of the request"
        );
    }
    assert!(!invite.is_consumed());
    assert!(invite.open_request(&req.bytes, T0 + 2).is_ok());
}

#[test]
fn a_tampered_or_foreign_response_is_refused() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    // Each flipped byte is tried on its own approval, because `finish` consumes the attempt.
    for at in [0, 5, 40] {
        let (victim, resp, rec) = approved(&mut a, &sk);
        let records = [c.device_record.clone(), rec];
        assert!(
            victim
                .finish(&flip(&resp, at), &c.vault_json, &records, pw(), T0 + 4)
                .is_err(),
            "flipped byte {at}"
        );
    }
    // A response meant for another device (other keys, other device id) is refused; the right one works.
    let (mine, resp, rec) = approved(&mut a, &sk);
    let (stranger, _, _) = approved(&mut a, &sk);
    let records = [c.device_record.clone(), rec];
    assert!(
        stranger
            .finish(&resp, &c.vault_json, &records, pw(), T0 + 4)
            .is_err()
    );
    assert!(
        mine.finish(&resp, &c.vault_json, &records, pw(), T0 + 4)
            .is_ok()
    );
}

#[test]
fn the_device_record_must_verify_and_exist() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let finish = |attempt: JoinAttempt, resp: &[u8], records: &[Vec<u8>], password: &str| {
        attempt
            .finish(resp, &c.vault_json, records, password, T0 + 4)
            .is_ok()
    };
    let (other_vault, other) = new_vault();

    let (att, resp, rec) = approved(&mut a, &sk);
    assert!(finish(att, &resp, &[c.device_record.clone(), rec], pw()));

    let (att, resp, rec) = approved(&mut a, &sk);
    let tampered = String::from_utf8(rec.clone())
        .unwrap()
        .replace("Phone", "Evil");
    assert!(!finish(
        att,
        &resp,
        &[c.device_record.clone(), tampered.into_bytes()],
        pw()
    ));

    let (att, resp, rec) = approved(&mut a, &sk);
    assert!(!finish(
        att,
        &resp,
        &[c.device_record.clone(), rec],
        "not the password"
    ));

    // No record for this device: the vault would have to make a fresh one, which a join never accepts.
    let (att, resp, _) = approved(&mut a, &sk);
    assert!(!finish(
        att,
        &resp,
        std::slice::from_ref(&c.device_record),
        pw()
    ));
    let (att, resp, _) = approved(&mut a, &sk);
    assert!(!finish(att, &resp, &[], pw()));

    // A record signed by another vault's authority is not a device here.
    let (att, resp, _) = approved(&mut a, &sk);
    assert!(!finish(
        att,
        &resp,
        &[c.device_record.clone(), other.device_record.clone()],
        pw()
    ));
    drop(other_vault);
}

#[test]
fn a_device_nobody_approved_is_unknown() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    add_login(&mut a, "Cloudflare", T0 + 10);
    let file = a.commit().unwrap().unwrap();

    // A second trusted device that was not told about the new one.
    let (mut a2, _) = Vault::unlock(
        &c.vault_json,
        pw(),
        &sk,
        DeviceKeys::generate().unwrap(),
        "Tablet",
        std::slice::from_ref(&c.device_record),
        T0 + 20,
    )
    .unwrap();
    a2.apply_file(&file.bytes, a.device_id(), 1, T0 + 30)
        .unwrap();

    let (mut joined, rec) = join(
        &mut a,
        &c.vault_json,
        std::slice::from_ref(&c.device_record),
        &sk,
        "Phone",
        T0 + 100,
    );
    add_login(&mut joined.vault, "From phone", T0 + 200);
    let theirs = joined.vault.commit().unwrap().unwrap();
    assert!(
        a2.apply_file(&theirs.bytes, joined.vault.device_id(), 1, T0 + 300)
            .is_err(),
        "unknown device"
    );
    // A tampered record does not make it known; the genuine one does.
    let mut bad = rec.bytes.clone();
    let at = bad.windows(5).position(|w| w == b"Phone").unwrap();
    bad[at] = b'X';
    assert!(a2.add_device_record(&bad).is_err());
    a2.add_device_record(&rec.bytes).unwrap();
    a2.apply_file(&theirs.bytes, joined.vault.device_id(), 1, T0 + 300)
        .unwrap();
}

#[test]
fn a_device_id_already_in_use_is_refused() {
    let (mut a, c) = new_vault();
    let sk = sk_copy(&c.secret_key);
    let clash = DeviceKeys {
        device_id: *a.device_id(),
        sign_seed: Key::random().unwrap(),
        agree_priv: Key::random().unwrap(),
    };
    let (mut invite, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let (_, req) = JoinAttempt::begin(&offer.code, &offer.inv, clash, "Clone", T0 + 1).unwrap();
    let opened = invite.open_request(&req.bytes, T0 + 2).unwrap();
    assert!(a.approve_join(&invite, &opened, &sk, T0 + 3).is_err());
}

#[test]
fn malformed_inbox_files_are_refused_and_classified() {
    let (a, _) = new_vault();
    let (_, offer) = Invite::create(a.vault_id(), T0).unwrap();
    let begin = |inv: &[u8]| {
        JoinAttempt::begin(
            &offer.code,
            inv,
            DeviceKeys::generate().unwrap(),
            "Phone",
            T0 + 1,
        )
        .is_ok()
    };
    assert!(begin(&offer.inv));
    let good: serde_json::Value = serde_json::from_slice(&offer.inv).unwrap();
    let with = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v = good.clone();
        f(&mut v);
        serde_json::to_vec(&v).unwrap()
    };
    assert!(!begin(b""));
    assert!(!begin(b"not json"));
    assert!(!begin(&with(&|v| v["v"] = 2.into())));
    assert!(!begin(&with(&|v| v["extra"] = 1.into())));
    assert!(!begin(&with(&|v| v["vault_id"] = "zz".into())));
    assert!(!begin(&with(&|v| v["expires_at"] = "soon".into())));
    assert!(!begin(&with(&|v| {
        v.as_object_mut().unwrap().remove("et_pub");
    })));
    // Someone else's invite id for this code.
    assert!(!begin(&with(&|v| v["invite_id"] = hex(&[9u8; 16]).into())));
    assert!(!begin(&vec![b' '; INBOX_MAX_BYTES + 1]));

    let (mut invite, _) = Invite::create(a.vault_id(), T0).unwrap();
    assert!(
        invite
            .open_request(&vec![0u8; INBOX_MAX_BYTES + 1], T0 + 1)
            .is_err()
    );
    let id = hex(&[1u8; 16]);
    assert_eq!(
        parse_inbox_path(&format!("inbox/{id}.req")),
        Some((InboxKind::Request, [1u8; 16]))
    );
    assert_eq!(
        parse_inbox_path(&format!("inbox/{id}.resp")),
        Some((InboxKind::Response, [1u8; 16]))
    );
    for bad in [
        "inbox/zz.req".to_owned(),
        format!("inbox/{id}.share"),
        format!("log/{id}.req"),
        "inbox/".to_owned(),
    ] {
        assert_eq!(parse_inbox_path(&bad), None, "{bad}");
    }
    assert!(!inbox_is_stale(INBOX_STALE_MS));
    assert!(inbox_is_stale(INBOX_STALE_MS + 1));
}
