//! Hybrid logical clock (`sync.md` 5).

use crate::{Error, Result};

/// Operations stamped more than this far ahead of the local clock are held, not applied.
pub const MAX_DRIFT_MS: u64 = 24 * 3600 * 1000;

/// `(wall_ms, counter)`. The derived order is the order of the spec's total order key without the device
/// id; ties between devices are broken by comparing device ids in [`crate::merge`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    pub wall: u64,
    pub counter: u16,
}

impl Stamp {
    /// Largest wall time the 48-bit wire form can carry.
    pub const MAX_WALL: u64 = (1 << 48) - 1;

    pub const ZERO: Stamp = Stamp {
        wall: 0,
        counter: 0,
    };

    /// The 8-byte form used in AADs and payloads: `u48(wall_ms) || u16(counter)`.
    ///
    /// Panics if `wall` exceeds [`Stamp::MAX_WALL`]; [`Hlc`] never produces such a stamp and decoded
    /// stamps always fit.
    pub fn to_bytes(self) -> [u8; 8] {
        assert!(
            self.wall <= Self::MAX_WALL,
            "stamp wall time exceeds 48 bits"
        );
        crate::wire::hlc_bytes(self.wall, self.counter)
    }

    pub fn from_bytes(b: &[u8; 8]) -> Self {
        let wall = u64::from_be_bytes([0, 0, b[0], b[1], b[2], b[3], b[4], b[5]]);
        Stamp {
            wall,
            counter: u16::from_be_bytes([b[6], b[7]]),
        }
    }
}

/// A device's clock. `last` must be persisted across restarts and raised to at least the current time on
/// start, so stamps never repeat or go backwards.
#[derive(Clone, Copy, Debug)]
pub struct Hlc {
    last: Stamp,
}

impl Hlc {
    pub fn new(last: Stamp) -> Self {
        Hlc { last }
    }

    pub fn last(&self) -> Stamp {
        self.last
    }

    /// Stamp for a local event. Fails if the counter would exceed `0xFFFF`; the caller waits one
    /// millisecond and retries.
    pub fn local(&mut self, now_ms: u64) -> Result<Stamp> {
        let next = if now_ms > self.last.wall {
            Stamp {
                wall: now_ms,
                counter: 0,
            }
        } else {
            Stamp {
                wall: self.last.wall,
                counter: self.last.counter.checked_add(1).ok_or(Error)?,
            }
        };
        if next.wall > Stamp::MAX_WALL {
            return Err(Error);
        }
        self.last = next;
        Ok(next)
    }

    /// Fold in a stamp received from another device.
    pub fn receive(&mut self, msg: Stamp, now_ms: u64) -> Result<Stamp> {
        let wall = self.last.wall.max(msg.wall).max(now_ms);
        let base = if wall == self.last.wall && wall == msg.wall {
            Some(self.last.counter.max(msg.counter))
        } else if wall == self.last.wall {
            Some(self.last.counter)
        } else if wall == msg.wall {
            Some(msg.counter)
        } else {
            None
        };
        let counter = match base {
            Some(c) => c.checked_add(1).ok_or(Error)?,
            None => 0,
        };
        if wall > Stamp::MAX_WALL {
            return Err(Error);
        }
        self.last = Stamp { wall, counter };
        Ok(self.last)
    }
}

/// True if an op with this wall time must be held until the local clock catches up.
pub fn is_held(op_wall_ms: u64, now_ms: u64) -> bool {
    op_wall_ms > now_ms.saturating_add(MAX_DRIFT_MS)
}
