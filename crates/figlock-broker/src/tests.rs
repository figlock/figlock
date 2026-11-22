//! Broker behaviour tests (FIG-34). Every credential and secret is generated per run.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use figlock_core::{ItemType, KdfParams, ZERO_PREV};
use figlock_session::config::Remote;
use figlock_session::sync::SyncError;
use figlock_store::BlobStore;
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

struct NoUi;

impl figlock_session::session::Ui for NoUi {
    fn password(&self) -> figlock_session::error::Res<zeroize::Zeroizing<String>> {
        Err(figlock_session::error::Fail::new("no"))
    }
    fn secret_key(&self) -> figlock_session::error::Res<figlock_core::SecretKey> {
        Err(figlock_session::error::Fail::new("no"))
    }
    fn notice(&self, _: &str) {}
    fn warn(&self, _: &str) {}
}

/// A session over a vault with a folder of its own and no other device.
fn solo(vault: figlock_core::Vault) -> Session {
    let dir = tempfile::tempdir().unwrap().keep();
    let store = figlock_store::LocalFolder::open(dir).unwrap();
    Session::from_parts(vault, Box::new(store), Box::new(NoUi))
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
    /// A token that may be sent to 127.0.0.1 only, for tests against a local server.
    local: ItemId,
    local_secret: String,
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
    let local_secret = random_hex(16);
    let local = vault
        .add_item(
            ItemType::Token,
            "personal",
            &[
                ("title", "Local API"),
                ("token", local_secret.as_str()),
                ("allowed_hosts/ef56", "127.0.0.1"),
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
            solo(vault),
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
        local,
        local_secret,
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
        solo(vault),
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

/// A system prompt that was not confirmed never makes a grant, whichever way it failed (FIG-128). The
/// approver is the only way to a grant, and it does not look at where the device keys are kept.
#[test]
fn an_unconfirmed_system_prompt_never_makes_a_grant() {
    use figlock_biometric::Outcome;
    let asked: [fn(&str, Duration) -> Outcome; 3] = [
        |_, _| Outcome::Declined,
        |_, _| Outcome::TimedOut,
        |_, _| Outcome::Unavailable,
    ];
    for confirm in asked {
        let (vault, _) =
            figlock_core::Vault::create(&random_hex(8), "L", KdfParams::FLOOR, T).unwrap();
        let writer = AuditWriter::with_sink(Box::new(Mem::default()), 0, 0, ZERO_PREV);
        let broker = Broker::start(
            solo(vault),
            writer,
            Box::new(approve::BiometricApprover::with(confirm)),
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
        assert_eq!(broker.grant_count(), 0);
    }
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
            solo(vault),
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

    // The same request again is answered by the grant, with no prompt (ADR-0039); a longer one is asked.
    let again = client::call(&socket, "grant.request", members.clone()).unwrap();
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(again["grant"]["id"], granted["grant"]["id"]);
    assert_eq!(approver.calls.load(Ordering::SeqCst), 1);
    let mut longer = members;
    longer["ttl"] = json!("1d");
    let denied = client::call(&socket, "grant.request", longer).unwrap();
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
pub(super) mod run_command {
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;
    use crate::run_cmd::{RunSpec, execute};

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

    pub(super) struct Served {
        pub(super) fx: Fx,
        pub(super) socket: PathBuf,
        pub(super) _dir: tempfile::TempDir,
    }

    pub(super) fn serve() -> Served {
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

/// `figlock http` end to end: a real broker, a real socket and a real HTTP server on this machine.
mod http_command {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    use super::run_command::{Served, serve};
    use super::*;
    use crate::http_cmd::{HttpSpec, Reply, execute};

    #[derive(Clone, Debug)]
    struct Seen {
        line: String,
        headers: Vec<(String, String)>,
        body: String,
    }

    impl Seen {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        }
    }

    struct Web {
        port: u16,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    fn read_request(s: &mut TcpStream) -> Option<Seen> {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while !buf.ends_with(b"\r\n\r\n") {
            if s.read(&mut byte).ok()? == 0 {
                return None;
            }
            buf.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&buf).into_owned();
        let mut lines = head.split("\r\n");
        let line = lines.next()?.to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
            .collect();
        let len = headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).ok()?;
        Some(Seen {
            line,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    /// A server whose `reply` decides each answer (`status line and headers`, body).
    fn web(reply: impl Fn(&Seen, u16) -> (String, Vec<u8>) + Send + 'static) -> Web {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let Some(req) = read_request(&mut s) else {
                    continue;
                };
                let (head, body) = reply(&req, port);
                log.lock().unwrap().push(req);
                let _ = write!(
                    s,
                    "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(&body);
            }
        });
        Web { port, seen }
    }

    fn ok(body: &str) -> (String, Vec<u8>) {
        ("HTTP/1.1 200 OK".to_owned(), body.as_bytes().to_vec())
    }

    fn redirect(status: u16, to: &str) -> (String, Vec<u8>) {
        (
            format!("HTTP/1.1 {status} Redirect\r\nLocation: {to}"),
            vec![],
        )
    }

    fn spec(item: &str, url: &str) -> HttpSpec {
        HttpSpec {
            item: item.to_owned(),
            method: "GET".to_owned(),
            url: url.to_owned(),
            headers: vec![],
            body: None,
            ttl: "15m".to_owned(),
            reason: Some("test".to_owned()),
            auth_header: None,
            auth_scheme: None,
            field: None,
        }
    }

    fn go(s: &Served, sp: &HttpSpec) -> Result<Reply, String> {
        execute(&s.socket, sp).map_err(|e| e.0)
    }

    fn text(r: &Reply) -> String {
        String::from_utf8_lossy(&r.body).into_owned()
    }

    fn audit(s: &Served, ev: &str) -> Vec<Map> {
        s.fx.events()
            .into_iter()
            .filter(|(e, _)| e == ev)
            .map(|(_, d)| d)
            .collect()
    }

    #[test]
    fn the_secret_is_added_for_an_allowed_host_and_never_comes_back() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        // A server that echoes what it was sent, as a debug endpoint would.
        let w = web(|req, _| ok(&format!("you sent: {:?}", req.headers)));
        let mut sp = spec(
            "Local API",
            &format!("http://127.0.0.1:{}/v1/things", w.port),
        );
        sp.method = "POST".into();
        sp.body = Some("{\"a\":1}".into());
        sp.headers = vec![("X-Trace".into(), "abc".into())];
        let r = go(&s, &sp).unwrap();
        assert_eq!(r.status, 200);
        let seen = w.seen.lock().unwrap()[0].clone();
        assert_eq!(seen.line, "POST /v1/things HTTP/1.1");
        assert_eq!(
            seen.header("authorization"),
            Some(format!("Bearer {}", s.fx.local_secret).as_str())
        );
        assert_eq!(seen.header("x-trace"), Some("abc"));
        assert_eq!(seen.body, "{\"a\":1}");
        // The server echoed the secret; the caller got it scrubbed.
        assert!(text(&r).contains("[REDACTED:token]"), "{}", text(&r));
        assert!(!text(&r).contains(&s.fx.local_secret));
        // The prompt says where the secret may go.
        let shown = s.fx.approver.shown.lock().unwrap()[0].clone();
        assert!(shown.contains("(only for 127.0.0.1)"), "{shown}");
    }

    #[test]
    fn another_scheme_header_and_field_can_be_chosen() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let w = web(|_, _| ok("fine"));
        let mut sp = spec("Local API", &format!("http://127.0.0.1:{}/", w.port));
        sp.auth_header = Some("X-Api-Key".into());
        sp.auth_scheme = Some(String::new());
        let r = go(&s, &sp).unwrap();
        assert_eq!(text(&r), "fine");
        let seen = w.seen.lock().unwrap()[0].clone();
        assert_eq!(seen.header("x-api-key"), Some(s.fx.local_secret.as_str()));
        assert!(seen.header("authorization").is_none());
    }

    #[test]
    fn a_host_not_on_the_allow_list_is_refused_before_anything_is_sent() {
        let s = serve();
        let w = web(|_, _| ok("x"));
        s.fx.approver.will(&[Decision::Allow]);
        // "localhost" is the same machine but not the name on the list.
        let e = go(
            &s,
            &spec("Local API", &format!("http://localhost:{}/", w.port)),
        )
        .unwrap_err();
        assert!(e.contains("not_allowed"), "{e}");
        assert!(w.seen.lock().unwrap().is_empty());
        let refused = audit(&s, "use.refused");
        assert_eq!(refused.last().unwrap()["host"], "localhost");
    }

    #[test]
    fn plain_http_to_another_machine_and_urls_with_credentials_are_refused() {
        let s = serve();
        for url in [
            "http://api.github.com/user",
            "ftp://api.github.com/user",
            "https://user:pass@api.github.com/user",
            "not a url",
        ] {
            s.fx.approver.will(&[Decision::Allow]);
            let e = go(&s, &spec("GitHub", url)).unwrap_err();
            assert!(e.contains("bad_request"), "{url}: {e}");
            // The cooldown after a deny is not involved; each was approved and then refused on its own.
        }
    }

    #[test]
    fn the_secrets_own_header_and_the_host_header_cannot_be_set_by_the_caller() {
        let s = serve();
        let w = web(|_, _| ok("x"));
        for header in ["Authorization", "authorization", "Host", "Content-Length"] {
            s.fx.approver.will(&[Decision::Allow]);
            let mut sp = spec("Local API", &format!("http://127.0.0.1:{}/", w.port));
            sp.headers = vec![(header.into(), "evil".into())];
            let e = go(&s, &sp).unwrap_err();
            assert!(e.contains("bad_request"), "{header}: {e}");
        }
        assert!(w.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_redirect_to_a_host_off_the_list_is_not_followed_and_gets_no_secret() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let thief = web(|_, _| ok("stolen"));
        let to = format!("http://localhost:{}/steal", thief.port);
        let w = web(move |_, _| redirect(302, &to));
        let r = go(
            &s,
            &spec("Local API", &format!("http://127.0.0.1:{}/", w.port)),
        )
        .unwrap();
        assert_eq!(r.status, 302, "the redirect is handed back, not followed");
        assert!(
            thief.seen.lock().unwrap().is_empty(),
            "nothing reached the other host"
        );
        let refused = audit(&s, "use.refused");
        assert_eq!(refused.last().unwrap()["host"], "localhost");
        assert_eq!(refused.last().unwrap()["code"], "not_allowed");
    }

    #[test]
    fn a_redirect_within_the_list_is_followed_with_the_secret_and_each_hop_is_recorded() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let end = web(|_, _| ok("arrived"));
        let to = format!("http://127.0.0.1:{}/final", end.port);
        let w = web(move |_, _| redirect(307, &to));
        let mut sp = spec("Local API", &format!("http://127.0.0.1:{}/", w.port));
        sp.method = "POST".into();
        sp.body = Some("payload".into());
        let r = go(&s, &sp).unwrap();
        assert_eq!((r.status, text(&r).as_str()), (200, "arrived"));
        let seen = end.seen.lock().unwrap()[0].clone();
        assert_eq!(
            seen.line, "POST /final HTTP/1.1",
            "a 307 keeps method and body"
        );
        assert_eq!(seen.body, "payload");
        assert_eq!(
            seen.header("authorization"),
            Some(format!("Bearer {}", s.fx.local_secret).as_str())
        );
        assert_eq!(
            audit(&s, "use.allowed").len(),
            2,
            "the first request and the hop"
        );
    }

    #[test]
    fn a_303_after_post_becomes_a_get_without_a_body() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let end = web(|_, _| ok("done"));
        let to = format!("http://127.0.0.1:{}/result", end.port);
        let w = web(move |_, _| redirect(303, &to));
        let mut sp = spec("Local API", &format!("http://127.0.0.1:{}/", w.port));
        sp.method = "POST".into();
        sp.body = Some("payload".into());
        go(&s, &sp).unwrap();
        let seen = end.seen.lock().unwrap()[0].clone();
        assert_eq!(seen.line, "GET /result HTTP/1.1");
        assert_eq!(seen.body, "");
    }

    #[test]
    fn a_redirect_loop_stops_and_a_once_grant_covers_the_request_and_its_hops() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow, Decision::Deny]);
        let w = web(|_, port| redirect(302, &format!("http://127.0.0.1:{port}/again")));
        let mut sp = spec("Local API", &format!("http://127.0.0.1:{}/", w.port));
        sp.ttl = "once".into();
        let r = go(&s, &sp).unwrap();
        assert_eq!(r.status, 302);
        assert_eq!(
            w.seen.lock().unwrap().len(),
            crate::http::MAX_REDIRECTS + 1,
            "the original request plus the limit, and no more"
        );
        // The once grant is spent; another request needs another approval, which is refused here.
        let e = go(&s, &sp).unwrap_err();
        assert!(e.contains("denied"), "{e}");
    }

    #[test]
    fn a_server_that_cannot_be_reached_is_an_upstream_error_without_the_secret() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        // Nothing listens on port 1.
        let e = go(&s, &spec("Local API", "http://127.0.0.1:1/")).unwrap_err();
        assert!(e.contains("upstream"), "{e}");
        assert!(!e.contains(&s.fx.local_secret));
    }

    #[test]
    fn a_long_response_is_cut_and_says_so() {
        let s = serve();
        s.fx.approver.will(&[Decision::Allow]);
        let big = vec![b'a'; crate::http::MAX_RESPONSE + 1000];
        let w = web(move |_, _| ("HTTP/1.1 200 OK".to_owned(), big.clone()));
        let r = go(
            &s,
            &spec("Local API", &format!("http://127.0.0.1:{}/", w.port)),
        )
        .unwrap();
        assert!(r.truncated);
        assert_eq!(r.body.len(), crate::http::MAX_RESPONSE);
    }

    #[test]
    fn the_wrong_mode_or_item_gets_nothing_and_no_secret_reaches_the_log() {
        let s = serve();
        let w = web(|_, _| ok("x"));
        // A run grant cannot be used for http.
        s.fx.approver.will(&[Decision::Allow]);
        let p = peer(4242);
        let run = s.fx.grant(&p, item_scope(&s.fx.local), "run", "15m");
        let r = s.fx.send(
            &p,
            json!({"v":1,"id":"h","op":"http.request","token":run,"item":hex::encode(s.fx.local),
                   "method":"GET","url":format!("http://127.0.0.1:{}/", w.port)}),
        );
        assert_eq!(Fx::code(&r), "not_allowed");
        // An http grant for one item cannot be used for another.
        let http = s.fx.grant(&p, item_scope(&s.fx.local), "http", "15m");
        let r = s.fx.send(
            &p,
            json!({"v":1,"id":"h","op":"http.request","token":http,"item":hex::encode(s.fx.github),
                   "method":"GET","url":"https://api.github.com/"}),
        );
        assert_eq!(Fx::code(&r), "not_allowed");
        assert!(w.seen.lock().unwrap().is_empty());
        let log = format!("{:?}", s.fx.events());
        let shown = s.fx.approver.shown.lock().unwrap().join("\n");
        for secret in [&s.fx.local_secret, &s.fx.github_secret] {
            assert!(!log.contains(secret.as_str()) && !shown.contains(secret.as_str()));
        }
    }
}

/// Two devices of one vault over one folder, as a sync leaves them: the broker runs on `a`, `b` is the
/// other device (a phone, or the desktop app).
struct Duo {
    broker: Arc<Broker>,
    approver: Arc<Scripted>,
    log: Mem,
    /// Another handle on the broker's device, to read the audit log back.
    reader: figlock_core::Vault,
    b: Session,
    a_id: figlock_core::DeviceId,
    password: String,
    secret_key: figlock_core::SecretKey,
    _dir: tempfile::TempDir,
}

fn duo() -> Duo {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let folder = move || figlock_store::LocalFolder::open(&root).unwrap();
    let password = random_hex(12);
    let (a, created) = figlock_core::Vault::create(&password, "A", KdfParams::FLOOR, T).unwrap();
    folder().put_new("vault.json", &created.vault_json).unwrap();
    let a_id = *a.device_id();
    let rec_a = format!("devices/{}.json", hex::encode(a_id));
    folder().put_new(&rec_a, &created.device_record).unwrap();
    let secret_key = figlock_core::SecretKey::parse(&created.secret_key.format()).unwrap();
    let auk = figlock_core::Vault::unlock_key(&created.vault_json, &password, &secret_key).unwrap();
    let (b, record) = figlock_core::Vault::unlock_with_key(
        &created.vault_json,
        &auk,
        figlock_core::DeviceKeys::generate().unwrap(),
        "B",
        std::slice::from_ref(&created.device_record),
        T + 1,
    )
    .unwrap();
    let rec_b = format!("devices/{}.json", hex::encode(b.device_id()));
    folder().put_new(&rec_b, &record.unwrap()).unwrap();
    let (reader, _) = figlock_core::Vault::unlock(
        &created.vault_json,
        &password,
        &secret_key,
        clone_device(&a),
        "A",
        std::slice::from_ref(&created.device_record),
        T + 1,
    )
    .unwrap();

    let approver = Arc::new(Scripted::default());
    let log = Mem::default();
    let writer = AuditWriter::with_sink(Box::new(log.clone()), 0, 0, ZERO_PREV);
    let broker = Arc::new(
        Broker::start(
            Session::from_parts(a, Box::new(folder()), Box::new(NoUi)),
            writer,
            Box::new(Shared(approver.clone())),
            Arc::new(TestEnv::new()),
            uid(),
        )
        .unwrap()
        .with_wait(Duration::from_secs(1)),
    );
    Duo {
        broker,
        approver,
        log,
        reader,
        b: Session::from_parts(b, Box::new(folder()), Box::new(NoUi)),
        a_id,
        password,
        secret_key,
        _dir: dir,
    }
}

impl Duo {
    /// An item the other device adds and writes.
    fn b_adds(&mut self, title: &str) -> ItemId {
        let id = self
            .b
            .vault
            .add_item(
                ItemType::Token,
                "personal",
                &[
                    ("title", title),
                    ("token", "x"),
                    ("allowed_hosts/a", "example.com"),
                ],
                T + 2,
            )
            .unwrap();
        self.b.commit().unwrap();
        id
    }

    fn b_deletes(&mut self, id: &ItemId) {
        self.b.vault.delete_item(id, T + 3).unwrap();
        self.b.commit().unwrap();
    }

    /// The other device revokes the broker's device, which rotates the vault key and rewrites `vault.json`.
    fn b_revokes_the_broker_device(&mut self) {
        let r = self
            .b
            .vault
            .revoke_device(&self.password, &self.secret_key, &self.a_id, T + 4)
            .unwrap();
        self.b.write_revocation(&r).unwrap();
    }

    fn request(&self, scope: Value) -> Value {
        let line =
            json!({"v":1,"id":"r","op":"grant.request","scope":scope,"mode":"http","ttl":"15m"});
        let reply = self.broker.handle_line(&peer(4242), &line.to_string());
        serde_json::from_str(&reply.line).unwrap()
    }

    fn grant(&self, scope: Value) -> Value {
        self.approver.will(&[Decision::Allow]);
        self.request(scope)
    }

    fn events(&self) -> Vec<(String, Map)> {
        let (chain, damaged) = self.reader.read_audit_log(&self.log.bytes.lock().unwrap());
        assert!(!damaged && chain.first_bad.is_none(), "the log must verify");
        chain.records.into_iter().map(|r| (r.ev, r.d)).collect()
    }

    fn names(&self) -> Vec<String> {
        self.events().into_iter().map(|(e, _)| e).collect()
    }

    fn grants(&self) -> usize {
        self.broker.grant_count()
    }
}

#[test]
fn an_item_another_device_adds_reaches_a_running_broker() {
    let mut d = duo();
    let id = d.b_adds("From the phone");
    assert_eq!(
        d.request(item_scope(&id))["error"]["code"],
        "not_allowed",
        "not known here yet"
    );
    assert_eq!(d.broker.absorb(), Absorbed::Updated);
    let r = d.grant(item_scope(&id));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        d.broker.absorb(),
        Absorbed::Same,
        "nothing new the second time"
    );
}

#[test]
fn a_grant_ends_when_its_item_is_deleted_on_another_device() {
    let mut d = duo();
    let id = d.b_adds("Short lived");
    d.broker.absorb();
    let token = d.grant(item_scope(&id))["grant"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        d.broker
            .authorize(&token, &id, Mode::Http, Some("example.com"))
            .is_ok()
    );
    d.b_deletes(&id);
    assert_eq!(d.broker.absorb(), Absorbed::Updated);
    assert_eq!(d.grants(), 0);
    assert_eq!(
        d.broker
            .authorize(&token, &id, Mode::Http, Some("example.com"))
            .err(),
        Some(Code::NoGrant)
    );
    let events = d.events();
    let revoked = events.iter().find(|(e, _)| e == "grant.revoked").unwrap();
    assert_eq!(revoked.1["reason"], "item_removed");
}

#[test]
fn a_revoked_device_locks_the_broker_and_ends_every_grant() {
    let mut d = duo();
    let id = d.b_adds("Prod token");
    d.broker.absorb();
    let token = d.grant(item_scope(&id))["grant"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(d.grants(), 1);

    d.b_revokes_the_broker_device();
    assert_eq!(d.broker.absorb(), Absorbed::Locked);

    assert!(d.broker.is_locked());
    assert_eq!(d.grants(), 0);
    assert_eq!(
        d.broker
            .authorize(&token, &id, Mode::Http, Some("example.com"))
            .err(),
        Some(Code::Locked)
    );
    assert_eq!(d.request(item_scope(&id))["error"]["code"], "locked");
    let events = d.events();
    let lock = events.iter().find(|(e, _)| e == "broker.lock").unwrap();
    assert_eq!(lock.1["reason"], "settings_changed");
    assert_eq!(d.broker.absorb(), Absorbed::Locked, "stays locked");
}

#[test]
fn a_grant_being_approved_when_the_revocation_lands_is_never_issued() {
    let mut d = duo();
    let id = d.b_adds("Prod token");
    d.broker.absorb();
    d.b_revokes_the_broker_device();
    // The person is still looking at the prompt when the broker takes the change in.
    let broker = d.broker.clone();
    *d.approver.hook.lock().unwrap() = Some(Box::new(move || {
        broker.absorb();
    }));
    let r = d.grant(item_scope(&id));
    assert_eq!(r["error"]["code"], "locked", "{r}");
    assert_eq!(d.grants(), 0);
    assert!(
        !d.names().contains(&"grant.approved".to_owned()),
        "no approval was recorded"
    );
}

#[test]
fn a_remote_that_fails_never_blocks_a_grant() {
    let mut d = duo();
    let id = d.b_adds("Prod token");
    d.broker.absorb();
    let t0 = std::time::Instant::now();
    let mut syncer = syncer::Syncer::new(t0);
    let calls = std::cell::Cell::new(0);
    let mut offline = |_: &Remote| {
        calls.set(calls.get() + 1);
        Err(SyncError::Offline)
    };
    for n in 0..4 {
        let at = t0 + Duration::from_secs(n * 400);
        assert!(syncer.step(at, &d.broker, &[Remote::Drive], &mut offline));
        assert_eq!(d.grant(item_scope(&id))["ok"], true);
    }
    assert_eq!(calls.get(), 4, "it keeps trying, at its own pace");
    assert!(
        !d.names().contains(&"sync.failed".to_owned()),
        "being offline is not worth a record"
    );
}

#[test]
fn failures_back_off_and_a_healthy_sync_returns_to_fifteen_seconds() {
    let d = duo();
    let t0 = std::time::Instant::now();
    let mut syncer = syncer::Syncer::new(t0);
    let calls = std::cell::Cell::new(0);
    let ok = std::cell::Cell::new(false);
    let mut run = |_: &Remote| {
        calls.set(calls.get() + 1);
        if ok.get() {
            Ok(figlock_session::sync::Report {
                pulled: 0,
                pushed: 0,
                warnings: Vec::new(),
            })
        } else {
            Err(SyncError::Offline)
        }
    };
    let secs = Duration::from_secs;
    syncer.step(t0, &d.broker, &[Remote::Drive], &mut run); // fails: next in 5 s
    syncer.step(t0 + secs(4), &d.broker, &[Remote::Drive], &mut run);
    assert_eq!(calls.get(), 1);
    syncer.step(t0 + secs(5), &d.broker, &[Remote::Drive], &mut run); // fails: next in 15 s
    syncer.step(t0 + secs(19), &d.broker, &[Remote::Drive], &mut run);
    assert_eq!(calls.get(), 2);
    ok.set(true);
    syncer.step(t0 + secs(20), &d.broker, &[Remote::Drive], &mut run); // works: next in 15 s
    syncer.step(t0 + secs(34), &d.broker, &[Remote::Drive], &mut run);
    assert_eq!(calls.get(), 3);
    syncer.step(t0 + secs(35), &d.broker, &[Remote::Drive], &mut run);
    assert_eq!(calls.get(), 4);
}

#[test]
fn each_remote_has_its_own_schedule_and_one_failing_never_holds_back_the_other() {
    let d = duo();
    let t0 = std::time::Instant::now();
    let mut syncer = syncer::Syncer::new(t0);
    let folder = Remote::Folder("/icloud/vault".into());
    let remotes = [Remote::Drive, folder.clone()];
    let drive_calls = std::cell::Cell::new(0);
    let folder_calls = std::cell::Cell::new(0);
    let mut run = |r: &Remote| {
        if *r == Remote::Drive {
            drive_calls.set(drive_calls.get() + 1);
            Err(SyncError::Offline)
        } else {
            folder_calls.set(folder_calls.get() + 1);
            Ok(figlock_session::sync::Report::default())
        }
    };
    let secs = Duration::from_secs;
    syncer.step(t0, &d.broker, &remotes, &mut run);
    assert_eq!((drive_calls.get(), folder_calls.get()), (1, 1));
    // Drive is in back-off (5 s) while the folder is healthy (15 s): neither follows the other's pace.
    syncer.step(t0 + secs(4), &d.broker, &remotes, &mut run);
    assert_eq!((drive_calls.get(), folder_calls.get()), (1, 1));
    syncer.step(t0 + secs(5), &d.broker, &remotes, &mut run);
    assert_eq!((drive_calls.get(), folder_calls.get()), (2, 1));
    syncer.step(t0 + secs(15), &d.broker, &remotes, &mut run);
    assert_eq!(folder_calls.get(), 2);
}

#[test]
fn a_remote_that_needs_the_person_stops_alone() {
    let d = duo();
    let t0 = std::time::Instant::now();
    let mut syncer = syncer::Syncer::new(t0);
    let remotes = [Remote::Drive, Remote::Folder("/icloud/vault".into())];
    let folder_calls = std::cell::Cell::new(0);
    let mut run = |r: &Remote| {
        if *r == Remote::Drive {
            Err(SyncError::SignIn)
        } else {
            folder_calls.set(folder_calls.get() + 1);
            Ok(figlock_session::sync::Report::default())
        }
    };
    for n in 0..3 {
        syncer.step(
            t0 + Duration::from_secs(n * 20),
            &d.broker,
            &remotes,
            &mut run,
        );
    }
    assert_eq!(folder_calls.get(), 3, "the folder keeps syncing");
    let failed = d
        .events()
        .iter()
        .filter(|(e, _)| e == "sync.failed")
        .count();
    assert_eq!(failed, 1, "Drive was blocked once");
}

#[test]
fn an_expired_sign_in_is_recorded_once_by_code_and_sync_stops_trying() {
    let d = duo();
    let t0 = std::time::Instant::now();
    let mut syncer = syncer::Syncer::new(t0);
    let calls = std::cell::Cell::new(0);
    let mut expired = |_: &Remote| {
        calls.set(calls.get() + 1);
        Err(SyncError::SignIn)
    };
    for n in 0..3 {
        assert!(syncer.step(
            t0 + Duration::from_secs(n * 600),
            &d.broker,
            &[Remote::Drive],
            &mut expired
        ));
    }
    assert_eq!(calls.get(), 1);
    let events = d.events();
    let failed: Vec<_> = events.iter().filter(|(e, _)| e == "sync.failed").collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(
        Value::Object(failed[0].1.clone()),
        json!({"reason": "sign_in"})
    );
    assert!(
        !d.broker.is_locked(),
        "the broker keeps serving what it has"
    );
}

#[test]
fn a_sync_that_brings_a_revocation_stops_the_syncer() {
    let mut d = duo();
    d.b_revokes_the_broker_device();
    let t0 = std::time::Instant::now();
    let mut syncer = syncer::Syncer::new(t0);
    let mut ok = |_: &Remote| {
        Ok(figlock_session::sync::Report {
            pulled: 3,
            pushed: 0,
            warnings: Vec::new(),
        })
    };
    assert!(!syncer.step(t0, &d.broker, &[Remote::Drive], &mut ok));
    assert!(syncer.locked_by_change);
    assert!(d.broker.is_locked());
}

#[test]
fn without_a_remote_the_local_folder_is_still_looked_at() {
    let mut d = duo();
    let id = d.b_adds("Written by another command");
    let t0 = std::time::Instant::now();
    let mut syncer = syncer::Syncer::new(t0);
    assert!(syncer.step(
        t0 + Duration::from_secs(16),
        &d.broker,
        &[],
        &mut |_: &Remote| Err(SyncError::NotEnabled)
    ));
    assert_eq!(d.grant(item_scope(&id))["ok"], true);
}

// ---- FIG-146 / ADR-0039: one prompt per request, nothing for a client that left, reuse of a grant ----

fn agent(pid: u32) -> PeerId {
    PeerId {
        parents: vec!["/bin/zsh".into(), "/opt/agent/agent".into()],
        ..peer(pid)
    }
}

fn token_of(r: &Value) -> String {
    r["grant"]["token"].as_str().unwrap().to_owned()
}

#[test]
fn a_repeat_request_inside_an_approved_grant_gets_a_new_token_and_no_prompt() {
    let fx = fixture();
    let first = fx.approver_will_allow_then(&agent(100), "github", "http", "15m");
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    // A new process each time: the pid differs, the verified program chain is the same.
    let again = fx.request(&agent(200), json!({"name": "github"}), "http", "15m");
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(
        fx.approver.calls.load(Ordering::SeqCst),
        calls,
        "nobody was asked"
    );
    assert_eq!(again["grant"]["id"], first["grant"]["id"]);
    assert_ne!(token_of(&again), token_of(&first));
    assert_eq!(fx.grants(), 1);
    // Both tokens work, and the expiry was not extended.
    let host = Some("api.github.com");
    for t in [token_of(&first), token_of(&again)] {
        assert!(
            fx.broker
                .authorize(&t, &fx.github, Mode::Http, host)
                .is_ok()
        );
    }
    fx.env.advance(Duration::from_secs(14 * 60));
    let late = fx.request(&agent(300), json!({"name": "github"}), "http", "15m");
    assert!(
        late["grant"]["expires_in"].as_u64().unwrap() <= 60,
        "{late}"
    );
    // Ending the grant ends every token of it.
    let id = first["grant"]["id"].as_str().unwrap();
    let r = fx.send(
        &agent(1),
        json!({"v":1,"id":"k","op":"grant.revoke","grant":id}),
    );
    assert_eq!(r["ok"], true);
    for t in [token_of(&first), token_of(&again), token_of(&late)] {
        assert!(
            fx.broker
                .authorize(&t, &fx.github, Mode::Http, host)
                .is_err()
        );
    }
}

#[test]
fn a_reused_request_is_logged_as_a_request_that_names_the_grant_and_has_no_approval() {
    let fx = fixture();
    let first = fx.approver_will_allow_then(&agent(100), "github", "http", "15m");
    let _ = fx.request(&agent(200), json!({"name": "github"}), "http", "15m");
    let names = fx.names();
    assert_eq!(
        names.iter().filter(|n| *n == "grant.approved").count(),
        1,
        "{names:?}"
    );
    let requested: Vec<_> = fx
        .events()
        .into_iter()
        .filter(|(e, _)| e == "grant.requested")
        .collect();
    assert_eq!(requested.len(), 2);
    assert!(requested[0].1.get("grant").is_none());
    assert_eq!(requested[1].1["grant"], first["grant"]["id"]);
}

#[test]
fn what_a_grant_does_not_cover_still_asks() {
    let fx = fixture();
    let p = agent(100);
    let _ = fx.approver_will_allow_then(&p, "github", "http", "15m");
    let asked = |fx: &Fx, p: &PeerId, name: &str, mode: &str, ttl: &str| {
        let before = fx.approver.calls.load(Ordering::SeqCst);
        fx.approver.will(&[Decision::Allow]);
        let r = fx.request(p, json!({"name": name}), mode, ttl);
        let _ = r;
        fx.approver.calls.load(Ordering::SeqCst) > before
    };
    // Longer, single use, another item or mode, another program chain, and a peer nobody can identify.
    assert!(asked(&fx, &p, "github", "http", "1d"));
    assert!(asked(&fx, &p, "github", "http", "once"));
    assert!(asked(&fx, &p, "github", "http", "session"));
    assert!(asked(&fx, &p, "deploy env", "run", "15m"));
    assert!(asked(&fx, &p, "github", "run", "15m"));
    let other = PeerId {
        parents: vec!["/bin/zsh".into(), "/opt/other/other".into()],
        ..peer(101)
    };
    assert!(asked(&fx, &other, "github", "http", "15m"));
    assert!(
        asked(&fx, &peer(102), "github", "http", "15m"),
        "no parents: always asked"
    );
    // Reveal is never reused.
    fx.approver.will(&[Decision::Allow, Decision::Allow]);
    let before = fx.approver.calls.load(Ordering::SeqCst);
    for _ in 0..2 {
        let r = fx.request(&p, item_scope(&fx.github), "reveal", "once");
        assert_eq!(r["ok"], true, "{r}");
    }
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), before + 2);
}

#[test]
fn a_shorter_request_is_covered_by_a_longer_grant_and_an_expired_grant_covers_nothing() {
    let fx = fixture();
    let p = agent(100);
    let first = fx.approver_will_allow_then(&p, "github", "http", "1d");
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    let again = fx.request(&agent(101), json!({"name": "github"}), "http", "15m");
    assert_eq!(again["grant"]["id"], first["grant"]["id"], "{again}");
    assert_eq!(
        again["grant"]["ttl"], "1d",
        "the answer says what covers it"
    );
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), calls);
    fx.env.advance(Duration::from_secs(25 * 60 * 60));
    fx.approver.will(&[Decision::Allow]);
    let r = fx.request(&agent(102), json!({"name": "github"}), "http", "1d");
    assert_eq!(r["ok"], true);
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), calls + 1);
}

#[test]
fn a_grant_keeps_at_most_32_tokens_and_the_oldest_goes_first() {
    let fx = fixture();
    let first = fx.approver_will_allow_then(&agent(1), "github", "http", "15m");
    let mut tokens = vec![token_of(&first)];
    for pid in 2..40 {
        let r = fx.request(&agent(pid), json!({"name": "github"}), "http", "15m");
        tokens.push(token_of(&r));
    }
    assert_eq!(fx.grants(), 1);
    let host = Some("api.github.com");
    let works = |t: &String| fx.broker.authorize(t, &fx.github, Mode::Http, host).is_ok();
    assert!(!works(&tokens[0]), "the oldest token was dropped");
    assert!(works(tokens.last().unwrap()));
}

/// The first prompt stays open until `n` requests have reached the gate, so the others are always queued
/// behind it (no sleeping, no timing margin).
fn hold_the_prompt_until_all_are_queued(fx: &Arc<Fx>, n: usize) {
    let broker = fx.broker.clone();
    *fx.approver.hook.lock().unwrap() = Some(Box::new(move || {
        let give_up = std::time::Instant::now() + Duration::from_secs(30);
        while broker.at_gate.load(Ordering::SeqCst) < n && std::time::Instant::now() < give_up {
            std::thread::yield_now();
        }
    }));
}

#[test]
fn identical_requests_that_queue_behind_a_prompt_cost_one_prompt() {
    let fx = Arc::new(fixture());
    fx.approver.will(&[Decision::Allow]);
    hold_the_prompt_until_all_are_queued(&fx, 4);
    let threads: Vec<_> = (0..4u32)
        .map(|i| {
            let fx = fx.clone();
            std::thread::spawn(move || {
                fx.request(&agent(500 + i), json!({"name": "github"}), "http", "15m")
            })
        })
        .collect();
    let answers: Vec<Value> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), 1, "{answers:?}");
    assert_eq!(fx.approver.max_in_flight.load(Ordering::SeqCst), 1);
    for a in &answers {
        assert_eq!(a["ok"], true, "{a}");
    }
    assert_eq!(fx.grants(), 1);
}

#[test]
fn identical_requests_that_queue_behind_a_prompt_that_is_denied_are_denied_too() {
    let fx = Arc::new(fixture());
    fx.approver.will(&[Decision::Deny]);
    hold_the_prompt_until_all_are_queued(&fx, 4);
    let threads: Vec<_> = (0..4u32)
        .map(|i| {
            let fx = fx.clone();
            std::thread::spawn(move || {
                fx.request(&agent(600 + i), json!({"name": "github"}), "http", "15m")
            })
        })
        .collect();
    let answers: Vec<Value> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), 1, "{answers:?}");
    for a in &answers {
        assert!(matches!(Fx::code(a), "denied" | "busy"), "{a}");
    }
    assert_eq!(fx.grants(), 0);
}

#[test]
fn a_request_whose_client_has_gone_opens_no_prompt() {
    let fx = fixture();
    let p = agent(100);
    let line = json!({"v":1,"id":"r","op":"grant.request","scope":{"name":"github"},
                      "mode":"http","ttl":"15m"})
    .to_string();
    fx.approver.will(&[Decision::Allow]);
    let r = fx.broker.handle_line_live(&p, &line, &|| false);
    assert_eq!(
        Fx::code(&serde_json::from_str::<Value>(&r.line).unwrap()),
        "denied"
    );
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(fx.grants(), 0);
    assert!(!fx.names().contains(&"grant.requested".to_owned()));
}

#[test]
fn a_client_that_leaves_while_the_person_decides_gets_no_grant() {
    let fx = fixture();
    let p = agent(100);
    let line = json!({"v":1,"id":"r","op":"grant.request","scope":{"name":"github"},
                      "mode":"http","ttl":"15m"})
    .to_string();
    fx.approver.will(&[Decision::Allow]);
    // Present when the prompt opens, gone when it is answered.
    let polls = AtomicUsize::new(0);
    let r = fx
        .broker
        .handle_line_live(&p, &line, &|| polls.fetch_add(1, Ordering::SeqCst) < 1);
    assert_eq!(
        Fx::code(&serde_json::from_str::<Value>(&r.line).unwrap()),
        "denied"
    );
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fx.grants(), 0);
    assert!(!fx.names().contains(&"grant.approved".to_owned()));
}

#[test]
fn the_server_sees_a_client_that_closed_its_end() {
    use std::os::unix::net::UnixStream;
    let (a, b) = UnixStream::pair().unwrap();
    assert!(server::still_connected(&a));
    drop(b);
    assert!(!server::still_connected(&a));
}

impl Fx {
    fn grants(&self) -> usize {
        self.broker.grant_count()
    }

    /// Approve one request for `name` and return the answer.
    fn approver_will_allow_then(&self, p: &PeerId, name: &str, mode: &str, ttl: &str) -> Value {
        self.approver.will(&[Decision::Allow]);
        let r = self.request(p, json!({"name": name}), mode, ttl);
        assert_eq!(r["ok"], true, "{r}");
        r
    }
}

// ---- FIG-148 / ADR-0040: the broker serves a vault some other part of the program holds ----

/// A host that owns nothing itself: it wraps another and counts how often the broker told it to lock.
struct Spy {
    inner: host::LocalHost,
    locks: AtomicUsize,
}

impl host::SessionHost for Spy {
    fn session(&self) -> Option<Box<dyn std::ops::DerefMut<Target = Session> + '_>> {
        self.inner.session()
    }
    fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }
    fn lock(&self) {
        self.locks.fetch_add(1, Ordering::SeqCst);
        self.inner.lock();
    }
}

#[test]
fn a_broker_on_a_host_it_does_not_own_serves_and_locks_through_it() {
    let vault = figlock_core::Vault::create(&random_hex(8), "L", KdfParams::FLOOR, T)
        .unwrap()
        .0;
    let host = Arc::new(Spy {
        inner: host::LocalHost::new(solo(vault)),
        locks: AtomicUsize::new(0),
    });
    let writer = AuditWriter::with_sink(Box::new(Mem::default()), 0, 0, ZERO_PREV);
    let broker = Broker::start_on(
        host.clone(),
        writer,
        Box::new(approve::DenyAll),
        Arc::new(TestEnv::new()),
        uid(),
    )
    .unwrap();
    let p = peer(1);
    let status = |b: &Broker| -> Value {
        serde_json::from_str(&b.handle_line(&p, r#"{"v":1,"id":"s","op":"status"}"#).line).unwrap()
    };
    assert_eq!(status(&broker)["locked"], false);
    // The host locks for its own reasons: the broker sees it and refuses to serve.
    host.lock();
    assert!(broker.is_locked());
    assert_eq!(status(&broker)["locked"], true);
    let r = broker.handle_line(&p, r#"{"v":1,"id":"g","op":"grant.list"}"#);
    assert_eq!(
        serde_json::from_str::<Value>(&r.line).unwrap()["error"]["code"],
        "locked"
    );
}

#[test]
fn locking_the_broker_locks_the_host() {
    let vault = figlock_core::Vault::create(&random_hex(8), "L", KdfParams::FLOOR, T)
        .unwrap()
        .0;
    let host = Arc::new(Spy {
        inner: host::LocalHost::new(solo(vault)),
        locks: AtomicUsize::new(0),
    });
    let writer = AuditWriter::with_sink(Box::new(Mem::default()), 0, 0, ZERO_PREV);
    let broker = Broker::start_on(
        host.clone(),
        writer,
        Box::new(approve::DenyAll),
        Arc::new(TestEnv::new()),
        uid(),
    )
    .unwrap();
    let r = broker.handle_line(&peer(1), r#"{"v":1,"id":"k","op":"lock"}"#);
    assert!(r.exit);
    assert_eq!(host.locks.load(Ordering::SeqCst), 1);
    assert!(host.is_locked());
}

// ---- FIG-149 / ADR-0040: one socket for a vault that locks and unlocks ----

fn started_broker() -> Arc<Broker> {
    let vault = figlock_core::Vault::create(&random_hex(8), "L", KdfParams::FLOOR, T)
        .unwrap()
        .0;
    let writer = AuditWriter::with_sink(Box::new(Mem::default()), 0, 0, ZERO_PREV);
    Arc::new(
        Broker::start(
            solo(vault),
            writer,
            Box::new(approve::DenyAll),
            Arc::new(TestEnv::new()),
            uid(),
        )
        .unwrap(),
    )
}

fn ask(hub: &hub::Hub, op: &str) -> Value {
    use server::Dispatch;
    let line = json!({"v":1,"id":"h","op":op}).to_string();
    serde_json::from_str(&hub.handle(&peer(1), &line, &|| true).line).unwrap()
}

#[test]
fn a_hub_with_nothing_unlocked_answers_locked_and_nothing_else() {
    use server::Dispatch;
    let hub = hub::Hub::new();
    assert_eq!(ask(&hub, "hello")["locked"], true);
    assert_eq!(ask(&hub, "status")["locked"], true);
    for op in ["grant.list", "lock"] {
        assert_eq!(Fx::code(&ask(&hub, op)), "locked", "{op}");
    }
    let bad = hub.handle(&peer(1), "not json", &|| true);
    assert_eq!(
        Fx::code(&serde_json::from_str::<Value>(&bad.line).unwrap()),
        "bad_request"
    );
    assert!(hub.admit(&peer(1)));
    let stranger = PeerId {
        uid: uid() + 1,
        ..peer(1)
    };
    assert!(!hub.admit(&stranger));
}

#[test]
fn a_hub_routes_to_the_current_broker_and_survives_it_locking() {
    use server::Dispatch;
    let hub = hub::Hub::new();
    let first = started_broker();
    hub.set(first.clone());
    assert_eq!(ask(&hub, "status")["locked"], false);
    // `lock` ends that broker, but the hub keeps serving.
    let line = json!({"v":1,"id":"k","op":"lock"}).to_string();
    let r = hub.handle(&peer(1), &line, &|| true);
    assert!(!r.exit, "the socket stays up for the next unlock");
    assert!(first.is_locked());
    assert_eq!(ask(&hub, "status")["locked"], true);
    // The next unlock is a new broker with no grants of the old one.
    hub.set(started_broker());
    assert_eq!(ask(&hub, "status")["locked"], false);
    assert_eq!(ask(&hub, "status")["grants"], 0);
    hub.clear();
    assert_eq!(ask(&hub, "hello")["locked"], true);
}

#[test]
fn a_hub_serves_a_real_socket_until_it_is_stopped() {
    let hub = Arc::new(hub::Hub::new());
    let tmp = tempfile::Builder::new()
        .prefix("fl")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = tmp.path().join("run").join("b.sock");
    let listener = server::bind(&socket).unwrap();
    assert!(server::live(&socket));
    let serving = {
        let hub = hub.clone();
        std::thread::spawn(move || server::serve(&listener, &hub))
    };
    let hello = client::call(&socket, "hello", json!({})).unwrap();
    assert_eq!(hello["locked"], true);
    hub.set(started_broker());
    assert_eq!(
        client::call(&socket, "status", json!({})).unwrap()["locked"],
        false
    );
    // A second broker may not take the socket while the hub answers on it.
    assert!(server::bind(&socket).is_err());
    hub.stop();
    serving.join().unwrap().unwrap();
}

#[test]
fn queued_identical_requests_never_cost_a_second_prompt_however_the_threads_interleave() {
    // The outcome of the prompt (the grant, or the denial) must be on record before the next request in line
    // is looked at; run it many times so a gap between the two shows up.
    let fx = Arc::new(fixture());
    let rounds = 150usize;
    for round in 0..rounds {
        let before = fx.approver.calls.load(Ordering::SeqCst);
        // Each round's prompt waits until its four requests have all reached the gate.
        let target = 4 * (round + 1);
        let broker = fx.broker.clone();
        *fx.approver.hook.lock().unwrap() = Some(Box::new(move || {
            let give_up = std::time::Instant::now() + Duration::from_secs(30);
            while broker.at_gate.load(Ordering::SeqCst) < target
                && std::time::Instant::now() < give_up
            {
                std::thread::yield_now();
            }
        }));
        let allow = round % 2 == 0;
        fx.approver.will(&[if allow {
            Decision::Allow
        } else {
            Decision::Deny
        }]);
        let threads: Vec<_> = (0..4u32)
            .map(|i| {
                let fx = fx.clone();
                std::thread::spawn(move || {
                    let pid = 1000 + u32::try_from(round).unwrap() * 4 + i;
                    fx.request(&agent(pid), json!({"name": "github"}), "http", "15m")
                })
            })
            .collect();
        let answers: Vec<Value> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        let prompts = fx.approver.calls.load(Ordering::SeqCst) - before;
        assert_eq!(prompts, 1, "round {round}: {answers:?}");
        for a in &answers {
            if allow {
                assert_eq!(a["ok"], true, "round {round}: {a}");
            } else {
                assert!(
                    matches!(Fx::code(a), "denied" | "busy"),
                    "round {round}: {a}"
                );
            }
        }
        // The cooldown after a denial ends, and the next round starts with no grant of this one.
        fx.env.advance(Duration::from_secs(6));
        for g in fx.send(&agent(1), json!({"v":1,"id":"l","op":"grant.list"}))["grants"]
            .as_array()
            .unwrap()
        {
            let id = g["id"].as_str().unwrap();
            fx.send(
                &agent(1),
                json!({"v":1,"id":"k","op":"grant.revoke","grant":id}),
            );
        }
    }
}

#[test]
fn the_next_request_in_line_cannot_get_between_the_answer_and_its_outcome() {
    for allow in [true, false] {
        let fx = Arc::new(fixture());
        // A long gap between "the person answered" and "the grant or denial is recorded".
        *fx.broker.stall_after_answer.lock().unwrap() = Duration::from_millis(400);
        fx.approver.will(&[if allow {
            Decision::Allow
        } else {
            Decision::Deny
        }]);
        let first = {
            let fx = fx.clone();
            std::thread::spawn(move || {
                fx.request(&agent(10), json!({"name": "github"}), "http", "15m")
            })
        };
        while fx.approver.calls.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        // Queued behind the prompt that has just been answered.
        let second = fx.request(&agent(11), json!({"name": "github"}), "http", "15m");
        let first = first.join().unwrap();
        assert_eq!(
            fx.approver.calls.load(Ordering::SeqCst),
            1,
            "allow={allow}: {first} / {second}"
        );
        if allow {
            assert_eq!(second["ok"], true, "{second}");
            assert_eq!(second["grant"]["id"], first["grant"]["id"]);
        } else {
            assert_eq!(Fx::code(&second), "denied", "{second}");
        }
    }
}

// ---- FIG-152: the cooldown after a denial follows the program, not the pid ----

#[test]
fn a_new_process_of_the_program_just_refused_is_told_busy_without_a_prompt() {
    let fx = fixture();
    fx.approver.will(&[Decision::Deny]);
    let first = fx.request(&agent(100), json!({"name": "github"}), "http", "15m");
    assert_eq!(Fx::code(&first), "denied");
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    // Every `figlock` call is a new process: a new pid, the same verified program and parents.
    for (pid, name) in [(101, "github"), (102, "deploy env"), (103, "no such item")] {
        let r = fx.request(&agent(pid), json!({"name": name}), "http", "15m");
        assert_eq!(Fx::code(&r), "busy", "{name}");
    }
    assert_eq!(
        fx.approver.calls.load(Ordering::SeqCst),
        calls,
        "nobody was asked again"
    );
    // After the cooldown the same program may ask again.
    fx.env.advance(Duration::from_secs(6));
    fx.approver.will(&[Decision::Allow]);
    let later = fx.request(&agent(104), json!({"name": "github"}), "http", "15m");
    assert_eq!(later["ok"], true, "{later}");
}

#[test]
fn another_program_is_not_slowed_by_a_denial_it_had_no_part_in() {
    let fx = fixture();
    fx.approver.will(&[Decision::Deny]);
    assert_eq!(
        Fx::code(&fx.request(&agent(100), json!({"name": "github"}), "http", "15m")),
        "denied"
    );
    let other = PeerId {
        parents: vec!["/bin/zsh".into(), "/opt/other/other".into()],
        ..peer(200)
    };
    fx.approver.will(&[Decision::Allow]);
    let r = fx.request(&other, json!({"name": "github"}), "http", "15m");
    assert_eq!(r["ok"], true, "{r}");
    // A peer that cannot be identified by its parents keeps only the pid rule.
    fx.approver.will(&[Decision::Deny]);
    assert_eq!(
        Fx::code(&fx.request(&peer(300), json!({"name": "github"}), "http", "once")),
        "denied"
    );
    fx.approver.will(&[Decision::Allow]);
    let other_pid = fx.request(&peer(301), json!({"name": "github"}), "http", "once");
    assert_eq!(other_pid["ok"], true, "{other_pid}");
}

// ---- FIG-151: a background broker locks itself when nobody is using it ----

#[test]
fn a_broker_is_idle_only_while_it_has_no_requests_and_no_live_grant() {
    let fx = fixture();
    assert_eq!(fx.broker.idle_for(), Some(Duration::ZERO));
    fx.env.advance(Duration::from_secs(60));
    assert_eq!(fx.broker.idle_for(), Some(Duration::from_secs(60)));
    // A request starts the count again.
    let _ = fx.send(&agent(1), json!({"v":1,"id":"s","op":"status"}));
    assert_eq!(fx.broker.idle_for(), Some(Duration::ZERO));
    // A live grant is never idle, however long it lasts.
    let _ = fx.approver_will_allow_then(&agent(2), "github", "http", "15m");
    fx.env.advance(Duration::from_secs(600));
    assert_eq!(fx.broker.idle_for(), None);
    // Once it has ended, the count starts from the last time it was seen alive. The watcher looks every few
    // seconds, so the first idle reading is at most that long.
    let mut first = None;
    for _ in 0..200 {
        fx.env.advance(Duration::from_secs(10));
        if let Some(idle) = fx.broker.idle_for() {
            first = Some(idle);
            break;
        }
    }
    assert!(
        first.is_some_and(|d| d <= Duration::from_secs(10)),
        "{first:?}"
    );
    fx.env.advance(Duration::from_secs(120));
    assert_eq!(
        fx.broker.idle_for(),
        Some(first.unwrap() + Duration::from_secs(120))
    );
}

#[test]
fn ending_an_idle_broker_ends_it_for_everyone() {
    let fx = fixture();
    fx.broker.end("idle");
    assert!(fx.broker.is_locked());
    let names = fx.names();
    assert_eq!(names.last().map(String::as_str), Some("broker.lock"));
    let lock = fx.events().into_iter().last().unwrap().1;
    assert_eq!(lock["reason"], "idle");
}

#[test]
fn a_locked_broker_is_unavailable_to_the_caller_and_other_refusals_are_not() {
    use figlock_session::error::FailKind;
    let locked =
        json!({"ok": false, "error": {"code": "locked", "message": "the broker is locked"}});
    assert_eq!(
        client::expect_ok(locked).unwrap_err().1,
        FailKind::Unavailable
    );
    let denied = json!({"ok": false, "error": {"code": "denied", "message": "no"}});
    assert_eq!(client::expect_ok(denied).unwrap_err().1, FailKind::Other);
}

// ---- FIG-191: the same agent is the same program whatever shells it went through ----

fn through(shells: &[&str], rest: &[&str], pid: u32) -> PeerId {
    PeerId {
        parents: shells.iter().chain(rest).map(|p| (*p).to_owned()).collect(),
        ..peer(pid)
    }
}

#[test]
fn the_same_agent_through_a_different_number_of_shells_is_the_same_program() {
    use grants::same_program;
    let agent = ["/opt/agent/claude", "/Applications/WezTerm.app/wezterm-gui"];
    let one = through(&["/bin/zsh"], &agent, 1);
    let three = through(&["/bin/zsh", "/bin/bash", "-zsh"], &agent, 2);
    let none = through(&[], &agent, 3);
    assert!(same_program(&one, &three));
    assert!(same_program(&one, &none));
    assert!(same_program(&three, &none));
    // Another agent, another program path, or another user is not the same.
    let other = through(&["/bin/zsh"], &["/opt/other/codex"], 4);
    assert!(!same_program(&one, &other));
    let other_exe = PeerId {
        exe: Some("/usr/local/bin/other".into()),
        ..one.clone()
    };
    assert!(!same_program(&one, &other_exe));
    let other_user = PeerId {
        uid: one.uid + 1,
        ..one.clone()
    };
    assert!(!same_program(&one, &other_user));
}

#[test]
fn a_chain_of_only_shells_is_never_the_same_program() {
    use grants::same_program;
    let a = through(&["/bin/zsh", "/bin/bash"], &[], 1);
    let b = through(&["/bin/zsh", "/bin/bash"], &[], 2);
    assert!(
        !same_program(&a, &b),
        "nothing says who is behind the shells"
    );
    assert!(!same_program(&peer(1), &peer(2)), "no parents at all");
}

#[test]
fn a_grant_is_reused_by_the_same_agent_through_another_shell_depth() {
    let fx = fixture();
    let agent = ["/opt/agent/claude"];
    let first = fx.approver_will_allow_then(
        &through(&["/bin/zsh"], &agent, 100),
        "github",
        "http",
        "15m",
    );
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    let again = fx.request(
        &through(&["/bin/zsh", "/bin/zsh", "/bin/sh"], &agent, 200),
        json!({"name": "github"}),
        "http",
        "15m",
    );
    assert_eq!(again["grant"]["id"], first["grant"]["id"], "{again}");
    assert_eq!(
        fx.approver.calls.load(Ordering::SeqCst),
        calls,
        "nobody was asked"
    );
}

#[test]
fn the_cooldown_after_a_denial_follows_the_agent_through_another_shell_depth() {
    let fx = fixture();
    let agent = ["/opt/agent/claude"];
    fx.approver.will(&[Decision::Deny]);
    let first = fx.request(
        &through(&["/bin/zsh"], &agent, 100),
        json!({"name": "github"}),
        "http",
        "15m",
    );
    assert_eq!(Fx::code(&first), "denied");
    let calls = fx.approver.calls.load(Ordering::SeqCst);
    let retry = fx.request(
        &through(&["/bin/zsh", "/bin/zsh"], &agent, 200),
        json!({"name": "github"}),
        "http",
        "15m",
    );
    assert_eq!(Fx::code(&retry), "busy", "{retry}");
    assert_eq!(fx.approver.calls.load(Ordering::SeqCst), calls);
}
