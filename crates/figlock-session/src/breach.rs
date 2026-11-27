//! The breach check of the password health report, shared by the CLI and the desktop app. It asks the Have
//! I Been Pwned range service for the 5 hex character prefix of each password's SHA-1 and matches the rest
//! here (k-anonymity), so neither a password nor its full hash is ever sent.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use figlock_core::health::{PREFIX_LEN, PwnedQuery};

const RANGE_URL: &str = "https://api.pwnedpasswords.com/range/";
/// A range answer is about 40 KB; this only stops a hostile or broken server filling memory.
const MAX_RANGE_BYTES: u64 = 4 * 1024 * 1024;

/// The range service could not be asked or its answer could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreachable;

/// Where breach lists come from. The real one is the range service; tests substitute one that records what
/// it was asked.
pub trait RangeSource {
    /// The `SUFFIX:COUNT` lines for hashes that start with `prefix`.
    fn range(&self, prefix: &str) -> Result<String, Unreachable>;
}

pub struct PwnedPasswords {
    agent: ureq::Agent,
    base: String,
}

impl PwnedPasswords {
    pub fn new() -> Self {
        Self::with_base(RANGE_URL)
    }

    /// Asks `base` (ending in a slash) instead of the public service: a mirror, or a stand-in in tests.
    pub fn with_base(base: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .max_redirects(0)
            .build()
            .into();
        PwnedPasswords {
            agent,
            base: base.to_owned(),
        }
    }
}

impl Default for PwnedPasswords {
    fn default() -> Self {
        Self::new()
    }
}

impl RangeSource for PwnedPasswords {
    fn range(&self, prefix: &str) -> Result<String, Unreachable> {
        debug_assert_eq!(prefix.len(), PREFIX_LEN);
        // `Add-Padding` makes every answer similar in size, so its length says little about the prefix.
        self.agent
            .get(&format!("{}{prefix}", self.base))
            .header("Add-Padding", "true")
            .call()
            .map_err(|_| Unreachable)?
            .body_mut()
            .with_config()
            .limit(MAX_RANGE_BYTES)
            .read_to_string()
            .map_err(|_| Unreachable)
    }
}

/// Item ids (hex) with the number of breaches each password is in. One request per distinct prefix, never
/// anything but the prefix. Any failed request fails the whole check, so a half-checked vault is never shown
/// as clean.
pub fn breaches(
    source: &dyn RangeSource,
    queries: &[(String, PwnedQuery)],
) -> Result<Vec<(String, u64)>, Unreachable> {
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
