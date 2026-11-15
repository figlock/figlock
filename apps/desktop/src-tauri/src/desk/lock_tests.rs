//! When the vault locks by itself: after sleep, when the screen locks, and after the idle time the person chose
//! (see `tests.rs` for how scenarios run).

use std::sync::{Arc, Mutex};

use super::device_tests::unlocked;
use super::tests::scenario_in;
use super::*;

fn scenario(name: &str) {
    scenario_in("lock_tests", name);
}

/// A clock the test moves by hand, with `Instant` and the wall clock apart as they are on a sleeping Mac.
#[derive(Clone)]
struct FakeClock(Arc<Mutex<(Instant, SystemTime)>>);

impl FakeClock {
    fn new() -> Self {
        FakeClock(Arc::new(Mutex::new((Instant::now(), SystemTime::now()))))
    }
    /// Time passes while the Mac is awake: both clocks move.
    fn awake(&self, by: Duration) {
        let mut c = self.0.lock().unwrap();
        c.0 += by;
        c.1 += by;
    }
    /// The Mac sleeps: `Instant` stands still and the wall clock moves on.
    fn sleep(&self, by: Duration) {
        self.0.lock().unwrap().1 += by;
    }
    /// The clock is set back (a time sync, or the person).
    fn set_back(&self, by: Duration) {
        self.0.lock().unwrap().1 -= by;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.0.lock().unwrap().0
    }
    fn wall(&self) -> SystemTime {
        self.0.lock().unwrap().1
    }
}

fn desk_on(clock: &FakeClock) -> Desk {
    Desk::with_clock(
        KdfParams::FLOOR,
        Duration::from_secs(300),
        Duration::ZERO,
        Box::new(clock.clone()),
    )
}

#[test]
fn a_sleep_that_instant_does_not_count_still_locks() {
    scenario("sleep_gap_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn sleep_gap_inner() {
    let clock = FakeClock::new();
    let d = desk_on(&clock);
    unlocked(&d);
    clock.awake(Duration::from_secs(1));
    assert!(!d.tick(), "ordinary ticks do not lock");
    // A two-minute sleep: `Instant` reads one second, the wall clock two minutes (FIG-131).
    clock.sleep(Duration::from_secs(120));
    clock.awake(Duration::from_secs(1));
    assert!(d.tick(), "the wall-clock gap locks the vault");
    assert_eq!(d.state().unwrap().phase, "locked");
}

#[test]
fn a_clock_set_back_does_not_lock() {
    scenario("clock_back_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn clock_back_inner() {
    let clock = FakeClock::new();
    let d = desk_on(&clock);
    unlocked(&d);
    clock.set_back(Duration::from_secs(3600));
    clock.awake(Duration::from_secs(1));
    assert!(!d.tick());
    assert_eq!(d.state().unwrap().phase, "unlocked");
}

#[test]
fn the_sleep_and_screen_lock_events_lock_at_once() {
    scenario("sleep_event_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn sleep_event_inner() {
    let clock = FakeClock::new();
    let d = desk_on(&clock);
    assert!(!d.lock_for_sleep(), "nothing to lock yet");
    unlocked(&d);
    assert!(d.lock_for_sleep());
    assert_eq!(d.state().unwrap().phase, "locked");
    assert!(!d.lock_for_sleep(), "a second event has nothing to do");
}

#[test]
fn the_auto_lock_time_is_kept_on_this_device_and_applies_at_once() {
    scenario("autolock_inner");
}

#[test]
#[ignore = "runs as a child of the test above"]
fn autolock_inner() {
    let clock = FakeClock::new();
    let d = desk_on(&clock);
    unlocked(&d);
    assert_eq!(d.state().unwrap().autolock_secs, 300);

    // One minute applies without a restart: it locks after 60 idle seconds, where 5 minutes would not.
    assert_eq!(d.set_autolock(60).unwrap().autolock_secs, 60);
    clock.awake(Duration::from_secs(10));
    assert!(!d.tick());
    // The ticks are 10 s apart here, under the sleep gap, as the one-second timer's are.
    for _ in 0..5 {
        clock.awake(Duration::from_secs(10));
        let _ = d.tick();
    }
    assert_eq!(d.state().unwrap().phase, "locked");

    // It is in this device's config, so a new run of the app starts from it.
    assert_eq!(config::load().unwrap().autolock_secs, Some(60));
    assert_eq!(Desk::new().state().unwrap().autolock_secs, 60);

    // Only the times Settings offers are accepted, and the default is stored as "no choice".
    for bad in [0, 1, 59, 61, 86_400] {
        assert!(d.set_autolock(bad).is_err(), "{bad}");
    }
    assert_eq!(d.state().unwrap().autolock_secs, 60);
    assert_eq!(d.set_autolock(300).unwrap().autolock_secs, 300);
    assert_eq!(config::load().unwrap().autolock_secs, None);
    for secs in AUTOLOCK_CHOICES {
        assert_eq!(d.set_autolock(secs).unwrap().autolock_secs, secs);
        assert_eq!(config::load().unwrap().autolock_secs.is_some(), secs != 300);
    }
}
