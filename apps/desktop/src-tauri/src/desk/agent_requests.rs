//! Agent requests in the window (ADR-0040). When the app hosts the broker and a request needs a prompt, the
//! window shows a card and posts a notification. **Approve on the card runs the system Touch ID prompt**, and
//! only an accepted prompt becomes an allow; a click on the card, or on the notification, never approves by
//! itself. If the card cannot be shown, the broker falls back to the direct Touch ID window.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use figlock_biometric::Outcome;
use figlock_broker::approve::{ApprovalRequest, Approver, Decision, duration, how, prompt_line};
use figlock_core::audit_clean_text;

/// What the card shows. Everything here is written by the broker from what the system verified, except the
/// two fields marked unchecked, which the asking program chose.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: u64,
    /// The program the system says is asking, and its full path.
    pub program: String,
    pub program_path: String,
    /// The path is an absolute one the system gave. When it is not, the card must not say it was checked.
    pub program_verified: bool,
    /// Programs that started it, nearest first (`zsh`, `claude`).
    pub started_by: Vec<String>,
    /// What the asking program calls itself. Not checked.
    pub says: Option<String>,
    /// Why it says it asks. Not checked.
    pub reason: Option<String>,
    /// What it wants, in the broker's words.
    pub wants: String,
    pub how: &'static str,
    pub duration: &'static str,
    /// Extra loud warnings (reveal, the whole vault).
    pub loud: Vec<&'static str>,
    /// Seconds until the request counts as no.
    pub seconds: u64,
}

fn base(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(path);
    audit_clean_text(name, 60)
}

impl Card {
    fn of(id: u64, req: &ApprovalRequest, wait: Duration) -> Card {
        let exe = req.peer.exe.as_deref().unwrap_or("an unknown program");
        Card {
            id,
            program: base(exe),
            program_path: audit_clean_text(exe, 300),
            program_verified: req.peer.exe.as_deref().is_some_and(|e| e.starts_with('/')),
            started_by: req.peer.parents.iter().map(|p| base(p)).collect(),
            says: req.label.as_deref().map(|l| audit_clean_text(l, 100)),
            reason: req.reason.as_deref().map(|r| audit_clean_text(r, 200)),
            wants: audit_clean_text(&req.scope_words, 400),
            how: how(req.mode),
            duration: duration(req.ttl),
            loud: req.loud.clone(),
            seconds: wait.as_secs(),
        }
    }

    /// One line for a notification.
    pub fn headline(&self) -> String {
        audit_clean_text(
            &format!(
                "{} wants {} for {}",
                self.program, self.wants, self.duration
            ),
            160,
        )
    }
}

/// What the window is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A request is waiting. The sink answers whether anyone can see it.
    Open(Box<Card>),
    /// It was answered, or timed out.
    Closed(u64),
}

type Sink = Box<dyn Fn(&Event) -> bool + Send + Sync>;

struct Entry {
    card: Card,
    /// The sentence the system prompt shows (`prompt_line`).
    prompt: String,
    reply: Sender<Decision>,
    /// The system prompt is open for this card; a second press must not open another.
    deciding: bool,
}

/// How a decision on the card ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decided {
    Approved,
    /// Not now, or the system prompt was cancelled or failed.
    NotApproved,
    /// The request is gone (it timed out, or was answered elsewhere).
    Gone,
}

pub struct Approvals {
    pending: Mutex<Vec<Entry>>,
    next: AtomicU64,
    confirm: fn(&str, Duration) -> Outcome,
    sink: Mutex<Option<Sink>>,
}

impl Default for Approvals {
    fn default() -> Self {
        Self::new(figlock_biometric::confirm)
    }
}

impl Approvals {
    pub fn new(confirm: fn(&str, Duration) -> Outcome) -> Approvals {
        Approvals {
            pending: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            confirm,
            sink: Mutex::new(None),
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Where cards are shown. Until one is set (a window that is not up), requests use the fallback.
    pub fn set_sink(&self, sink: Sink) {
        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
    }

    fn tell(&self, event: &Event) -> bool {
        self.sink
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|s| s(event))
    }

    /// The cards waiting now, for a window that opened after the request came in.
    pub fn waiting(&self) -> Vec<Card> {
        self.pending().iter().map(|e| e.card.clone()).collect()
    }

    /// Show the request and wait for the person. `None` when it could not be shown, so the caller asks another
    /// way. No answer in `wait` is a no.
    pub fn ask(&self, req: &ApprovalRequest, wait: Duration) -> Option<Decision> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let card = Card::of(id, req, wait);
        let (reply, answer) = channel();
        self.pending().push(Entry {
            card: card.clone(),
            prompt: prompt_line(req),
            reply,
            deciding: false,
        });
        if !self.tell(&Event::Open(Box::new(card))) {
            self.pending().retain(|e| e.card.id != id);
            return None;
        }
        let decision = answer.recv_timeout(wait).unwrap_or(Decision::Deny);
        self.pending().retain(|e| e.card.id != id);
        self.tell(&Event::Closed(id));
        Some(decision)
    }

    /// The person pressed a button on card `id`. Approve is only a request for the system prompt: it becomes an
    /// allow when Touch ID or the device password is accepted, and a cancelled or failed prompt is a no.
    pub fn decide(&self, id: u64, approve: bool) -> Decided {
        let Some((prompt, reply)) = ({
            let mut pending = self.pending();
            pending
                .iter_mut()
                .find(|e| e.card.id == id && !e.deciding)
                .map(|e| {
                    e.deciding = approve;
                    (e.prompt.clone(), e.reply.clone())
                })
        }) else {
            return Decided::Gone;
        };
        let allowed =
            approve && (self.confirm)(&prompt, Duration::from_secs(60)) == Outcome::Confirmed;
        let sent = reply
            .send(if allowed {
                Decision::Allow
            } else {
                Decision::Deny
            })
            .is_ok();
        match (sent, allowed) {
            (false, _) => Decided::Gone,
            (true, true) => Decided::Approved,
            (true, false) => Decided::NotApproved,
        }
    }
}

/// The approver the app's broker uses: the card when it can be shown, otherwise `fallback` (the direct system
/// prompt, or deny).
pub struct CardApprover {
    pub approvals: Arc<Approvals>,
    pub fallback: Box<dyn Approver>,
}

impl Approver for CardApprover {
    fn approve(&self, req: &ApprovalRequest, wait: Duration) -> Decision {
        self.approvals
            .ask(req, wait)
            .unwrap_or_else(|| self.fallback.approve(req, wait))
    }
}
