//! End-to-end tests of `figlock health` against a real folder store: weak, reused and old passwords, the
//! exit code, JSON output, and the breach check against a local stand-in for the range service. The stand-in
//! records every request, so these also check what would have gone over the network: only a 5 character hash
//! prefix, never a password. Passwords are generated here, never literal secrets of the user.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;
use sha1::{Digest, Sha1};
use tempfile::TempDir;

const PW: &str = "unit-test-password";
/// Rated weak by the strength estimate.
const WEAK: &str = "password1";

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

struct Env {
    home: TempDir,
    store: TempDir,
    secret_key: String,
    extra: Vec<(String, String)>,
}

impl Env {
    fn init() -> Env {
        let mut e = Env {
            home: TempDir::new().unwrap(),
            store: TempDir::new().unwrap(),
            secret_key: String::new(),
            extra: Vec::new(),
        };
        let store = e.store.path().to_str().unwrap().to_owned();
        let out = e.exec(&["init", "--store", &store, "--kdf", "floor"], "", None);
        assert!(out.status.success(), "{}", text(&out.stderr));
        e.secret_key = text(&out.stdout)
            .lines()
            .find_map(|l| l.strip_prefix("Secret Key: "))
            .expect("secret key printed")
            .to_owned();
        e
    }

    fn exec(&self, args: &[&str], stdin: &str, sk: Option<&str>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_figlock"));
        c.args(args)
            .env("FIGLOCK_HOME", self.home.path())
            .env("FIGLOCK_DEVICE_KEYS", "file")
            .env("FIGLOCK_PASSWORD", PW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(sk) = sk {
            c.env("FIGLOCK_SECRET_KEY", sk);
        }
        for (k, v) in &self.extra {
            c.env(k, v);
        }
        let mut child = c.spawn().unwrap();
        // The CLI may exit before reading stdin, so a broken pipe is fine.
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.exec(args, "", Some(&self.secret_key))
    }

    fn add(&self, title: &str, password: &str) {
        let out = self.exec(
            &[
                "add", "--type", "login", "--title", title, "--secret", "password",
            ],
            &format!("{password}\n"),
            Some(&self.secret_key),
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
    }

    fn strong(&self) -> String {
        text(&self.run(&["generate", "--length", "32"]).stdout)
            .trim()
            .to_owned()
    }

    fn health_json(&self, args: &[&str]) -> (Value, Output) {
        let mut a = vec!["--json", "health"];
        a.extend_from_slice(args);
        let out = self.run(&a);
        (serde_json::from_str(&text(&out.stdout)).unwrap(), out)
    }
}

/// A local range service. It answers every prefix with the line for `listed` (when its hash starts with the
/// requested prefix) plus a zero-count padding line, and records each request path.
struct RangeService {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
}

fn range_service(listed: &str, count: u64) -> RangeService {
    let hash: String = Sha1::digest(listed.as_bytes())
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/range/", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = req.split_whitespace().nth(1).unwrap_or_default().to_owned();
            let prefix = path.rsplit('/').next().unwrap_or_default().to_owned();
            log.lock().unwrap().push(path);
            let mut body = String::from("0018A45C4D1DEF81644B54AB7F969B88D65:0\r\n");
            if hash.starts_with(&prefix) {
                body.push_str(&format!("{}:{count}\r\n", &hash[5..]));
            }
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    RangeService { base, seen }
}

#[test]
fn a_vault_of_strong_unique_passwords_is_clean_and_exits_zero() {
    let e = Env::init();
    e.add("Mail", &e.strong());
    e.add("Bank", &e.strong());
    let out = e.run(&["health"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).is_empty());
    assert!(text(&out.stderr).contains("no problems found"));
    let (json, _) = e.health_json(&[]);
    assert_eq!(json["checked"], 2);
    assert_eq!(json["problems"], 0);
    assert_eq!(json["breach_checked"], false);
}

#[test]
fn weak_and_reused_passwords_are_listed_by_title_and_exit_nonzero() {
    let e = Env::init();
    let shared = e.strong();
    e.add("Weak One", WEAK);
    e.add("Twin A", &shared);
    e.add("Twin B", &shared);
    e.add("Fine", &e.strong());

    let out = e.run(&["health"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("Weak One") && stdout.contains("weak ("),
        "{stdout}"
    );
    assert!(
        stdout.contains("Twin A") && stdout.contains("Twin B"),
        "{stdout}"
    );
    assert!(stdout.contains("reused with 1 other item"), "{stdout}");
    assert!(!stdout.contains("Fine"), "{stdout}");
    for secret in [WEAK, shared.as_str()] {
        assert!(!stdout.contains(secret) && !text(&out.stderr).contains(secret));
    }

    let (json, out) = e.health_json(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json["checked"], 4);
    assert_eq!(json["problems"], 3);
    let items = json["items"].as_array().unwrap();
    let weak = items.iter().find(|i| i["title"] == "Weak One").unwrap();
    assert!(weak["weak_bits"].as_u64().is_some());
    assert_eq!(weak["reused_with"], 0);
    let twin = items.iter().find(|i| i["title"] == "Twin A").unwrap();
    assert_eq!(twin["reused_with"], 1);
    assert!(twin["weak_bits"].is_null());
    assert!(!text(&out.stdout).contains(WEAK));

    // --no-fail keeps the report and turns the exit code off.
    let quiet = e.run(&["health", "--no-fail"]);
    assert!(quiet.status.success());
    assert!(text(&quiet.stdout).contains("Weak One"));
}

#[test]
fn old_days_sets_when_a_password_counts_as_old() {
    let e = Env::init();
    e.add("Fresh", &e.strong());
    assert!(e.run(&["health"]).status.success());
    let out = e.run(&["health", "--old-days", "0"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stdout).contains("Fresh") && text(&out.stdout).contains("unchanged for 0 days")
    );
}

#[test]
fn deleted_items_and_items_without_a_password_are_not_checked() {
    let e = Env::init();
    e.add("Gone", WEAK);
    e.add("Fine", &e.strong());
    assert!(e.run(&["edit", "gone", "--delete"]).status.success());
    let out = e.run(&["add", "--type", "note", "--title", "A note"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let (json, out) = e.health_json(&[]);
    assert!(out.status.success());
    assert_eq!(json["checked"], 1);
}

#[test]
fn breaches_sends_only_a_five_character_prefix_and_reports_the_count() {
    let mut e = Env::init();
    e.add("Leaked", WEAK);
    e.add("Safe", &e.strong());
    let svc = range_service(WEAK, 4242);
    e.extra
        .push(("FIGLOCK_PWNED_RANGE_URL".into(), svc.base.clone()));

    let (json, out) = e.health_json(&["--breaches"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json["breach_checked"], true);
    assert_eq!(json["breach_failed"], false);
    let leaked = json["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["title"] == "Leaked")
        .unwrap();
    assert_eq!(leaked["breached"], 4242);
    assert!(
        json["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["title"] != "Safe")
    );

    let seen = svc.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "one request per distinct prefix: {seen:?}");
    for path in &seen {
        let prefix = path.strip_prefix("/range/").expect("range path");
        assert_eq!(prefix.len(), 5, "{path}");
        assert!(prefix.bytes().all(|b| b.is_ascii_hexdigit()), "{path}");
    }
    assert!(!seen.iter().any(|p| p.contains(WEAK)));

    // Text output names the breach count too.
    let text_out = text(&e.run(&["health", "--breaches"]).stdout);
    assert!(
        text_out.contains("Leaked") && text_out.contains("in 4242 known breaches"),
        "{text_out}"
    );
}

#[test]
fn a_strong_password_found_in_a_breach_is_still_reported() {
    let mut e = Env::init();
    let pw = e.strong();
    e.add("Strong But Leaked", &pw);
    let svc = range_service(&pw, 7);
    e.extra
        .push(("FIGLOCK_PWNED_RANGE_URL".into(), svc.base.clone()));
    let (json, out) = e.health_json(&["--breaches"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json["items"][0]["title"], "Strong But Leaked");
    assert_eq!(json["items"][0]["breached"], 7);
    assert!(json["items"][0]["weak_bits"].is_null());
}

#[test]
fn without_breaches_nothing_goes_over_the_network() {
    let mut e = Env::init();
    e.add("Leaked", WEAK);
    let svc = range_service(WEAK, 1);
    e.extra
        .push(("FIGLOCK_PWNED_RANGE_URL".into(), svc.base.clone()));
    let (json, _) = e.health_json(&[]);
    assert_eq!(json["breach_checked"], false);
    assert!(svc.seen.lock().unwrap().is_empty());
}

#[test]
fn an_unreachable_breach_service_fails_soft() {
    let mut e = Env::init();
    e.add("Weak One", WEAK);
    // Nothing listens on port 1.
    e.extra.push((
        "FIGLOCK_PWNED_RANGE_URL".into(),
        "http://127.0.0.1:1/range/".into(),
    ));
    let (json, out) = e.health_json(&["--breaches"]);
    assert_eq!(json["breach_checked"], false);
    assert_eq!(json["breach_failed"], true);
    assert_eq!(json["items"][0]["title"], "Weak One");
    // The local findings still decide the exit code, and the warning says what was skipped.
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("could not reach the breach service"));

    // A clean vault stays a success when only the breach check failed.
    let clean = Env::init();
    clean.add("Fine", &clean.strong());
    let mut clean = clean;
    clean.extra.push((
        "FIGLOCK_PWNED_RANGE_URL".into(),
        "http://127.0.0.1:1/range/".into(),
    ));
    let (json, out) = clean.health_json(&["--breaches"]);
    assert!(out.status.success());
    assert_eq!(json["breach_failed"], true);
}

#[test]
fn the_wrong_password_reports_nothing() {
    let e = Env::init();
    e.add("Weak One", WEAK);
    let out = e.exec(&["health"], "", None);
    assert!(!out.status.success());
    assert!(text(&out.stdout).is_empty());
}
