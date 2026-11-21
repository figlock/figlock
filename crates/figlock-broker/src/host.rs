//! Where the unlocked vault lives (ADR-0040). The broker does not own a session: it asks a [`SessionHost`]
//! for it, so the same rules serve the CLI daemon (which owns one) and, later, the desktop app (whose window
//! already holds one).

use std::ops::DerefMut;
use std::sync::{Mutex, MutexGuard};

use figlock_session::session::Session;

/// The unlocked session, or the news that there is none.
pub trait SessionHost: Send + Sync {
    /// The session while the vault is unlocked, `None` once it is locked. The guard holds the host's lock
    /// until it is dropped, and that lock is not reentrant: take the session, use it, drop it, and never
    /// call back into the broker meanwhile.
    fn session(&self) -> Option<Box<dyn DerefMut<Target = Session> + '_>>;

    /// Whether there is no session.
    fn is_locked(&self) -> bool;

    /// Forget the session and its keys. Called by the broker when it locks (a revocation, a changed vault,
    /// `broker lock`). A host that locks for its own reasons (auto-lock, sleep) calls `Broker::lock` first so
    /// the grants end with it.
    fn lock(&self);
}

/// A host that owns its session outright: the CLI daemon, and the tests.
pub struct LocalHost {
    session: Mutex<Option<Session>>,
}

impl LocalHost {
    pub fn new(session: Session) -> LocalHost {
        LocalHost {
            session: Mutex::new(Some(session)),
        }
    }

    fn guard(&self) -> MutexGuard<'_, Option<Session>> {
        // A panic while holding the lock must not turn into a bypass; a poisoned lock still guards the data.
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The session behind the lock, shown as a plain `&mut Session`. Only built when there is one.
struct Open<'a>(MutexGuard<'a, Option<Session>>);

impl std::ops::Deref for Open<'_> {
    type Target = Session;
    fn deref(&self) -> &Session {
        self.0.as_ref().expect("checked when built")
    }
}

impl DerefMut for Open<'_> {
    fn deref_mut(&mut self) -> &mut Session {
        self.0.as_mut().expect("checked when built")
    }
}

impl SessionHost for LocalHost {
    fn session(&self) -> Option<Box<dyn DerefMut<Target = Session> + '_>> {
        let guard = self.guard();
        guard.is_some().then(|| Box::new(Open(guard)) as _)
    }

    fn is_locked(&self) -> bool {
        self.guard().is_none()
    }

    fn lock(&self) {
        if let Some(s) = self.guard().take() {
            s.vault.lock();
        }
    }
}
