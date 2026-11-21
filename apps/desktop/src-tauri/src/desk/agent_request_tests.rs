//! The card and the rule behind it: a button is never an approval, only an accepted system prompt is.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use figlock_biometric::Outcome;
use figlock_broker::approve::{ApprovalRequest, Approver, Decision};
use figlock_broker::peer::PeerId;
use figlock_broker::proto::{Mode, Ttl};

use super::agent_requests::{Approvals, CardApprover, Decided, Event};

fn request() -> ApprovalRequest {
    ApprovalRequest {
        peer: PeerId {
            uid: 501,
            pid: Some(4242),
            exe: Some("/usr/local/bin/figlock".into()),
            parents: vec!["/bin/zsh".into(), "/opt/agent/claude".into()],
        },
        label: Some("figlock http".into()),
        reason: Some("list open issues".into()),
        scope_words: "the token \"GitHub\" (only for api.github.com)".into(),
        mode: Mode::Http,
        ttl: Ttl::Min15,
        loud: vec![],
    }
}

fn confirmed(_: &str, _: Duration) -> Outcome {
    Outcome::Confirmed
}
fn declined(_: &str, _: Duration) -> Outcome {
    Outcome::Declined
}
fn unavailable(_: &str, _: Duration) -> Outcome {
    Outcome::Unavailable
}

/// A window that records what it was shown.
fn window(a: &Approvals) -> Arc<Mutex<Vec<Event>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    a.set_sink(Box::new(move |e| {
        log.lock().unwrap().push(e.clone());
        true
    }));
    seen
}

/// Ask on another thread and answer the card from this one.
fn ask_and(
    a: &Arc<Approvals>,
    wait: Duration,
    respond: impl FnOnce(u64) -> Decided,
) -> (Decision, Decided) {
    let asking = {
        let a = a.clone();
        std::thread::spawn(move || a.ask(&request(), wait))
    };
    let id = loop {
        if let Some(c) = a.waiting().first() {
            break c.id;
        }
        std::thread::yield_now();
    };
    let decided = respond(id);
    (asking.join().unwrap().expect("a window was there"), decided)
}

#[test]
fn the_card_says_what_the_system_verified_and_marks_what_the_program_claimed() {
    let a = Arc::new(Approvals::new(confirmed));
    let seen = window(&a);
    let _ = ask_and(&a, Duration::from_secs(5), |id| a.decide(id, false));
    let Event::Open(card) = seen.lock().unwrap()[0].clone() else {
        panic!("a card was opened");
    };
    let card = *card;
    assert_eq!(card.program, "figlock");
    assert_eq!(card.started_by, ["zsh", "claude"]);
    assert_eq!(card.says.as_deref(), Some("figlock http"));
    assert_eq!(card.reason.as_deref(), Some("list open issues"));
    assert!(card.wants.contains("api.github.com"));
    assert_eq!(card.duration, "15 minutes");
    assert!(card.headline().starts_with("figlock wants the token"));
    assert_eq!(seen.lock().unwrap().last(), Some(&Event::Closed(card.id)));
}

#[test]
fn approve_is_an_allow_only_when_the_system_prompt_is_accepted() {
    for (confirm, want, decided) in [
        (
            confirmed as fn(&str, Duration) -> Outcome,
            Decision::Allow,
            Decided::Approved,
        ),
        (declined, Decision::Deny, Decided::NotApproved),
        (unavailable, Decision::Deny, Decided::NotApproved),
    ] {
        let a = Arc::new(Approvals::new(confirm));
        window(&a);
        let (decision, got) = ask_and(&a, Duration::from_secs(5), |id| a.decide(id, true));
        assert_eq!((decision, got), (want, decided));
    }
}

#[test]
fn not_now_denies_without_ever_opening_the_system_prompt() {
    static PROMPTS: AtomicUsize = AtomicUsize::new(0);
    fn counting(_: &str, _: Duration) -> Outcome {
        PROMPTS.fetch_add(1, Ordering::SeqCst);
        Outcome::Confirmed
    }
    let a = Arc::new(Approvals::new(counting));
    window(&a);
    let (decision, decided) = ask_and(&a, Duration::from_secs(5), |id| a.decide(id, false));
    assert_eq!((decision, decided), (Decision::Deny, Decided::NotApproved));
    assert_eq!(PROMPTS.load(Ordering::SeqCst), 0);
}

#[test]
fn nobody_answering_is_a_no_and_the_card_is_withdrawn() {
    let a = Arc::new(Approvals::new(confirmed));
    let seen = window(&a);
    let started = std::time::Instant::now();
    let decision = a.ask(&request(), Duration::from_millis(100));
    assert_eq!(decision, Some(Decision::Deny));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(a.waiting().is_empty());
    assert!(matches!(
        seen.lock().unwrap().last(),
        Some(Event::Closed(_))
    ));
}

#[test]
fn a_late_or_unknown_answer_does_nothing() {
    let a = Approvals::new(confirmed);
    window(&a);
    assert_eq!(a.decide(999, true), Decided::Gone);
    // Answered after the request timed out.
    let _ = a.ask(&request(), Duration::from_millis(10));
    assert_eq!(a.decide(1, true), Decided::Gone);
}

#[test]
fn without_a_window_the_request_is_not_taken_and_the_fallback_asks() {
    struct Fallback(AtomicUsize);
    struct Shared(Arc<Fallback>);
    impl Approver for Shared {
        fn approve(&self, _: &ApprovalRequest, _: Duration) -> Decision {
            self.0.0.fetch_add(1, Ordering::SeqCst);
            Decision::Deny
        }
    }
    let a = Arc::new(Approvals::new(confirmed));
    assert_eq!(a.ask(&request(), Duration::from_secs(1)), None, "no sink");
    assert!(a.waiting().is_empty());
    // A sink that says nobody can see the card is the same.
    a.set_sink(Box::new(|_| false));
    assert_eq!(a.ask(&request(), Duration::from_secs(1)), None);
    assert!(a.waiting().is_empty());
    let fallback = Arc::new(Fallback(AtomicUsize::new(0)));
    let approver = CardApprover {
        approvals: a,
        fallback: Box::new(Shared(fallback.clone())),
    };
    assert_eq!(
        approver.approve(&request(), Duration::from_secs(1)),
        Decision::Deny
    );
    assert_eq!(fallback.0.load(Ordering::SeqCst), 1);
}

#[test]
fn a_second_press_while_the_system_prompt_is_open_does_not_open_another() {
    static PROMPTS: AtomicUsize = AtomicUsize::new(0);
    fn slow(_: &str, _: Duration) -> Outcome {
        PROMPTS.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        Outcome::Confirmed
    }
    let a = Arc::new(Approvals::new(slow));
    window(&a);
    let (decision, decided) = ask_and(&a, Duration::from_secs(5), |id| {
        let first = {
            let a = a.clone();
            std::thread::spawn(move || a.decide(id, true))
        };
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(a.decide(id, true), Decided::Gone, "already being decided");
        first.join().unwrap()
    });
    assert_eq!((decision, decided), (Decision::Allow, Decided::Approved));
    assert_eq!(PROMPTS.load(Ordering::SeqCst), 1);
}
