//! Keeping a running broker current (`docs/spec/broker.md` 8, ADR-0026): the same background sync as the
//! desktop app, every 15 seconds with backoff after a failure, then taking in what arrived.
//!
//! The network work runs on this thread with no broker lock held, so a slow or dead remote never delays a
//! grant. Taking in what the local folder holds is cheap and local, so it runs on every tick, with or without
//! any remote: that also carries a change made by another command on this machine, or by a folder sync such as
//! iCloud, to the broker.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use figlock_session::config::{self, Remote};
use figlock_session::sync::{self, INTERVAL, Report, SyncError, backoff};

use super::{Absorbed, Broker};

/// How often the thread checks whether anything is due.
const TICK: Duration = Duration::from_secs(1);

/// How one remote is doing: when it is next due, and how many attempts in a row have failed. Each remote
/// has its own, so a dead one backs off without slowing a healthy one (FIG-196).
struct Schedule {
    sync_at: Instant,
    failures: usize,
    /// The person has to act (sign in again, confirm an upload, pick the right vault); retrying cannot help.
    blocked: bool,
}

/// When the next syncs and the next local look are due, and how each remote's sync is going.
pub struct Syncer {
    start: Instant,
    remotes: HashMap<String, Schedule>,
    absorb_at: Instant,
    /// The broker locked itself because the vault changed on another device.
    pub locked_by_change: bool,
}

impl Syncer {
    pub fn new(now: Instant) -> Syncer {
        Syncer {
            start: now,
            remotes: HashMap::new(),
            absorb_at: now + INTERVAL,
            locked_by_change: false,
        }
    }

    /// Do whatever is due at `now`: one sync for each of `remotes` that is due, with `sync`, then taking in
    /// what arrived. One remote failing or being blocked never holds back another. Returns false once the
    /// broker is locked and there is nothing more to do.
    pub fn step(
        &mut self,
        now: Instant,
        broker: &Broker,
        remotes: &[Remote],
        sync: &mut dyn FnMut(&Remote) -> Result<Report, SyncError>,
    ) -> bool {
        if broker.is_locked() {
            return false;
        }
        let mut absorbed = false;
        for remote in remotes {
            let start = self.start;
            let sched = self.remotes.entry(remote.id()).or_insert(Schedule {
                sync_at: start,
                failures: 0,
                blocked: false,
            });
            if sched.blocked || now < sched.sync_at {
                continue;
            }
            match sync(remote) {
                Ok(_) => {
                    sched.failures = 0;
                    sched.sync_at = now + INTERVAL;
                    absorbed = true;
                }
                // Another sync on this device (a command's) is doing this work.
                Err(SyncError::Busy) => sched.sync_at = now + backoff(0),
                Err(e @ (SyncError::SignIn | SyncError::RemoteLost | SyncError::ForeignVault)) => {
                    sched.blocked = true;
                    broker.sync_failed(match e {
                        SyncError::SignIn => "sign_in",
                        SyncError::RemoteLost => "lost",
                        _ => "foreign",
                    });
                }
                Err(_) => {
                    sched.sync_at = now + backoff(sched.failures);
                    sched.failures += 1;
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
            // Read afresh each tick, so a remote turned on or off is picked up. No broker lock is held while
            // a remote is synced.
            let cfg = config::load().ok();
            let remotes = cfg.as_ref().map_or(&[][..], |c| c.place.remotes.as_slice());
            let mut run = |r: &Remote| match &cfg {
                Some(c) => sync::run_remote(c, r, false),
                None => Err(SyncError::NotEnabled),
            };
            if !syncer.step(Instant::now(), &broker, remotes, &mut run) {
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
