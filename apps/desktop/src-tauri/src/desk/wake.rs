//! Lets the sync thread sleep until there is something to do (FIG-261).
//!
//! The thread used to look every second whether anything was due. Now it waits for the next deadline (at most
//! one sync interval away) or for a save, whichever comes first: a save still reaches the remotes at once, and
//! an idle app wakes four times a minute instead of sixty.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
pub(super) struct Waker {
    poked: Mutex<bool>,
    cv: Condvar,
}

impl Waker {
    /// Something happened that the sync thread should look at now.
    pub(super) fn notify(&self) {
        *self.poked.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cv.notify_all();
    }

    /// Sleep for at most `limit`, or until `notify`. A notify that came before the wait is not lost.
    pub(super) fn wait(&self, limit: Duration) {
        let mut poked = self.poked.lock().unwrap_or_else(|e| e.into_inner());
        if !*poked {
            poked = self
                .cv
                .wait_timeout(poked, limit)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *poked = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn a_notify_wakes_a_waiter_long_before_its_limit() {
        let w = Arc::new(Waker::default());
        let w2 = Arc::clone(&w);
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            w2.notify();
        });
        let started = Instant::now();
        w.wait(Duration::from_secs(30));
        assert!(started.elapsed() < Duration::from_secs(5));
        t.join().unwrap();
    }

    #[test]
    fn a_notify_before_the_wait_is_not_lost() {
        let w = Waker::default();
        w.notify();
        let started = Instant::now();
        w.wait(Duration::from_secs(30));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn without_a_notify_the_wait_ends_at_its_limit_and_waits_again_next_time() {
        let w = Waker::default();
        let started = Instant::now();
        w.wait(Duration::from_millis(60));
        assert!(started.elapsed() >= Duration::from_millis(50));
        w.notify();
        w.wait(Duration::from_secs(30));
        // The poke was used up by that wait: the next one sleeps again.
        let started = Instant::now();
        w.wait(Duration::from_millis(60));
        assert!(started.elapsed() >= Duration::from_millis(50));
    }
}
