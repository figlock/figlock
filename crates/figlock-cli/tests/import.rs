//! End-to-end tests of `figlock import` against a real folder store: dry run, CSV and notes imports, and the
//! negative cases (wrong format, hostile file names and contents, wrong password, secrets in output).
//! Passwords and tokens are generated here, never literal.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

const PW: &str = "unit-test-master-password";

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn sample(alphabet: &str, n: usize, seed: usize) -> String {
    let a: Vec<char> = alphabet.chars().collect();
    (0..n)
        .map(|i| a[(i * 7 + seed * 13 + 3) % a.len()])
        .collect()
}

const ALNUM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

struct Env {
    home: TempDir,
    store: TempDir,
    files: TempDir,
    secret_key: String,
}

impl Env {
    fn init() -> Env {
        let mut e = Env {
            home: TempDir::new().unwrap(),
            store: TempDir::new().unwrap(),
            files: TempDir::new().unwrap(),
            secret_key: String::new(),
        };
        let store = e.store.path().to_str().unwrap().to_owned();
        let out = e.run_as(&["init", "--store", &store, "--kdf", "floor"], PW, None);
        assert!(out.status.success(), "{}", text(&out.stderr));
        e.secret_key = text(&out.stdout)
            .lines()
            .find_map(|l| l.strip_prefix("Secret Key: "))
            .expect("secret key printed")
            .to_owned();
        e
    }

    fn run_as(&self, args: &[&str], pw: &str, sk: Option<&str>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_figlock"));
        c.args(args)
            .env("FIGLOCK_HOME", self.home.path())
            .env("FIGLOCK_PASSWORD", pw)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(sk) = sk {
            c.env("FIGLOCK_SECRET_KEY", sk);
        }
        c.output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_as(args, PW, Some(&self.secret_key))
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        text(&out.stdout)
    }

    fn file(&self, name: &str, content: &[u8]) -> PathBuf {
        let p = self.files.path().join(name);
        fs::write(&p, content).unwrap();
        p
    }

    fn import(&self, file: &Path, from: &str, extra: &[&str]) -> Output {
        let mut args = vec!["import", file.to_str().unwrap(), "--from", from];
        args.extend_from_slice(extra);
        self.run(&args)
    }

    fn export(&self) -> Value {
        let out = self.ok(&["export", "--include-secrets"]);
        serde_json::from_str(&out).unwrap()
    }

    fn op_files(&self) -> usize {
        let mut n = 0;
        let Ok(devices) = fs::read_dir(self.store.path().join("log")) else {
            return 0;
        };
        for dev in devices {
            n += fs::read_dir(dev.unwrap().path()).unwrap().count();
        }
        n
    }

    /// Every byte in the store, to check nothing readable was written.
    fn store_bytes(&self) -> Vec<u8> {
        fn walk(dir: &Path, out: &mut Vec<u8>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.extend(fs::read(&p).unwrap());
                }
            }
        }
        let mut all = Vec::new();
        walk(self.store.path(), &mut all);
        all
    }
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn chrome_csv(password: &str) -> String {
    format!(
        "name,url,username,password,note\nExample Mail,https://mail.example.com,ada,\"{password}\",hello\n,https://git.example.org/x,bob,pw2,\n"
    )
}

#[test]
fn dry_run_reports_without_a_vault_a_password_or_a_write() {
    let e = Env::init();
    let pw = sample(ALNUM, 20, 1);
    let csv = e.file("chrome.csv", chrome_csv(&pw).as_bytes());
    let before = e.op_files();
    // No vault configured and no password in the environment: a dry run needs neither.
    let bare = Command::new(env!("CARGO_BIN_EXE_figlock"))
        .args([
            "import",
            csv.to_str().unwrap(),
            "--from",
            "chrome",
            "--dry-run",
        ])
        .env("FIGLOCK_HOME", TempDir::new().unwrap().path())
        .env_remove("FIGLOCK_PASSWORD")
        .env_remove("FIGLOCK_SECRET_KEY")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(bare.status.success(), "{}", text(&bare.stderr));
    assert!(text(&bare.stdout).contains("Found 2 logins, 0 notes, 0 API tokens."));
    assert!(!text(&bare.stdout).contains(&pw) && !text(&bare.stderr).contains(&pw));
    let out = e.import(&csv, "chrome", &["--dry-run"]);
    assert!(out.status.success());
    assert_eq!(e.op_files(), before, "dry run wrote nothing");
}

#[test]
fn chrome_import_is_findable_and_hides_the_password() {
    let e = Env::init();
    let pw = sample(ALNUM, 20, 2);
    let csv = e.file("chrome.csv", chrome_csv(&pw).as_bytes());
    let out = e.import(&csv, "chrome", &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("Imported 2 logins"));
    assert!(!text(&out.stdout).contains(&pw) && !text(&out.stderr).contains(&pw));
    // The tip about deleting the plain-text export is on stderr.
    assert!(text(&out.stderr).contains("Delete it securely"));

    let found = e.ok(&["find", "mail"]);
    assert!(found.contains("Example Mail"), "{found}");
    // The second row had no title, so its host stands in.
    assert!(
        e.ok(&["find", "git.example.org"])
            .contains("git.example.org")
    );
    let shown = e.ok(&["get", "Example Mail"]);
    assert!(!shown.contains(&pw));
    let revealed = e.ok(&["get", "Example Mail", "--field", "password", "--reveal"]);
    assert_eq!(revealed.trim_end(), pw);
    // On disk everything is ciphertext.
    let disk = e.store_bytes();
    assert!(!contains(&disk, &pw));
    assert!(!contains(&disk, "Example Mail"));
}

#[test]
fn notes_import_files_tokens_and_keeps_them_out_of_notes_and_search_text() {
    let e = Env::init();
    let token = format!("ghp_{}", sample(ALNUM, 36, 3));
    let aws = format!(
        "AKIA{}",
        sample("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789", 16, 4)
    );
    let dump = format!("Dev keys\nGITHUB_TOKEN={token}\n---\nShopping\nmilk\n---\nAWS\n{aws}\n");
    let file = e.file("notes.txt", dump.as_bytes());
    let out = e.import(&file, "notes", &["--json"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let summary: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(summary["notes"], 3);
    assert_eq!(summary["tokens"], 2);
    assert_eq!(summary["tokens_detected"], 2);
    assert_eq!(summary["imported"], true);
    assert!(!text(&out.stdout).contains(&token) && !text(&out.stderr).contains(&token));

    let all = e.export();
    let items = all["items"].as_array().unwrap();
    for item in items {
        let fields = item["fields"].as_object().unwrap();
        for (name, value) in fields {
            if item["type"] != "token" || name != "token" {
                assert!(
                    !value.as_str().unwrap().contains(&token),
                    "{name} leaks the token"
                );
            }
        }
    }
    let gh = items
        .iter()
        .find(|i| i["fields"]["token"] == token.as_str())
        .expect("token item");
    assert_eq!(gh["fields"]["provider"], "github");
    assert_eq!(gh["fields"]["env_var"], "GITHUB_TOKEN");
    // Searching by the well-known prefix finds the token item without indexing the value.
    assert!(e.ok(&["find", "ghp_"]).contains("GitHub token"));
    assert!(!contains(&e.store_bytes(), &token));
    // The default export shows no token value.
    assert!(!e.ok(&["export"]).contains(&token));
}

#[test]
fn bitwarden_import_skips_cards_and_counts_them() {
    let e = Env::init();
    let csv = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
        ,,login,Site,,,0,https://a.example,ada,pw,\n\
        ,,card,Visa,,,0,,,,\n";
    let file = e.file("bw.csv", csv.as_bytes());
    let out = e.import(&file, "bitwarden", &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.contains("Imported 1 logins"), "{stdout}");
    assert!(stdout.contains("1 of an unsupported type"), "{stdout}");
}

#[test]
fn a_file_from_another_source_is_refused_without_echoing_it() {
    let e = Env::init();
    let marker = sample(ALNUM, 24, 5);
    let file = e.file("x.csv", format!("{marker},{marker}\n{marker}\n").as_bytes());
    let before = e.op_files();
    let out = e.import(&file, "chrome", &[]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("does not look like an export"));
    assert!(!text(&out.stderr).contains(&marker) && !text(&out.stdout).contains(&marker));
    assert_eq!(e.op_files(), before);

    let bad = e.file(
        "bad.csv",
        format!("name,url,username,password\n\"{marker}\n").as_bytes(),
    );
    let out = e.import(&bad, "chrome", &[]);
    assert!(!out.status.success());
    assert!(!text(&out.stderr).contains(&marker));
}

#[test]
fn bad_paths_and_sizes_fail_cleanly() {
    let e = Env::init();
    let missing = e.files.path().join("nope-secret-folder-name.csv");
    let out = e.import(&missing, "chrome", &[]);
    assert!(!out.status.success());
    assert!(
        !text(&out.stderr).contains("nope-secret-folder-name"),
        "the path is not echoed"
    );
    let dir = e.import(e.files.path(), "chrome", &[]);
    assert!(text(&dir.stderr).contains("not a file"));
    let huge = e.file("huge.csv", &vec![b'a'; 16 * 1024 * 1024 + 1]);
    let out = e.import(&huge, "notes", &[]);
    assert!(text(&out.stderr).contains("too large"));
    let empty = e.file("empty.txt", b"");
    let out = e.import(&empty, "notes", &[]);
    assert!(text(&out.stderr).contains("nothing to import"));
    let nonutf = e.file("bin.csv", &[0xff, 0xfe, 0x00]);
    assert!(text(&e.import(&nonutf, "chrome", &[]).stderr).contains("UTF-8"));
}

#[test]
fn wrong_password_imports_nothing() {
    let e = Env::init();
    let pw = sample(ALNUM, 20, 6);
    let csv = e.file("chrome.csv", chrome_csv(&pw).as_bytes());
    let before = e.op_files();
    let out = e.run_as(
        &["import", csv.to_str().unwrap(), "--from", "chrome"],
        "not the password",
        Some(&e.secret_key),
    );
    assert!(!out.status.success());
    assert!(!text(&out.stderr).contains(&pw));
    assert_eq!(e.op_files(), before);
}

#[test]
fn a_large_import_is_split_into_several_op_files() {
    let e = Env::init();
    let mut csv = String::from("name,url,username,password\n");
    for i in 0..250 {
        csv.push_str(&format!("Site {i},https://s{i}.example,u,p{i}\n"));
    }
    let file = e.file("many.csv", csv.as_bytes());
    let before = e.op_files();
    let out = e.import(&file, "chrome", &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(e.op_files() - before, 3, "100 + 100 + 50");
    let all = e.export();
    assert_eq!(all["items"].as_array().unwrap().len(), 250);
}
