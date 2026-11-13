//! End-to-end test of adding a device from the command line: `figlock device add` on a trusted device,
//! `figlock join` on a new one, and `figlock open` with the Secret Key. Two homes stand for two machines that
//! share one vault folder. Every value is generated per run.
//!
//! Devices here keep no Secret Key (`FIGLOCK_DEVICE_KEYS=file`), so commands after the setup are given the
//! Secret Key from the environment, as a script would.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Output, Stdio};

use tempfile::TempDir;

struct Machine {
    home: TempDir,
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

impl Machine {
    fn new() -> Machine {
        Machine {
            home: TempDir::new().unwrap(),
        }
    }

    fn command(&self, args: &[&str], password: &str, secret_key: Option<&str>) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_figlock"));
        c.args(args)
            .env("FIGLOCK_HOME", self.home.path())
            .env("FIGLOCK_DEVICE_KEYS", "file")
            .env("FIGLOCK_PASSWORD", password)
            .env_remove("FIGLOCK_SECRET_KEY")
            .env_remove("XDG_RUNTIME_DIR")
            .stdin(Stdio::null());
        if let Some(key) = secret_key {
            c.env("FIGLOCK_SECRET_KEY", key);
        }
        c
    }

    fn run(&self, args: &[&str], password: &str, secret_key: Option<&str>) -> Output {
        self.command(args, password, secret_key).output().unwrap()
    }

    fn spawn(&self, args: &[&str], password: &str, secret_key: Option<&str>) -> Child {
        let mut c = self.command(args, password, secret_key);
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
        c.spawn().unwrap()
    }
}

/// A password for one test: generated, never a literal.
fn password() -> String {
    format!("pw-{}", hex::encode(rand_bytes()))
}

fn rand_bytes() -> [u8; 8] {
    let key = figlock_core::SecretKey::random().unwrap();
    key.expose()[..8].try_into().unwrap()
}

struct Vault {
    store: TempDir,
    first: Machine,
    password: String,
    secret_key: String,
}

/// A vault with one item, created on the first machine.
fn vault() -> Vault {
    let v = Vault {
        store: TempDir::new().unwrap(),
        first: Machine::new(),
        password: password(),
        secret_key: String::new(),
    };
    let store = v.store.path().to_str().unwrap();
    let out = v.first.run(
        &[
            "init",
            "--store",
            store,
            "--kdf",
            "floor",
            "--no-quick-unlock",
        ],
        &v.password,
        None,
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let secret_key = text(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Secret Key: "))
        .expect("secret key printed")
        .to_owned();
    let v = Vault { secret_key, ..v };
    let added = v.first.run(
        &[
            "add",
            "-t",
            "note",
            "--title",
            "Shared note",
            "--set",
            "body=hello",
        ],
        &v.password,
        Some(&v.secret_key),
    );
    assert!(added.status.success(), "{}", text(&added.stderr));
    v
}

/// `device add` on the first machine, up to the invite code it prints.
fn invite(v: &Vault, extra: &[&str]) -> (Child, String) {
    let mut args = vec!["device", "add"];
    args.extend_from_slice(extra);
    let mut child = v.first.spawn(&args, &v.password, Some(&v.secret_key));
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    let code = line.trim().to_owned();
    assert!(!code.is_empty(), "no invite code printed");
    (child, code)
}

fn finds_note(m: &Machine, v: &Vault) -> bool {
    let out = m.run(&["find", "shared"], &v.password, Some(&v.secret_key));
    out.status.success() && text(&out.stdout).contains("Shared note")
}

#[test]
fn a_new_device_joins_when_the_trusted_one_approves() {
    let v = vault();
    let store = v.store.path().to_str().unwrap();
    let (adder, code) = invite(&v, &["--yes"]);
    let second = Machine::new();
    let joined = second.run(
        &[
            "join",
            &code,
            "--store",
            store,
            "--device-name",
            "second",
            "--no-quick-unlock",
        ],
        &v.password,
        None,
    );
    assert!(joined.status.success(), "{}", text(&joined.stderr));
    let added = adder.wait_with_output().unwrap();
    assert!(added.status.success(), "{}", text(&added.stderr));
    assert!(text(&added.stdout).contains("Added \"second\""));
    assert!(finds_note(&second, &v));
    let devices = second.run(&["device", "list"], &v.password, Some(&v.secret_key));
    assert_eq!(text(&devices.stdout).lines().count(), 2);
}

#[test]
fn an_invite_that_is_not_approved_adds_nothing() {
    let v = vault();
    let store = v.store.path().to_str().unwrap();
    // Without --yes and with no terminal, the question is answered No.
    let (adder, code) = invite(&v, &[]);
    let second = Machine::new();
    let joined = second.run(
        &["join", &code, "--store", store, "--timeout", "6"],
        &v.password,
        None,
    );
    assert!(!joined.status.success());
    let added = adder.wait_with_output().unwrap();
    assert!(!added.status.success());
    assert!(text(&added.stderr).contains("not approved"));
    assert!(!finds_note(&second, &v), "the device was never set up");
}

#[test]
fn a_wrong_master_password_does_not_set_the_device_up() {
    let v = vault();
    let store = v.store.path().to_str().unwrap();
    let (adder, code) = invite(&v, &["--yes"]);
    let second = Machine::new();
    let joined = second.run(
        &["join", &code, "--store", store, "--no-quick-unlock"],
        &password(),
        None,
    );
    assert!(!joined.status.success());
    assert!(text(&joined.stderr).contains("wrong master password"));
    assert!(adder.wait_with_output().unwrap().status.success());
    assert!(!finds_note(&second, &v));
}

#[test]
fn a_wrong_invite_code_is_refused() {
    let v = vault();
    let store = v.store.path().to_str().unwrap();
    let second = Machine::new();
    let joined = second.run(&["join", "not a code", "--store", store], &v.password, None);
    assert!(!joined.status.success());
    assert!(text(&joined.stderr).contains("invite code"));
}

#[test]
fn open_unlocks_with_the_master_password_and_secret_key() {
    let v = vault();
    let store = v.store.path().to_str().unwrap();
    let second = Machine::new();
    let opened = second.run(
        &[
            "open",
            "--store",
            store,
            "--device-name",
            "second",
            "--no-quick-unlock",
        ],
        &v.password,
        Some(&v.secret_key),
    );
    assert!(opened.status.success(), "{}", text(&opened.stderr));
    assert!(finds_note(&second, &v));
}

#[test]
fn open_with_a_wrong_secret_key_sets_nothing_up() {
    let v = vault();
    let store = v.store.path().to_str().unwrap();
    let second = Machine::new();
    let other = figlock_core::SecretKey::random().unwrap().format();
    let opened = second.run(
        &["open", "--store", store, "--no-quick-unlock"],
        &v.password,
        Some(other.as_str()),
    );
    assert!(!opened.status.success());
    assert!(!finds_note(&second, &v));
    // Nothing was left behind, so the right key still works.
    let again = second.run(
        &["open", "--store", store, "--no-quick-unlock"],
        &v.password,
        Some(&v.secret_key),
    );
    assert!(again.status.success(), "{}", text(&again.stderr));
}
