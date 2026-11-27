//! Getting a broker for a command that needs one and finds none (ADR-0040). The commands an agent runs cannot
//! type a password, so a broker is started for them only when quick unlock is valid (one Touch ID unlocks it);
//! otherwise the command stops at once with exit code 69 and says what the person can do, instead of
//! waiting out a timeout.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use figlock_session::error::{Fail, FailKind, Res};
use figlock_session::{quick, session};

use super::server;

/// Long enough for a person to find their finger and unlock with Touch ID.
const START_WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

const NO_BROKER: &str = "no broker is running and none can be started without you: open FigLock and unlock it, or run `figlock broker start` in a terminal";

/// Whether Touch ID alone would unlock the vault now (no password to type). Never shows a prompt.
fn quick_unlock_ready() -> bool {
    let Ok(vault) = session::peek_vault_id() else {
        return false;
    };
    let Ok(quick) = quick::system() else {
        return false;
    };
    quick.usable(&vault, session::now_ms() / 1000)
}

/// Make sure something answers on `socket`: a running broker or the FigLock app, or one started now in the
/// background. A broker that answers `locked` is left for the caller's request to report.
pub fn ensure(socket: &Path) -> Res<()> {
    if server::live(socket) {
        return Ok(());
    }
    if !quick_unlock_ready() {
        return Err(Fail::of(FailKind::Unavailable, NO_BROKER));
    }
    eprintln!("no broker is running; starting one in the background (unlock with Touch ID)");
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.args(["broker", "start", "--background"]);
    start_with(cmd, socket, &socket.with_extension("log"), START_WAIT)
}

/// Run `cmd` detached and wait until it answers on `socket`. Its error output goes to `log`, and the last
/// line of it is the reason when it stops early.
pub fn start_with(mut cmd: Command, socket: &Path, log: &Path, wait: Duration) -> Res<()> {
    if let Some(dir) = log.parent() {
        figlock_session::config::private_dir(dir)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(log)?;
    // Its own process group, so it outlives this command and a Ctrl-C meant for it.
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(file))
        .process_group(0)
        .spawn()
        .map_err(|e| Fail::new(format!("could not start a background broker: {}", e.kind())))?;
    let until = Instant::now() + wait;
    loop {
        if server::live(socket) {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            // Another start may have won the socket in the meantime.
            if server::live(socket) {
                return Ok(());
            }
            let why = last_line(log);
            return Err(Fail::of(
                FailKind::Unavailable,
                format!(
                    "the background broker stopped ({status}){}; open FigLock and unlock it, or run `figlock broker start` in a terminal",
                    if why.is_empty() {
                        String::new()
                    } else {
                        format!(": {why}")
                    }
                ),
            ));
        }
        if Instant::now() >= until {
            return Err(Fail::of(
                FailKind::Unavailable,
                "the background broker did not start in time (was the Touch ID prompt answered?)",
            ));
        }
        std::thread::sleep(POLL);
    }
}

/// The last non-empty line of the child's output, safe to show.
fn last_line(log: &Path) -> String {
    let text = fs::read_to_string(log).unwrap_or_default();
    let line = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    figlock_core::audit_clean_text(line.trim(), 200)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn short_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("fl")
            .tempdir_in("/tmp")
            .unwrap()
    }

    fn sh(script: &str) -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", script]);
        c
    }

    #[test]
    fn waits_until_the_socket_answers() {
        let dir = short_dir();
        let socket = dir.path().join("b.sock");
        let log = dir.path().join("b.log");
        // The "broker" appears a moment after the process starts, as it does once Touch ID is accepted.
        let listener_path = socket.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let l = UnixListener::bind(listener_path).unwrap();
            std::thread::sleep(Duration::from_secs(2));
            drop(l);
        });
        let started = Instant::now();
        start_with(sh("sleep 5"), &socket, &log, Duration::from_secs(10)).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(300));
        t.join().unwrap();
    }

    #[test]
    fn a_broker_that_stops_early_is_explained_by_its_last_line() {
        let dir = short_dir();
        let socket = dir.path().join("b.sock");
        let log = dir.path().join("b.log");
        let e = start_with(
            sh("echo 'first line' >&2; echo 'error: a background broker cannot ask for the password' >&2; exit 3"),
            &socket,
            &log,
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert_eq!(e.1, FailKind::Unavailable);
        assert!(e.0.contains("cannot ask for the password"), "{e}");
        assert!(e.0.contains("figlock broker start"), "{e}");
        assert!(!e.0.contains("first line"));
    }

    #[test]
    fn a_broker_that_never_answers_is_given_up_on() {
        let dir = short_dir();
        let socket = dir.path().join("b.sock");
        let log = dir.path().join("b.log");
        let started = Instant::now();
        let e = start_with(sh("sleep 5"), &socket, &log, Duration::from_millis(400)).unwrap_err();
        assert_eq!(e.1, FailKind::Unavailable);
        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(e.0.contains("did not start in time"), "{e}");
    }

    #[test]
    fn a_program_that_cannot_be_run_is_reported() {
        let dir = short_dir();
        let e = start_with(
            Command::new("/nonexistent/figlock"),
            &dir.path().join("b.sock"),
            &dir.path().join("b.log"),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(e.0.contains("could not start"), "{e}");
    }

    #[test]
    fn something_already_answering_needs_nothing_started() {
        let dir = short_dir();
        let socket = dir.path().join("b.sock");
        let _l = UnixListener::bind(&socket).unwrap();
        ensure(&socket).unwrap();
    }
}
