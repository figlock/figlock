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
pub mod peer;
pub mod proto;
pub mod server;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use figlock_core::{ItemId, ItemType, ItemView, Vault, audit_clean_text};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use approve::{APPROVAL_WAIT, ApprovalRequest, Approver, Decision};
use audit::AuditWriter;
use grants::{Authorized, Env, GrantTable, ItemInfo, MAX_GRANTS};
use peer::PeerId;
use proto::{Code, GrantRequest, MAX_VARS, Mode, Op, Scope, Ttl, env_name, err, ok, parse_request};

/// After a denial, the same process is told `busy` for this long (approval fatigue, G8).
const DENY_COOLDOWN: Duration = Duration::from_secs(5);

struct State {
    /// `None` once locked: the vault and its keys are dropped.
    vault: Option<Vault>,
    grants: GrantTable,
    audit: AuditWriter,
    denied_until: HashMap<u32, Duration>,
}

impl State {
    /// Write one audit record. Fails closed with `audit_failed`.
    fn log(&mut self, t: u64, ev: &str, d: Value) -> Result<(), Code> {
        let vault = self.vault.as_ref().ok_or(Code::Locked)?;
        self.audit
            .append(vault, t, ev, d)
            .map_err(|()| Code::AuditFailed)
    }
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
        vault: Vault,
        audit: AuditWriter,
        approver: Box<dyn Approver>,
        env: Arc<dyn Env>,
        uid: u32,
    ) -> Result<Broker, Code> {
        let mut st = State {
            vault: Some(vault),
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
        self.lock_state().vault.is_none()
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
            "locked": st.vault.is_none(),
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
        let mut st = self.lock_state();
        let t = self.env.unix_ms();
        let _ = st.log(t, "broker.lock", json!({}));
        st.grants.clear();
        if let Some(v) = st.vault.take() {
            v.lock();
        }
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
                format!("the {} \"{}\"", view.item_type.as_str(), title_of(&view))
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
        let vault = st.vault.as_mut().ok_or(Code::Locked)?;
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
            let vault = st.vault.as_mut().ok_or(Code::Locked)?;
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
        if st.vault.is_none() {
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

    /// What an item hands a program when the caller names no variables. Names only, no values.
    fn default_vars(&self, item: &ItemId) -> Vec<(String, String)> {
        let mut st = self.lock_state();
        let Some(vault) = st.vault.as_mut() else {
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

        let Some(vault) = st.vault.as_mut() else {
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
