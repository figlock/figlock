//! End-to-end test of `figlock broker` as a person (or an agent) meets it: a real daemon process, its real
//! socket, the audit log it writes and `figlock audit` reading it back, with the negative cases from
//! `AGENTS.md` (denied request, tampered log, second broker, wrong password, secret in output).
//!
//! The daemon runs with `--deny-all`, so nothing here can open a prompt on the tester's terminal. The
//! approve path is covered over a real socket in `src/broker/tests.rs`.
#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

const PW: &str = "unit-test-master-password";
const SECRET: &str = "Zq9-broker-test-secret-9";

struct Env {
    /// Unix socket paths are short; the default temp folder on macOS is not.
    home: TempDir,
    store: TempDir,
    secret_key: String,
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

impl Env {
    fn new() -> Env {
        let mut e = Env {
            home: tempfile::Builder::new()
                .prefix("fl")
                .tempdir_in("/tmp")
                .unwrap(),
            store: TempDir::new().unwrap(),
            secret_key: String::new(),
        };
        let store = e.store.path().to_str().unwrap().to_owned();
        let out = e.run(
            &["init", "--store", &store, "--kdf", "floor"],
            PW,
            None,
            None,
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
        e.secret_key = text(&out.stdout)
            .lines()
            .find_map(|l| l.strip_prefix("Secret Key: "))
            .expect("secret key printed")
            .to_owned();
        e
    }

    fn command(&self, args: &[&str], pw: &str, sk: Option<&str>) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_figlock"));
        c.args(args)
            .env("FIGLOCK_HOME", self.home.path())
            .env("FIGLOCK_PASSWORD", pw)
            .env_remove("XDG_RUNTIME_DIR");
        if let Some(sk) = sk {
            c.env("FIGLOCK_SECRET_KEY", sk);
        }
        c
    }

    fn run(&self, args: &[&str], pw: &str, sk: Option<&str>, stdin: Option<&str>) -> Output {
        let mut c = self.command(args, pw, sk);
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
        c.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = c.spawn().unwrap();
        if let Some(input) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        child.wait_with_output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args, PW, Some(&self.secret_key), None);
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        text(&out.stdout) + &text(&out.stderr)
    }

    fn socket(&self) -> PathBuf {
        self.home.path().join("run").join("broker.sock")
    }

    fn audit_file(&self) -> PathBuf {
        let dir = self.home.path().join("audit");
        let mut files: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        files.remove(0)
    }

    fn start_broker(&self) -> Child {
        let mut c = self.command(
            &["broker", "start", "--deny-all"],
            PW,
            Some(&self.secret_key),
        );
        c.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = c.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while UnixStream::connect(self.socket()).is_err() {
            assert!(Instant::now() < deadline, "the broker did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        child
    }
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn raw(socket: &Path, line: &str) -> Value {
    let mut s = UnixStream::connect(socket).unwrap();
    s.write_all(line.as_bytes()).unwrap();
    s.write_all(b"\n").unwrap();
    let mut out = String::new();
    BufReader::new(&s).read_line(&mut out).unwrap();
    serde_json::from_str(&out).unwrap_or_else(|_| panic!("not json: {out:?}"))
}

#[test]
fn a_broker_serves_denies_logs_and_the_log_verifies() {
    let env = Env::new();
    let add = env.run(
        &[
            "add",
            "--type",
            "token",
            "--title",
            "GitHub",
            "--secret",
            "token",
            "--set",
            "allowed_hosts/ab12=api.github.com",
        ],
        PW,
        Some(&env.secret_key),
        Some(&format!("{SECRET}\n")),
    );
    assert!(add.status.success(), "{}", text(&add.stderr));
    let item = text(&add.stdout).trim().to_owned();

    let mut broker = env.start_broker();

    // Permissions: the folder is private and the socket is closed to everyone but the owner.
    assert_eq!(mode(&env.socket()), 0o600);
    assert_eq!(mode(env.socket().parent().unwrap()), 0o700);
    assert_eq!(mode(&env.audit_file()), 0o600);
    assert_eq!(mode(env.audit_file().parent().unwrap()), 0o700);

    // A second broker for the same user is refused while one is alive.
    let second = env.run(
        &["broker", "start", "--deny-all"],
        PW,
        Some(&env.secret_key),
        None,
    );
    assert!(!second.status.success());
    assert!(
        text(&second.stderr).contains("already running"),
        "{}",
        text(&second.stderr)
    );

    assert!(
        env.ok(&["broker", "status"])
            .contains("unlocked, 0 grant(s)")
    );
    assert!(env.ok(&["broker", "grants"]).contains("no active grants"));

    // Asked for a secret, with nobody to approve: denied, with no token anywhere in the answer.
    let item_scope = json!({"item": item});
    let req = json!({"v":1,"id":"x","op":"grant.request","scope":item_scope,"mode":"http","ttl":"15m",
                     "label":"Anthropic Claude Code","reason":"deploy"});
    let r = raw(&env.socket(), &req.to_string());
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"]["code"], "denied");
    assert!(r.get("grant").is_none());
    // Using a made-up token gets nothing either.
    let bogus = raw(
        &env.socket(),
        r#"{"v":1,"id":"y","op":"grant.revoke","grant":"0123456789abcdef"}"#,
    );
    assert_eq!(bogus["error"]["code"], "no_grant");
    // Junk is refused.
    assert_eq!(
        raw(&env.socket(), "{nonsense")["error"]["code"],
        "bad_request"
    );

    // Lock: the broker forgets everything, exits, and removes its socket.
    assert!(env.ok(&["broker", "lock"]).contains("locked"));
    let status = broker.wait().unwrap();
    assert!(status.success());
    assert!(!env.socket().exists());
    let late = env.run(&["broker", "status"], PW, Some(&env.secret_key), None);
    assert!(!late.status.success());
    assert!(text(&late.stderr).contains("no broker is running"));

    // The log verifies and tells the story, and holds no secret.
    let audit = env.ok(&["audit"]);
    for ev in [
        "broker.start",
        "grant.requested",
        "grant.denied",
        "broker.lock",
    ] {
        assert!(audit.contains(ev), "{ev} missing from:\n{audit}");
    }
    assert!(!audit.contains(SECRET));
    let rows: Value = serde_json::from_str(&env.ok(&["--json", "audit"])).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 4);
    assert_eq!(rows[1]["event"], "grant.requested");
    assert!(
        rows[1]["detail"]["peer"]["uid"].is_u64(),
        "who asked is recorded"
    );
    assert_eq!(rows[1]["detail"]["label"], "Anthropic Claude Code");
    assert_eq!(env.ok(&["audit", "--tail", "1"]).lines().count(), 1);
    let on_disk = fs::read(env.audit_file()).unwrap();
    assert!(!String::from_utf8_lossy(&on_disk).contains(SECRET));
    assert!(
        !String::from_utf8_lossy(&on_disk).contains("grant.requested"),
        "records are encrypted"
    );

    // A flipped bit anywhere stops both `audit` and the next broker, which leaves the file alone.
    let mut bad = on_disk.clone();
    let middle = bad.len() / 2;
    bad[middle] ^= 1;
    fs::write(env.audit_file(), &bad).unwrap();
    let shown = env.run(&["audit"], PW, Some(&env.secret_key), None);
    assert!(!shown.status.success());
    assert!(
        text(&shown.stderr).contains("failed verification"),
        "{}",
        text(&shown.stderr)
    );
    let restart = env.run(
        &["broker", "start", "--deny-all"],
        PW,
        Some(&env.secret_key),
        None,
    );
    assert!(!restart.status.success());
    assert!(text(&restart.stderr).contains("failed verification"));
    assert!(
        !env.socket().exists(),
        "no socket for a broker that did not start"
    );
    assert_eq!(
        fs::read(env.audit_file()).unwrap(),
        bad,
        "the damaged log was not touched"
    );

    // Put the good log back: the broker starts again and the chain continues.
    fs::write(env.audit_file(), &on_disk).unwrap();
    let mut again = env.start_broker();
    assert!(env.ok(&["broker", "lock"]).contains("locked"));
    assert!(again.wait().unwrap().success());
    let rows: Value = serde_json::from_str(&env.ok(&["--json", "audit"])).unwrap();
    assert_eq!(
        rows.as_array().unwrap().len(),
        6,
        "start and lock were appended"
    );
}

#[test]
fn the_wrong_password_or_secret_key_never_opens_a_socket() {
    let env = Env::new();
    let other = Env::new();
    for (pw, sk) in [
        ("wrong password", env.secret_key.as_str()),
        (PW, other.secret_key.as_str()),
    ] {
        let sk = Some(sk);
        let out = env.run(&["broker", "start", "--deny-all"], pw, sk, None);
        assert!(!out.status.success());
        assert!(
            text(&out.stderr).contains("could not unlock"),
            "{}",
            text(&out.stderr)
        );
        assert!(!env.socket().exists());
    }
}

#[test]
fn talking_to_a_missing_broker_says_so() {
    let env = Env::new();
    for cmd in [
        ["broker", "status"],
        ["broker", "grants"],
        ["broker", "lock"],
    ] {
        let out = env.run(&cmd, PW, Some(&env.secret_key), None);
        assert!(!out.status.success());
        assert!(text(&out.stderr).contains("no broker is running"));
    }
    // An empty log is fine to show.
    let out = env.run(&["audit"], PW, Some(&env.secret_key), None);
    assert!(out.status.success(), "{}", text(&out.stderr));
}
