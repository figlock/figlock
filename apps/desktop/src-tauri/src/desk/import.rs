//! Importing another manager's export (FIG-285). Parsing is `figlock_core::parse_import`, the one `figlock import`
//! uses. The window never sees a value from the file: choosing a file parses it here and keeps the items in
//! memory (wiped on drop), the window gets only counts and titles to preview, and a second step writes them.
//! Nothing is written until that second step, and locking the vault drops what was held.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use figlock_core::{ImportSummary, MAX_INPUT_BYTES, Report, Source, parse_import};
use figlock_session::session::{VAULT_NAME, now_ms};
use serde::Serialize;
use zeroize::Zeroizing;

use super::{CmdError, CmdRes, Desk};

/// Items per commit, so one op file stays well under the store's blob limit (as `figlock import`).
const COMMIT_EVERY: usize = 100;
/// Titles the preview lists; the counts cover the rest.
const PREVIEW_TITLES: usize = 50;

/// The sources the Mac app imports. Bitwarden and 1Password stay on the command line for now.
const SOURCES: [Source; 3] = [Source::Chrome, Source::Apple, Source::Notes];

/// A parsed file waiting for the person to confirm.
pub(super) struct Pending {
    report: Report,
}

/// What the preview shows. No secret value.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub logins: usize,
    pub notes: usize,
    pub tokens: usize,
    pub tokens_detected: usize,
    pub skipped: usize,
    pub titles: Vec<String>,
    /// Items beyond the listed titles.
    pub more: usize,
}

fn source_of(name: &str) -> CmdRes<Source> {
    Source::parse(name)
        .filter(|s| SOURCES.contains(s))
        .ok_or_else(|| CmdError::new("bad_source", "Choose where the file came from."))
}

fn read_export(path: &Path) -> CmdRes<Zeroizing<Vec<u8>>> {
    let unreadable = || CmdError::new("unreadable", "That file could not be read.");
    let file = File::open(path).map_err(|_| unreadable())?;
    if !file.metadata().map_err(|_| unreadable())?.is_file() {
        return Err(unreadable());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    Ok(bytes)
}

fn preview_of(s: &ImportSummary, total: usize) -> ImportPreview {
    ImportPreview {
        logins: s.logins,
        notes: s.notes,
        tokens: s.tokens,
        tokens_detected: s.tokens_detected,
        skipped: s.skipped_empty + s.skipped_too_large + s.skipped_unsupported,
        more: total.saturating_sub(s.titles.len()),
        titles: s.titles.clone(),
    }
}

impl Desk {
    /// Parse the file at `path` as an export of `source` and hold the items for `import_commit`. Replaces
    /// anything held before. Fails, holding nothing, when the file is not that kind of export or has nothing
    /// to import. Needs an unlocked vault only so the held items cannot outlive a lock.
    pub fn import_prepare(&self, source: &str, path: &Path) -> CmdRes<ImportPreview> {
        let source = source_of(source)?;
        self.with_session(|inner| {
            inner.importing = None;
            let bytes = read_export(path)?;
            let report = parse_import(source, &bytes)
                .map_err(|e| CmdError::new("bad_file", e.to_string()))?;
            drop(bytes);
            if report.items.is_empty() {
                return Err(CmdError::new(
                    "nothing_to_import",
                    "That file has nothing to import.",
                ));
            }
            let preview = preview_of(&report.summary(PREVIEW_TITLES), report.items.len());
            inner.importing = Some(Pending { report });
            Ok(preview)
        })
    }

    /// Write the held items to the vault, a hundred per commit. Returns how many were added. If a commit fails
    /// the earlier ones stay (as on the command line) and the error says how many.
    pub fn import_commit(&self) -> CmdRes<usize> {
        self.with_session(|inner| {
            let pending = inner.importing.take().ok_or_else(|| {
                CmdError::new("nothing_to_import", "Choose a file to import first.")
            })?;
            let now = now_ms();
            let mut done = 0;
            for chunk in pending.report.items.chunks(COMMIT_EVERY) {
                Self::save(inner, |s| {
                    for d in chunk {
                        let fields: Vec<(&str, &str)> = d
                            .fields
                            .iter()
                            .map(|(n, v)| (n.as_str(), v.as_str()))
                            .collect();
                        s.vault.add_item(d.item_type, VAULT_NAME, &fields, now)?;
                    }
                    Ok(())
                })
                .map_err(|e| {
                    if done == 0 {
                        e
                    } else {
                        CmdError::new(
                            e.code,
                            format!(
                                "The import stopped after {done} items were added; those are in the vault, the rest are not."
                            ),
                        )
                    }
                })?;
                done += chunk.len();
            }
            Ok(done)
        })
    }

    /// Forget the held items (the person cancelled or went back).
    pub fn import_discard(&self) {
        self.inner().importing = None;
    }
}
