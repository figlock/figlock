//! Pure decisions of the sync reader (`sync.md` 8 and 9): no I/O, so every rule is testable.

use crate::hlc::Stamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// Storage showed less than we have already accepted.
    Rollback,
    /// A device's log was forked or its hash chain broke.
    Fork,
}

/// What to do with one device's log folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogPlan {
    /// Highest sequence that may be applied now (strictly in order).
    pub applied: u64,
    /// The missing sequence number the reader is waiting for, if a later file exists.
    pub waiting_for: Option<u64>,
    /// Stop applying this device until the user decides.
    pub stopped: bool,
    pub warning: Option<Warning>,
}

/// Plan one device's files.
///
/// - `applied`: highest sequence already applied.
/// - `snapshot_hw`: the device's high-water mark in the newest valid snapshot (0 if none); files up to it
///   are covered and are not a gap.
/// - `present`: sequence numbers listed in storage.
/// - `forked`: sequences for which two different valid files exist.
/// - `prev_mismatch`: a sequence whose `prev` hash does not match the previously accepted file.
pub fn plan_log(
    applied: u64,
    snapshot_hw: u64,
    present: &[u64],
    forked: &[u64],
    prev_mismatch: Option<u64>,
) -> LogPlan {
    let mut next = applied.max(snapshot_hw);
    let have = |s: u64| present.contains(&s);
    loop {
        let Some(s) = next.checked_add(1) else {
            return LogPlan {
                applied: next,
                waiting_for: None,
                stopped: true,
                warning: Some(Warning::Fork),
            };
        };
        if forked.contains(&s) || prev_mismatch == Some(s) {
            return LogPlan {
                applied: next,
                waiting_for: None,
                stopped: true,
                warning: Some(Warning::Fork),
            };
        }
        if have(s) {
            next = s;
        } else {
            break;
        }
    }
    let waiting_for = present.iter().any(|&p| p > next + 1).then_some(next + 1);
    LogPlan {
        applied: next,
        waiting_for,
        stopped: false,
        warning: None,
    }
}

/// Storage listing shows fewer files than we already applied: warn, and never lower local state.
pub fn check_listing(applied: u64, listing_max: u64) -> Option<Warning> {
    (listing_max < applied).then_some(Warning::Rollback)
}

/// A `vault.json` with a lower `revision` than one already accepted is ignored.
pub fn accept_vault_json(accepted_revision: u64, seen_revision: u64) -> (u64, Option<Warning>) {
    if seen_revision < accepted_revision {
        (accepted_revision, Some(Warning::Rollback))
    } else {
        (seen_revision, None)
    }
}

/// Whether to take a revoked device's file: only if it was stamped before `revoked_at` and written in an
/// epoch before the rotation (`sync.md` 9).
pub fn accept_from_revoked(
    revoked_at: Stamp,
    op_stamp: Stamp,
    file_epoch: u32,
    rotation_epoch: u32,
) -> bool {
    op_stamp < revoked_at && file_epoch < rotation_epoch
}
