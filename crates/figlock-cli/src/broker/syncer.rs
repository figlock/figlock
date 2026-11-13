//! Keeping a running broker current (`docs/spec/broker.md` 8, ADR-0026): the same background sync as the
//! desktop app, every 15 seconds with backoff after a failure, then taking in what arrived.
//!
//! The network work runs on this thread with no broker lock held, so a slow or dead remote never delays a
//! grant. Taking in what the local folder holds is cheap and local, so it runs on every tick, with or without
//! a remote: that also carries a change made by another command on this machine, or by a folder sync such as
//! iCloud, to the broker.

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use figlock_session::config;
use figlock_session::sync::{self, INTERVAL, Report, SyncError, backoff};

use super::{Absorbed, Broker};

/// How often the thread checks whether anything is due.
const TICK: Duration = Duration::from_secs(1);

/// When the next sync and the next local look are due, and how the sync is going.
pub struct Syncer {
    sync_at: Instant,
    absorb_at: Instant,
    failures: usize,
    /// The person has to act (sign in again, confirm an upload, pick the right vault); retrying cannot help.
    blocked: bool,
    /// The broker locked itself because the vault changed on another device.
    pub locked_by_change: bool,
}

impl Syncer {
    pub fn new(now: Instant) -> Syncer {
        Syncer {
            sync_at: now,
            absorb_at: now + INTERVAL,
            failures: 0,
            blocked: false,
            locked_by_change: false,
        }
    }

    /// Do whatever is due at `now`. `sync` runs one sync, or is `None` when no remote is configured.
    /// Returns false once the broker is locked and there is nothing more to do.
    pub fn step(
        &mut self,
        now: Instant,
        broker: &Broker,
        sync: Option<&mut dyn FnMut() -> Result<Report, SyncError>>,
    ) -> bool {
        if broker.is_locked() {
            return false;
        }
        let mut absorbed = false;
        if let Some(sync) = sync.filter(|_| !self.blocked && now >= self.sync_at) {
            match sync() {
                Ok(_) => {
                    self.failures = 0;
                    self.sync_at = now + INTERVAL;
                    absorbed = true;
                }
                // Another sync on this device (a command's) is doing this work.
                Err(SyncError::Busy) => self.sync_at = now + backoff(0),
                Err(e @ (SyncError::SignIn | SyncError::RemoteLost | SyncError::ForeignVault)) => {
                    self.blocked = true;
                    broker.sync_failed(match e {
                        SyncError::SignIn => "sign_in",
                        SyncError::RemoteLost => "lost",
                        _ => "foreign",
                    });
                }
                Err(_) => {
                    self.sync_at = now + backoff(self.failures);
                    self.failures += 1;
                }
            }
        }
        if absorbed || now >= self.absorb_at {
            self.absorb_at = now + INTERVAL;
            if broker.absorb() == Absorbed::Locked {
                self.locked_by_change = true;
                return false;
            }
        }
        true
    }
}

/// Run [`Syncer`] on its own thread until the broker locks.
pub fn spawn(broker: Arc<Broker>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut syncer = Syncer::new(Instant::now());
        loop {
            let cfg = config::load().ok().filter(|c| c.place.remote.is_some());
            let mut run = cfg.as_ref().map(|c| move || sync::run(c, false));
            let sync = run
                .as_mut()
                .map(|f| f as &mut dyn FnMut() -> Result<Report, SyncError>);
            if !syncer.step(Instant::now(), &broker, sync) {
                if syncer.locked_by_change {
                    eprintln!(
                        "the vault changed on another device or could not be read, so the broker locked; start it again to unlock"
                    );
                }
                return;
            }
            thread::sleep(TICK);
        }
    })
}
