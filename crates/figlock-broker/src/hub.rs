//! One socket for a host whose vault locks and unlocks (ADR-0040). The CLI daemon unlocks once and serves
//! until it locks; the desktop app is unlocked and locked many times while it runs. The hub keeps the socket
//! open the whole time and sends each request to the broker of the current unlock, or answers `locked` when
//! there is none, so an agent is told to unlock FigLock instead of finding no broker.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::peer::{PeerId, my_uid};
use super::proto::{Code, Op, err, ok, parse_request};
use super::server::Dispatch;
use super::{Broker, Reply};

pub struct Hub {
    current: Mutex<Option<Arc<Broker>>>,
    stop: AtomicBool,
    uid: u32,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Hub {
        Hub {
            current: Mutex::new(None),
            stop: AtomicBool::new(false),
            uid: my_uid(),
        }
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<Arc<Broker>>> {
        self.current.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Serve the broker of a new unlock. The previous one, if any, is dropped with its grants.
    pub fn set(&self, broker: Arc<Broker>) {
        *self.slot() = Some(broker);
    }

    /// Nothing is unlocked: answer `locked`.
    pub fn clear(&self) {
        *self.slot() = None;
    }

    pub fn current(&self) -> Option<Arc<Broker>> {
        self.slot().clone()
    }

    /// End the accept loop (the app is quitting).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Dispatch for Hub {
    fn admit(&self, peer: &PeerId) -> bool {
        match self.current() {
            Some(b) => b.admit(peer),
            None => peer.uid == self.uid,
        }
    }

    fn handle(&self, peer: &PeerId, line: &str, alive: &dyn Fn() -> bool) -> Reply {
        if let Some(b) = self.current() {
            let mut reply = b.handle_line_live(peer, line, alive);
            // A `lock` ends this broker; the socket stays, because the app unlocks again.
            reply.exit = false;
            return reply;
        }
        let req = match parse_request(line) {
            Ok(r) => r,
            Err(m) => return Reply::line(err(&m.id, Code::BadRequest)),
        };
        let line = match req.op {
            Op::Hello => ok(&req.id, json!({"version": 1, "locked": true})),
            Op::Status => ok(
                &req.id,
                json!({"locked": true, "grants": 0, "audit_seq": 0, "audit_head": "0".repeat(64)}),
            ),
            _ => err(&req.id, Code::Locked),
        };
        Reply::line(line)
    }

    fn finished(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn exit_ends_serving(&self) -> bool {
        false
    }
}
