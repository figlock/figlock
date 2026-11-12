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
            .env("FIGLOCK_DEVICE_KEYS", "file")
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
fn a_folder_without_vault_json_says_it_is_not_found() {
    let e = Env::init();
    fs::remove_file(e.store.path().join("vault.json")).unwrap();
    let out = e.run(&["find"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("vault.json not found"));
}

#[test]
fn before_init_commands_explain_what_to_do() {
    let home = TempDir::new().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_figlock"))
        .args(["find"])
        .env("FIGLOCK_HOME", home.path())
        .env("FIGLOCK_DEVICE_KEYS", "file")
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

#[test]
fn init_without_a_terminal_leaves_quick_unlock_off() {
    let e = Env::init();
    assert!(e.ok(&["quick-unlock", "status"]).contains("off"));
    assert!(e.ok(&["lock"]).contains("Locked"));
    assert!(e.ok(&["quick-unlock", "disable"]).contains("off"));
    // Turning it off twice is fine, and nothing was written for it.
    assert!(e.ok(&["quick-unlock", "disable"]).contains("off"));
    assert!(!e.home.path().join("quick-unlock.json").exists());
    // Ordinary commands still ask for the full unlock.
    let out = e.run_with(&["find"], None, "wrong password", Some(&e.secret_key));
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("could not unlock"));
}

#[test]
fn a_device_with_no_keystore_keeps_no_secret_key_and_says_so() {
    // The tests run with `FIGLOCK_DEVICE_KEYS=file`, like a headless machine: there is nowhere to keep it.
    let e = Env::init();
    let shown = e.run(&["secret-key", "show"]);
    assert!(!shown.status.success());
    assert!(text(&shown.stderr).contains("no Secret Key is kept on this device"));
    assert!(!text(&shown.stdout).contains(&e.secret_key));
    // Forgetting what is not kept is fine, and the full unlock still works by typing the key.
    assert!(e.ok(&["secret-key", "forget"]).contains("no longer kept"));
    let found = e.run_with(&["find"], None, PW, Some(&e.secret_key));
    assert!(found.status.success(), "{}", text(&found.stderr));
    for name in ["secret-key.qk", "secret-key.blob"] {
        assert!(!e.home.path().join(name).exists());
    }
}

/// Bytes that are not valid UTF-8 and do not look like text, so a lossy path would corrupt them.
fn file_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).unwrap();
    b[0] = 0xff;
    b
}

#[test]
fn a_file_goes_in_and_out_byte_for_byte_and_never_shows_in_listings() {
    use std::os::unix::fs::PermissionsExt;
    let e = Env::init();
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("signing-cert.p12");
    let bytes = file_bytes(4_000);
    fs::write(&src, &bytes).unwrap();

    let out = e.run_stdin(
        &[
            "add",
            "--type",
            "doc",
            "--title",
            "Apple signing",
            "--set",
            "url/1=https://developer.apple.com",
            "--set",
            "tags/ci=",
            "--secret",
            "password",
            "--file",
            src.to_str().unwrap(),
        ],
        &format!("{SECRET}\n"),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_no_secret("add", &out);
    let id = text(&out.stdout).trim().to_owned();

    // Listings and plain get show the file's name and size, never its contents or the password.
    let b64 = figlock_b64(&bytes);
    for args in [
        vec!["get", &id],
        vec!["get", &id, "--json"],
        vec!["find", "apple"],
    ] {
        let out = e.run(&args);
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        assert_no_secret("listing", &out);
        assert!(
            !text(&out.stdout).contains(&b64[..40]),
            "{args:?} printed the file"
        );
    }
    let shown = e.ok(&["get", &id]);
    assert!(shown.contains("signing-cert.p12"), "{shown}");
    assert!(shown.contains("4000 bytes"), "{shown}");

    // The file name is searchable, the contents are not.
    assert!(e.ok(&["find", "signing-cert"]).contains("Apple signing"));
    assert!(e.run(&["find", &b64[10..50]]).stdout.is_empty());

    // Out to a new path, owner-only, identical bytes.
    let dest = dir.path().join("restored.p12");
    let d = dest.to_str().unwrap();
    let out = e.run(&["get", &id, "--file", "signing-cert.p12", "--out", d]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(fs::read(&dest).unwrap(), bytes);
    assert_eq!(
        fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // Not overwritten unless asked.
    let again = e.run(&["get", &id, "--file", "signing-cert.p12", "--out", d]);
    assert!(!again.status.success());
    assert!(text(&again.stderr).contains("already exists"));
    assert_eq!(fs::read(&dest).unwrap(), bytes);
    assert!(
        e.run(&[
            "get",
            &id,
            "--file",
            "signing-cert.p12",
            "--out",
            d,
            "--force"
        ])
        .status
        .success()
    );
    // Stdout is binary-safe.
    assert_eq!(
        e.run(&["get", &id, "--file", "signing-cert.p12", "--out", "-"])
            .stdout,
        bytes
    );
    assert!(
        !e.run(&["get", &id, "--file", "nope.p12", "--out", "-"])
            .status
            .success()
    );

    // Nothing readable on the storage side: not the name, not the contents, not the password.
    for op in e.ops() {
        let raw = fs::read(op).unwrap();
        assert!(!contains(&raw, b"signing-cert"), "file name in ciphertext");
        assert!(!contains(&raw, &bytes[..64]), "file bytes in ciphertext");
        assert!(!contains(&raw, SECRET.as_bytes()), "password in ciphertext");
    }

    // Removing the file removes its fields and the item stays.
    e.ok(&["edit", &id, "--remove-file", "signing-cert.p12"]);
    assert!(!e.ok(&["get", &id]).contains("file/"));
}

#[test]
fn files_are_checked_before_anything_is_written() {
    let e = Env::init();
    let dir = TempDir::new().unwrap();
    let big = dir.path().join("big.bin");
    fs::write(&big, vec![7u8; 786_432 + 1]).unwrap();
    let empty = dir.path().join("empty.bin");
    fs::write(&empty, b"").unwrap();
    let ok = dir.path().join("ok.bin");
    fs::write(&ok, b"tiny").unwrap();
    e.add_login("seed", &[]);
    let before = e.ops().len();
    for (args, needle) in [
        (
            vec![
                "add",
                "-t",
                "doc",
                "--title",
                "x",
                "--file",
                big.to_str().unwrap(),
            ],
            "768 KiB",
        ),
        (
            vec![
                "add",
                "-t",
                "doc",
                "--title",
                "x",
                "--file",
                empty.to_str().unwrap(),
            ],
            "768 KiB",
        ),
        (
            vec![
                "add",
                "-t",
                "doc",
                "--title",
                "x",
                "--file",
                dir.path().to_str().unwrap(),
            ],
            "regular file",
        ),
        (
            vec![
                "add",
                "-t",
                "doc",
                "--title",
                "x",
                "--file",
                "/nonexistent/f.bin",
            ],
            "cannot read",
        ),
        (
            vec![
                "add",
                "-t",
                "login",
                "--title",
                "x",
                "--file",
                ok.to_str().unwrap(),
            ],
            "doc items",
        ),
    ] {
        let out = e.run(&args);
        assert!(!out.status.success(), "{args:?}");
        assert!(
            text(&out.stderr).contains(needle),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
    assert_eq!(e.ops().len(), before, "a refused add wrote an op");
    // Too many files on one item is refused as well.
    let mut args = vec!["add", "-t", "doc", "--title", "many"];
    let path = ok.to_str().unwrap();
    for _ in 0..9 {
        args.extend(["--file", path]);
    }
    assert!(!e.run(&args).status.success());
    assert_eq!(e.ops().len(), before);
}

#[test]
fn an_oversize_text_field_is_refused() {
    let e = Env::init();
    let long = format!("title={}", "t".repeat(2000));
    let out = e.run(&[
        "add",
        "-t",
        "note",
        "--title",
        &"t".repeat(2000),
        "--set",
        &long,
    ]);
    assert!(!out.status.success());
    assert!(
        e.run(&["add", "-t", "note", "--title", &"t".repeat(2000)])
            .status
            .code()
            != Some(0)
    );
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn figlock_b64(bytes: &[u8]) -> String {
    figlock_core::encode_file(bytes).unwrap().to_string()
}

#[test]
fn init_quick_unlock_flags_conflict_and_no_flag_turns_it_off() {
    let home = TempDir::new().unwrap();
    let store = TempDir::new().unwrap();
    let run = |extra: &[&str]| {
        let mut args = vec![
            "init",
            "--store",
            store.path().to_str().unwrap(),
            "--kdf",
            "floor",
        ];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_figlock"))
            .args(args)
            .env("FIGLOCK_HOME", home.path())
            .env("FIGLOCK_DEVICE_KEYS", "file")
            .env("FIGLOCK_PASSWORD", PW)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let both = run(&["--quick-unlock", "--no-quick-unlock"]);
    assert!(!both.status.success());
    assert!(text(&both.stderr).contains("cannot be used with"));
    let out = run(&["--no-quick-unlock"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!text(&out.stderr).to_lowercase().contains("quick unlock"));
    assert!(!home.path().join("quick-unlock.json").exists());
}

const NEW_PW: &str = "a-different-generated-passphrase-for-tests";

#[test]
fn password_change_locks_out_the_old_password_and_keeps_the_items() {
    let mut e = Env::init();
    e.add_login("Kept", &[]);
    e.extra
        .push(("FIGLOCK_NEW_PASSWORD".to_owned(), NEW_PW.to_owned()));
    let out = e.run(&["password", "change"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("full unlock"));

    let old = e.run(&["find"]);
    assert!(!old.status.success());
    assert!(text(&old.stderr).contains("could not unlock"));
    let new = e.run_with(&["find"], None, NEW_PW, Some(&e.secret_key));
    assert!(new.status.success(), "{}", text(&new.stderr));
    assert!(text(&new.stdout).contains("Kept"));
    assert_no_secret("password change", &out);
}

#[test]
fn password_change_with_the_wrong_current_password_changes_nothing() {
    let mut e = Env::init();
    e.extra
        .push(("FIGLOCK_NEW_PASSWORD".to_owned(), NEW_PW.to_owned()));
    let out = e.run_with(&["password", "change"], None, "wrong", Some(&e.secret_key));
    assert!(!out.status.success());
    assert!(e.run(&["find"]).status.success());
}

#[test]
fn secret_key_rotation_replaces_the_old_key_and_shows_the_new_one_once() {
    let e = Env::init();
    e.add_login("Kept", &[]);
    let out = e.run(&["secret-key", "rotate"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let new_sk = text(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("Secret Key: "))
        .expect("new Secret Key printed")
        .to_owned();
    assert_ne!(new_sk, e.secret_key);
    assert!(
        !text(&out.stderr).contains("was not kept"),
        "{}",
        text(&out.stderr)
    );

    let old = e.run(&["find"]);
    assert!(!old.status.success());
    let new = e.run_with(&["find"], None, PW, Some(&new_sk));
    assert!(new.status.success(), "{}", text(&new.stderr));
    assert!(text(&new.stdout).contains("Kept"));
}

#[test]
fn device_list_marks_this_device_and_it_cannot_revoke_itself() {
    let e = Env::init();
    let list = e.ok(&["device", "list"]);
    assert!(list.contains("(this device)"), "{list}");
    let id = list.split_whitespace().next().unwrap().to_owned();
    let out = e.run(&["device", "revoke", &id]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("device you are using"));
    let bad = e.run(&["device", "revoke", "zz"]);
    assert!(text(&bad.stderr).contains("hex digits"));
}

// ---- Drive sync (ADR-0026); nothing here talks to Google ----

#[test]
fn sync_is_off_for_a_folder_vault_and_says_how_to_turn_it_on() {
    let e = Env::init();
    let status = e.ok(&["sync", "status"]);
    assert!(status.contains("Google Drive sync: off"), "{status}");
    let json: Value = serde_json::from_str(&e.ok(&["--json", "sync", "status"])).unwrap();
    assert_eq!(json["remote"], Value::Null);
    assert_eq!(
        json["local"],
        e.store.path().canonicalize().unwrap().to_str().unwrap()
    );

    let out = e.run(&["sync"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("figlock sync on"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        e.run(&["sync", "off"]).status.success(),
        "off when off is calm"
    );
}

#[test]
fn sync_on_needs_a_sign_in_and_changes_nothing_without_one() {
    let e = Env::init();
    let before = fs::read(e.home.path().join("config.json")).unwrap();
    let out = e.run(&["sync", "on"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("figlock drive login"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(fs::read(e.home.path().join("config.json")).unwrap(), before);
}

#[test]
fn commands_keep_working_and_keep_the_data_locally_when_the_remote_cannot_be_reached() {
    let e = Env::init();
    // A device with Drive sync on but no sign-in: the same as a revoked or offline remote.
    let path = e.home.path().join("config.json");
    let mut cfg: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    cfg["remote"] = Value::from("drive");
    fs::write(&path, cfg.to_string()).unwrap();

    let status = e.ok(&["sync", "status"]);
    assert!(status.contains("Google Drive sync: on"), "{status}");
    assert!(status.contains("Signed in to Google: no"), "{status}");

    let out = e.run_stdin(
        &[
            "add",
            "--type",
            "login",
            "--title",
            "Offline item",
            "--secret",
            "password",
        ],
        &format!("{SECRET}\n"),
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        text(&out.stderr)
            .matches("not synced with Google Drive")
            .count(),
        1,
        "one warning for the command, not one for the pull and one for the push: {}",
        text(&out.stderr)
    );
    assert!(
        text(&out.stderr).contains("not synced with Google Drive"),
        "{}",
        text(&out.stderr)
    );
    let found = e.ok(&["find", "Offline item"]);
    assert!(found.contains("Offline item"), "{found}");
    assert_eq!(e.ops().len(), 1, "the write landed in the local folder");

    let sync = e.run(&["sync"]);
    assert!(!sync.status.success());
    assert!(
        text(&sync.stderr).contains("figlock drive login"),
        "{}",
        text(&sync.stderr)
    );
}
