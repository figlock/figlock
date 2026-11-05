//! Broker behaviour tests (FIG-34). Every credential and secret is generated per run.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use figlock_core::{ItemType, KdfParams, ZERO_PREV};
use serde_json::{Value, json};

use super::approve::{ApprovalRequest, Approver, Decision, describe};
use super::audit::{AuditWriter, Sink};
use super::grants::Env;
use super::grants::testenv::TestEnv;
use super::peer::PeerId;
use super::proto::{Code, Mode};
use super::*;

const T: u64 = 1_700_000_000_000;
/// The real user id: a test that goes over a real socket is only served when it matches.
fn uid() -> u32 {
    peer::my_uid()
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).unwrap();
    hex::encode(b)
}

/// Answers from a script and remembers what it was shown.
#[derive(Default)]
struct Scripted {
    answers: Mutex<VecDeque<Decision>>,
    shown: Mutex<Vec<String>>,
    calls: AtomicUsize,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    pause: Mutex<Duration>,
    /// Runs while the user is "deciding".
    hook: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Scripted {
    fn will(&self, d: &[Decision]) {
        self.answers.lock().unwrap().extend(d.iter().copied());
    }
}

struct Shared(Arc<Scripted>);

impl Approver for Shared {
    fn approve(&self, req: &ApprovalRequest, _: Duration) -> Decision {
        let s = &self.0;
        s.calls.fetch_add(1, Ordering::SeqCst);
        let now = s.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        s.max_in_flight.fetch_max(now, Ordering::SeqCst);
        s.shown.lock().unwrap().push(describe(req));
        std::thread::sleep(*s.pause.lock().unwrap());
        if let Some(hook) = s.hook.lock().unwrap().as_ref() {
            hook();
        }
        s.in_flight.fetch_sub(1, Ordering::SeqCst);
        s.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Decision::Deny)
    }
}

/// A sink that keeps the bytes and can be made to fail.
#[derive(Clone, Default)]
struct Mem {
    bytes: Arc<Mutex<Vec<u8>>>,
    fail: Arc<AtomicBool>,
}

impl Sink for Mem {
    fn append(&mut self, b: &[u8]) -> io::Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(io::Error::other("disk full"));
        }
        self.bytes.lock().unwrap().extend_from_slice(b);
        Ok(())
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.bytes
            .lock()
            .unwrap()
            .truncate(usize::try_from(len).unwrap());
        Ok(())
    }
}

struct Fx {
    broker: Arc<Broker>,
    approver: Arc<Scripted>,
    env: Arc<TestEnv>,
    log: Mem,
    /// A copy of the vault, to read the audit log back.
    reader: figlock_core::Vault,
    github: ItemId,
    github_secret: String,
    login: ItemId,
    login_secret: String,
    no_hosts: ItemId,
    bundle: ItemId,
    bundle_a: String,
    bundle_b: String,
}

fn peer(pid: u32) -> PeerId {
    PeerId {
        uid: uid(),
        pid: Some(pid),
        exe: Some("/usr/bin/example-agent".into()),
        parents: Vec::new(),
    }
}

fn fixture() -> Fx {
    let pw = random_hex(8);
    let (mut vault, created) =
        figlock_core::Vault::create(&pw, "Laptop", KdfParams::FLOOR, T).unwrap();
    let github_secret = random_hex(16);
    let login_secret = random_hex(16);
    let github = vault
        .add_item(
            ItemType::Token,
            "personal",
            &[
                ("title", "GitHub"),
                ("token", github_secret.as_str()),
                ("env_var", "GH_TOKEN"),
                ("allowed_hosts/ab12", "api.github.com"),
                ("allowed_hosts/cd34", "*.githubusercontent.com"),
                ("tags/work", "1"),
            ],
            T,
        )
        .unwrap();
    let login = vault
        .add_item(
            ItemType::Login,
            "personal",
            &[
                ("title", "Example"),
                ("password", login_secret.as_str()),
                ("tags/home", "1"),
            ],
            T,
        )
        .unwrap();
    let no_hosts = vault
        .add_item(
            ItemType::Token,
            "personal",
            &[("title", "Orphan"), ("token", "x"), ("tags/work", "1")],
            T,
        )
        .unwrap();
    let (bundle_a, bundle_b) = (random_hex(12), random_hex(12));
    let bundle = vault
        .add_item(
            ItemType::EnvBundle,
            "personal",
            &[
                ("title", "Deploy env"),
                ("env/DB_URL", bundle_a.as_str()),
                ("env/STRIPE_KEY", bundle_b.as_str()),
            ],
            T,
        )
        .unwrap();
    // A second handle on the same vault for reading the log: unlock with the same credentials and device.
    let (reader, _) = figlock_core::Vault::unlock(
        &created.vault_json,
        &pw,
        &figlock_core::SecretKey::parse(&created.secret_key.format()).unwrap(),
        clone_device(&vault),
        "Laptop",
        std::slice::from_ref(&created.device_record),
        T + 1,
    )
    .unwrap();

    let approver = Arc::new(Scripted::default());
    let env = Arc::new(TestEnv::new());
    env.alive.lock().unwrap().extend([4242, 4243]);
    let log = Mem::default();
    let writer = AuditWriter::with_sink(Box::new(log.clone()), 0, 0, ZERO_PREV);
    let broker = Arc::new(
        Broker::start(
            vault,
            writer,
            Box::new(Shared(approver.clone())),
            env.clone(),
            uid(),
        )
        .unwrap()
        .with_wait(Duration::from_secs(1)),
    );
    Fx {
        broker,
        approver,
        env,
        log,
        reader,
        github,
        github_secret,
        login,
        login_secret,
        no_hosts,
        bundle,
        bundle_a,
        bundle_b,
    }
}

fn clone_device(v: &figlock_core::Vault) -> figlock_core::DeviceKeys {
    let d = v.device_keys();
    figlock_core::DeviceKeys {
        device_id: d.device_id,
        sign_seed: figlock_core::Key::from_bytes(*d.sign_seed.expose()),
        agree_priv: figlock_core::Key::from_bytes(*d.agree_priv.expose()),
    }
}

impl Fx {
    fn send(&self, p: &PeerId, v: Value) -> Value {
        let reply = self.broker.handle_line(p, &v.to_string());
        serde_json::from_str(&reply.line).unwrap()
    }

    fn request(&self, p: &PeerId, scope: Value, mode: &str, ttl: &str) -> Value {
        self.send(
            p,
            json!({"v":1,"id":"r","op":"grant.request","scope":scope,"mode":mode,"ttl":ttl,
                   "label":"example agent","reason":"deploy"}),
        )
    }

    /// Approve once and return the token.
    fn grant(&self, p: &PeerId, scope: Value, mode: &str, ttl: &str) -> String {
        self.approver.will(&[Decision::Allow]);
        let r = self.request(p, scope, mode, ttl);
        assert_eq!(r["ok"], true, "{r}");
        r["grant"]["token"].as_str().unwrap().to_owned()
    }

    fn events(&self) -> Vec<(String, Map)> {
        let (chain, damaged) = self.reader.read_audit_log(&self.log.bytes.lock().unwrap());
        assert!(!damaged && chain.first_bad.is_none(), "the log must verify");
        chain.records.into_iter().map(|r| (r.ev, r.d)).collect()
    }

    fn names(&self) -> Vec<String> {
        self.events().into_iter().map(|(e, _)| e).collect()
    }

    fn code(r: &Value) -> &str {
        r["error"]["code"].as_str().unwrap_or("")
    }
}

type Map = serde_json::Map<String, Value>;

fn item_scope(id: &ItemId) -> Value {
    json!({"item": hex::encode(id)})
}

#[test]
fn an_approved_http_grant_lets_the_right_host_through_and_nothing_else() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "http", "15m");
    assert_eq!(token.len(), 64);

    let a = fx
        .broker
        .authorize(&token, &fx.github, Mode::Http, Some("api.github.com"))
        .unwrap();
    assert_eq!(a.grant_id.len(), 16);
    assert!(
        fx.broker
            .authorize(
                &token,
                &fx.github,
                Mode::Http,
                Some("RAW.githubusercontent.com")
            )
            .is_ok()
    );
    for (host, why) in [
        (Some("evil.example"), "host not on the allow-list"),
        (Some("githubusercontent.com"), "wildcard needs a label"),
        (Some("api.github.com.evil.net"), "suffix trick"),
        (None, "no host"),
    ] {
        assert_eq!(
            fx.broker
                .authorize(&token, &fx.github, Mode::Http, host)
                .err(),
            Some(Code::NotAllowed),
            "{why}"
        );
    }
    assert_eq!(
        fx.broker
            .authorize(&token, &fx.login, Mode::Http, Some("api.github.com"))
            .err(),
        Some(Code::NotAllowed),
        "another item"
    );
    assert_eq!(
        fx.broker
            .authorize(&token, &fx.github, Mode::Run, None)
            .err(),
        Some(Code::NotAllowed),
        "another mode"
    );
    let names = fx.names();
    assert_eq!(
        &names[..3],
        ["broker.start", "grant.requested", "grant.approved"]
    );
    assert_eq!(names.iter().filter(|n| *n == "use.allowed").count(), 2);
    assert_eq!(names.iter().filter(|n| *n == "use.refused").count(), 6);
}

#[test]
fn a_request_without_a_valid_grant_is_refused_and_recorded() {
    let fx = fixture();
    for bad in [random_hex(32), "short".to_owned(), String::new()] {
        assert_eq!(
            fx.broker
                .authorize(&bad, &fx.github, Mode::Http, Some("api.github.com"))
                .err(),
            Some(Code::NoGrant)
        );
    }
    let refused: Vec<_> = fx
        .events()
        .into_iter()
        .filter(|(e, _)| e == "use.refused")
        .collect();
    assert_eq!(refused.len(), 3);
    assert!(refused.iter().all(|(_, d)| d["code"] == "no_grant"));
}

#[test]
fn a_denied_request_gets_no_grant_and_a_cooldown() {
    let fx = fixture();
    let p = peer(4242);
    let r = fx.request(&p, item_scope(&fx.github), "http", "once");
    assert_eq!(Fx::code(&r), "denied");
    assert_eq!(fx.broker.grant_count(), 0);
    // Straight away again: not even a prompt.
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    let r = fx.request(&p, item_scope(&fx.github), "http", "once");
    assert_eq!(Fx::code(&r), "busy");
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), calls);
    // Another process is not held back, and the first is free again after the cooldown.
    fx.approver.will(&[Decision::Allow]);
    assert_eq!(
        fx.request(&peer(4243), item_scope(&fx.github), "http", "once")["ok"],
        true
    );
    fx.env.advance(Duration::from_secs(6));
    fx.approver.will(&[Decision::Allow]);
    assert_eq!(
        fx.request(&p, item_scope(&fx.github), "http", "once")["ok"],
        true
    );
    assert!(fx.names().contains(&"grant.denied".to_owned()));
}

#[test]
fn nobody_to_ask_means_no() {
    let (vault, _) = figlock_core::Vault::create(&random_hex(8), "L", KdfParams::FLOOR, T).unwrap();
    let writer = AuditWriter::with_sink(Box::new(Mem::default()), 0, 0, ZERO_PREV);
    let broker = Broker::start(
        vault,
        writer,
        Box::new(approve::DenyAll),
        Arc::new(TestEnv::new()),
        uid(),
    )
    .unwrap();
    let r = broker.handle_line(
        &peer(1),
        r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":true},"mode":"run","ttl":"once"}"#,
    );
    let v: Value = serde_json::from_str(&r.line).unwrap();
    assert_eq!(Fx::code(&v), "denied");
}

#[test]
fn reveal_is_one_item_once_and_needs_a_fresh_approval_every_time() {
    let fx = fixture();
    let p = peer(4242);
    for (scope, ttl) in [
        (item_scope(&fx.github), "15m"),
        (item_scope(&fx.github), "session"),
        (item_scope(&fx.github), "until-lock"),
        (json!({"tag":"work"}), "once"),
        (json!({"vault":true}), "once"),
    ] {
        let r = fx.request(&p, scope.clone(), "reveal", ttl);
        assert_eq!(Fx::code(&r), "bad_request", "{scope} {ttl}");
    }
    assert_eq!(
        fx.approver.calls.load(Ordering::SeqCst),
        0,
        "never even asked"
    );

    let token = fx.grant(&p, item_scope(&fx.github), "reveal", "once");
    let v = fx
        .broker
        .authorize_field(&token, &fx.github, Mode::Reveal, None, "token")
        .unwrap();
    assert_eq!(v.as_str(), fx.github_secret);
    assert_eq!(
        fx.broker
            .authorize_field(&token, &fx.github, Mode::Reveal, None, "token")
            .err(),
        Some(Code::NoGrant),
        "a second reveal with the same grant"
    );
    // A second reveal needs a second approval.
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    let r = fx.request(&p, item_scope(&fx.github), "reveal", "once");
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), calls + 1);
    assert_eq!(Fx::code(&r), "denied");
    let shown = fx.approver.shown.lock().unwrap();
    assert!(shown[0].contains("!! THE PROGRAM WILL BE GIVEN THE SECRET VALUE ITSELF"));
}

#[test]
fn time_limits_are_enforced_by_the_broker() {
    let fx = fixture();
    let p = peer(4242);
    let m15 = fx.grant(&p, item_scope(&fx.github), "run", "15m");
    let once = fx.grant(&p, item_scope(&fx.github), "run", "once");
    let day = fx.grant(&p, item_scope(&fx.github), "run", "1d");
    let lock = fx.grant(&p, item_scope(&fx.github), "run", "until-lock");
    let session = fx.grant(&p, item_scope(&fx.github), "run", "session");
    let use_it = |t: &str| fx.broker.authorize(t, &fx.github, Mode::Run, None);
    fx.env.advance(Duration::from_secs(16 * 60));
    assert_eq!(use_it(&m15).err(), Some(Code::Expired));
    assert_eq!(
        use_it(&once).err(),
        Some(Code::Expired),
        "an unused once ends too"
    );
    assert!(use_it(&day).is_ok());
    assert!(use_it(&session).is_ok());
    fx.env.alive.lock().unwrap().remove(&4242);
    assert_eq!(
        use_it(&session).err(),
        Some(Code::Expired),
        "its process is gone"
    );
    fx.env.advance(Duration::from_secs(25 * 3600));
    assert_eq!(use_it(&day).err(), Some(Code::Expired));
    assert!(use_it(&lock).is_ok());
    let refused: Vec<_> = fx
        .events()
        .into_iter()
        .filter(|(e, _)| e == "use.refused")
        .collect();
    assert!(
        refused
            .iter()
            .filter(|(_, d)| d["code"] == "expired")
            .count()
            >= 4
    );
}

#[test]
fn a_session_grant_needs_a_known_process() {
    let fx = fixture();
    let nameless = PeerId {
        uid: uid(),
        pid: None,
        exe: None,
        parents: Vec::new(),
    };
    let r = fx.request(&nameless, item_scope(&fx.github), "run", "session");
    assert_eq!(Fx::code(&r), "bad_request");
}

#[test]
fn tag_and_vault_scopes_cover_what_they_say() {
    let fx = fixture();
    let p = peer(4242);
    let work = fx.grant(&p, json!({"tag":"work"}), "run", "15m");
    assert!(
        fx.broker
            .authorize(&work, &fx.github, Mode::Run, None)
            .is_ok()
    );
    assert!(
        fx.broker
            .authorize(&work, &fx.no_hosts, Mode::Run, None)
            .is_ok()
    );
    assert_eq!(
        fx.broker.authorize(&work, &fx.login, Mode::Run, None).err(),
        Some(Code::NotAllowed)
    );
    let all = fx.grant(&p, json!({"vault":true}), "run", "15m");
    assert!(
        fx.broker
            .authorize(&all, &fx.login, Mode::Run, None)
            .is_ok()
    );
    let shown = fx.approver.shown.lock().unwrap().join("\n");
    assert!(shown.contains("every item tagged \"work\" (2)"));
    assert!(shown.contains("!! THIS COVERS EVERY ITEM IN YOUR VAULT"));
    drop(shown);

    // A tag nobody has, an unknown item and an http grant that could never reach a host: never asked.
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    for (scope, mode) in [
        (json!({"tag":"nothing"}), "run"),
        (item_scope(&[9; 16]), "run"),
        (item_scope(&fx.no_hosts), "http"),
        (item_scope(&fx.login), "http"),
    ] {
        let r = fx.request(&p, scope.clone(), mode, "once");
        assert_eq!(Fx::code(&r), "not_allowed", "{scope} {mode}");
    }
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), calls);
}

#[test]
fn another_user_is_turned_away_and_recorded() {
    let fx = fixture();
    let stranger = PeerId {
        uid: uid() + 1,
        pid: Some(1),
        exe: None,
        parents: Vec::new(),
    };
    assert!(!fx.broker.admit(&stranger));
    assert!(fx.broker.admit(&peer(4242)));
    let events = fx.events();
    let refused = events
        .iter()
        .find(|(e, _)| e == "conn.refused")
        .expect("recorded");
    assert_eq!(refused.1["peer"]["uid"], uid() + 1);
}

#[test]
fn a_spoofed_label_changes_nothing_about_who_is_shown() {
    let fx = fixture();
    fx.approver.will(&[Decision::Allow]);
    let r = fx.send(
        &peer(4242),
        json!({"v":1,"id":"s","op":"grant.request","scope":item_scope(&fx.github),"mode":"run",
               "ttl":"once","label":"Claude Code (Anthropic)\u{1b}[2J","reason":"x\nprogram   /bin/login"}),
    );
    assert_eq!(r["ok"], true);
    let shown = fx.approver.shown.lock().unwrap()[0].clone();
    assert!(
        shown.contains("/usr/bin/example-agent"),
        "the OS-reported program is shown"
    );
    assert!(shown.contains("[checked by the system]"));
    assert!(shown.contains("[not checked]"));
    assert!(!shown.contains('\u{1b}'));
    assert_eq!(shown.matches("\n  program ").count(), 1);
    let events = fx.events();
    let requested = events.iter().find(|(e, _)| e == "grant.requested").unwrap();
    assert_eq!(requested.1["peer"]["exe"], "/usr/bin/example-agent");
    assert!(!requested.1["label"].as_str().unwrap().contains('\u{1b}'));
}

#[test]
fn no_secret_value_reaches_the_audit_log_a_prompt_or_a_response() {
    let fx = fixture();
    let p = peer(4242);
    let mut seen = String::new();
    for (scope, mode, ttl) in [
        (item_scope(&fx.github), "http", "once"),
        (item_scope(&fx.login), "reveal", "once"),
        (json!({"vault":true}), "run", "15m"),
    ] {
        fx.approver.will(&[Decision::Allow]);
        let r = fx.request(&p, scope, mode, ttl);
        seen.push_str(&r.to_string());
    }
    let reveal = fx.grant(&p, item_scope(&fx.login), "reveal", "once");
    let got = fx
        .broker
        .authorize_field(&reveal, &fx.login, Mode::Reveal, None, "password")
        .unwrap();
    assert_eq!(
        got.as_str(),
        fx.login_secret,
        "the broker itself can read it"
    );
    let _ = fx.broker.authorize(&reveal, &fx.login, Mode::Reveal, None);
    seen.push_str(
        &fx.send(&p, json!({"v":1,"id":"l","op":"grant.list"}))
            .to_string(),
    );
    seen.push_str(
        &fx.send(&p, json!({"v":1,"id":"l","op":"status"}))
            .to_string(),
    );
    seen.push_str(&fx.approver.shown.lock().unwrap().join("\n"));
    for (e, d) in fx.events() {
        seen.push_str(&format!("{e} {d:?}"));
    }
    seen.push_str(&String::from_utf8_lossy(&fx.log.bytes.lock().unwrap()));
    for secret in [&fx.github_secret, &fx.login_secret] {
        assert!(!seen.contains(secret.as_str()), "a secret value leaked");
    }
}

#[test]
fn listing_shows_grants_without_tokens_and_revoking_ends_one() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "run", "15m");
    let list = fx.send(&p, json!({"v":1,"id":"l","op":"grant.list"}));
    let g = &list["grants"][0];
    assert_eq!(g["mode"], "run");
    assert_eq!(g["expires_in"], 900);
    assert_eq!(g["peer"]["pid"], 4242);
    assert!(!list.to_string().contains(&token));
    let id = g["id"].as_str().unwrap().to_owned();

    let r = fx.send(
        &p,
        json!({"v":1,"id":"x","op":"grant.revoke","grant":"0123456789abcdef"}),
    );
    assert_eq!(Fx::code(&r), "no_grant");
    let r = fx.send(&p, json!({"v":1,"id":"x","op":"grant.revoke","grant":id}));
    assert_eq!(r["ok"], true);
    assert_eq!(
        fx.broker
            .authorize(&token, &fx.github, Mode::Run, None)
            .err(),
        Some(Code::NoGrant)
    );
    assert!(fx.names().contains(&"grant.revoked".to_owned()));
}

#[test]
fn locking_forgets_everything_and_stops_the_server() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "run", "until-lock");
    let reply = fx.broker.handle_line(&p, r#"{"v":1,"id":"k","op":"lock"}"#);
    assert!(reply.exit);
    assert!(fx.broker.is_locked());
    assert_eq!(fx.broker.grant_count(), 0);
    assert_eq!(fx.names().last().map(String::as_str), Some("broker.lock"));
    assert_eq!(
        fx.broker
            .authorize(&token, &fx.github, Mode::Run, None)
            .err(),
        Some(Code::Locked)
    );
    let r = fx.request(&p, item_scope(&fx.github), "run", "once");
    assert_eq!(Fx::code(&r), "locked");
    let hello = fx.send(&p, json!({"v":1,"id":"h","op":"hello"}));
    assert_eq!(hello["locked"], true);
}

#[test]
fn if_the_audit_log_cannot_be_written_nothing_is_granted_and_nothing_is_released() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "reveal", "once");
    let other = fx.grant(&p, item_scope(&fx.login), "run", "15m");

    fx.log.fail.store(true, Ordering::SeqCst);
    // A new grant is not created.
    fx.approver.will(&[Decision::Allow]);
    let r = fx.request(&p, item_scope(&fx.github), "run", "once");
    assert_eq!(Fx::code(&r), "audit_failed");
    assert_eq!(fx.broker.grant_count(), 2, "nothing was created");
    // A use is not allowed and the value is not returned.
    assert_eq!(
        fx.broker
            .authorize_field(&token, &fx.github, Mode::Reveal, None, "token")
            .err(),
        Some(Code::AuditFailed)
    );
    // A refusal is still a refusal.
    assert_eq!(
        fx.broker
            .authorize(&other, &fx.github, Mode::Run, None)
            .err(),
        Some(Code::NotAllowed)
    );
    // Revoking is not done silently either.
    let id = fx
        .broker
        .lock_state()
        .grants
        .iter()
        .next()
        .unwrap()
        .id
        .clone();
    let r = fx.send(&p, json!({"v":1,"id":"x","op":"grant.revoke","grant":id}));
    assert_eq!(Fx::code(&r), "audit_failed");
    assert_eq!(fx.broker.grant_count(), 1);
    // But locking always works.
    fx.broker.handle_line(&p, r#"{"v":1,"id":"k","op":"lock"}"#);
    assert!(fx.broker.is_locked());
}

#[test]
fn a_log_that_fails_after_the_user_said_yes_leaves_no_grant_behind() {
    let fx = fixture();
    let fail = fx.log.fail.clone();
    *fx.approver.hook.lock().unwrap() = Some(Box::new(move || fail.store(true, Ordering::SeqCst)));
    fx.approver.will(&[Decision::Allow]);
    let r = fx.request(&peer(4242), item_scope(&fx.github), "run", "until-lock");
    assert_eq!(Fx::code(&r), "audit_failed");
    assert_eq!(
        fx.broker.grant_count(),
        0,
        "approved but unrecorded means never granted"
    );
    assert!(!r.to_string().contains("token"));
}

#[test]
fn only_one_prompt_is_shown_at_a_time() {
    let fx = Arc::new(fixture());
    *fx.approver.pause.lock().unwrap() = Duration::from_millis(60);
    fx.approver.will(&[Decision::Allow; 4]);
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let fx = fx.clone();
            std::thread::spawn(move || {
                let p = peer(4242 + (i % 2));
                fx.request(&p, item_scope(&fx.github), "run", "once")["ok"] == true
            })
        })
        .collect();
    let granted = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(granted, 4);
    assert_eq!(fx.approver.max_in_flight.load(Ordering::SeqCst), 1);
}

#[test]
fn malformed_requests_are_refused_without_asking_anyone() {
    let fx = fixture();
    let p = peer(4242);
    for line in [
        "",
        "{",
        r#"{"v":1,"id":"a","op":"grant.request"}"#,
        r#"{"v":1,"id":"a","op":"grant.request","scope":{"vault":true},"mode":"http","ttl":"once","extra":1}"#,
        r#"{"v":1,"id":"a","op":"hello"}{"v":1,"id":"b","op":"lock"}"#,
    ] {
        let r: Value = serde_json::from_str(&fx.broker.handle_line(&p, line).line).unwrap();
        assert_eq!(Fx::code(&r), "bad_request", "{line:?}");
    }
    assert!(!fx.broker.is_locked(), "the smuggled lock did not run");
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn the_number_of_live_grants_is_bounded() {
    let fx = fixture();
    let p = peer(4242);
    for _ in 0..MAX_GRANTS {
        fx.grant(&p, item_scope(&fx.github), "run", "until-lock");
    }
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    let r = fx.request(&p, item_scope(&fx.github), "run", "once");
    assert_eq!(Fx::code(&r), "busy");
    assert_eq!(
        fx.approver.calls.load(Ordering::SeqCst),
        calls,
        "not even asked"
    );
}

#[test]
fn status_reports_the_audit_position() {
    let fx = fixture();
    let s = fx.send(&peer(1), json!({"v":1,"id":"s","op":"status"}));
    assert_eq!(s["locked"], false);
    assert_eq!(s["audit_seq"], 1);
    assert_eq!(s["audit_head"].as_str().unwrap().len(), 64);
    let h = fx.send(&peer(1), json!({"v":1,"id":"h","op":"hello"}));
    assert_eq!(
        (h["version"].as_u64(), h["locked"].as_bool()),
        (Some(1), Some(false))
    );
    let _ = fx.env.unix_ms();
}

/// The same rules over a real Unix socket, with the real peer check and the real client.
#[test]
fn over_a_real_socket_approval_use_and_lock_work_end_to_end() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let mut vault = figlock_core::Vault::create(&random_hex(8), "L", KdfParams::FLOOR, T)
        .unwrap()
        .0;
    let item = vault
        .add_item(
            ItemType::Token,
            "personal",
            &[
                ("title", "GitHub"),
                ("token", random_hex(8).as_str()),
                ("allowed_hosts/ab12", "api.github.com"),
            ],
            T,
        )
        .unwrap();
    let approver = Arc::new(Scripted::default());
    let writer = AuditWriter::with_sink(Box::new(Mem::default()), 0, 0, ZERO_PREV);
    let broker = Arc::new(
        Broker::start(
            vault,
            writer,
            Box::new(Shared(approver.clone())),
            Arc::new(TestEnv::new()),
            peer::my_uid(),
        )
        .unwrap(),
    );
    let tmp = tempfile::Builder::new()
        .prefix("fl")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = tmp.path().join("run").join("b.sock");
    let listener = server::bind(&socket).unwrap();
    let serving = {
        let broker = broker.clone();
        std::thread::spawn(move || server::serve(&listener, &broker))
    };

    assert_eq!(
        client::call(&socket, "hello", json!({})).unwrap()["ok"],
        true
    );

    approver.will(&[Decision::Allow, Decision::Deny]);
    let members = json!({"scope": {"item": hex::encode(item)}, "mode": "http", "ttl": "15m",
                         "label": "claimed", "reason": "deploy"});
    let granted = client::call(&socket, "grant.request", members.clone()).unwrap();
    assert_eq!(granted["ok"], true, "{granted}");
    let token = granted["grant"]["token"].as_str().unwrap().to_owned();
    let host = Some("api.github.com");
    assert!(broker.authorize(&token, &item, Mode::Http, host).is_ok());
    let shown = approver.shown.lock().unwrap()[0].clone();
    assert!(
        shown.contains(&format!("pid {}", std::process::id())),
        "the prompt names the real caller: {shown}"
    );

    let denied = client::call(&socket, "grant.request", members).unwrap();
    assert_eq!(denied["error"]["code"], "denied");
    let listed = client::call(&socket, "grant.list", json!({})).unwrap();
    assert_eq!(listed["grants"].as_array().unwrap().len(), 1);
    assert!(!listed.to_string().contains(&token));

    // Garbage and over-long lines get an error and a closed connection, and the broker carries on.
    for junk in [b"not json\n".to_vec(), vec![b'x'; proto::MAX_LINE + 10]] {
        let mut s = UnixStream::connect(&socket).unwrap();
        s.write_all(&junk).unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        assert!(line.contains("bad_request"), "{line}");
    }
    assert_eq!(
        client::call(&socket, "hello", json!({})).unwrap()["ok"],
        true
    );

    // Several requests on one connection are answered in order.
    let mut s = UnixStream::connect(&socket).unwrap();
    s.write_all(
        b"{\"v\":1,\"id\":\"1\",\"op\":\"hello\"}\n{\"v\":1,\"id\":\"2\",\"op\":\"status\"}\n",
    )
    .unwrap();
    let mut r = BufReader::new(&s);
    for want in ["\"1\"", "\"2\""] {
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains(want), "{line}");
    }

    assert_eq!(
        client::call(&socket, "lock", json!({})).unwrap()["ok"],
        true
    );
    serving.join().unwrap().unwrap();
    assert!(broker.is_locked());
}

#[test]
fn a_refused_field_or_missing_item_does_not_burn_a_once_grant() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "reveal", "once");
    for field in ["tokne", ""] {
        assert_eq!(
            fx.broker
                .authorize_field(&token, &fx.github, Mode::Reveal, None, field)
                .err(),
            Some(Code::NotAllowed)
        );
    }
    let v = fx
        .broker
        .authorize_field(&token, &fx.github, Mode::Reveal, None, "token")
        .unwrap();
    assert_eq!(v.as_str(), fx.github_secret);
    assert_eq!(
        fx.broker
            .authorize_field(&token, &fx.github, Mode::Reveal, None, "token")
            .err(),
        Some(Code::NoGrant),
        "the successful use consumed it"
    );

    let gone: ItemId = [7; 16];
    let any = fx.grant(&p, json!({"vault":true}), "run", "once");
    assert_eq!(
        fx.broker.authorize(&any, &gone, Mode::Run, None).err(),
        Some(Code::NotAllowed)
    );
    assert!(
        fx.broker
            .authorize(&any, &fx.github, Mode::Run, None)
            .is_ok()
    );
    assert_eq!(
        fx.broker.authorize(&any, &fx.github, Mode::Run, None).err(),
        Some(Code::NoGrant)
    );

    let codes: Vec<_> = fx
        .events()
        .into_iter()
        .filter(|(e, _)| e == "use.refused")
        .map(|(_, d)| d["code"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        codes,
        [
            "not_allowed",
            "not_allowed",
            "no_grant",
            "not_allowed",
            "no_grant"
        ]
    );
}

fn run_env(fx: &Fx, p: &PeerId, token: &str, item: &ItemId, vars: Value) -> Value {
    let mut req = json!({"v":1,"id":"e","op":"run.env","token":token,"item":hex::encode(item)});
    if !vars.is_null() {
        req["vars"] = vars;
    }
    fx.send(p, req)
}

#[test]
fn a_run_grant_hands_a_token_item_its_own_variable() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "run", "15m");
    let r = run_env(&fx, &p, &token, &fx.github, Value::Null);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["env"], json!({"GH_TOKEN": fx.github_secret}));
}

#[test]
fn a_bundle_gives_every_variable_and_a_once_grant_covers_all_of_them() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.bundle), "run", "once");
    let r = run_env(&fx, &p, &token, &fx.bundle, Value::Null);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        r["env"],
        json!({"DB_URL": fx.bundle_a, "STRIPE_KEY": fx.bundle_b})
    );
    // Consumed by that one use.
    let again = run_env(&fx, &p, &token, &fx.bundle, Value::Null);
    assert_eq!(Fx::code(&again), "no_grant");
    // One record per field read (the log lower-cases names), none holding a value.
    let used: Vec<String> = fx
        .events()
        .into_iter()
        .filter(|(e, _)| e == "use.allowed")
        .map(|(_, d)| d["field"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(used, ["env/db_url", "env/stripe_key"]);
}

#[test]
fn explicit_variables_map_a_name_to_a_field() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "run", "15m");
    let r = run_env(
        &fx,
        &p,
        &token,
        &fx.github,
        json!({"TOKEN_FOR_TOOL": "token"}),
    );
    assert_eq!(r["env"], json!({"TOKEN_FOR_TOOL": fx.github_secret}));
}

#[test]
fn run_env_is_refused_without_the_right_grant() {
    let fx = fixture();
    let p = peer(4242);
    // No grant, a made-up token, the wrong mode, the wrong item, a missing field, nothing to give.
    let nothing = run_env(&fx, &p, &"0".repeat(64), &fx.github, Value::Null);
    assert_eq!(Fx::code(&nothing), "no_grant");
    let http = fx.grant(&p, item_scope(&fx.github), "http", "15m");
    let wrong_mode = run_env(&fx, &p, &http, &fx.github, Value::Null);
    assert_eq!(Fx::code(&wrong_mode), "not_allowed");
    let run = fx.grant(&p, item_scope(&fx.github), "run", "15m");
    let wrong_item = run_env(&fx, &p, &run, &fx.bundle, Value::Null);
    assert_eq!(Fx::code(&wrong_item), "not_allowed");
    let missing = run_env(&fx, &p, &run, &fx.github, json!({"X": "no_such_field"}));
    assert_eq!(Fx::code(&missing), "not_allowed");
    // A login has no defaults: nothing to give is a refusal, not an empty success.
    let login = fx.grant(&p, item_scope(&fx.login), "run", "15m");
    let empty = run_env(&fx, &p, &login, &fx.login, Value::Null);
    assert_eq!(Fx::code(&empty), "not_allowed");
    // None of those refusals burned the good grant.
    let ok = run_env(&fx, &p, &run, &fx.github, Value::Null);
    assert_eq!(ok["ok"], true);
}

#[test]
fn an_ended_run_grant_is_refused() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.github), "run", "15m");
    fx.env.advance(Duration::from_secs(901));
    let r = run_env(&fx, &p, &token, &fx.github, Value::Null);
    assert_eq!(Fx::code(&r), "expired");
}

#[test]
fn an_item_can_be_asked_for_by_title_and_the_grant_holds_its_id() {
    let fx = fixture();
    let p = peer(4242);
    fx.approver.will(&[Decision::Allow]);
    let r = fx.request(&p, json!({"name": "github"}), "run", "15m");
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["grant"]["scope"], item_scope(&fx.github));
    // The user is shown the title the broker found, not what the caller typed.
    assert!(fx.approver.shown.lock().unwrap()[0].contains("\"GitHub\""));
    // An unknown title is refused without asking anyone.
    let before = fx.approver.calls.load(Ordering::SeqCst);
    let none = fx.request(&p, json!({"name": "no such item"}), "run", "15m");
    assert_eq!(Fx::code(&none), "not_allowed");
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), before);
}

#[test]
fn titles_get_the_same_busy_answer_during_a_cooldown() {
    let fx = fixture();
    let p = peer(4242);
    let r = fx.request(&p, json!({"name": "github"}), "run", "15m");
    assert_eq!(Fx::code(&r), "denied");
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    for name in ["github", "no such item"] {
        let r = fx.request(&p, json!({"name": name}), "run", "15m");
        assert_eq!(Fx::code(&r), "busy", "{name}");
    }
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), calls);
}

#[test]
fn run_env_values_never_reach_the_log_or_a_prompt() {
    let fx = fixture();
    let p = peer(4242);
    let token = fx.grant(&p, item_scope(&fx.bundle), "run", "15m");
    let _ = run_env(&fx, &p, &token, &fx.bundle, Value::Null);
    let log = format!("{:?}", fx.events());
    let shown = fx.approver.shown.lock().unwrap().join("\n");
    for secret in [&fx.bundle_a, &fx.bundle_b] {
        assert!(!log.contains(secret.as_str()) && !shown.contains(secret.as_str()));
    }
}

/// `figlock run` end to end: a real broker on a real socket, real child processes.
mod run_command {
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;
    use crate::run::{RunSpec, execute};

    #[derive(Clone, Default)]
    struct Buf(Arc<Mutex<Vec<u8>>>);

    impl Write for Buf {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Buf {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    struct Served {
        fx: Fx,
        socket: PathBuf,
        _dir: tempfile::TempDir,
    }

    fn serve() -> Served {
        let fx = fixture();
        let dir = tempfile::Builder::new()
            .prefix("fl")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = dir.path().join("run").join("b.sock");
        let listener = server::bind(&socket).unwrap();
        let broker = fx.broker.clone();
        std::thread::spawn(move || server::serve(&listener, &broker));
        Served {
            fx,
            socket,
            _dir: dir,
        }
    }

    fn spec(item: &str, vars: &[(&str, &str)], script: &str) -> RunSpec {
        RunSpec {
            item: item.to_owned(),
            vars: vars
                .iter()
                .map(|(n, f)| ((*n).to_owned(), (*f).to_owned()))
                .collect(),
            ttl: "15m".to_owned(),
            reason: Some("test".to_owned()),
            command: vec!["/bin/sh".into(), "-c".into(), script.to_owned()],
        }
    }

    fn go(s: &Served, spec: &RunSpec) -> (Result<i32, String>, String, String) {
        let (out, err) = (Buf::default(), Buf::default());
        let r = execute(&s.socket, spec, out.clone(), err.clone()).map_err(|e| e.0);
        (r, out.text(), err.text())
    }

    #[test]
    fn the_program_gets_the_secret_and_its_output_does_not() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let secret = &s.fx.github_secret;
        let script = "echo \"token=$GH_TOKEN\"; echo \"oops $GH_TOKEN\" >&2; \
                      printf %s \"$GH_TOKEN\" | base64; [ -n \"$GH_TOKEN\" ] && echo has-it; exit 3";
        let (code, out, err) = go(&s, &spec("GitHub", &[], script));
        assert_eq!(code, Ok(3), "the program's own exit code is kept");
        assert!(out.starts_with("token=[REDACTED:GH_TOKEN]\n"), "{out}");
        assert!(out.contains("has-it"), "the program really had the value");
        assert_eq!(err, "oops [REDACTED:GH_TOKEN]\n");
        assert!(!out.contains(secret.as_str()) && !err.contains(secret.as_str()));
        // Its base64 form is scrubbed too.
        assert_eq!(out.matches("[REDACTED:GH_TOKEN]").count(), 2, "{out}");
        // The approval named the item and the asking program.
        let shown = s.fx.approver.shown.lock().unwrap()[0].clone();
        assert!(
            shown.contains("\"GitHub\"") && shown.contains("run:"),
            "{shown}"
        );
    }

    #[test]
    fn a_bundle_sets_every_variable_and_all_of_them_are_scrubbed() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let script = "echo \"$DB_URL|$STRIPE_KEY\"";
        let (code, out, _) = go(&s, &spec("Deploy env", &[], script));
        assert_eq!(code, Ok(0));
        assert_eq!(out, "[REDACTED:DB_URL]|[REDACTED:STRIPE_KEY]\n");
    }

    #[test]
    fn an_explicit_variable_maps_to_a_field() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let (code, out, _) = go(
            &s,
            &spec("GitHub", &[("MY_TOOL_KEY", "token")], "echo $MY_TOOL_KEY"),
        );
        assert_eq!(code, Ok(0));
        assert_eq!(out, "[REDACTED:MY_TOOL_KEY]\n");
    }

    #[test]
    fn a_denied_request_never_starts_the_program() {
        let s = serve();
        s.fx.approver.will(&[Decision::Deny]);
        let marker = s._dir.path().join("ran");
        let script = format!("touch {}", marker.display());
        let (code, out, err) = go(&s, &spec("GitHub", &[], &script));
        assert!(code.unwrap_err().contains("denied"));
        assert!(!marker.exists());
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn an_unknown_item_or_one_with_nothing_to_give_is_refused_without_running() {
        let s = serve();
        let marker = s._dir.path().join("ran");
        let script = format!("touch {}", marker.display());
        let before = s.fx.approver.calls.load(Ordering::SeqCst);
        let (code, _, _) = go(&s, &spec("No such item", &[], &script));
        assert!(code.unwrap_err().contains("not_allowed"));
        assert_eq!(
            s.fx.approver.calls.load(Ordering::SeqCst),
            before,
            "nobody was asked"
        );
        // Approved, but a login item defines no variables.
        s.fx.approver.will(&[Decision::Allow]);
        let (code, _, _) = go(&s, &spec("Example", &[], &script));
        assert!(code.unwrap_err().contains("not_allowed"));
        assert!(!marker.exists());
        // A field that does not exist.
        s.fx.approver.will(&[Decision::Allow]);
        let (code, _, _) = go(&s, &spec("GitHub", &[("X", "nope")], &script));
        assert!(code.unwrap_err().contains("not_allowed"));
        assert!(!marker.exists());
    }

    #[test]
    fn no_broker_is_a_clear_error() {
        let s = serve();
        let gone = s._dir.path().join("nothing.sock");
        let (out, err) = (Buf::default(), Buf::default());
        let e = execute(&gone, &spec("GitHub", &[], "true"), out, err).unwrap_err();
        assert!(e.0.contains("no broker is running"), "{e}");
    }

    #[test]
    fn a_program_that_cannot_start_is_reported_without_its_arguments() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let mut sp = spec("GitHub", &[], "");
        sp.command = vec![
            "/no/such/program".into(),
            "--password=hunter2hunter2".into(),
        ];
        let (code, _, _) = go(&s, &sp);
        let e = code.unwrap_err();
        assert!(e.contains("could not start program"), "{e}");
        assert!(!e.contains("hunter2"));
    }
}
