//! Storage path validation. Paths are lower case ASCII, `/` separated, with no traversal.

use crate::{Result, StoreError};

const MAX_PATH: usize = 512;
const MAX_SEGMENT: usize = 200;
const MAX_DEPTH: usize = 4;

fn valid_segment(seg: &str) -> bool {
    !seg.is_empty()
        && seg.len() <= MAX_SEGMENT
        // A leading dot is reserved for temporary files and cloud placeholders, which clients never address.
        && !seg.starts_with('.')
        && seg.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

/// Validate a full blob path such as `log/ab12.../00000001.op`.
pub fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > MAX_PATH {
        return Err(StoreError::InvalidPath);
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > MAX_DEPTH || !segments.iter().all(|s| valid_segment(s)) {
        return Err(StoreError::InvalidPath);
    }
    Ok(())
}

/// A `list` prefix: empty, a directory (`log/`, `log/ab12/`) or a name prefix (`snap/ab12-`). The final
/// segment may be empty (trailing `/`) or partial.
pub(crate) fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.len() > MAX_PATH {
        return Err(StoreError::InvalidPath);
    }
    if prefix.is_empty() {
        return Ok(());
    }
    let segments: Vec<&str> = prefix.split('/').collect();
    let (last, dirs) = segments.split_last().ok_or(StoreError::InvalidPath)?;
    let last_ok = last.is_empty() || valid_segment(last);
    if segments.len() > MAX_DEPTH + 1 || !dirs.iter().all(|s| valid_segment(s)) || !last_ok {
        return Err(StoreError::InvalidPath);
    }
    Ok(())
}
