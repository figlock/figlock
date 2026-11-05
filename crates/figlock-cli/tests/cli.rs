//! End-to-end tests of the `figlock` binary against a real folder store, the way a person uses it:
//! init, add, find, get, copy, edit, export, plus the negative cases from `AGENTS.md` (wrong password,
//! wrong Secret Key, tampered file, secret in output, hostile text).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

const PW: &str = "unit-test-master-password";
const SECRET: &str = "Zq9-test-secret-value-1";

struct Env {
    home: TempDir,
    store: TempDir,
    secret_key: String,
    extra: Vec<(String, String)>,
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
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
        let out = e.run_with(
            &["init", "--store", &store, "--kdf", "floor"],
            None,
            PW,
            None,
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        e.secret_key = stdout
            .lines()
            .find_map(|l| l.strip_prefix("Secret Key: "))
            .expect("secret key printed")
            .to_owned();
        e
    }

    fn run_with(&self, args: &[&str], stdin: Option<&str>, pw: &str, sk: Option<&str>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_figlock"));
        c.args(args)
            .env("FIGLOCK_HOME", self.home.path())
            .env("FIGLOCK_PASSWORD", pw)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(sk) = sk {
            c.env("FIGLOCK_SECRET_KEY", sk);
        }
        for (k, v) in &self.extra {
            c.env(k, v);
        }
        c.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = c.spawn().unwrap();
        if let Some(input) = stdin {
            // The CLI may reject its arguments and exit before reading stdin, so a broken pipe is fine.
            match child.stdin.take().unwrap().write_all(input.as_bytes()) {
                Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => panic!("{e}"),
                _ => {}
            }
        }
        child.wait_with_output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, None, PW, Some(&self.secret_key))
    }

    fn run_stdin(&self, args: &[&str], stdin: &str) -> Output {
        self.run_with(args, Some(stdin), PW, Some(&self.secret_key))
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        text(&out.stdout)
    }

    fn add_login(&self, title: &str, extra: &[&str]) -> String {
        let mut args = vec![
            "add", "--type", "login", "--title", title, "--secret", "password",
        ];
        args.extend_from_slice(extra);
        let out = self.run_stdin(&args, &format!("{SECRET}\n"));
        assert!(out.status.success(), "{}", text(&out.stderr));
        text(&out.stdout).trim().to_owned()
    }

    fn ops(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let log = self.store.path().join("log");
        for dev in fs::read_dir(log).unwrap() {
            for f in fs::read_dir(dev.unwrap().path()).unwrap() {
                files.push(f.unwrap().path());
            }
        }
        files.sort();
        files
    }
}

fn assert_no_secret(label: &str, out: &Output) {
    assert!(
        !text(&out.stdout).contains(SECRET),
        "{label}: secret on stdout"
    );
    assert!(
        !text(&out.stderr).contains(SECRET),
        "{label}: secret on stderr"
    );
}

#[test]
fn add_find_get_flow_never_prints_secrets_unasked() {
    let e = Env::init();
    let id = e.add_login(
        "Example Mail",
        &[
            "--set",
            "username=ada",
            "--set",
            "url/1=https://mail.example.com",
        ],
    );
    assert_eq!(id.len(), 32);
    e.add_login("Other Site", &[]);

    let found = e.run(&["find", "exmpl"]);
    assert!(found.status.success());
    assert_no_secret("find", &found);
    let listing = text(&found.stdout);
    assert!(
        listing.contains("Example Mail") && listing.contains("ada"),
        "{listing}"
    );
    assert!(!listing.contains("Other Site"));
    assert!(listing.starts_with(&id[..8]));

    let shown = e.run(&["get", &id[..8]]);
    assert_no_secret("get", &shown);
    let shown = text(&shown.stdout);
    assert!(shown.contains("password: ********"), "{shown}");
    assert!(shown.contains("username: ada"));

    let revealed = e.run(&["get", &id, "--reveal"]);
    assert!(text(&revealed.stdout).contains(SECRET));

    assert_eq!(e.ok(&["get", &id, "--field", "username"]).trim(), "ada");
    let refused = e.run(&["get", &id, "--field", "password"]);
    assert!(!refused.status.success());
    assert_no_secret("get --field password", &refused);

    let json: Value = serde_json::from_str(&e.ok(&["--json", "get", &id])).unwrap();
    assert_eq!(json["fields"]["username"], "ada");
    assert!(json["fields"].get("password").is_none());
    assert_eq!(json["secret_fields"][0], "password");
    let listing: Value = serde_json::from_str(&e.ok(&["--json", "find"])).unwrap();
    assert_eq!(listing.as_array().unwrap().len(), 2);
    assert!(!listing.to_string().contains(SECRET));
}

#[test]
fn edit_delete_restore_and_export() {
    let e = Env::init();
    let id = e.add_login(
        "Bank",
        &[
            "--set",
            "username=old",
            "--set",
            "url/1=https://bank.example.com",
        ],
    );

    e.ok(&["edit", &id, "--set", "username=new", "--unset", "url/1"]);
    let shown = e.ok(&["get", &id]);
    assert!(
        shown.contains("username: new") && !shown.contains("url/1"),
        "{shown}"
    );

    let out = e.run_stdin(&["edit", &id, "--secret", "password"], "rotated-value-2\n");
    assert!(out.status.success());
    assert!(e.ok(&["get", &id, "--reveal"]).contains("rotated-value-2"));

    let plain: Value = serde_json::from_str(&e.ok(&["export"])).unwrap();
    assert_eq!(plain["secrets_included"], false);
    assert!(!plain.to_string().contains("rotated-value-2"));
    let full = e.run(&["export", "--include-secrets"]);
    let full: Value = serde_json::from_str(&text(&full.stdout)).unwrap();
    assert_eq!(full["items"][0]["fields"]["password"], "rotated-value-2");
    assert!(text(&e.run(&["export", "--include-secrets"]).stderr).contains("secrets"));

    e.ok(&["edit", &id, "--delete"]);
    assert!(e.ok(&["find", "bank"]).trim().is_empty());
    assert!(!e.run(&["get", &id]).status.success());
    let exported: Value = serde_json::from_str(&e.ok(&["export"])).unwrap();
    assert!(exported["items"].as_array().unwrap().is_empty());
    let with_deleted: Value =
        serde_json::from_str(&e.ok(&["export", "--include-deleted"])).unwrap();
    assert_eq!(with_deleted["items"][0]["deleted"], true);

    e.ok(&["edit", &id, "--restore"]);
    assert!(e.ok(&["find", "bank"]).contains("Bank"));
}

#[test]
fn every_command_appends_a_chained_op_file() {
    let e = Env::init();
    e.add_login("One", &[]);
    e.add_login("Two", &[]);
    e.ok(&["edit", "two", "--set", "username=x"]);
    let names: Vec<String> = e
        .ops()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["00000001.op", "00000002.op", "00000003.op"]);
    assert!(text(&e.run(&["find"]).stdout).contains("One"));
}

#[test]
fn wrong_credentials_are_refused_without_detail() {
    let e = Env::init();
    e.add_login("Keep", &[]);
    let wrong_pw = e.run_with(&["find"], None, "not-the-password", Some(&e.secret_key));
    assert!(!wrong_pw.status.success());
    assert!(text(&wrong_pw.stderr).contains("could not unlock"));
    assert!(text(&wrong_pw.stdout).is_empty());

    let other_sk = "FL1-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA";
    let wrong_sk = e.run_with(&["find"], None, PW, Some(other_sk));
    assert!(!wrong_sk.status.success());
    assert!(text(&wrong_sk.stdout).is_empty());

    let mut typo = e.secret_key.clone();
    typo.pop();
    typo.push(if e.secret_key.ends_with('A') {
        'B'
    } else {
        'A'
    });
    let bad = e.run_with(&["find"], None, PW, Some(&typo));
    assert!(
        text(&bad.stderr).contains("not valid"),
        "{}",
        text(&bad.stderr)
    );
}

#[test]
fn input_mistakes_are_caught_before_anything_is_written() {
    let e = Env::init();
    let args = |extra: &[&'static str]| {
        let mut a = vec!["add", "--type", "login", "--title", "T"];
        a.extend_from_slice(extra);
        a
    };
    let on_argv = e.run(&args(&["--set", "password=hunter2-in-argv"]));
    assert!(!on_argv.status.success());
    assert!(text(&on_argv.stderr).contains("--secret"));
    assert!(!text(&on_argv.stderr).contains("hunter2-in-argv"));
    assert!(!e.run(&args(&["--set", "private_key=x"])).status.success());
    assert!(
        !e.run(&args(&["--set", "username=a", "--set", "username=b"]))
            .status
            .success()
    );
    assert!(
        !e.run_stdin(&args(&["--secret", "username"]), "x\n")
            .status
            .success()
    );
    assert!(
        !e.run(&args(&["--secret", "password"])).status.success(),
        "no stdin value"
    );
    assert!(
        !e.run(&["add", "--type", "bogus", "--title", "T"])
            .status
            .success()
    );
    assert!(!e.run(&["edit", "nothing-here"]).status.success());
    assert!(!e.run(&["get", "nothing-here"]).status.success());
    assert!(!e.run(&["edit", "abcd1234"]).status.success());
    assert!(
        !e.store.path().join("log").exists(),
        "failed commands must not write ops"
    );
}

#[test]
fn ambiguous_queries_list_candidates_instead_of_guessing() {
    let e = Env::init();
    e.add_login("Cloud Alpha", &[]);
    e.add_login("Cloud Beta", &[]);
    let out = e.run(&["get", "cloud"]);
    assert!(!out.status.success());
    let err = text(&out.stderr);
    assert!(
        err.contains("several items match")
            && err.contains("Cloud Alpha")
            && err.contains("Cloud Beta"),
        "{err}"
    );
    assert!(e.run(&["get", "alpha"]).status.success());
}

#[test]
fn tampered_op_file_is_ignored_with_a_warning() {
    let e = Env::init();
    e.add_login("Genuine", &[]);
    e.add_login("Victim", &[]);
    let second = e.ops().pop().unwrap();
    let mut bytes = fs::read(&second).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x01;
    fs::write(&second, bytes).unwrap();

    let out = e.run(&["find"]);
    assert!(out.status.success());
    assert!(
        text(&out.stderr).contains("failed verification"),
        "{}",
        text(&out.stderr)
    );
    let listing = text(&out.stdout);
    assert!(
        listing.contains("Genuine") && !listing.contains("Victim"),
        "{listing}"
    );
}

#[test]
fn stored_text_cannot_inject_terminal_escapes() {
    let e = Env::init();
    e.ok(&[
        "add",
        "--type",
        "login",
        "--title",
        "Evil\u{1b}[2J\u{7}title",
        "--set",
        "username=u\rsurprise",
    ]);
    let listing = text(&e.run(&["find"]).stdout);
    assert!(listing.contains("Evil?[2J?title"), "{listing:?}");
    assert!(!listing.contains('\u{1b}') && !listing.contains('\r'));
    let shown = e.ok(&["get", "evil"]);
    assert!(!shown.contains('\u{1b}') && !shown.contains('\r'));
}

#[test]
fn init_is_not_repeatable() {
    let e = Env::init();
    let store = e.store.path().to_str().unwrap();
    let again = e.run_with(
        &["init", "--store", store, "--kdf", "floor"],
        None,
        PW,
        None,
    );
    assert!(!again.status.success());
    assert!(text(&again.stderr).contains("already"));

    let other_home = Env {
        home: TempDir::new().unwrap(),
        store: TempDir::new().unwrap(),
        secret_key: String::new(),
        extra: Vec::new(),
    };
    let into_existing = other_home.run_with(
        &["init", "--store", store, "--kdf", "floor"],
        None,
        PW,
        None,
    );
    assert!(!into_existing.status.success());
    assert!(text(&into_existing.stderr).contains("vault already exists"));
    assert!(
        !other_home.run(&["find"]).status.success(),
        "a refused init sets nothing up"
    );
}

#[test]
fn failed_init_removes_only_what_it_wrote() {
    let blocked = Env {
        home: TempDir::new().unwrap(),
        store: TempDir::new().unwrap(),
        secret_key: String::new(),
        extra: Vec::new(),
    };
    fs::write(blocked.store.path().join("devices"), b"in the way").unwrap();
    let store = blocked.store.path().to_str().unwrap();
    let out = blocked.run_with(
        &["init", "--store", store, "--kdf", "floor"],
        None,
        PW,
        None,
    );
    assert!(!out.status.success());
    assert!(
        !blocked.store.path().join("vault.json").exists(),
        "the vault.json this run wrote is rolled back"
    );
    assert!(!blocked.run(&["find"]).status.success());

    let raced = Env {
        home: TempDir::new().unwrap(),
        store: TempDir::new().unwrap(),
        secret_key: String::new(),
        extra: Vec::new(),
    };
    let placeholder = raced.store.path().join(".vault.json.icloud");
    fs::write(&placeholder, b"other device's vault").unwrap();
    let store = raced.store.path().to_str().unwrap();
    let out = raced.run_with(
        &["init", "--store", store, "--kdf", "floor"],
        None,
        PW,
        None,
    );
    assert!(!out.status.success());
    assert_eq!(fs::read(&placeholder).unwrap(), b"other device's vault");
    assert!(!raced.store.path().join("devices").exists());
    assert!(!raced.run(&["find"]).status.success());
}

#[test]
fn before_init_commands_explain_what_to_do() {
    let home = TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_figlock"))
        .args(["find"])
        .env("FIGLOCK_HOME", home.path())
        .env("FIGLOCK_PASSWORD", PW)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("figlock init"));
}

#[cfg(unix)]
#[test]
fn device_keys_are_owner_only_and_refused_when_loosened() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::init();
    let keys = e.home.path().join("device.json");
    assert_eq!(
        fs::metadata(&keys).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(e.home.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o644)).unwrap();
    let out = e.run(&["find"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("readable by other users"));
}

#[test]
fn generate_prints_a_password_and_checks_bounds() {
    let e = Env::init();
    let p = e.ok(&["generate"]);
    assert_eq!(p.trim().len(), 20);
    assert_eq!(
        e.ok(&["generate", "--length", "40", "--no-symbols"])
            .trim()
            .len(),
        40
    );
    assert!(
        e.ok(&["generate", "--no-symbols"])
            .trim()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric())
    );
    assert!(!e.run(&["generate", "--length", "3"]).status.success());
    let json: Value = serde_json::from_str(&e.ok(&["--json", "generate"])).unwrap();
    assert_eq!(json["password"].as_str().unwrap().len(), 20);
}

#[cfg(unix)]
mod clipboard {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> String {
        let p = dir.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p.to_str().unwrap().to_owned()
    }

    fn with_clipboard(e: &mut Env, dir: &Path, paste_body: &str) -> PathBuf {
        let file = dir.join("clip.txt");
        e.extra = vec![
            ("CLIP".into(), file.to_str().unwrap().into()),
            (
                "FIGLOCK_CLIPBOARD_COPY".into(),
                script(dir, "copy.sh", "cat > \"$CLIP\""),
            ),
            (
                "FIGLOCK_CLIPBOARD_PASTE".into(),
                script(dir, "paste.sh", paste_body),
            ),
        ];
        file
    }

    #[test]
    fn copy_puts_the_secret_on_the_clipboard_and_clears_it() {
        let mut e = Env::init();
        let tools = TempDir::new().unwrap();
        let clip = with_clipboard(&mut e, tools.path(), "cat \"$CLIP\"");
        e.add_login("Copy Me", &[]);
        let out = e.run(&["copy", "copy me", "--clear-after", "1"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_no_secret("copy", &out);
        assert_eq!(
            fs::read_to_string(clip).unwrap(),
            "",
            "cleared after the timeout"
        );
    }

    #[test]
    fn copy_without_clearing_keeps_the_value() {
        let mut e = Env::init();
        let tools = TempDir::new().unwrap();
        let clip = with_clipboard(&mut e, tools.path(), "cat \"$CLIP\"");
        e.add_login("Keep Me", &[]);
        assert!(
            e.run(&["copy", "keep me", "--clear-after", "0"])
                .status
                .success()
        );
        assert_eq!(fs::read_to_string(clip).unwrap(), SECRET);
    }

    #[test]
    fn a_clipboard_someone_else_changed_is_left_alone() {
        let mut e = Env::init();
        let tools = TempDir::new().unwrap();
        let clip = with_clipboard(&mut e, tools.path(), "printf 'something else'");
        e.add_login("Other", &[]);
        let out = e.run(&["copy", "other", "--clear-after", "1"]);
        assert!(text(&out.stderr).contains("changed in the meantime"));
        assert_eq!(fs::read_to_string(clip).unwrap(), SECRET);
    }

    #[test]
    fn copy_errors_for_missing_field_or_tool() {
        let mut e = Env::init();
        let tools = TempDir::new().unwrap();
        with_clipboard(&mut e, tools.path(), "true");
        e.add_login("No Totp", &[]);
        let out = e.run(&["copy", "no totp", "--field", "totp", "--clear-after", "0"]);
        assert!(!out.status.success());
        e.extra = vec![(
            "FIGLOCK_CLIPBOARD_COPY".into(),
            "/nonexistent/clipboard-tool".into(),
        )];
        let out = e.run(&["copy", "no totp", "--clear-after", "0"]);
        assert!(!out.status.success());
        assert!(text(&out.stderr).contains("no clipboard tool"));
    }
}
