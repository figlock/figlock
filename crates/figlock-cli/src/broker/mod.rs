//! The broker daemon (`docs/spec/broker.md`, ADR-0019): an unlocked vault behind a private socket that
//! hands out approved, time-boxed grants and writes an audit record before every decision takes effect.
//!
//! [`Broker`] holds the rules and no I/O beyond the audit file, so tests drive it directly; [`server`]
//! puts it on a Unix socket.

pub mod approve;
pub mod audit;
pub mod cli;
pub mod client;
pub mod grants;
pub mod http;
pub mod peer;
pub mod proto;
pub mod server;
pub mod syncer;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use figlock_core::{ItemId, ItemType, ItemView, Vault, audit_clean_text};
use figlock_session::session::Session;
use serde_json::{Value, json};
use url::Url;
use zeroize::Zeroizing;

use crate::scrub::Scrubber;
use approve::{APPROVAL_WAIT, ApprovalRequest, Approver, Decision};
use audit::AuditWriter;
use grants::{Authorized, Env, GrantTable, ItemInfo, MAX_GRANTS};
use peer::PeerId;
use proto::{
    Code, GrantRequest, HttpRequest, MAX_VARS, Mode, Op, Scope, Ttl, env_name, err, ok,
    parse_request,
};

/// After a denial, the same process is told `busy` for this long (approval fatigue, G8).
const DENY_COOLDOWN: Duration = Duration::from_secs(5);

struct State {
    /// `None` once locked: the vault, its keys and its store are dropped. The session (not a bare vault) is
    /// kept so a sync can be taken in while the broker runs.
    session: Option<Session>,
    grants: GrantTable,
    audit: AuditWriter,
    denied_until: HashMap<u32, Duration>,
}

impl State {
    fn vault(&mut self) -> Option<&mut Vault> {
        self.session.as_mut().map(|s| &mut s.vault)
    }

    /// Write one audit record. Fails closed with `audit_failed`.
    fn log(&mut self, t: u64, ev: &str, d: Value) -> Result<(), Code> {
        let vault = &self.session.as_ref().ok_or(Code::Locked)?.vault;
        self.audit
            .append(vault, t, ev, d)
            .map_err(|()| Code::AuditFailed)
    }
}

/// What [`Broker::absorb`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absorbed {
    /// Nothing new.
    Same,
    /// New items or devices were taken in.
    Updated,
    /// The broker is locked, now or already.
    Locked,
}

pub struct Reply {
    pub line: String,
    /// The broker locked; the server stops after sending this.
    pub exit: bool,
}

impl Reply {
    fn line(line: String) -> Reply {
        Reply { line, exit: false }
    }
}

pub struct Broker {
    state: Mutex<State>,
    /// One approval prompt at a time.
    gate: Mutex<()>,
    approver: Box<dyn Approver>,
    env: Arc<dyn Env>,
    uid: u32,
    wait: Duration,
}

fn peer_json(p: &PeerId) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("uid".into(), json!(p.uid));
    if let Some(pid) = p.pid {
        o.insert("pid".into(), json!(pid));
    }
    if let Some(exe) = &p.exe {
        o.insert("exe".into(), json!(audit_clean_text(exe, 1024)));
    }
    Value::Object(o)
}

/// The audit log's `peer` member needs both ids; a peer without a pid is logged with 0.
fn audit_peer(p: &PeerId) -> Value {
    let mut v = peer_json(p);
    if let Some(o) = v.as_object_mut() {
        o.entry("pid").or_insert(json!(0));
    }
    v
}

/// A name from the caller, kept only if the audit log may hold it as is.
fn plain_name(s: &str, max: usize, extra: &[u8]) -> Option<String> {
    (!s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b)))
    .then(|| s.to_ascii_lowercase())
}

fn item_info(view: &ItemView) -> ItemInfo {
    ItemInfo {
        id: view.id,
        tags: view
            .fields
            .keys()
            .filter_map(|k| k.strip_prefix("tags/").map(str::to_owned))
            .collect(),
        hosts: view
            .fields
            .iter()
            .filter(|(k, _)| k.starts_with("allowed_hosts/"))
            .map(|(_, v)| v.as_str().to_owned())
            .collect(),
    }
}

fn title_of(view: &ItemView) -> String {
    let t = view
        .fields
        .get("title")
        .map_or("(untitled)", |t| t.as_str());
    audit_clean_text(t, 80)
}

impl Broker {
    /// Take over an unlocked vault. Writes `broker.start`; fails if the audit log cannot be written.
    pub fn start(
        session: Session,
        audit: AuditWriter,
        approver: Box<dyn Approver>,
        env: Arc<dyn Env>,
        uid: u32,
    ) -> Result<Broker, Code> {
        let mut st = State {
            session: Some(session),
            grants: GrantTable::default(),
            audit,
            denied_until: HashMap::new(),
        };
        st.log(env.unix_ms(), "broker.start", json!({}))?;
        Ok(Broker {
            state: Mutex::new(st),
            gate: Mutex::new(()),
            approver,
            env,
            uid,
            wait: APPROVAL_WAIT,
        })
    }

    #[cfg(test)]
    pub fn with_wait(mut self, wait: Duration) -> Broker {
        self.wait = wait;
        self
    }

    fn lock_state(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock must not turn into a bypass; a poisoned lock still guards the data.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether to serve this connection: only the broker's own user. A refusal is recorded.
    pub fn admit(&self, peer: &PeerId) -> bool {
        if peer.uid == self.uid {
            return true;
        }
        let mut st = self.lock_state();
        let t = self.env.unix_ms();
        let _ = st.log(
            t,
            "conn.refused",
            json!({"peer": audit_peer(peer), "code": Code::NotAllowed.as_str()}),
        );
        false
    }

    pub fn is_locked(&self) -> bool {
        self.lock_state().session.is_none()
    }

    #[cfg(test)]
    pub fn grant_count(&self) -> usize {
        self.lock_state().grants.len()
    }

    /// Serve one request line from an admitted peer.
    pub fn handle_line(&self, peer: &PeerId, line: &str) -> Reply {
        let req = match parse_request(line) {
            Ok(r) => r,
            Err(m) => return Reply::line(err(&m.id, Code::BadRequest)),
        };
        let id = req.id;
        let locked = self.is_locked();
        let result = match req.op {
            Op::Hello => Ok(json!({"version": 1, "locked": locked})),
            Op::Status => Ok(self.status()),
            _ if locked => Err(Code::Locked),
            Op::GrantList => Ok(self.grant_list()),
            Op::GrantRevoke { grant } => self.revoke(&grant).map(|()| json!({})),
            Op::GrantRequest(g) => self.grant_request(peer, g),
            Op::RunEnv { token, item, vars } => self.run_env(&token, &item, &vars),
            Op::HttpRequest(r) => self.http_request(&r),
            Op::Lock => {
                self.lock();
                return Reply {
                    line: ok(&id, json!({})),
                    exit: true,
                };
            }
        };
        Reply::line(match result {
            Ok(members) => ok(&id, members),
            Err(code) => err(&id, code),
        })
    }

    fn status(&self) -> Value {
        let st = self.lock_state();
        json!({
            "locked": st.session.is_none(),
            "grants": st.grants.len(),
            "audit_seq": st.audit.seq(),
            "audit_head": hex::encode(st.audit.head()),
        })
    }

    /// Remove ended grants, recording each. A record that cannot be written does not keep a grant alive.
    fn sweep(&self, st: &mut State) {
        let t = self.env.unix_ms();
        for g in st.grants.sweep(self.env.as_ref()) {
            let _ = st.log(t, "grant.expired", json!({"grant": g.id}));
        }
    }

    fn grant_list(&self) -> Value {
        let mut st = self.lock_state();
        self.sweep(&mut st);
        let grants: Vec<Value> = st
            .grants
            .iter()
            .map(|g| {
                let left = st.grants.remaining(g, self.env.as_ref());
                json!({
                    "id": g.id,
                    "mode": g.mode.as_str(),
                    "scope": g.scope.to_json(),
                    "ttl": g.ttl.as_str(),
                    "expires_in": left.map(|d| d.as_secs()),
                    "peer": peer_json(&g.peer),
                    "label": g.label,
                })
            })
            .collect();
        json!({ "grants": grants })
    }

    fn revoke(&self, id: &str) -> Result<(), Code> {
        let mut st = self.lock_state();
        let g = st.grants.find_id(id).ok_or(Code::NoGrant)?;
        let (mode, scope, ttl) = (g.mode, g.scope.label(), g.ttl);
        let t = self.env.unix_ms();
        st.log(
            t,
            "grant.revoked",
            json!({"grant": id, "mode": mode.as_str(), "scope": scope, "ttl": ttl.as_str()}),
        )?;
        st.grants.revoke(id);
        Ok(())
    }

    /// Forget the vault and every grant. Locking never waits on the audit log.
    fn lock(&self) {
        self.lock_for(None);
    }

    /// [`Broker::lock`], recording why when the broker locked itself.
    fn lock_for(&self, reason: Option<&str>) {
        let mut st = self.lock_state();
        let t = self.env.unix_ms();
        let d = reason.map_or_else(|| json!({}), |r| json!({ "reason": r }));
        let _ = st.log(t, "broker.lock", d);
        st.grants.clear();
        if let Some(s) = st.session.take() {
            s.vault.lock();
        }
    }

    /// Take in what the local folder now holds (a sync pulled it, or another command on this machine wrote
    /// it) so an agent never uses a stale vault (`broker.md` 1).
    ///
    /// New items and devices are applied and grants on items that are gone end. A change to `vault.json`
    /// (a password or Secret Key change, a rotation, a revoked device) cannot be absorbed by an open vault,
    /// and neither can a folder that cannot be read: the broker locks, so every grant ends with it, and a
    /// fresh start is the fresh unlock. Failing closed here is what makes a revocation reach a running broker.
    pub fn absorb(&self) -> Absorbed {
        let mut st = self.lock_state();
        let Some(session) = st.session.as_mut() else {
            return Absorbed::Locked;
        };
        let reason = match session.refresh() {
            Ok(r) if r.settings_changed => "settings_changed",
            Ok(r) => {
                if r.applied > 0 {
                    self.end_grants_on_gone_items(&mut st);
                    return Absorbed::Updated;
                }
                return Absorbed::Same;
            }
            Err(_) => "refresh_failed",
        };
        drop(st);
        self.lock_for(Some(reason));
        Absorbed::Locked
    }

    /// End the grants whose single item was deleted elsewhere. A record that cannot be written does not keep
    /// a grant alive (refusing is always safe).
    fn end_grants_on_gone_items(&self, st: &mut State) {
        let held: Vec<(String, ItemId)> = st
            .grants
            .iter()
            .filter_map(|g| match &g.scope {
                Scope::Item(id) => Some((g.id.clone(), *id)),
                _ => None,
            })
            .collect();
        for (id, item) in held {
            let alive = st
                .vault()
                .is_some_and(|v| v.item(&item).is_ok_and(|view| !view.deleted));
            if alive {
                continue;
            }
            let Some(g) = st.grants.find_id(&id) else {
                continue;
            };
            let d = json!({
                "grant": id, "mode": g.mode.as_str(), "scope": g.scope.label(),
                "ttl": g.ttl.as_str(), "reason": "item_removed",
            });
            let t = self.env.unix_ms();
            let _ = st.log(t, "grant.revoked", d);
            st.grants.revoke(&id);
        }
    }

    /// Record that background sync stopped because the person has to act (`why`: `sign_in`, `lost` or
    /// `foreign`). Only that word is written, so nothing about the vault or the provider leaks. The broker keeps
    /// serving what it holds; the grants are unaffected.
    pub fn sync_failed(&self, why: &str) {
        let mut st = self.lock_state();
        let t = self.env.unix_ms();
        let _ = st.log(t, "sync.failed", json!({ "reason": why }));
    }

    /// The scope in plain words, and loud warnings, written from the vault so the caller cannot word them.
    fn scope_words(
        vault: &mut Vault,
        scope: &Scope,
        mode: Mode,
    ) -> Result<(String, Vec<&'static str>), Code> {
        let mut loud = Vec::new();
        if mode == Mode::Reveal {
            loud.push("THE PROGRAM WILL BE GIVEN THE SECRET VALUE ITSELF AND CAN KEEP IT");
        }
        let words = match scope {
            Scope::Item(id) => {
                let view = vault.item(id).map_err(|_| Code::NotAllowed)?;
                if view.deleted {
                    return Err(Code::NotAllowed);
                }
                if mode == Mode::Http && item_info(&view).hosts.is_empty() {
                    // Nothing could ever be sent anywhere; do not bother the user.
                    return Err(Code::NotAllowed);
                }
                let mut words = format!("the {} \"{}\"", view.item_type.as_str(), title_of(&view));
                if mode == Mode::Http {
                    // Where the secret may go, from the item and not from the caller.
                    let hosts: Vec<String> = item_info(&view)
                        .hosts
                        .iter()
                        .take(5)
                        .map(|h| audit_clean_text(h, 60))
                        .collect();
                    words.push_str(&format!(" (only for {})", hosts.join(", ")));
                }
                words
            }
            Scope::Tag(tag) => {
                let key = format!("tags/{tag}");
                let (items, _) = vault.items(false);
                let titles: Vec<String> = items
                    .iter()
                    .filter(|i| i.fields.contains_key(&key))
                    .map(title_of)
                    .collect();
                if titles.is_empty() {
                    return Err(Code::NotAllowed);
                }
                let shown: Vec<String> =
                    titles.iter().take(5).map(|t| format!("\"{t}\"")).collect();
                let more = titles.len().saturating_sub(5);
                format!(
                    "every item tagged \"{tag}\" ({}): {}{}",
                    titles.len(),
                    shown.join(", "),
                    if more > 0 {
                        format!(" and {more} more")
                    } else {
                        String::new()
                    }
                )
            }
            // Resolved to an item before this is called.
            Scope::Title(_) => return Err(Code::Internal),
            Scope::Vault => {
                let (items, _) = vault.items(false);
                loud.push("THIS COVERS EVERY ITEM IN YOUR VAULT");
                format!("EVERY item in your vault ({} items)", items.len())
            }
        };
        Ok((words, loud))
    }

    /// Turn a title the caller gave into the item it names. No match and several matches are the same
    /// answer, so a caller learns nothing it could not learn by asking the user.
    fn resolve_scope(&self, scope: Scope) -> Result<Scope, Code> {
        let Scope::Title(title) = scope else {
            return Ok(scope);
        };
        let mut st = self.lock_state();
        let vault = st.vault().ok_or(Code::Locked)?;
        let (items, _) = vault.items(false);
        let mut found = items.iter().filter(|i| {
            i.fields
                .get("title")
                .is_some_and(|t| t.as_str().eq_ignore_ascii_case(&title))
        });
        match (found.next(), found.next()) {
            (Some(item), None) => Ok(Scope::Item(item.id)),
            _ => Err(Code::NotAllowed),
        }
    }

    fn busy_locked(&self, st: &State, peer: &PeerId) -> bool {
        let now = self.env.now();
        peer.pid
            .and_then(|p| st.denied_until.get(&p))
            .is_some_and(|until| now < *until)
            || st.grants.len() >= MAX_GRANTS
    }

    fn is_busy(&self, peer: &PeerId) -> bool {
        let mut st = self.lock_state();
        self.sweep(&mut st);
        self.busy_locked(&st, peer)
    }

    fn grant_request(&self, peer: &PeerId, g: GrantRequest) -> Result<Value, Code> {
        if self.is_busy(peer) {
            return Err(Code::Busy);
        }
        let g = GrantRequest {
            scope: self.resolve_scope(g.scope)?,
            ..g
        };
        if g.mode == Mode::Reveal && !(matches!(g.scope, Scope::Item(_)) && g.ttl == Ttl::Once) {
            return Err(Code::BadRequest);
        }
        if g.ttl == Ttl::Session && peer.pid.is_none() {
            return Err(Code::BadRequest);
        }
        let label = g.label.as_deref().map(|l| audit_clean_text(l, 100));
        let reason = g.reason.as_deref().map(|r| audit_clean_text(r, 200));
        let scope_label = g.scope.label();

        let request = {
            let mut st = self.lock_state();
            self.sweep(&mut st);
            if self.busy_locked(&st, peer) {
                return Err(Code::Busy);
            }
            let vault = st.vault().ok_or(Code::Locked)?;
            let (scope_words, loud) = Self::scope_words(vault, &g.scope, g.mode)?;
            let mut d = json!({
                "peer": audit_peer(peer), "scope": scope_label,
                "mode": g.mode.as_str(), "ttl": g.ttl.as_str(),
            });
            if let (Some(o), Some(l)) = (d.as_object_mut(), &label) {
                o.insert("label".into(), json!(l));
            }
            if let (Some(o), Some(r)) = (d.as_object_mut(), &reason) {
                o.insert("reason".into(), json!(r));
            }
            let t = self.env.unix_ms();
            st.log(t, "grant.requested", d)?;
            ApprovalRequest {
                peer: peer.clone(),
                label: label.clone(),
                reason,
                scope_words,
                mode: g.mode,
                ttl: g.ttl,
                loud,
            }
        };

        // The state lock is not held while the user decides; only one prompt is shown at a time.
        let decision = {
            let _one_at_a_time = self.gate.lock().unwrap_or_else(|e| e.into_inner());
            self.approver.approve(&request, self.wait)
        };

        let mut st = self.lock_state();
        if st.session.is_none() {
            return Err(Code::Locked);
        }
        let t = self.env.unix_ms();
        if decision == Decision::Deny {
            if let Some(pid) = peer.pid {
                let until = self.env.now() + DENY_COOLDOWN;
                st.denied_until.insert(pid, until);
            }
            let _ = st.log(
                t,
                "grant.denied",
                json!({
                    "peer": audit_peer(peer), "scope": scope_label, "mode": g.mode.as_str(),
                    "ttl": g.ttl.as_str(), "code": Code::Denied.as_str(),
                }),
            );
            return Err(Code::Denied);
        }
        // Created first so its id can be logged; nobody can see it until this function returns, so if the
        // record cannot be written the grant is removed again and never existed.
        let (id, token) = st.grants.issue(
            self.env.as_ref(),
            g.scope.clone(),
            g.mode,
            g.ttl,
            peer.clone(),
            label,
        )?;
        let logged = st.log(
            t,
            "grant.approved",
            json!({
                "peer": audit_peer(peer), "grant": id, "scope": scope_label,
                "mode": g.mode.as_str(), "ttl": g.ttl.as_str(),
            }),
        );
        if let Err(code) = logged {
            st.grants.revoke(&id);
            return Err(code);
        }
        let left = st
            .grants
            .find_id(&id)
            .and_then(|gr| st.grants.remaining(gr, self.env.as_ref()));
        Ok(json!({"grant": {
            "id": id, "token": token, "mode": g.mode.as_str(), "scope": g.scope.to_json(),
            "ttl": g.ttl.as_str(), "expires_in": left.map(|d| d.as_secs()),
        }}))
    }

    /// The one check every use of a grant goes through (`broker.md` 3.3), then a record. On success the
    /// grant (and a `once` grant is consumed) covers `item` in `mode`, and for `http` also `host`.
    #[allow(
        dead_code,
        reason = "called by `figlock http` (FIG-37); covered by tests"
    )]
    pub fn authorize(
        &self,
        token: &str,
        item: &ItemId,
        mode: Mode,
        host: Option<&str>,
    ) -> Result<Authorized, Code> {
        self.use_grant(token, item, mode, host, &[], false)
            .map(|(a, _)| a)
    }

    /// [`authorize`](Self::authorize), then read one field of the item. This is how later modes get a
    /// value: only inside the broker's process, and only after the check and the record.
    #[allow(
        dead_code,
        reason = "called by `figlock http` (FIG-37); covered by tests"
    )]
    pub fn authorize_field(
        &self,
        token: &str,
        item: &ItemId,
        mode: Mode,
        host: Option<&str>,
        field: &str,
    ) -> Result<Zeroizing<String>, Code> {
        self.use_grant(token, item, mode, host, &[field], false)
            .and_then(|(_, mut v)| v.pop().ok_or(Code::Internal))
    }

    /// `run.env`: the variables a program started by the caller gets, after the one check for all of them
    /// (so a `once` grant covers the whole set, not its first variable).
    fn run_env(
        &self,
        token: &str,
        item: &ItemId,
        vars: &[(String, String)],
    ) -> Result<Value, Code> {
        let pairs = if vars.is_empty() {
            self.default_vars(item)
        } else {
            vars.to_vec()
        };
        let fields: Vec<&str> = pairs.iter().map(|(_, f)| f.as_str()).collect();
        // With nothing to give, the use is refused (and recorded) like any other that finds no field.
        let (_, values) =
            self.use_grant(token, item, Mode::Run, None, &fields, pairs.is_empty())?;
        let mut env = serde_json::Map::new();
        for ((name, _), value) in pairs.iter().zip(values) {
            env.insert(name.clone(), json!(value.as_str()));
        }
        Ok(json!({ "env": env }))
    }

    /// `http.request` (`broker.md` 3.5): make the request for the caller with the secret added, never
    /// returning the secret.
    fn http_request(&self, r: &HttpRequest) -> Result<Value, Code> {
        let url = Url::parse(&r.url).map_err(|_| Code::BadRequest)?;
        if !http::scheme_ok(&url) || !url.username().is_empty() || url.password().is_some() {
            return Err(Code::BadRequest);
        }
        let host = url.host_str().ok_or(Code::BadRequest)?.to_owned();
        let (auth, mut values) = self.use_grant(
            &r.token,
            &r.item,
            Mode::Http,
            Some(&host),
            &[r.field.as_str()],
            false,
        )?;
        let secret = values.pop().ok_or(Code::Internal)?;
        let value = Zeroizing::new(if r.auth_scheme.is_empty() {
            secret.as_str().to_owned()
        } else {
            format!("{} {}", r.auth_scheme, secret.as_str())
        });
        let out = http::Outgoing {
            method: r.method.clone(),
            url,
            headers: r.headers.clone(),
            body: r.body.as_ref().map(|b| b.clone().into_bytes()),
            auth_header: r.auth_header.clone(),
        };
        let mut allow = |h: &str| self.allow_hop(&r.item, &auth.grant_id, h);
        let got = http::fetch(&out, &value, &mut allow).map_err(|()| Code::Upstream)?;
        // A server may echo the secret (a debug endpoint, an error page): it does not get to hand it back.
        let name = plain_name(&r.field, 64, b"_/.-").unwrap_or_else(|| "secret".to_owned());
        let secrets = [(name, secret)];
        let clean = |bytes: &[u8]| {
            let mut s = Scrubber::new(&secrets);
            let mut o = s.feed(bytes);
            o.extend(s.finish());
            o
        };
        let headers: serde_json::Map<String, Value> = got
            .headers
            .iter()
            .map(|(n, v)| {
                let v = String::from_utf8_lossy(&clean(v.as_bytes())).into_owned();
                (n.clone(), json!(v))
            })
            .collect();
        Ok(json!({
            "status": got.status,
            "headers": headers,
            "body_b64": crate::b64::encode(&clean(&got.body)),
            "truncated": got.truncated,
        }))
    }

    /// May the secret go to `host` on a redirect? The item's allow-list decides, and the answer is recorded
    /// first; a record that cannot be written is a no.
    fn allow_hop(&self, item: &ItemId, grant: &str, host: &str) -> bool {
        let mut st = self.lock_state();
        let t = self.env.unix_ms();
        let allowed = st
            .vault()
            .and_then(|v| v.item(item).ok())
            .filter(|v| !v.deleted)
            .is_some_and(|v| grants::host_allowed(&item_info(&v).hosts, host));
        let mut d = json!({"item": hex::encode(item), "mode": "http", "grant": grant});
        if let (Some(o), Some(h)) = (d.as_object_mut(), plain_name(host, 253, b".-*_")) {
            o.insert("host".into(), json!(h));
        }
        if allowed {
            st.log(t, "use.allowed", d).is_ok()
        } else {
            if let Some(o) = d.as_object_mut() {
                o.insert("code".into(), json!(Code::NotAllowed.as_str()));
            }
            let _ = st.log(t, "use.refused", d);
            false
        }
    }

    /// What an item hands a program when the caller names no variables. Names only, no values.
    fn default_vars(&self, item: &ItemId) -> Vec<(String, String)> {
        let mut st = self.lock_state();
        let Some(vault) = st.vault() else {
            return Vec::new();
        };
        let Ok(view) = vault.item(item) else {
            return Vec::new();
        };
        if view.deleted {
            return Vec::new();
        }
        match view.item_type {
            ItemType::EnvBundle => view
                .fields
                .keys()
                .filter_map(|k| Some((k.strip_prefix("env/")?.to_owned(), k.clone())))
                .filter(|(name, _)| env_name(name))
                .take(MAX_VARS)
                .collect(),
            ItemType::Token => view
                .fields
                .get("env_var")
                .map(|v| v.as_str().to_owned())
                .filter(|n| env_name(n))
                .map(|n| vec![(n, "token".to_owned())])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// One check and one record per field read. `fields` are read only if the item has all of them;
    /// `unusable` forces a refusal (nothing to give).
    fn use_grant(
        &self,
        token: &str,
        item: &ItemId,
        mode: Mode,
        host: Option<&str>,
        fields: &[&str],
        unusable: bool,
    ) -> Result<(Authorized, Vec<Zeroizing<String>>), Code> {
        let mut st = self.lock_state();
        self.sweep(&mut st);
        let t = self.env.unix_ms();
        let mut detail = json!({"item": hex::encode(item), "mode": mode.as_str()});
        if let (Some(o), Some(h)) = (
            detail.as_object_mut(),
            host.and_then(|h| plain_name(h, 253, b".-*_")),
        ) {
            o.insert("host".into(), json!(h));
        }
        let field_name = |f: &str| plain_name(f, 64, b"_/.-");
        let refuse = |st: &mut State, code: Code, grant: Option<String>, mut d: Value| {
            if let Some(o) = d.as_object_mut() {
                o.insert("code".into(), json!(code.as_str()));
                if let Some(g) = grant {
                    o.insert("grant".into(), json!(g));
                }
                if let [only] = fields
                    && let Some(f) = field_name(only)
                {
                    o.insert("field".into(), json!(f));
                }
            }
            // Refusing is always safe, so a record that cannot be written does not change the answer.
            let _ = st.log(t, "use.refused", d);
            code
        };

        let Some(vault) = st.vault() else {
            return Err(Code::Locked);
        };
        let view = vault.item(item).ok().filter(|v| !v.deleted);
        let info = view.as_ref().map_or_else(
            || ItemInfo {
                id: *item,
                tags: Default::default(),
                hosts: Vec::new(),
            },
            item_info,
        );
        let values: Option<Vec<Zeroizing<String>>> = view.as_ref().and_then(|v| {
            fields
                .iter()
                .map(|f| {
                    v.fields
                        .get(*f)
                        .map(|val| Zeroizing::new(val.as_str().to_owned()))
                })
                .collect()
        });
        let usable = view.is_some() && values.is_some() && !unusable;
        let authorized = st
            .grants
            .authorize(self.env.as_ref(), token, &info, mode, host, usable);
        let auth = match authorized {
            Ok(a) => a,
            Err(r) => return Err(refuse(&mut st, r.code, r.grant_id, detail)),
        };
        if let Some(o) = detail.as_object_mut() {
            o.insert("grant".into(), json!(auth.grant_id));
        }
        // Written before any value leaves this function: one record for each field read.
        if fields.is_empty() {
            st.log(t, "use.allowed", detail.clone())?;
        }
        for f in fields {
            let mut d = detail.clone();
            if let (Some(o), Some(name)) = (d.as_object_mut(), field_name(f)) {
                o.insert("field".into(), json!(name));
            }
            st.log(t, "use.allowed", d)?;
        }
        Ok((auth, values.unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests;
