//! Grants and the one authorisation check (`docs/spec/broker.md` 3.1 and 3.3).
//!
//! A token is 32 random bytes returned once; only its SHA-256 is kept. Time is a monotonic duration from an
//! injected [`Env`], so the system clock cannot extend a grant and tests can move time.

use std::collections::{BTreeSet, VecDeque};
use std::time::Duration;

use figlock_core::ItemId;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::peer::PeerId;
use super::proto::{Code, Mode, Scope, Ttl};

pub const MAX_GRANTS: usize = 64;
const MIN15: Duration = Duration::from_secs(15 * 60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const SESSION_CAP: Duration = Duration::from_secs(12 * 60 * 60);
/// How many ended grants are remembered, so a late use gets `expired` rather than `no_grant`.
const TOMBSTONES: usize = 256;

/// Everything the grant table needs from the outside world.
pub trait Env: Send + Sync {
    /// Monotonic time since an arbitrary start.
    fn now(&self) -> Duration;
    /// Wall clock, for the audit log only.
    fn unix_ms(&self) -> u64;
    fn pid_alive(&self, pid: u32) -> bool;
}

pub struct Grant {
    pub id: String,
    token_hash: [u8; 32],
    pub scope: Scope,
    pub mode: Mode,
    pub ttl: Ttl,
    pub peer: PeerId,
    pub label: Option<String>,
    expires: Option<Duration>,
    session_pid: Option<u32>,
}

/// What the table needs to know about the item being used.
pub struct ItemInfo {
    pub id: ItemId,
    pub tags: BTreeSet<String>,
    pub hosts: Vec<String>,
}

#[derive(Default)]
pub struct GrantTable {
    grants: Vec<Grant>,
    ended: VecDeque<[u8; 32]>,
}

fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn random_hex<const N: usize>() -> Result<String, Code> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| Code::Internal)?;
    Ok(hex::encode(b))
}

/// What `authorize` hands back on success.
pub struct Authorized {
    pub grant_id: String,
}

/// A refusal, with the grant it concerned when there was one (for the audit log).
pub struct Refused {
    pub code: Code,
    pub grant_id: Option<String>,
}

fn refuse(code: Code) -> Refused {
    Refused {
        code,
        grant_id: None,
    }
}

impl GrantTable {
    pub fn len(&self) -> usize {
        self.grants.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Grant> {
        self.grants.iter()
    }

    pub fn clear(&mut self) {
        self.grants.clear();
        self.ended.clear();
    }

    /// Time left, `None` when the grant lasts until lock.
    pub fn remaining(&self, g: &Grant, env: &dyn Env) -> Option<Duration> {
        g.expires.map(|e| e.saturating_sub(env.now()))
    }

    fn live(g: &Grant, env: &dyn Env) -> bool {
        g.expires.is_none_or(|e| env.now() < e) && g.session_pid.is_none_or(|p| env.pid_alive(p))
    }

    fn tombstone(&mut self, hash: [u8; 32]) {
        if self.ended.len() == TOMBSTONES {
            self.ended.pop_front();
        }
        self.ended.push_back(hash);
    }

    /// Remove grants whose time or session ended; returns them for the audit log.
    pub fn sweep(&mut self, env: &dyn Env) -> Vec<Grant> {
        let (live, dead): (Vec<Grant>, Vec<Grant>) = std::mem::take(&mut self.grants)
            .into_iter()
            .partition(|g| Self::live(g, env));
        self.grants = live;
        for g in &dead {
            self.tombstone(g.token_hash);
        }
        dead
    }

    /// Create a grant and return `(id, token)`. The caller has already been approved and has written the
    /// audit record.
    pub fn issue(
        &mut self,
        env: &dyn Env,
        scope: Scope,
        mode: Mode,
        ttl: Ttl,
        peer: PeerId,
        label: Option<String>,
    ) -> Result<(String, String), Code> {
        if self.grants.len() >= MAX_GRANTS {
            return Err(Code::Busy);
        }
        let session_pid = match ttl {
            Ttl::Session => Some(peer.pid.ok_or(Code::BadRequest)?),
            _ => None,
        };
        let span = match ttl {
            Ttl::Once | Ttl::Min15 => Some(MIN15),
            Ttl::Session => Some(SESSION_CAP),
            Ttl::Day => Some(DAY),
            Ttl::UntilLock => None,
        };
        let token = random_hex::<32>()?;
        let id = random_hex::<8>()?;
        self.grants.push(Grant {
            id: id.clone(),
            token_hash: hash(&token),
            scope,
            mode,
            ttl,
            peer,
            label,
            expires: span.map(|s| env.now() + s),
            session_pid,
        });
        Ok((id, token))
    }

    /// Remove a grant by id.
    pub fn revoke(&mut self, id: &str) -> Option<Grant> {
        let at = self.grants.iter().position(|g| g.id == id)?;
        Some(self.grants.remove(at))
    }

    pub fn find_id(&self, id: &str) -> Option<&Grant> {
        self.grants.iter().find(|g| g.id == id)
    }

    fn position(&self, token_hash: &[u8; 32]) -> Option<usize> {
        // Visit every grant so the time taken does not depend on which one matched.
        let mut found = None;
        for (i, g) in self.grants.iter().enumerate() {
            if bool::from(g.token_hash.ct_eq(token_hash)) {
                found = Some(i);
            }
        }
        found
    }

    /// The check every use of a grant goes through, in the order of `broker.md` 3.3 after `locked`.
    /// A `once` grant is consumed here, in the same step as the check.
    pub fn authorize(
        &mut self,
        env: &dyn Env,
        token: &str,
        item: &ItemInfo,
        mode: Mode,
        host: Option<&str>,
    ) -> Result<Authorized, Refused> {
        let well_formed = token.len() == 64
            && token
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !well_formed {
            return Err(refuse(Code::NoGrant));
        }
        let h = hash(token);
        let Some(at) = self.position(&h) else {
            let code = if self.ended.iter().any(|e| bool::from(e.ct_eq(&h))) {
                Code::Expired
            } else {
                Code::NoGrant
            };
            return Err(refuse(code));
        };
        let id = self.grants[at].id.clone();
        let refused = |code| Refused {
            code,
            grant_id: Some(id.clone()),
        };
        if !Self::live(&self.grants[at], env) {
            self.grants.remove(at);
            self.tombstone(h);
            return Err(refused(Code::Expired));
        }
        let g = &self.grants[at];
        let in_scope = match &g.scope {
            Scope::Item(i) => *i == item.id,
            Scope::Tag(t) => item.tags.contains(t),
            Scope::Vault => true,
        };
        let host_ok = match mode {
            Mode::Http => host.is_some_and(|h| host_allowed(&item.hosts, h)),
            Mode::Run | Mode::Reveal => true,
        };
        if g.mode != mode || !in_scope || !host_ok {
            return Err(refused(Code::NotAllowed));
        }
        if g.ttl == Ttl::Once {
            self.grants.remove(at);
            // A consumed grant is simply gone: its token is `no_grant`, not `expired`.
        }
        Ok(Authorized { grant_id: id })
    }
}

fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Host allow-list matching (`broker.md` 3.3): ASCII case-insensitive equality, or `*.suffix` for any
/// host with at least one more label. An empty list allows nothing. Ports and schemes are not part of it.
pub fn host_allowed(entries: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    if !valid_host(&host) {
        return false;
    }
    entries.iter().any(|e| {
        let e = e.to_ascii_lowercase();
        match e.strip_prefix("*.") {
            Some(suffix) => {
                valid_host(suffix)
                    && host.len() > suffix.len() + 1
                    && host.ends_with(&format!(".{suffix}"))
            }
            None => valid_host(&e) && e == host,
        }
    })
}

#[cfg(test)]
pub mod testenv {
    use super::*;
    use std::collections::BTreeSet as Set;
    use std::sync::Mutex;

    /// A clock the test moves by hand and a set of live pids.
    pub struct TestEnv {
        pub now: Mutex<Duration>,
        pub alive: Mutex<Set<u32>>,
    }

    impl TestEnv {
        pub fn new() -> Self {
            TestEnv {
                now: Mutex::new(Duration::from_secs(1000)),
                alive: Mutex::new(Set::new()),
            }
        }

        pub fn advance(&self, d: Duration) {
            *self.now.lock().unwrap() += d;
        }
    }

    impl Env for TestEnv {
        fn now(&self) -> Duration {
            *self.now.lock().unwrap()
        }
        fn unix_ms(&self) -> u64 {
            1_700_000_000_000 + u64::try_from(self.now().as_millis()).unwrap()
        }
        fn pid_alive(&self, pid: u32) -> bool {
            self.alive.lock().unwrap().contains(&pid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testenv::TestEnv;
    use super::*;

    fn peer(pid: Option<u32>) -> PeerId {
        PeerId {
            uid: 501,
            pid,
            exe: None,
        }
    }

    fn item(id: u8, tags: &[&str], hosts: &[&str]) -> ItemInfo {
        ItemInfo {
            id: [id; 16],
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
        }
    }

    fn issue(t: &mut GrantTable, env: &TestEnv, scope: Scope, mode: Mode, ttl: Ttl) -> String {
        t.issue(env, scope, mode, ttl, peer(Some(77)), None)
            .unwrap()
            .1
    }

    fn code(r: Result<Authorized, Refused>) -> Code {
        r.err().expect("should be refused").code
    }

    #[test]
    fn host_matching_rules() {
        let list = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let api = list(&["API.Example.com"]);
        assert!(host_allowed(&api, "api.example.com"));
        assert!(host_allowed(&api, "API.EXAMPLE.COM"));
        assert!(!host_allowed(&api, "example.com"));
        assert!(!host_allowed(&api, "evil-api.example.com"));
        assert!(!host_allowed(&api, "api.example.com.evil.net"));
        let wild = list(&["*.example.com"]);
        assert!(host_allowed(&wild, "api.example.com"));
        assert!(host_allowed(&wild, "a.b.example.com"));
        assert!(!host_allowed(&wild, "example.com"));
        assert!(!host_allowed(&wild, "notexample.com"));
        assert!(!host_allowed(&wild, ".example.com"));
        assert!(
            !host_allowed(&[], "api.example.com"),
            "empty list allows nothing"
        );
        for bad in [
            "",
            ".",
            "a..b",
            "a.b.",
            "-a.com",
            "a b.com",
            "a.com/x",
            "a.com:443",
            "a@b.com",
            "ex\u{e4}mple.com",
        ] {
            assert!(!host_allowed(&list(&["*.com", "a.com"]), bad), "{bad:?}");
        }
        assert!(
            !host_allowed(&list(&["*."]), "a.b"),
            "a wildcard needs a suffix"
        );
        assert!(host_allowed(
            &list(&["xn--bcher-kva.example"]),
            "xn--bcher-kva.example"
        ));
    }

    #[test]
    fn unknown_malformed_and_revoked_tokens_are_refused() {
        let env = TestEnv::new();
        let mut t = GrantTable::default();
        let i = item(1, &[], &["api.example.com"]);
        let token = issue(&mut t, &env, Scope::Vault, Mode::Http, Ttl::UntilLock);
        let ok = |t: &mut GrantTable, tok: &str| {
            t.authorize(&env, tok, &i, Mode::Http, Some("api.example.com"))
        };
        assert!(ok(&mut t, &token).is_ok());
        for bad in [
            "",
            "abc",
            &"0".repeat(64),
            &token.to_uppercase(),
            &format!("{token}0"),
        ] {
            assert_eq!(code(ok(&mut t, bad)), Code::NoGrant, "{bad}");
        }
        let id = t.iter().next().unwrap().id.clone();
        t.revoke(&id).unwrap();
        assert_eq!(code(ok(&mut t, &token)), Code::NoGrant);
    }

    #[test]
    fn once_is_consumed_by_the_first_use_and_a_refusal_does_not_consume_it() {
        let env = TestEnv::new();
        let mut t = GrantTable::default();
        let i = item(1, &[], &["api.example.com"]);
        let token = issue(&mut t, &env, Scope::Item([1; 16]), Mode::Http, Ttl::Once);
        // A refused use (wrong host) leaves it available.
        assert_eq!(
            code(t.authorize(&env, &token, &i, Mode::Http, Some("evil.example"))),
            Code::NotAllowed
        );
        assert!(
            t.authorize(&env, &token, &i, Mode::Http, Some("api.example.com"))
                .is_ok()
        );
        assert_eq!(
            code(t.authorize(&env, &token, &i, Mode::Http, Some("api.example.com"))),
            Code::NoGrant
        );
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn two_racing_uses_of_a_once_grant_cannot_both_succeed() {
        use std::sync::{Arc, Mutex};
        let env = Arc::new(TestEnv::new());
        let table = Arc::new(Mutex::new(GrantTable::default()));
        let token = issue(
            &mut table.lock().unwrap(),
            &env,
            Scope::Vault,
            Mode::Run,
            Ttl::Once,
        );
        let wins: usize = (0..8)
            .map(|_| {
                let (table, env, token) = (table.clone(), env.clone(), token.clone());
                std::thread::spawn(move || {
                    let i = item(1, &[], &[]);
                    table
                        .lock()
                        .unwrap()
                        .authorize(env.as_ref(), &token, &i, Mode::Run, None)
                        .is_ok()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum();
        assert_eq!(wins, 1);
    }

    #[test]
    fn time_limits_end_grants() {
        let env = TestEnv::new();
        let mut t = GrantTable::default();
        let i = item(1, &[], &[]);
        let use_it = |t: &mut GrantTable, tok: &str| t.authorize(&env, tok, &i, Mode::Run, None);
        let once = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::Once);
        let m15 = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::Min15);
        let day = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::Day);
        let lock = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::UntilLock);
        env.advance(Duration::from_secs(14 * 60 + 59));
        assert!(use_it(&mut t, &m15).is_ok());
        env.advance(Duration::from_secs(2));
        assert_eq!(code(use_it(&mut t, &m15)), Code::Expired, "15m ended");
        assert_eq!(
            code(use_it(&mut t, &once)),
            Code::Expired,
            "unused once ended after 15m"
        );
        assert!(use_it(&mut t, &day).is_ok());
        env.advance(Duration::from_secs(24 * 3600));
        assert_eq!(code(use_it(&mut t, &day)), Code::Expired, "1d ended");
        env.advance(Duration::from_secs(365 * 24 * 3600));
        assert!(use_it(&mut t, &lock).is_ok(), "until-lock has no timer");
        // Once expired, the answer stays `expired` after a sweep too.
        let swept = t.sweep(&env);
        assert_eq!(swept.len(), 0, "already removed by the failed uses");
        assert_eq!(code(use_it(&mut t, &m15)), Code::Expired);
    }

    #[test]
    fn sweep_returns_ended_grants_and_keeps_them_expired() {
        let env = TestEnv::new();
        let mut t = GrantTable::default();
        let tok = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::Min15);
        env.advance(MIN15 + Duration::from_secs(1));
        let swept = t.sweep(&env);
        assert_eq!(swept.len(), 1);
        let i = item(1, &[], &[]);
        assert_eq!(
            code(t.authorize(&env, &tok, &i, Mode::Run, None)),
            Code::Expired
        );
    }

    #[test]
    fn a_session_ends_with_its_process_or_after_twelve_hours() {
        let env = TestEnv::new();
        env.alive.lock().unwrap().insert(77);
        let mut t = GrantTable::default();
        let i = item(1, &[], &[]);
        let tok = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::Session);
        env.advance(Duration::from_secs(3600));
        assert!(t.authorize(&env, &tok, &i, Mode::Run, None).is_ok());
        env.alive.lock().unwrap().clear();
        assert_eq!(
            code(t.authorize(&env, &tok, &i, Mode::Run, None)),
            Code::Expired
        );

        env.alive.lock().unwrap().insert(77);
        let tok2 = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::Session);
        env.advance(SESSION_CAP + Duration::from_secs(1));
        assert_eq!(
            code(t.authorize(&env, &tok2, &i, Mode::Run, None)),
            Code::Expired
        );

        // No known pid: a session cannot be created.
        let r = t.issue(
            &env,
            Scope::Vault,
            Mode::Run,
            Ttl::Session,
            peer(None),
            None,
        );
        assert_eq!(r.err(), Some(Code::BadRequest));
    }

    #[test]
    fn scope_mode_and_host_are_enforced() {
        let env = TestEnv::new();
        let mut t = GrantTable::default();
        let tagged = item(2, &["work"], &["api.example.com"]);
        let other = item(3, &["home"], &["api.example.com"]);
        let no_hosts = item(2, &["work"], &[]);

        let by_item = issue(
            &mut t,
            &env,
            Scope::Item([2; 16]),
            Mode::Http,
            Ttl::UntilLock,
        );
        let by_tag = issue(
            &mut t,
            &env,
            Scope::Tag("work".into()),
            Mode::Http,
            Ttl::UntilLock,
        );
        let run = issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::UntilLock);
        let h = Some("api.example.com");

        assert!(t.authorize(&env, &by_item, &tagged, Mode::Http, h).is_ok());
        assert_eq!(
            code(t.authorize(&env, &by_item, &other, Mode::Http, h)),
            Code::NotAllowed
        );
        assert!(t.authorize(&env, &by_tag, &tagged, Mode::Http, h).is_ok());
        assert_eq!(
            code(t.authorize(&env, &by_tag, &other, Mode::Http, h)),
            Code::NotAllowed
        );
        assert_eq!(
            code(t.authorize(&env, &by_item, &tagged, Mode::Http, Some("evil.example"))),
            Code::NotAllowed,
            "host not on the allow-list"
        );
        assert_eq!(
            code(t.authorize(&env, &by_item, &tagged, Mode::Http, None)),
            Code::NotAllowed,
            "no host"
        );
        assert_eq!(
            code(t.authorize(&env, &by_item, &no_hosts, Mode::Http, h)),
            Code::NotAllowed,
            "empty list"
        );
        assert_eq!(
            code(t.authorize(&env, &by_item, &tagged, Mode::Run, None)),
            Code::NotAllowed,
            "wrong mode"
        );
        assert_eq!(
            code(t.authorize(&env, &run, &tagged, Mode::Http, h)),
            Code::NotAllowed,
            "run grant for http"
        );
        assert!(t.authorize(&env, &run, &other, Mode::Run, None).is_ok());
    }

    #[test]
    fn the_table_is_bounded_and_tokens_are_only_kept_as_hashes() {
        let env = TestEnv::new();
        let mut t = GrantTable::default();
        let mut tokens = Vec::new();
        for _ in 0..MAX_GRANTS {
            tokens.push(issue(&mut t, &env, Scope::Vault, Mode::Run, Ttl::UntilLock));
        }
        let r = t.issue(
            &env,
            Scope::Vault,
            Mode::Run,
            Ttl::UntilLock,
            peer(Some(1)),
            None,
        );
        assert_eq!(r.err(), Some(Code::Busy));
        let unique: BTreeSet<_> = tokens.iter().collect();
        assert_eq!(unique.len(), MAX_GRANTS, "tokens are random");
        assert!(tokens.iter().all(|t| t.len() == 64));
        let ids: BTreeSet<_> = t.iter().map(|g| g.id.clone()).collect();
        assert!(tokens.iter().all(|tok| !ids.contains(tok)));
    }
}
