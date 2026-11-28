//! Importing another manager's export on the phone (FIG-286). The app reads the file the person picked and passes
//! the bytes to [`prepare_import`], which parses them with the same core importers `figlock import` uses and holds
//! the items behind an [`ImportPlan`] handle. The preview carries counts and titles only. Nothing reaches the vault
//! until the app calls [`ImportPlan::add_next`], a chunk at a time, committing between chunks.

use std::sync::{Arc, Mutex};

use figlock_core::{ImportError, Report, Source, parse_import};

use crate::error::{FigLockError, Result, id_hex};
use crate::session::VaultSession;

/// Titles the preview lists; the counts cover the rest.
const PREVIEW_TITLES: usize = 50;

/// Why a file could not be prepared. Fixed cases, never text from the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Error)]
pub enum ImportFailure {
    /// The source name is not one the phone imports (`chrome`, `apple`, `notes`).
    UnknownSource,
    /// The header row is not the one that source writes.
    WrongFormat,
    TooLarge,
    NotUtf8,
    Malformed,
    TooManyRecords,
    /// The file is fine but holds nothing to import.
    Empty,
}

impl std::fmt::Display for ImportFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ImportFailure::UnknownSource => "unknown source",
            ImportFailure::WrongFormat => "the file does not look like an export from that source",
            ImportFailure::TooLarge => "the file is too large to import",
            ImportFailure::NotUtf8 => "the file is not valid UTF-8 text",
            ImportFailure::Malformed => "the file is not well-formed CSV",
            ImportFailure::TooManyRecords => "the file has too many records to import",
            ImportFailure::Empty => "the file has nothing to import",
        })
    }
}

impl std::error::Error for ImportFailure {}

impl From<ImportError> for ImportFailure {
    fn from(e: ImportError) -> Self {
        match e {
            ImportError::TooLarge => ImportFailure::TooLarge,
            ImportError::NotUtf8 => ImportFailure::NotUtf8,
            ImportError::Malformed => ImportFailure::Malformed,
            ImportError::WrongFormat => ImportFailure::WrongFormat,
            ImportError::TooManyRecords => ImportFailure::TooManyRecords,
        }
    }
}

/// What the preview shows. No secret value.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ImportPreview {
    pub logins: u32,
    pub notes: u32,
    pub tokens: u32,
    /// Tokens found inside notes and filed as token items.
    pub tokens_detected: u32,
    /// Records skipped (empty, too large or of an unsupported type).
    pub skipped: u32,
    pub titles: Vec<String>,
    /// Items beyond the listed titles.
    pub more: u32,
}

struct Pending {
    report: Report,
    next: usize,
}

/// A parsed file waiting to be added. Values stay in Rust and are wiped when the handle is dropped.
#[derive(uniffi::Object)]
pub struct ImportPlan {
    pending: Mutex<Pending>,
    preview: ImportPreview,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Parse `bytes` as an export of `source` (`chrome`, `apple` or `notes`). Writes nothing and needs no vault.
#[uniffi::export]
pub fn prepare_import(
    source: String,
    bytes: Vec<u8>,
) -> core::result::Result<Arc<ImportPlan>, ImportFailure> {
    let source = Source::parse(&source)
        .filter(|s| matches!(s, Source::Chrome | Source::Apple | Source::Notes))
        .ok_or(ImportFailure::UnknownSource)?;
    let bytes = zeroize::Zeroizing::new(bytes);
    let report = parse_import(source, &bytes)?;
    drop(bytes);
    if report.items.is_empty() {
        return Err(ImportFailure::Empty);
    }
    let s = report.summary(PREVIEW_TITLES);
    let preview = ImportPreview {
        logins: count(s.logins),
        notes: count(s.notes),
        tokens: count(s.tokens),
        tokens_detected: count(s.tokens_detected),
        skipped: count(s.skipped_empty + s.skipped_too_large + s.skipped_unsupported),
        more: count(report.items.len().saturating_sub(s.titles.len())),
        titles: s.titles,
    };
    Ok(Arc::new(ImportPlan {
        pending: Mutex::new(Pending { report, next: 0 }),
        preview,
    }))
}

#[uniffi::export]
impl ImportPlan {
    pub fn preview(&self) -> ImportPreview {
        self.preview.clone()
    }

    /// Items not yet added.
    pub fn remaining(&self) -> Result<u32> {
        let p = self.pending.lock().map_err(|_| FigLockError::Failed)?;
        Ok(count(p.report.items.len() - p.next))
    }

    /// Add up to `max` more items to `session` (not yet committed: the caller commits and stores, then calls
    /// again). Returns the new item ids; empty when everything is added. If the vault refuses an item the
    /// ones added in this call are still pending, so the caller must discard them.
    pub fn add_next(&self, session: &VaultSession, max: u32, now_ms: u64) -> Result<Vec<String>> {
        let mut p = self.pending.lock().map_err(|_| FigLockError::Failed)?;
        let end = (p.next + max as usize).min(p.report.items.len());
        let mut ids = Vec::new();
        session.mutate(|v| {
            for d in &p.report.items[p.next..end] {
                let fields: Vec<(&str, &str)> = d
                    .fields
                    .iter()
                    .map(|(n, val)| (n.as_str(), val.as_str()))
                    .collect();
                ids.push(id_hex(&v.add_item(
                    d.item_type,
                    "personal",
                    &fields,
                    now_ms,
                )?));
            }
            Ok(())
        })?;
        p.next = end;
        Ok(ids)
    }
}
