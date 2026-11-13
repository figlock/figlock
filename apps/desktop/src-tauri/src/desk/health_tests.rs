//! The password health report on an unlocked vault, against a real folder. Each scenario runs as a child with
//! its own device config (see `tests.rs`). The breach service is replaced by a recorder, so these also check
//! what would have gone over the network.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::health::RangeSource;
use super::items::ItemDraft;
use super::tests::{code_of, created_key, desk, password, scenario_in};
use super::*;

fn scenario(name: &str) {
    scenario_in("health_tests", name);
}

fn unlocked(d: &Desk) {
    let pw = password();
    let key = created_key(d, &pw);
    let groups: Vec<&str> = key.split('-').collect();
    assert!(
        d.check_kit_key(&groups[groups.len() - 2..].join("-"))
            .unwrap()
    );
    d.recovery_skip(true).unwrap();
    assert_eq!(d.finish_create(false).unwrap().phase, "unlocked");
}

fn login(d: &Desk, title: &str, pw: &str) -> String {
    let fields: BTreeMap<String, String> = [("title", title), ("password", pw)]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    d.add_item(
        "login",
        ItemDraft {
            fields,
            clear: Vec::new(),
            add_files: Vec::new(),
            remove_files: Vec::new(),
        },
    )
    .unwrap()
}

/// Knows one breached password, the published example of the range API: SHA-1("P@ssw0rd") is
/// 21BD12DC183F740EE76F27B78EB39C8AD972A757. Remembers every prefix it was asked for.
struct Recorder {
    asked: RefCell<Vec<String>>,
    fail: bool,
}

const BREACHED: &str = "P@ssw0rd";

impl RangeSource for Recorder {
    fn range(&self, prefix: &str) -> Result<String, ()> {
        self.asked.borrow_mut().push(prefix.to_owned());
        if self.fail {
            return Err(());
        }
        Ok(if prefix == "21BD1" {
            "003D68EB55068C33ACE09247EE4C639306B:0\r\n2DC183F740EE76F27B78EB39C8AD972A757:52579\r\n"
        } else {
            "003D68EB55068C33ACE09247EE4C639306B:0\r\n"
        }
        .to_owned())
    }
}

#[test]
fn the_report_finds_weak_reused_and_breached_passwords() {
    scenario("report_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn report_inner() {
    let d = desk();
    unlocked(&d);
    let strong = password();
    let weak = login(&d, "Forum", "summer2020");
    let a = login(&d, "Shop", &strong);
    let b = login(&d, "Bank", &strong);
    let breached = login(&d, "Mail", BREACHED);
    let clean = login(&d, "Notes app", &password());

    // Without the breach check: local findings only, and nothing is asked of any service.
    let r = d.health_report(None).unwrap();
    assert_eq!(r.checked, 5);
    assert!(!r.breach_checked && !r.breach_failed);
    let ids: Vec<&str> = r.rows.iter().map(|x| x.id.as_str()).collect();
    assert!(ids.contains(&weak.as_str()));
    assert!(ids.contains(&a.as_str()) && ids.contains(&b.as_str()));
    assert!(!ids.contains(&clean.as_str()) && !ids.contains(&breached.as_str()));
    let row = |id: &str| r.rows.iter().find(|x| x.id == id).unwrap();
    assert!(row(&weak).weak_bits.is_some());
    assert_eq!(row(&a).reused_with, 1);
    assert!(r.rows.iter().all(|x| x.breached.is_none()));

    // With it: the breached login appears even though nothing else is wrong with it.
    let rec = Recorder {
        asked: RefCell::new(Vec::new()),
        fail: false,
    };
    let r = d.health_report(Some(&rec)).unwrap();
    assert!(r.breach_checked && !r.breach_failed);
    assert_eq!(
        r.rows.iter().find(|x| x.id == breached).unwrap().breached,
        Some(52579)
    );
    assert!(
        r.rows
            .iter()
            .all(|x| x.id == breached || x.breached.is_none())
    );

    // Only 5-character hex prefixes were asked for: no password, no full hash, one request per prefix.
    let asked = rec.asked.borrow();
    assert!(!asked.is_empty());
    for p in asked.iter() {
        assert_eq!(p.len(), 5, "{p}");
        assert!(p.bytes().all(|c| c.is_ascii_hexdigit()), "{p}");
    }
    let mut unique = asked.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), asked.len(), "a prefix was asked for twice");
    for secret in ["summer2020", BREACHED, strong.as_str()] {
        assert!(asked.iter().all(|p| !p.contains(secret)));
    }

    // Nothing the window gets holds a password.
    let json = serde_json::to_string(&r).unwrap();
    for secret in ["summer2020", BREACHED, strong.as_str()] {
        assert!(!json.contains(secret), "the report leaked a password");
    }
}

#[test]
fn an_unreachable_breach_service_is_reported_not_treated_as_clean() {
    scenario("offline_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn offline_inner() {
    let d = desk();
    unlocked(&d);
    login(&d, "Forum", "summer2020");
    login(&d, "Mail", BREACHED);
    let rec = Recorder {
        asked: RefCell::new(Vec::new()),
        fail: true,
    };
    let r = d.health_report(Some(&rec)).unwrap();
    assert!(r.breach_failed && !r.breach_checked);
    assert!(r.rows.iter().all(|x| x.breached.is_none()));
    // The local findings are still there.
    assert_eq!(r.rows.len(), 1);
}

#[test]
fn a_locked_vault_has_no_report() {
    let d = desk();
    assert_eq!(code_of(d.health_report(None).unwrap_err()), "locked");
}
