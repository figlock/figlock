//! The desktop app as the agent broker's host (ADR-0040). While the window is unlocked it serves the broker
//! socket from the session it already holds, so an agent needs no `figlock broker start` and nobody unlocks
//! twice. Approvals here are still the system Touch ID prompt; the notification and card come next.
//!
//! Every unlock gets its own broker, tied to that unlock's `epoch`. When the window locks, sleeps, auto-locks
//! or is replaced by another unlock, the epoch moves on and that broker answers `locked` to everything, so a
//! grant can never outlive the session it was approved for. Lock paths that hold the session's mutex
//! need not call into the broker at all, which keeps the lock order simple (broker state, then session).

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

use figlock_broker::approve::{Approver, BiometricApprover, DenyAll};
use figlock_broker::audit::{self, AuditWriter};
use figlock_broker::host::SessionHost;
use figlock_broker::hub::Hub;
use figlock_broker::peer::{SystemEnv, my_uid};
use figlock_broker::{Broker, client, server};
use figlock_session::session::Session;

use super::{CmdError, CmdRes, Desk, Inner};

/// What a test answers prompts with, instead of the system's Touch ID prompt.
#[cfg(test)]
pub(super) type MakeApprover = fn() -> Box<dyn Approver>;

#[cfg(test)]
pub(super) static TEST_APPROVER: Mutex<Option<MakeApprover>> = Mutex::new(None);

/// How long a CLI broker gets to stop when the person asks the app to take over.
const TAKEOVER_WAIT: Duration = Duration::from_secs(5);

/// Who answers on the broker socket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Nobody (not started, or the socket could not be made).
    #[default]
    Off,
    /// This window.
    Hosting,
    /// A `figlock broker start` process that was there first. The window offers to take over.
    Cli,
}

#[derive(Default)]
pub struct Hosting {
    hub: Mutex<Option<Arc<Hub>>>,
    mode: Mutex<Mode>,
    /// Held while a new unlock's broker is built and set, so overlapping unlocks cannot leave an older one in place.
    starting: Mutex<()>,
}

impl Hosting {
    fn hub(&self) -> Option<Arc<Hub>> {
        self.hub.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, hub: Option<Arc<Hub>>, mode: Mode) {
        *self.hub.lock().unwrap_or_else(|e| e.into_inner()) = hub;
        *self.mode.lock().unwrap_or_else(|e| e.into_inner()) = mode;
    }

    pub fn mode(&self) -> Mode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The window's session, as the broker sees it: present only while the unlock it was started for is still the
/// current one.
pub struct DeskHost {
    inner: Arc<Mutex<Inner>>,
    epoch: u64,
    /// The broker locked itself (a revoked device, a changed vault). The window stays as it is.
    ended: AtomicBool,
}

impl DeskHost {
    fn guard(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

struct Open<'a>(MutexGuard<'a, Inner>);

impl Deref for Open<'_> {
    type Target = Session;
    fn deref(&self) -> &Session {
        self.0.session.as_ref().expect("checked when built")
    }
}

impl DerefMut for Open<'_> {
    fn deref_mut(&mut self) -> &mut Session {
        self.0.session.as_mut().expect("checked when built")
    }
}

impl SessionHost for DeskHost {
    fn session(&self) -> Option<Box<dyn DerefMut<Target = Session> + '_>> {
        if self.ended.load(Ordering::SeqCst) {
            return None;
        }
        let g = self.guard();
        (g.epoch == self.epoch && g.session.is_some()).then(|| Box::new(Open(g)) as _)
    }

    fn is_locked(&self) -> bool {
        if self.ended.load(Ordering::SeqCst) {
            return true;
        }
        let g = self.guard();
        g.epoch != self.epoch || g.session.is_none()
    }

    fn lock(&self) {
        // Ends the broker, not the window: the person is still looking at an unlocked vault.
        self.ended.store(true, Ordering::SeqCst);
    }
}

impl Desk {
    /// Start answering on the broker socket, or find out that a CLI broker already does. Call once when the app
    /// starts; [`Desk::take_over_broker`] retries after the person agrees to replace a CLI broker.
    pub fn host_broker(&self) {
        let Ok(socket) = server::socket_path() else {
            return;
        };
        match server::bind(&socket) {
            Ok(listener) => {
                let hub = Arc::new(Hub::new());
                self.hosting.set(Some(hub.clone()), Mode::Hosting);
                std::thread::spawn(move || {
                    let _ = server::serve(&listener, &hub);
                    drop(listener);
                    let _ = std::fs::remove_file(&socket);
                });
                // A window that was already unlocked (a takeover) serves from its session straight away.
                self.start_broker();
            }
            Err(_) => {
                let mode = if server::live(&socket) {
                    Mode::Cli
                } else {
                    Mode::Off
                };
                self.hosting.set(None, mode);
            }
        }
    }

    pub fn broker_mode(&self) -> Mode {
        self.hosting.mode()
    }

    /// Stop a CLI broker (its own `lock` request ends its grants and exits it) and host here instead.
    pub fn take_over_broker(&self) -> CmdRes<Mode> {
        if self.hosting.mode() != Mode::Cli {
            return Ok(self.hosting.mode());
        }
        let socket = server::socket_path().map_err(CmdError::from)?;
        let _ = client::call(&socket, "lock", serde_json::json!({}));
        let until = Instant::now() + TAKEOVER_WAIT;
        while server::live(&socket) {
            if Instant::now() >= until {
                return Err(CmdError::new(
                    "broker_busy",
                    "The broker running in a terminal did not stop. Stop it there and try again.",
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.host_broker();
        Ok(self.hosting.mode())
    }

    /// The app is quitting: free the socket.
    pub fn stop_broker(&self) {
        if let Some(hub) = self.hosting.hub() {
            hub.stop();
            // The serving thread removes it too, but the process may end first.
            if let Ok(socket) = server::socket_path() {
                let _ = std::fs::remove_file(socket);
            }
        }
    }

    /// A new unlock: serve it with a broker of its own. A failure (the audit log cannot be opened or written)
    /// leaves the socket answering `locked`, and the window works as before.
    pub(super) fn start_broker(&self) {
        let Some(hub) = self.hosting.hub() else {
            return;
        };
        let _starting = self.hosting.starting.lock().unwrap_or_else(|e| e.into_inner());
        hub.clear();
        let (epoch, opened) = {
            let inner = self.inner();
            let Some(session) = inner.session.as_ref() else {
                return;
            };
            let writer = audit::path_for(&session.vault)
                .and_then(|path| AuditWriter::open(&path, &session.vault));
            (inner.epoch, writer)
        };
        let Ok(writer) = opened else {
            eprintln!(
                "the agent broker could not open its audit log, so agents cannot ask for secrets"
            );
            return;
        };
        let host = Arc::new(DeskHost {
            inner: self.inner.clone(),
            epoch,
            ended: AtomicBool::new(false),
        });
        let approver: Box<dyn Approver> = match BiometricApprover::open() {
            Some(b) => Box::new(b),
            None => Box::new(DenyAll),
        };
        #[cfg(test)]
        let approver = TEST_APPROVER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map_or(approver, |make| make());
        match Broker::start_on(host, writer, approver, Arc::new(SystemEnv::new()), my_uid()) {
            Ok(broker) => hub.set(Arc::new(broker)),
            Err(_) => eprintln!("the agent broker could not start"),
        }
    }

    /// What `absorb` found, passed on to the broker: a changed vault or a removed device ends every grant,
    /// and changed items end the grants on items that are gone. Called with no session lock held.
    pub(super) fn tell_broker(&self, changed: bool, stale: bool) {
        let Some(broker) = self.hosting.hub().and_then(|h| h.current()) else {
            return;
        };
        if stale {
            broker.end("settings_changed");
        } else if changed {
            broker.items_changed();
        }
    }

    /// Serve the current unlock with a new broker, as an unlock does (a test that ended the first one).
    #[cfg(test)]
    pub(super) fn unlock_for_test_restart(&self, _hub: &Hub) {
        self.start_broker();
    }

    #[cfg(test)]
    pub(super) fn host_for_test(&self) -> Arc<Hub> {
        let hub = Arc::new(Hub::new());
        self.hosting.set(Some(hub.clone()), Mode::Hosting);
        hub
    }
}
