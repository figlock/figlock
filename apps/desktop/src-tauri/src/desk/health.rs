//! The password health report as the window sees it (FIG-89). Weak, reused and old come from
//! `figlock_core::health` and never leave the machine. The breach check is opt-in per run: it asks the Have
//! I Been Pwned range service for the 5 hex character prefix of each password's SHA-1 and matches the rest
//! here (k-anonymity), so neither a password nor its full hash is ever sent. What crosses to the window is
//! item ids and numbers, no password.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use figlock_core::generate::Level;
use figlock_core::health::{self, PREFIX_LEN, PwnedQuery};
use figlock_session::session::{id_hex, now_ms};
use serde::Serialize;

use super::{CmdRes, Desk};

const RANGE_URL: &str = "https://api.pwnedpasswords.com/range/";
/// A range answer is about 40 KB; this only stops a hostile or broken server filling memory.
const MAX_RANGE_BYTES: u64 = 4 * 1024 * 1024;

/// Where breach lists come from. The real one is the range service; tests substitute one that records what
/// it was asked.
pub trait RangeSource {
    /// The `SUFFIX:COUNT` lines for hashes that start with `prefix`.
    fn range(&self, prefix: &str) -> Result<String, ()>;
}

pub struct PwnedPasswords(ureq::Agent);

impl PwnedPasswords {
    pub fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .max_redirects(0)
            .build()
            .into();
        PwnedPasswords(agent)
    }
}

impl Default for PwnedPasswords {
    fn default() -> Self {
        Self::new()
    }
}

impl RangeSource for PwnedPasswords {
    fn range(&self, prefix: &str) -> Result<String, ()> {
        debug_assert_eq!(prefix.len(), PREFIX_LEN);
        // `Add-Padding` makes every answer similar in size, so its length says little about the prefix.
        self.0
            .get(&format!("{RANGE_URL}{prefix}"))
            .header("Add-Padding", "true")
            .call()
            .map_err(|_| ())?
            .body_mut()
            .with_config()
            .limit(MAX_RANGE_BYTES)
            .read_to_string()
            .map_err(|_| ())
    }
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthRow {
    pub id: String,
    /// Guessability in bits, when the password is weak.
    pub weak_bits: Option<u32>,
    /// How many other items use the same password.
    pub reused_with: usize,
    /// Days since the item last changed, when that is a year or more.
    pub old_days: Option<u64>,
    /// Times the password appears in known breaches, when the breach check ran and found it.
    pub breached: Option<u64>,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    /// Passwords looked at.
    pub checked: usize,
    pub rows: Vec<HealthRow>,
    /// The breach check ran to the end.
    pub breach_checked: bool,
    /// The breach check was asked for but the service could not be reached, so only the local results are here.
    pub breach_failed: bool,
}

const DAY_MS: u64 = 86_400_000;

impl Desk {
    /// Checks every saved password. The vault is only held while the local analysis runs; the network
    /// (when `source` is given) is used after it is released, so a slow connection never freezes the app.
    pub fn health_report(&self, source: Option<&dyn RangeSource>) -> CmdRes<HealthReport> {
        let (report, queries) = self.with_session(|inner| {
            let session = inner.session.as_mut().ok_or_else(super::items::locked)?;
            let views = session.items(false);
            let report = health::analyze(&views, now_ms());
            let queries: Vec<(String, PwnedQuery)> = if source.is_some() {
                views
                    .iter()
                    .filter_map(|v| {
                        let pw = v.fields.get("password")?;
                        (!pw.is_empty()).then(|| (id_hex(&v.id), health::pwned_query(pw)))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Ok((report, queries))
        })?;

        let mut rows: BTreeMap<String, HealthRow> = BTreeMap::new();
        let order: Vec<String> = report.findings.iter().map(|f| id_hex(&f.id)).collect();
        for f in &report.findings {
            rows.insert(
                id_hex(&f.id),
                HealthRow {
                    id: id_hex(&f.id),
                    weak_bits: f.weak.filter(|s| s.level == Level::Weak).map(|s| s.bits),
                    reused_with: f.reused_with,
                    old_days: f.old_ms.map(|ms| ms / DAY_MS),
                    breached: None,
                },
            );
        }

        let (mut breach_checked, mut breach_failed) = (false, false);
        let mut breached_only = Vec::new();
        if let Some(source) = source {
            match breaches(source, &queries) {
                Ok(found) => {
                    breach_checked = true;
                    for (id, n) in found {
                        match rows.get_mut(&id) {
                            Some(row) => row.breached = Some(n),
                            None => breached_only.push((id, n)),
                        }
                    }
                }
                Err(()) => breach_failed = true,
            }
        }

        let mut out: Vec<HealthRow> = order.iter().filter_map(|id| rows.remove(id)).collect();
        breached_only.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out.extend(breached_only.into_iter().map(|(id, n)| HealthRow {
            id,
            weak_bits: None,
            reused_with: 0,
            old_days: None,
            breached: Some(n),
        }));
        Ok(HealthReport {
            checked: report.checked,
            rows: out,
            breach_checked,
            breach_failed,
        })
    }
}

/// Item ids (hex) with the number of breaches each password is in. One request per distinct prefix, never
/// anything but the prefix. Any failed request fails the whole check, so a half-checked vault is never shown
/// as clean.
fn breaches(
    source: &dyn RangeSource,
    queries: &[(String, PwnedQuery)],
) -> Result<Vec<(String, u64)>, ()> {
    let prefixes: BTreeSet<&str> = queries.iter().map(|(_, q)| q.prefix()).collect();
    let mut lists: BTreeMap<&str, String> = BTreeMap::new();
    for prefix in prefixes {
        lists.insert(prefix, source.range(prefix)?);
    }
    Ok(queries
        .iter()
        .filter_map(|(id, q)| {
            let n = q.count_in(&lists[q.prefix()]);
            (n > 0).then(|| (id.clone(), n))
        })
        .collect())
}
