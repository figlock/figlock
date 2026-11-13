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
mod tests;
