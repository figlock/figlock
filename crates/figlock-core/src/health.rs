//! Password health: which saved passwords are weak, reused or old, and the query side of the breach check.
//!
//! Pure and in memory, like search: [`analyze`] reads decrypted [`ItemView`]s and returns ids and numbers,
//! never a password. The breach check follows the Have I Been Pwned range protocol (k-anonymity): only the
//! first five hex characters of the SHA-1 of a password may leave the process, in a [`PwnedQuery`]. The
//! other 35 characters stay here and are matched against the list the service returns. SHA-1 is used only
//! because that service is keyed by it; it protects nothing here.
//!
//! Only items with a non-empty `password` field are looked at (logins and documents). A password counts as
//! old when its item has not been written to for [`OLD_AFTER_MS`]; the vault keeps one write time per item,
//! so editing any field of the item, not only its password, starts the clock again.

use std::collections::HashMap;

use sha1::{Digest, Sha1};
use zeroize::Zeroizing;

use crate::generate::{Level, Strength, password_strength};
use crate::{ItemId, ItemView};

const DAY_MS: u64 = 86_400_000;
/// A password this long unchanged is reported as old.
pub const OLD_AFTER_MS: u64 = 365 * DAY_MS;
/// The first part of the hash that may be sent to the breach service.
pub const PREFIX_LEN: usize = 5;
const SUFFIX_LEN: usize = 40 - PREFIX_LEN;

/// What is wrong with one item's password. Every field is derived; none holds the password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: ItemId,
    /// Set when the password is weak.
    pub weak: Option<Strength>,
    /// How many other items use exactly this password; zero when it is unique.
    pub reused_with: usize,
    /// How long the item has gone unchanged, when that is [`OLD_AFTER_MS`] or more.
    pub old_ms: Option<u64>,
}

impl Finding {
    fn has_issue(&self) -> bool {
        self.weak.is_some() || self.reused_with > 0 || self.old_ms.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Passwords looked at.
    pub checked: usize,
    /// Items with at least one issue, worst first (most issues, then weakest).
    pub findings: Vec<Finding>,
}

/// Checks every live item that holds a password. `now_ms` is the wall clock in milliseconds.
pub fn analyze(items: &[ItemView], now_ms: u64) -> Report {
    let held: Vec<(&ItemView, &str)> = items
        .iter()
        .filter(|i| !i.deleted)
        .filter_map(|i| {
            i.fields
                .get("password")
                .map(|p| p.as_str())
                .filter(|p| !p.is_empty())
                .map(|p| (i, p))
        })
        .collect();

    let mut users: HashMap<&str, usize> = HashMap::new();
    for (_, pw) in &held {
        *users.entry(pw).or_default() += 1;
    }

    let mut findings: Vec<Finding> = held
        .iter()
        .map(|(item, pw)| {
            let strength = password_strength(pw);
            let age = now_ms.saturating_sub(item.updated.wall);
            Finding {
                id: item.id,
                weak: (strength.level == Level::Weak).then_some(strength),
                reused_with: users[pw] - 1,
                old_ms: (age >= OLD_AFTER_MS).then_some(age),
            }
        })
        .filter(Finding::has_issue)
        .collect();
    findings.sort_by_key(|f| {
        let issues = usize::from(f.weak.is_some())
            + usize::from(f.reused_with > 0)
            + usize::from(f.old_ms.is_some());
        (
            std::cmp::Reverse(issues),
            f.weak.map_or(u32::MAX, |s| s.bits),
            f.id,
        )
    });
    Report {
        checked: held.len(),
        findings,
    }
}

/// The two halves of a password's SHA-1, for the range protocol. Only [`PwnedQuery::prefix`] may be sent
/// anywhere.
pub struct PwnedQuery {
    prefix: String,
    suffix: Zeroizing<String>,
}

/// Hashes `password` for the breach check.
pub fn pwned_query(password: &str) -> PwnedQuery {
    let digest = Sha1::digest(password.as_bytes());
    let hex = Zeroizing::new(
        digest
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<String>(),
    );
    PwnedQuery {
        prefix: hex[..PREFIX_LEN].to_owned(),
        suffix: Zeroizing::new(hex[PREFIX_LEN..].to_owned()),
    }
}

impl PwnedQuery {
    /// Five uppercase hex characters: what the breach service is asked for.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// How many times this password is in the breach list the service returned for [`Self::prefix`], zero
    /// when it is not. `range` is the response body: lines of `SUFFIX:COUNT`. Malformed lines and the
    /// zero-count padding lines the service adds on request are ignored.
    pub fn count_in(&self, range: &str) -> u64 {
        range
            .lines()
            .filter_map(|l| l.trim().split_once(':'))
            .find(|(suffix, _)| {
                suffix.len() == SUFFIX_LEN && suffix.eq_ignore_ascii_case(&self.suffix)
            })
            .and_then(|(_, n)| n.trim().parse().ok())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;
    use crate::{ItemType, Stamp};

    const NOW: u64 = 1_800_000_000_000;

    fn item(n: u8, password: Option<&str>, updated_ms: u64, deleted: bool) -> ItemView {
        let mut fields = BTreeMap::new();
        if let Some(p) = password {
            fields.insert("password".to_owned(), Zeroizing::new(p.to_owned()));
        }
        ItemView {
            id: [n; 16],
            item_type: ItemType::Login,
            vault: "personal".to_owned(),
            deleted,
            fields,
            updated: Stamp {
                wall: updated_ms,
                counter: 0,
            },
        }
    }

    const STRONG_A: &str = "tr0ub4dor&3-horse-staple-battery-9x";
    const STRONG_B: &str = "kq7-Zm2!vXw9-Lp4#Rt8-Yn5$Hc3";

    #[test]
    fn strong_unique_recent_passwords_are_clean() {
        let items = [
            item(1, Some(STRONG_A), NOW, false),
            item(2, Some(STRONG_B), NOW - DAY_MS, false),
        ];
        let r = analyze(&items, NOW);
        assert_eq!(r.checked, 2);
        assert!(r.findings.is_empty());
    }

    #[test]
    fn weak_reused_and_old_are_each_reported() {
        let items = [
            item(1, Some("summer2020"), NOW, false),
            item(2, Some(STRONG_A), NOW, false),
            item(3, Some(STRONG_A), NOW, false),
            item(4, Some(STRONG_B), NOW - OLD_AFTER_MS, false),
        ];
        let r = analyze(&items, NOW);
        assert_eq!(r.checked, 4);
        let by = |n: u8| r.findings.iter().find(|f| f.id == [n; 16]).unwrap();
        assert!(by(1).weak.is_some() && by(1).reused_with == 0 && by(1).old_ms.is_none());
        assert_eq!(by(2).reused_with, 1);
        assert_eq!(by(3).reused_with, 1);
        assert!(by(2).weak.is_none());
        assert_eq!(by(4).old_ms, Some(OLD_AFTER_MS));
    }

    #[test]
    fn deleted_items_and_items_without_a_password_are_skipped() {
        let items = [
            item(1, Some("abc"), NOW, true),
            item(2, None, 0, false),
            item(3, Some(""), 0, false),
        ];
        let r = analyze(&items, NOW);
        assert_eq!(r.checked, 0);
        assert!(r.findings.is_empty());
    }

    #[test]
    fn a_deleted_copy_does_not_make_a_password_reused() {
        let items = [
            item(1, Some(STRONG_A), NOW, false),
            item(2, Some(STRONG_A), NOW, true),
        ];
        assert!(analyze(&items, NOW).findings.is_empty());
    }

    #[test]
    fn a_future_write_time_is_not_old() {
        let items = [item(1, Some(STRONG_A), NOW + OLD_AFTER_MS, false)];
        assert!(analyze(&items, NOW).findings.is_empty());
    }

    #[test]
    fn worst_findings_come_first() {
        let items = [
            item(1, Some(STRONG_A), NOW - OLD_AFTER_MS, false),
            item(2, Some("abc"), NOW - OLD_AFTER_MS, false),
            item(3, Some("abcd"), NOW, false),
        ];
        let ids: Vec<u8> = analyze(&items, NOW)
            .findings
            .iter()
            .map(|f| f.id[0])
            .collect();
        assert_eq!(ids, [2, 3, 1]);
    }

    /// The published example of the range API: SHA-1("P@ssw0rd") = 21BD1 | 2DC183F740EE76F27B78EB39C8AD972A757.
    #[test]
    fn query_splits_the_sha1_the_way_the_range_api_does() {
        let q = pwned_query("P@ssw0rd");
        assert_eq!(q.prefix(), "21BD1");
        assert_eq!(&*q.suffix, "2DC183F740EE76F27B78EB39C8AD972A757");
    }

    #[test]
    fn count_is_read_from_the_range_response() {
        let q = pwned_query("P@ssw0rd");
        let body = "0018A45C4D1DEF81644B54AB7F969B88D65:0\r\n\
                    2DC183F740EE76F27B78EB39C8AD972A757:52579\r\n\
                    garbage line\r\n\
                    003D68EB55068C33ACE09247EE4C639306B:3\r\n";
        assert_eq!(q.count_in(body), 52579);
        assert_eq!(q.count_in("003D68EB55068C33ACE09247EE4C639306B:3"), 0);
        assert_eq!(q.count_in(""), 0);
    }

    #[test]
    fn padding_and_malformed_counts_read_as_not_found() {
        let q = pwned_query("P@ssw0rd");
        assert_eq!(q.count_in("2DC183F740EE76F27B78EB39C8AD972A757:0"), 0);
        assert_eq!(q.count_in("2DC183F740EE76F27B78EB39C8AD972A757:x"), 0);
        assert_eq!(q.count_in("757:5"), 0);
    }

    proptest! {
        #[test]
        fn prefix_is_five_upper_hex_and_leaks_nothing_else(pw in ".{0,64}") {
            let q = pwned_query(&pw);
            prop_assert_eq!(q.prefix().len(), PREFIX_LEN);
            prop_assert!(q.prefix().bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)));
            prop_assert_eq!(q.suffix.len(), SUFFIX_LEN);
        }

        #[test]
        fn a_listed_suffix_is_always_found(pw in ".{0,64}", n in 1u64..1_000_000) {
            let q = pwned_query(&pw);
            let body = format!("{}:{n}\r\nFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF:1", *q.suffix);
            prop_assert_eq!(q.count_in(&body), n);
        }

        /// Reuse is symmetric and exact: n copies of a password each report n - 1 others, and the
        /// analysis never reports an id it was not given.
        #[test]
        fn reuse_groups_are_counted_exactly(copies in 1usize..6, others in 0usize..4) {
            let mut items = Vec::new();
            for i in 0..copies {
                items.push(item(i as u8, Some(STRONG_A), NOW, false));
            }
            for j in 0..others {
                // Distinct strong passwords.
                let pw = format!("{STRONG_B}-{j}");
                items.push(item((100 + j) as u8, Some(&pw), NOW, false));
            }
            let r = analyze(&items, NOW);
            prop_assert_eq!(r.checked, copies + others);
            for f in &r.findings {
                prop_assert!(items.iter().any(|i| i.id == f.id));
                prop_assert_eq!(f.reused_with, copies - 1);
            }
            prop_assert_eq!(r.findings.len(), if copies > 1 { copies } else { 0 });
        }
    }
}
