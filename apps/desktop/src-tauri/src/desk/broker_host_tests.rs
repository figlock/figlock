//! The app and the socket (ADR-0040): who answers on it, and the takeover from a broker started in a terminal
//! (see `tests.rs` for how scenarios run).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use figlock_broker::peer::PeerId;
use figlock_broker::server::{self, Dispatch};
use figlock_broker::{Reply, client};

use super::broker_host::Mode;
use super::device_tests::unlocked;
use super::tests::scenario_in;
use super::*;

fn scenario(name: &str) {
    scenario_in("broker_host_tests", name);
}

/// Stands in for `figlock broker start`: answers on the socket, and a `lock` request ends it and frees the
/// socket, as the CLI daemon does.
struct TerminalBroker {
    done: AtomicBool,
}

impl Dispatch for TerminalBroker {
    fn admit(&self, peer: &PeerId) -> bool {
        peer.uid == figlock_broker::peer::my_uid()
    }
    fn handle(&self, _: &PeerId, line: &str, _: &dyn Fn() -> bool) -> Reply {
        let lock = line.contains(r#""op":"lock""#);
        if lock {
            self.done.store(true, Ordering::SeqCst);
        }
        Reply {
            line: r#"{"v":1,"id":"cli","ok":true}"#.to_owned(),
            exit: lock,
        }
    }
    fn finished(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

#[test]
fn the_window_hosts_the_socket_and_answers_locked_until_it_is_unlocked() {
    scenario("hosts_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn hosts_inner() {
    let d = Desk::with(KdfParams::FLOOR, Duration::from_secs(300), Duration::ZERO);
    assert_eq!(d.broker_mode(), Mode::Off);
    d.host_broker();
    assert_eq!(d.broker_mode(), Mode::Hosting);
    let socket = server::socket_path().unwrap();
    // Agents are told to unlock, not that nothing is there.
    let status = client::call(&socket, "status", serde_json::json!({})).unwrap();
    assert_eq!(status["locked"], true);
    let denied = client::call(&socket, "grant.list", serde_json::json!({})).unwrap();
    assert_eq!(denied["error"]["code"], "locked");
    unlocked(&d);
    let status = client::call(&socket, "status", serde_json::json!({})).unwrap();
    assert_eq!(status["locked"], false, "{status}");
    // A second app (or `figlock broker start`) cannot take the socket.
    assert!(server::bind(&socket).is_err());
    d.lock();
    assert_eq!(
        client::call(&socket, "status", serde_json::json!({})).unwrap()["locked"],
        true
    );
    // Quitting frees the socket.
    d.stop_broker();
    assert!(!socket.exists());
}

#[test]
fn a_broker_started_in_a_terminal_keeps_the_socket_until_the_person_lets_the_window_take_over() {
    scenario("takeover_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn takeover_inner() {
    let socket = server::socket_path().unwrap();
    let terminal = Arc::new(TerminalBroker {
        done: AtomicBool::new(false),
    });
    let listener = server::bind(&socket).unwrap();
    let serving = {
        let terminal = terminal.clone();
        let socket = socket.clone();
        std::thread::spawn(move || {
            let _ = server::serve(&listener, &terminal);
            drop(listener);
            let _ = std::fs::remove_file(socket);
        })
    };

    let d = Desk::with(KdfParams::FLOOR, Duration::from_secs(300), Duration::ZERO);
    d.host_broker();
    // The terminal broker was there first, so the window leaves it alone and says so.
    assert_eq!(d.broker_mode(), Mode::Cli);
    assert!(server::live(&socket));

    // The person clicks: it is told to lock, and the window hosts.
    assert_eq!(d.take_over_broker().unwrap(), Mode::Hosting);
    serving.join().unwrap();
    let status = client::call(&socket, "status", serde_json::json!({})).unwrap();
    assert_eq!(status["locked"], true);
    // Nothing to take over any more.
    assert_eq!(d.take_over_broker().unwrap(), Mode::Hosting);
    d.stop_broker();
}
