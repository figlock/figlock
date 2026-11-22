//! iCloud Drive and other synced folders as remotes (FIG-197, ADR-0043), from the command line. A temp folder
//! stands in for `iCloud Drive` through `FIGLOCK_ICLOUD_DIR`, so nothing here touches a real iCloud account.
//! Every value is generated per run.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn password() -> String {
    format!(
        "pw-{}",
        hex::encode(&figlock_core::SecretKey::random().unwrap().expose()[..8])
    )
}

/// One computer: its own config home, sharing `icloud` with the others.
struct Machine {
    home: TempDir,
    icloud: PathBuf,
    password: String,
    secret_key: Option<String>,
}

impl Machine {
    fn new(icloud: &Path, password: &str) -> Machine {
        Machine {
            home: TempDir::new().unwrap(),
            icloud: icloud.into(),
            password: password.into(),
            secret_key: None,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_figlock"));
        c.args(args)
            .env("FIGLOCK_HOME", self.home.path())
            .env("FIGLOCK_DEVICE_KEYS", "file")
            .env("FIGLOCK_ICLOUD_DIR", &self.icloud)
            .env("FIGLOCK_PASSWORD", &self.password)
            .env_remove("FIGLOCK_SECRET_KEY")
            .env_remove("XDG_RUNTIME_DIR")
            .stdin(Stdio::null());
        if let Some(k) = &self.secret_key {
            c.env("FIGLOCK_SECRET_KEY", k);
        }
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> Output {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{args:?}\nstdout: {}\nstderr: {}",
            text(&out.stdout),
            text(&out.stderr)
        );
        out
    }

    fn init(&mut self, extra: &[&str]) -> Output {
        let mut args = vec!["init", "--kdf", "floor", "--no-quick-unlock"];
        args.extend_from_slice(extra);
        let out = self.run(&args);
        if out.status.success() {
            self.secret_key = text(&out.stdout)
                .lines()
                .find_map(|l| l.strip_prefix("Secret Key: "))
                .map(str::to_owned);
        }
        out
    }

    fn add_note(&self, title: &str) {
        self.ok(&["add", "-t", "note", "--title", title, "--set", "body=hello"]);
    }

    fn finds(&self, what: &str) -> bool {
        let out = self.run(&["find", what]);
        out.status.success() && text(&out.stdout).to_lowercase().contains(what)
    }

    fn status(&self) -> Value {
        serde_json::from_slice(&self.ok(&["--json", "sync", "status"]).stdout).unwrap()
    }
}

fn icloud_root() -> TempDir {
    TempDir::new().unwrap()
}

#[test]
fn init_with_icloud_drive_off_says_what_to_switch_on_and_creates_nothing() {
    let root = icloud_root();
    let off = root.path().join("not-signed-in");
    let mut m = Machine::new(&off, &password());
    let out = m.init(&["--icloud"]);
    assert!(!out.status.success());
    let err = text(&out.stderr);
    assert!(
        err.contains("System Settings") && err.contains("Drive"),
        "{err}"
    );
    assert!(!off.exists());
    assert!(!m.home.path().join("config.json").exists());
}

#[test]
fn init_with_icloud_makes_the_folder_a_readme_and_a_synced_vault() {
    let root = icloud_root();
    let mut m = Machine::new(root.path(), &password());
    let out = m.init(&["--icloud"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let folder = root.path().join("FigLock");
    assert!(folder.join("vault.json").is_file());
    let readme = std::fs::read_to_string(folder.join("README.txt")).unwrap();
    assert!(
        readme.contains("do not open, rename, move or delete"),
        "{readme}"
    );
    assert!(text(&out.stdout).contains("Synced to iCloud Drive"));

    // A write reaches the folder on its own, and status lists the remote.
    m.add_note("Shared note");
    let log = std::fs::read_dir(folder.join("log")).unwrap().count();
    assert!(log > 0, "the change was pushed");
    let s = m.status();
    assert_eq!(s["details"][0]["kind"], "icloud");
    assert_eq!(s["details"][0]["ready"], true);
    assert!(s["details"][0]["last_sync_seconds_ago"].is_u64());
    let human = text(&m.ok(&["sync", "status"]).stdout);
    assert!(
        human.contains("iCloud Drive: on") && human.contains("Last sync"),
        "{human}"
    );
}

#[test]
fn a_second_mac_opens_the_vault_from_icloud_and_sees_the_items() {
    let root = icloud_root();
    let pw = password();
    let mut first = Machine::new(root.path(), &pw);
    first.init(&["--icloud"]);
    first.add_note("Shared note");

    let mut second = Machine::new(root.path(), &pw);
    second.secret_key = first.secret_key.clone();
    second.ok(&[
        "open",
        "--icloud",
        "--device-name",
        "second",
        "--no-quick-unlock",
    ]);
    assert!(second.finds("shared"));
    // It keeps syncing: a change made on the second reaches the first.
    second.add_note("From second");
    first.ok(&["sync"]);
    assert!(first.finds("second"));
    assert_eq!(second.status()["details"][0]["kind"], "icloud");
}

#[test]
fn a_new_mac_joins_from_icloud_when_the_first_approves() {
    let root = icloud_root();
    let pw = password();
    let mut first = Machine::new(root.path(), &pw);
    first.init(&["--icloud"]);
    first.add_note("Shared note");
    let mut adder = first.command(&["device", "add", "--yes"]);
    let mut adder = adder
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(adder.stdout.as_mut().unwrap()),
        &mut line,
    )
    .unwrap();
    let code = line.trim().to_owned();
    let mut second = Machine::new(root.path(), &pw);
    second.secret_key = first.secret_key.clone();
    let joined = second.run(&[
        "join",
        &code,
        "--icloud",
        "--device-name",
        "second",
        "--no-quick-unlock",
    ]);
    assert!(joined.status.success(), "{}", text(&joined.stderr));
    assert!(adder.wait().unwrap().success());
    assert!(second.finds("shared"));
}

/// The other Mac's `FigLock` folder arrives in pieces: first nothing, then the vault. The command waits and says
/// so, and never says there is no vault.
#[test]
fn open_waits_for_a_folder_that_is_still_arriving_from_icloud() {
    let root = icloud_root();
    let pw = password();
    // The vault was made elsewhere, in a plain folder, and will be dropped into iCloud Drive in a moment.
    let origin = TempDir::new().unwrap();
    let mut first = Machine::new(root.path(), &pw);
    first.init(&["--store", origin.path().to_str().unwrap()]);
    first.add_note("Shared note");

    let mut second = Machine::new(root.path(), &pw);
    second.secret_key = first.secret_key.clone();
    let child = second
        .command(&[
            "open",
            "--icloud",
            "--wait-for-sync",
            "30",
            "--no-quick-unlock",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(3000));
    assert!(
        !root.path().join("FigLock").exists(),
        "waiting must not create the folder: iCloud would keep a second one"
    );
    copy_dir(origin.path(), &root.path().join("FigLock"));
    let out = child.wait_with_output().unwrap();
    let err = text(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(
        err.contains("Waiting for iCloud to finish syncing"),
        "{err}"
    );
    assert!(!err.contains("no FigLock vault"), "{err}");
    assert!(second.finds("shared"));
}

#[test]
fn open_gives_up_with_a_clear_message_and_still_creates_nothing() {
    let root = icloud_root();
    let m = Machine::new(root.path(), &password());
    let out = m.run(&["open", "--icloud", "--wait-for-sync", "1"]);
    assert!(!out.status.success());
    let err = text(&out.stderr);
    assert!(
        err.contains("has not arrived from iCloud Drive yet"),
        "{err}"
    );
    assert!(!err.contains("no FigLock vault"), "{err}");
    assert!(!root.path().join("FigLock").exists());
    assert!(!m.home.path().join("config.json").exists());
}

#[test]
fn init_refuses_a_folder_that_is_still_receiving_another_vault() {
    let root = icloud_root();
    let folder = root.path().join("FigLock");
    std::fs::create_dir_all(folder.join("log")).unwrap();
    std::fs::write(folder.join("README.txt"), "x").unwrap();
    let mut m = Machine::new(root.path(), &password());
    let out = m.init(&["--icloud"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("still be arriving"),
        "{}",
        text(&out.stderr)
    );
    assert!(!folder.join("vault.json").exists());
    assert!(!m.home.path().join("config.json").exists());
}

#[test]
fn icloud_and_another_folder_can_both_be_on_and_each_turned_off_alone() {
    let root = icloud_root();
    let other = TempDir::new().unwrap();
    let other_path = other.path().join("dropbox").to_str().unwrap().to_owned();
    let mut m = Machine::new(root.path(), &password());
    m.ok_init(&[]);
    m.add_note("Shared note");

    let on = m.ok(&["sync", "on", "--icloud", "--sync-folder", &other_path]);
    assert!(
        text(&on.stdout).contains("iCloud Drive is on"),
        "{}",
        text(&on.stdout)
    );
    let s = m.status();
    assert_eq!(s["remotes"].as_array().unwrap().len(), 2, "{s}");
    assert!(Path::new(&other_path).join("vault.json").is_file());
    assert!(root.path().join("FigLock/vault.json").is_file());

    // `sync` reports each place by name.
    let run = text(&m.ok(&["sync"]).stdout);
    assert!(
        run.contains("iCloud Drive:") && run.contains("dropbox"),
        "{run}"
    );

    // A change reaches both.
    m.add_note("Both");
    assert!(
        std::fs::read_dir(Path::new(&other_path).join("log"))
            .unwrap()
            .count()
            > 0
    );

    // Turning iCloud off leaves the folder remote working.
    let off = m.ok(&["sync", "off", "--icloud"]);
    assert!(
        text(&off.stdout).contains("iCloud Drive is off"),
        "{}",
        text(&off.stdout)
    );
    let s = m.status();
    assert_eq!(s["details"].as_array().unwrap().len(), 1);
    assert_eq!(s["details"][0]["kind"], "folder");
    assert!(
        root.path().join("FigLock/vault.json").is_file(),
        "the copy in iCloud is left alone"
    );
    m.add_note("After off");
    m.ok(&["sync"]);

    // Off with nothing named turns off whatever is left; again is a calm no-op.
    m.ok(&["sync", "off"]);
    assert!(text(&m.ok(&["sync", "off"]).stdout).contains("already off"));
    assert!(m.status()["remotes"].as_array().unwrap().is_empty());
}

#[test]
fn turning_icloud_on_twice_is_a_no_op_and_a_failed_remote_is_not_kept() {
    let root = icloud_root();
    let mut m = Machine::new(root.path(), &password());
    m.ok_init(&[]);
    m.ok(&["sync", "on", "--icloud"]);
    assert!(text(&m.ok(&["sync", "on", "--icloud"]).stdout).contains("already on"));

    // A folder that holds a different vault is refused and not left on.
    let other = Machine::new(root.path(), &password());
    let mut other = other;
    let foreign = TempDir::new().unwrap();
    other.init(&["--store", foreign.path().to_str().unwrap()]);
    let out = m.run(&[
        "sync",
        "on",
        "--sync-folder",
        foreign.path().to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("nothing was changed"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(m.status()["remotes"].as_array().unwrap().len(), 1);
}

#[test]
fn status_names_a_folder_that_went_missing_and_the_last_problem() {
    let root = icloud_root();
    let mut m = Machine::new(root.path(), &password());
    m.ok_init(&["--icloud"]);
    // iCloud Drive signed out: its folder disappears.
    std::fs::remove_dir_all(root.path()).unwrap();
    let human = text(&m.ok(&["sync", "status"]).stdout);
    assert!(human.contains("off or signed out"), "{human}");
    let run = m.run(&["sync"]);
    assert!(!run.status.success());
    assert!(
        text(&run.stderr).contains("System Settings"),
        "{}",
        text(&run.stderr)
    );
    // The problem is kept for the next status, with no network call.
    let s = m.status();
    assert!(
        s["details"][0]["last_error"]
            .as_str()
            .unwrap()
            .contains("System Settings"),
        "{s}"
    );
    assert!(
        !root.path().exists(),
        "FigLock never recreates iCloud Drive's folder"
    );
}

impl Machine {
    /// `init` that must work, for tests that only need a vault.
    fn ok_init(&mut self, extra: &[&str]) {
        let out = self.init(extra);
        assert!(out.status.success(), "{}", text(&out.stderr));
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}
