//! Pairing from the new device's side through the exported API, with the trusted device played by
//! `figlock-core`, plus the device list and the helpers the join screens use. Values are generated.

use std::sync::Arc;

use figlock_core::{Invite, KdfParams, Vault};
use figlock_ffi::{
    DeviceKeysHandle, FigLockError, fingerprint_of, invite_code_canonical, invite_code_is_valid,
    invite_path, start_join,
};

const T0: u64 = 1_800_000_000_000;
const PASSWORD: &str = "generated-join-password-1";

struct World {
    vault: Vault,
    created: figlock_core::Created,
    invite: Invite,
    code: String,
    inv: Vec<u8>,
    inv_path: String,
}

fn world() -> World {
    let (vault, created) = Vault::create(PASSWORD, "first", KdfParams::FLOOR, T0).unwrap();
    let (invite, offer) = Invite::create(vault.vault_id(), T0).unwrap();
    World {
        vault,
        created,
        invite,
        code: offer.code.to_string(),
        inv: offer.inv.clone(),
        inv_path: offer.inv_path.clone(),
    }
}

fn new_device() -> Arc<DeviceKeysHandle> {
    Arc::new(DeviceKeysHandle::generate().unwrap())
}

#[test]
fn a_new_device_joins_through_the_exported_api() {
    let mut w = world();
    assert_eq!(
        invite_path(w.code.clone(), w.created.vault_json.clone()).unwrap(),
        w.inv_path
    );

    let device = new_device();
    let start = start_join(
        w.code.clone(),
        w.inv.clone(),
        device.clone(),
        "phone".into(),
        T0 + 1,
    )
    .unwrap();
    assert_eq!(start.request.path, w.invite.req_path());
    assert_eq!(start.handle.response_path(), w.invite.resp_path());
    assert_eq!(
        start.handle.record_path(),
        format!("devices/{}.json", device.device_id())
    );
    // Both screens show the same fingerprint.
    assert_eq!(start.handle.fingerprint(), device.fingerprint());
    let req = w.invite.open_request(&start.request.bytes, T0 + 2).unwrap();
    assert_eq!(req.fingerprint(), start.handle.fingerprint());

    let approval = w
        .vault
        .approve_join(&w.invite, &req, &w.created.secret_key, T0 + 3)
        .unwrap();
    let records = vec![
        w.created.device_record.clone(),
        approval.record.bytes.clone(),
    ];

    let out = start
        .handle
        .finish(
            approval.response.bytes.clone(),
            w.created.vault_json.clone(),
            records.clone(),
            PASSWORD.into(),
            T0 + 4,
        )
        .unwrap();
    assert_eq!(
        out.secret_key_text,
        w.created.secret_key.format().to_string()
    );
    assert_eq!(out.session.device_id().unwrap(), device.device_id());
    assert_eq!(out.session.vault_id().unwrap().len(), 32);

    // The device list names both devices and marks this one.
    let list = out.session.devices().unwrap();
    assert_eq!(list.len(), 2);
    let me = list.iter().find(|d| d.is_this_device).unwrap();
    assert_eq!(me.name.as_deref(), Some("phone"));
    assert_eq!(me.fingerprint, device.fingerprint());
    assert!(list.iter().all(|d| d.revoked_ms.is_none()));

    // Negative cases: wrong password, tampered answer, answer for another device, missing own record.
    let again = |resp: Vec<u8>, recs: Vec<Vec<u8>>, pw: &str| {
        start
            .handle
            .finish(resp, w.created.vault_json.clone(), recs, pw.into(), T0 + 5)
    };
    assert_eq!(
        again(
            approval.response.bytes.clone(),
            records.clone(),
            "wrong-password"
        )
        .err(),
        Some(FigLockError::Failed)
    );
    let mut tampered = approval.response.bytes.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert_eq!(
        again(tampered, records.clone(), PASSWORD).err(),
        Some(FigLockError::Failed)
    );
    assert_eq!(
        again(
            approval.response.bytes.clone(),
            vec![w.created.device_record.clone()],
            PASSWORD
        )
        .err(),
        Some(FigLockError::Failed)
    );
}

#[test]
fn joining_refuses_a_wrong_code_a_bad_invite_and_an_expired_invite() {
    let w = world();
    let other = world();
    let start = |code: &str, inv: &[u8], now: u64| {
        start_join(code.into(), inv.to_vec(), new_device(), "phone".into(), now).err()
    };
    assert_eq!(
        start(&other.code, &w.inv, T0 + 1),
        Some(FigLockError::Failed)
    );
    assert_eq!(
        start(&w.code, b"not json", T0 + 1),
        Some(FigLockError::Failed)
    );
    assert_eq!(
        start(&w.code, &w.inv, T0 + 60 * 60 * 1000),
        Some(FigLockError::Failed)
    );
    assert_eq!(
        start("too-short", &w.inv, T0 + 1),
        Some(FigLockError::Failed)
    );
    assert_eq!(
        start_join(
            w.code.clone(),
            w.inv.clone(),
            new_device(),
            "".into(),
            T0 + 1
        )
        .err(),
        Some(FigLockError::Failed)
    );
    assert_eq!(
        invite_path("nope".into(), w.created.vault_json.clone()).err(),
        Some(FigLockError::InvalidInput)
    );
    assert_eq!(
        invite_path(w.code.clone(), b"{}".to_vec()).err(),
        Some(FigLockError::InvalidInput)
    );
}

#[test]
fn invite_code_helpers() {
    let w = world();
    assert!(invite_code_is_valid(w.code.clone()));
    assert!(invite_code_is_valid(
        w.code.to_lowercase().replace('-', " ")
    ));
    assert_eq!(
        invite_code_canonical(w.code.to_lowercase().replace('-', "")).unwrap(),
        w.code
    );
    assert!(!invite_code_is_valid("ABCD-EFGH".into()));
    assert_eq!(
        invite_code_canonical("ABCD".into()).err(),
        Some(FigLockError::InvalidInput)
    );
    assert_eq!(fingerprint_of("00".repeat(32)).unwrap().len(), 19);
    assert_eq!(
        fingerprint_of("zz".repeat(32)).err(),
        Some(FigLockError::InvalidInput)
    );
    assert_eq!(
        fingerprint_of("00".into()).err(),
        Some(FigLockError::InvalidInput)
    );
}
