//! Password health for the apps (FIG-292): the same analysis as the Mac Health page, over `figlock_core::health`.
//!
//! Weak, reused and old are computed here and cross the boundary as ids and numbers, never a password. The
//! breach check is the Have I Been Pwned range protocol (k-anonymity) split in two so the app owns the
//! network: [`VaultSession::start_breach_check`] returns a [`BreachCheck`] that lists the 5 character
//! prefixes to ask for, and `finish` matches the answers here. The rest of each SHA-1 never leaves Rust.

use std::collections::HashMap;
use std::sync::Arc;

use figlock_core::generate::Level;
use figlock_core::health::{self, PwnedQuery};

use crate::error::{FigLockError, Result, id_hex};
use crate::session::VaultSession;

const DAY_MS: u64 = 86_400_000;

/// One item with something wrong with its password. Ids and numbers only.
#[derive(uniffi::Record)]
pub struct HealthFinding {
    /// 32 hex characters.
    pub id: String,
    /// Guessability in bits, when the password is weak.
    pub weak_bits: Option<u32>,
    /// How many other items use exactly this password.
    pub reused_with: u32,
    /// Days since the item last changed, when that is a year or more.
    pub old_days: Option<u64>,
}

#[derive(uniffi::Record)]
pub struct HealthSummary {
    /// Passwords looked at.
    pub checked: u32,
    /// Items with at least one issue, worst first.
    pub findings: Vec<HealthFinding>,
}

/// The service's answer for one prefix: the `SUFFIX:COUNT` lines.
#[derive(uniffi::Record)]
pub struct RangeAnswer {
    pub prefix: String,
    pub body: String,
}

/// A password found in known breaches.
#[derive(uniffi::Record)]
pub struct BreachHit {
    /// 32 hex characters.
    pub id: String,
    /// Times the password appears in known breaches.
    pub count: u64,
}

/// A breach check in progress: the hashes of every password, ready to be matched against the lists the app
/// fetches. Holds no more than the sessions's own copy of the passwords' hashes, zeroized on drop.
#[derive(uniffi::Object)]
pub struct BreachCheck {
    queries: Vec<(String, PwnedQuery)>,
}

#[uniffi::export]
impl BreachCheck {
    /// The distinct prefixes to ask the range service for (`GET /range/<prefix>`), 5 uppercase hex characters
    /// each and nothing else. Sorted, so a run is repeatable.
    pub fn prefixes(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .queries
            .iter()
            .map(|(_, q)| q.prefix().to_owned())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Matches the answers against the passwords. Every prefix from [`Self::prefixes`] must be answered: a
    /// missing one fails with `InvalidInput`, so a half-checked vault is never shown as clean. Worst first.
    pub fn finish(&self, answers: Vec<RangeAnswer>) -> Result<Vec<BreachHit>> {
        let lists: HashMap<&str, &str> = answers
            .iter()
            .map(|a| (a.prefix.as_str(), a.body.as_str()))
            .collect();
        let mut hits = Vec::new();
        for (id, q) in &self.queries {
            let body = lists.get(q.prefix()).ok_or(FigLockError::InvalidInput)?;
            let count = q.count_in(body);
            if count > 0 {
                hits.push(BreachHit {
                    id: id.clone(),
                    count,
                });
            }
        }
        hits.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.id.cmp(&b.id)));
        Ok(hits)
    }
}

#[uniffi::export]
impl VaultSession {
    /// Weak, reused and old passwords across every live item. Needs no network. `now_ms` is the wall clock.
    pub fn health_report(&self, now_ms: u64) -> Result<HealthSummary> {
        self.with_vault(|v| {
            let (views, _) = v.items(false);
            let report = health::analyze(&views, now_ms);
            Ok(HealthSummary {
                checked: u32::try_from(report.checked).unwrap_or(u32::MAX),
                findings: report
                    .findings
                    .iter()
                    .map(|f| HealthFinding {
                        id: id_hex(&f.id),
                        weak_bits: f.weak.filter(|s| s.level == Level::Weak).map(|s| s.bits),
                        reused_with: u32::try_from(f.reused_with).unwrap_or(u32::MAX),
                        old_days: f.old_ms.map(|ms| ms / DAY_MS),
                    })
                    .collect(),
            })
        })
    }

    /// Hashes every live password for the breach check. Opt-in: nothing is sent until the app asks the
    /// service for the prefixes of the returned [`BreachCheck`].
    pub fn start_breach_check(&self) -> Result<Arc<BreachCheck>> {
        self.with_vault(|v| {
            let (views, _) = v.items(false);
            let queries = views
                .iter()
                .filter_map(|view| {
                    let pw = view.fields.get("password")?;
                    (!pw.is_empty()).then(|| (id_hex(&view.id), health::pwned_query(pw)))
                })
                .collect();
            Ok(Arc::new(BreachCheck { queries }))
        })
    }
}
