//! The password health calls as the iPhone uses them (FIG-292): weak, reused and old from the vault, and the
//! two-step breach check that never lets more than a 5 character prefix out. Values are generated.

use std::sync::Arc;

use figlock_ffi::{
    FieldInput, FigLockError, RangeAnswer, VaultSession, create_vault, minimum_kdf_settings,
};
use sha1::{Digest, Sha1};

const T0: u64 = 1_800_000_000_000;
const YEAR_MS: u64 = 365 * 86_400_000;
const STRONG_A: &str = "tr0ub4dor&3-horse-staple-battery-9x";
const STRONG_B: &str = "kq7-Zm2!vXw9-Lp4#Rt8-Yn5$Hc3";

fn session() -> Arc<VaultSession> {
    create_vault(
        "generated-test-password-1".into(),
        "test phone".into(),
        minimum_kdf_settings(),
        T0 - 2 * YEAR_MS,
    )
    .unwrap()
    .take_session()
    .unwrap()
}

fn login(s: &VaultSession, title: &str, password: &str, at: u64) -> String {
    s.add_item(
        "login".into(),
        "personal".into(),
        vec![
            FieldInput {
                name: "title".into(),
                value: title.into(),
            },
            FieldInput {
                name: "password".into(),
                value: password.into(),
            },
        ],
        at,
    )
    .unwrap()
}

fn sha1_hex(password: &str) -> String {
    Sha1::digest(password.as_bytes())
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}

#[test]
fn report_lists_weak_reused_and_old_without_a_password() {
    let s = session();
    // The vault clock never goes back, so the old item is written first.
    let old = login(&s, "Old", STRONG_B, T0 - YEAR_MS);
    let weak = login(&s, "Weak", "summer2020", T0);
    let a = login(&s, "A", STRONG_A, T0);
    let b = login(&s, "B", STRONG_A, T0);
    let clean = login(&s, "Clean", "n8#Qw-4Lz!Vx-2Rm-7Kp", T0);

    let r = s.health_report(T0 + 10).unwrap();
    assert_eq!(r.checked, 5);
    let by = |id: &str| r.findings.iter().find(|f| f.id == id);
    assert!(by(&weak).unwrap().weak_bits.is_some());
    assert_eq!(by(&a).unwrap().reused_with, 1);
    assert_eq!(by(&b).unwrap().reused_with, 1);
    assert!(by(&old).unwrap().old_days.unwrap() >= 365);
    assert!(by(&clean).is_none());
}

#[test]
fn deleted_items_are_not_checked() {
    let s = session();
    let id = login(&s, "Gone", "summer2020", T0);
    s.delete_item(id, T0 + 1).unwrap();
    let r = s.health_report(T0 + 2).unwrap();
    assert_eq!(r.checked, 0);
    assert!(r.findings.is_empty());
}

#[test]
fn breach_check_asks_for_prefixes_only_and_matches_the_rest_here() {
    let s = session();
    let hit = login(&s, "Hit", "summer2020", T0);
    login(&s, "Fine", STRONG_B, T0);
    let check = s.start_breach_check().unwrap();

    let prefixes = check.prefixes();
    assert_eq!(prefixes.len(), 2);
    assert!(prefixes.iter().all(|p| p.len() == 5));

    let full = sha1_hex("summer2020");
    let answers = prefixes
        .iter()
        .map(|p| RangeAnswer {
            prefix: p.clone(),
            body: if *p == full[..5] {
                format!(
                    "{}:4242\r\n00000000000000000000000000000000000:0",
                    &full[5..]
                )
            } else {
                "0000000000000000000000000000000000A:3".to_owned()
            },
        })
        .collect();
    let hits = check.finish(answers).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, hit);
    assert_eq!(hits[0].count, 4242);
}

#[test]
fn a_missing_answer_is_an_error_not_a_clean_result() {
    let s = session();
    login(&s, "One", "summer2020", T0);
    let check = s.start_breach_check().unwrap();
    assert!(matches!(
        check.finish(Vec::new()),
        Err(FigLockError::InvalidInput)
    ));
}

#[test]
fn a_locked_session_refuses() {
    let s = session();
    s.lock();
    assert!(matches!(s.health_report(T0), Err(FigLockError::Locked)));
    assert!(s.start_breach_check().is_err());
}
