//! Items on the unlocked vault: list, search, read, add, edit, delete, restore, and the files of document items.
//!
//! What crosses to the window is what a screen shows. Secret values stay here until the person asks: a reveal
//! sends one value for a few seconds, a copy goes to the clipboard without passing through the window at all
//! (ADR-0020). Search runs on the core's index, which never holds secret values.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use figlock_core::{
    Index, ItemId, ItemType, ItemView, MAX_FIELDS_PER_ITEM, MAX_FILES_PER_ITEM, is_secret_field,
    max_value_bytes, validate_field,
};
use figlock_session::session::{VAULT_NAME, id_hex, now_ms};
use figlock_session::{Fail, clipboard, docfile};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{CmdError, CmdRes, Desk, Inner};

/// The most items one search or list returns.
const LIST_LIMIT: usize = 10_000;

fn locked() -> CmdError {
    CmdError::new("locked", "The vault is locked. Unlock it to continue.")
}

fn not_found() -> CmdError {
    CmdError::new("not_found", "That item is no longer in the vault.")
}

/// One row of a list. Nothing secret: it is built from the same fields the search index reads.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub subtitle: String,
    pub updated_ms: u64,
}

/// What the item page shows without a reveal: every field that is not secret, and which secrets exist.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    pub id: String,
    pub kind: &'static str,
    pub deleted: bool,
    pub updated_ms: u64,
    /// Non-secret fields by schema id (`title`, `username`, `url/<id>`, `tags/<tag>`, ...), files excluded.
    pub fields: BTreeMap<String, String>,
    /// Ids of the secret fields the item holds; their values are not here.
    pub secrets: Vec<String>,
    pub files: Vec<FileInfo>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: usize,
}

/// An item for the editor: like the page, no secret value. A secret field is only said to exist.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ItemForEdit {
    pub id: String,
    pub kind: &'static str,
    pub fields: BTreeMap<String, String>,
    /// Ids of the secret fields the item holds; their values are not here.
    pub secrets: Vec<String>,
    pub files: Vec<FileInfo>,
}

/// What the editor sends back on Save: the non-secret fields the item should have, the secret values the person
/// typed, and file changes. A secret field the item holds and the draft does not name stays as it is.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ItemDraft {
    pub fields: BTreeMap<String, String>,
    /// Ids of secret fields to remove.
    #[serde(default)]
    pub clear: Vec<String>,
    /// Staged files to attach.
    #[serde(default)]
    pub add_files: Vec<String>,
    /// Ids of attached files to drop.
    #[serde(default)]
    pub remove_files: Vec<String>,
}

/// A file picked for a document item; the contents wait here until Save.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StagedFile {
    pub token: String,
    pub name: String,
    pub size: usize,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    pub files: Vec<StagedFile>,
    /// A file that was refused, with why, in the order it was given.
    pub refused: Vec<Refused>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Refused {
    pub name: String,
    pub message: String,
}

pub(super) struct StagedData {
    file: docfile::Loaded,
}

/// What a copy left on the clipboard, until it is cleared.
pub(super) struct Copied {
    value: Zeroizing<String>,
    clear_at: Instant,
}

impl Copied {
    /// Take the value off the clipboard, unless something else has been copied since.
    pub(super) fn clear(self) {
        let _ = clipboard::clear_if_unchanged(&self.value);
    }
}

/// The list rows and the search index, built once per change to the vault.
pub(super) struct Cache {
    live: Vec<ItemSummary>,
    deleted: Vec<ItemSummary>,
    index: Index,
}

pub(super) fn parse_id(id: &str) -> CmdRes<ItemId> {
    let bytes = hex::decode(id).map_err(|_| not_found())?;
    bytes.try_into().map_err(|_| not_found())
}

fn kind_of(t: ItemType) -> &'static str {
    t.as_str()
}

fn parse_kind(kind: &str) -> CmdRes<ItemType> {
    ItemType::parse(kind).map_err(|_| CmdError::new("failed", "That item type is not supported."))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The ids and values of fields `<prefix><suffix>` as a sorted list.
fn suffixed<'a>(item: &'a ItemView, prefix: &str) -> Vec<&'a str> {
    item.fields
        .iter()
        .filter(|(f, _)| f.starts_with(prefix))
        .map(|(_, v)| v.as_str())
        .collect()
}

fn subtitle_of(item: &ItemView) -> String {
    let field = |f: &str| item.fields.get(f).map_or("", |v| v.as_str());
    match item.item_type {
        ItemType::Login => field("username").to_owned(),
        ItemType::Token => {
            let provider = if field("provider").is_empty() {
                "API token"
            } else {
                field("provider")
            };
            match suffixed(item, "allowed_hosts/").first() {
                Some(host) => format!("{provider} - {host}"),
                None => provider.to_owned(),
            }
        }
        ItemType::Note => "Secure note".to_owned(),
        ItemType::EnvBundle => plural(
            item.fields.keys().filter(|f| f.starts_with("env/")).count(),
            "variable",
            "variables",
        ),
        ItemType::SshKey => "SSH key".to_owned(),
        ItemType::Document => plural(files_of(item).len(), "file", "files"),
    }
}

fn summary_of(item: &ItemView) -> ItemSummary {
    ItemSummary {
        id: id_hex(&item.id),
        kind: kind_of(item.item_type),
        title: item
            .fields
            .get("title")
            .map_or_else(|| "(untitled)".to_owned(), |t| t.to_string()),
        subtitle: subtitle_of(item),
        updated_ms: item.updated.wall,
    }
}

fn files_of(item: &ItemView) -> Vec<FileInfo> {
    docfile::list(item)
        .into_iter()
        .map(|(id, name, mime)| {
            let size = item
                .fields
                .get(&format!("file/{id}/data"))
                .map_or(0, |d| decoded_len(d));
            FileInfo {
                id,
                name,
                mime,
                size,
            }
        })
        .collect()
}

/// The size of the file a base64 value holds, without decoding it.
fn decoded_len(b64: &str) -> usize {
    let pad = b64.bytes().rev().take_while(|b| *b == b'=').count();
    (b64.len() / 4 * 3).saturating_sub(pad)
}

fn is_file_field(f: &str) -> bool {
    f.starts_with("file/")
}

/// The non-secret fields of an item with their values, and the ids of its secret fields. Files are left out.
fn split_fields(item: &ItemView) -> (BTreeMap<String, String>, Vec<String>) {
    let mut fields = BTreeMap::new();
    let mut secrets = Vec::new();
    for (f, value) in &item.fields {
        if is_file_field(f) {
            continue;
        }
        if is_secret_field(f) {
            secrets.push(f.clone());
        } else {
            fields.insert(f.clone(), value.to_string());
        }
    }
    (fields, secrets)
}

impl Desk {
    fn with_session<T>(&self, f: impl FnOnce(&mut Inner) -> CmdRes<T>) -> CmdRes<T> {
        let mut inner = self.inner();
        if inner.session.is_none() {
            return Err(locked());
        }
        f(&mut inner)
    }

    /// Rebuild the list rows and search index from the vault when something changed.
    fn cache(inner: &mut Inner) -> CmdRes<&Cache> {
        if inner.cache.is_none() {
            let session = inner.session.as_mut().ok_or_else(locked)?;
            let views = session.items(true);
            let mut live = Vec::new();
            let mut deleted = Vec::new();
            for v in &views {
                if v.deleted {
                    deleted.push(summary_of(v));
                } else {
                    live.push(summary_of(v));
                }
            }
            deleted.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.id.cmp(&b.id)));
            inner.cache = Some(Cache {
                live,
                deleted,
                index: Index::build(views.iter()),
            });
        }
        inner.cache.as_ref().ok_or_else(locked)
    }

    /// Items matching `query`, best first. An empty query lists every item, most recently changed first.
    pub fn list_items(&self, query: &str) -> CmdRes<Vec<ItemSummary>> {
        self.with_session(|inner| {
            let cache = Self::cache(inner)?;
            let hits = cache.index.search(query, now_ms(), LIST_LIMIT);
            let by_id: BTreeMap<&str, &ItemSummary> =
                cache.live.iter().map(|s| (s.id.as_str(), s)).collect();
            Ok(hits
                .iter()
                .filter_map(|h| by_id.get(id_hex(&h.id).as_str()).map(|s| (*s).clone()))
                .collect())
        })
    }

    /// Deleted items, most recently deleted first, for the Recently deleted list.
    pub fn list_deleted(&self) -> CmdRes<Vec<ItemSummary>> {
        self.with_session(|inner| Ok(Self::cache(inner)?.deleted.clone()))
    }

    fn view(inner: &mut Inner, id: &str) -> CmdRes<ItemView> {
        let id = parse_id(id)?;
        let session = inner.session.as_mut().ok_or_else(locked)?;
        session.vault.item(&id).map_err(|_| not_found())
    }

    /// An item for its page: no secret value in it.
    pub fn item_detail(&self, id: &str) -> CmdRes<ItemDetail> {
        self.with_session(|inner| {
            let v = Self::view(inner, id)?;
            let (fields, secrets) = split_fields(&v);
            Ok(ItemDetail {
                id: id_hex(&v.id),
                kind: kind_of(v.item_type),
                deleted: v.deleted,
                updated_ms: v.updated.wall,
                fields,
                secrets,
                files: files_of(&v),
            })
        })
    }

    /// One secret value, because the person asked to see it. The window keeps it for a few seconds.
    pub fn reveal_field(&self, id: &str, field: &str) -> CmdRes<String> {
        self.with_session(|inner| {
            let v = Self::view(inner, id)?;
            v.fields
                .get(field)
                .filter(|_| !is_file_field(field))
                .map(|value| value.to_string())
                .ok_or_else(not_found)
        })
    }

    /// Put a field on the clipboard, and clear it again after the delay. The value never goes to the window.
    /// Returns the seconds until it is cleared.
    pub fn copy_field(&self, id: &str, field: &str) -> CmdRes<u64> {
        self.with_session(|inner| {
            let v = Self::view(inner, id)?;
            let value = v
                .fields
                .get(field)
                .filter(|_| !is_file_field(field))
                .ok_or_else(not_found)?;
            clipboard::copy(value).map_err(CmdError::from)?;
            inner.copied = Some(Copied {
                value: Zeroizing::new(value.to_string()),
                clear_at: Instant::now() + self.clipboard_clear,
            });
            Ok(self.clipboard_clear.as_secs())
        })
    }

    /// Copies a value the window just generated, with the same timed clear as a stored field. Resolves the
    /// seconds until it is cleared.
    pub fn copy_text(&self, value: &str) -> CmdRes<u64> {
        self.with_session(|inner| {
            clipboard::copy(value).map_err(CmdError::from)?;
            inner.copied = Some(Copied {
                value: Zeroizing::new(value.to_string()),
                clear_at: Instant::now() + self.clipboard_clear,
            });
            Ok(self.clipboard_clear.as_secs())
        })
    }

    /// Clear what a copy left on the clipboard once its time is up, or at once when `now`. Whatever else was
    /// copied since is left alone.
    pub(super) fn clear_clipboard(&self, now: bool) {
        let due = {
            let mut inner = self.inner();
            match &inner.copied {
                Some(c) if now || Instant::now() >= c.clear_at => inner.copied.take(),
                _ => None,
            }
        };
        if let Some(c) = due {
            let _ = clipboard::clear_if_unchanged(&c.value);
        }
    }

    // ---- save ----

    /// Reject a draft the schema would refuse, before anything is changed.
    fn check_draft(
        t: ItemType,
        draft: &ItemDraft,
        kept_files: usize,
        staged: usize,
        retained: usize,
    ) -> CmdRes<Vec<(String, String)>> {
        let mut out = Vec::new();
        if draft
            .clear
            .iter()
            .any(|f| !is_secret_field(f) || is_file_field(f))
        {
            return Err(CmdError::new(
                "failed",
                "This item has a field its type does not allow.",
            ));
        }
        for (f, value) in &draft.fields {
            if value.is_empty() {
                continue;
            }
            if is_file_field(f) || validate_field(t, f).is_err() {
                return Err(CmdError::new(
                    "failed",
                    "This item has a field its type does not allow.",
                ));
            }
            if value.len() > max_value_bytes(f) {
                return Err(CmdError::new(
                    "field_too_long",
                    format!(
                        "A value is too long to store ({} bytes at most).",
                        max_value_bytes(f)
                    ),
                ));
            }
            out.push((f.clone(), value.clone()));
        }
        if !out
            .iter()
            .any(|(f, v)| f == "title" && !v.trim().is_empty())
        {
            return Err(CmdError::new(
                "title_required",
                "Add a title so you can find this item later.",
            ));
        }
        if t == ItemType::Document {
            if kept_files + staged > MAX_FILES_PER_ITEM {
                return Err(CmdError::new(
                    "too_many_files",
                    format!("A document holds up to {MAX_FILES_PER_ITEM} files."),
                ));
            }
            if kept_files + staged == 0 {
                return Err(CmdError::new("no_files", "Add at least one file."));
            }
        } else if staged > 0 {
            return Err(CmdError::new("failed", "Only documents hold files."));
        }
        if out.len() + retained + 3 * (kept_files + staged) > MAX_FIELDS_PER_ITEM {
            return Err(CmdError::new(
                "failed",
                "This item has too many fields. Remove some and try again.",
            ));
        }
        Ok(out)
    }

    /// Take the staged files a save names, refusing a token that is not staged.
    fn take_staged(inner: &mut Inner, tokens: &[String]) -> CmdRes<Vec<docfile::Loaded>> {
        let unique: BTreeSet<&String> = tokens.iter().collect();
        if unique.len() != tokens.len() || tokens.iter().any(|t| !inner.staged.contains_key(t)) {
            return Err(CmdError::new(
                "failed",
                "A chosen file is no longer available. Choose it again.",
            ));
        }
        Ok(tokens
            .iter()
            .filter_map(|t| inner.staged.remove(t))
            .map(|s| s.file)
            .collect())
    }

    pub fn add_item(&self, kind: &str, draft: ItemDraft) -> CmdRes<String> {
        let t = parse_kind(kind)?;
        self.with_session(|inner| {
            let staged_n = draft.add_files.len();
            let mut fields: Vec<(String, Zeroizing<String>)> =
                Self::check_draft(t, &draft, 0, staged_n, 0)?
                    .into_iter()
                    .map(|(f, v)| (f, Zeroizing::new(v)))
                    .collect();
            for file in Self::take_staged(inner, &draft.add_files)? {
                fields.extend(docfile::fields_of(file)?);
            }
            let refs: Vec<(&str, &str)> = fields
                .iter()
                .map(|(f, v)| (f.as_str(), v.as_str()))
                .collect();
            let id = Self::save(inner, |s| {
                s.vault
                    .add_item(t, VAULT_NAME, &refs, now_ms())
                    .map_err(Fail::from)
            })?;
            inner.staged.clear();
            Ok(id_hex(&id))
        })
    }

    /// The item as it is now, without secret values, so the editor can show and change the rest.
    pub fn item_for_edit(&self, id: &str) -> CmdRes<ItemForEdit> {
        self.with_session(|inner| {
            let v = Self::view(inner, id)?;
            if v.deleted {
                return Err(CmdError::new(
                    "failed",
                    "Restore this item before you edit it.",
                ));
            }
            let (fields, secrets) = split_fields(&v);
            Ok(ItemForEdit {
                id: id_hex(&v.id),
                kind: kind_of(v.item_type),
                fields,
                secrets,
                files: files_of(&v),
            })
        })
    }

    pub fn update_item(&self, id: &str, draft: ItemDraft) -> CmdRes<()> {
        self.with_session(|inner| {
            let current = Self::view(inner, id)?;
            if current.deleted {
                return Err(CmdError::new(
                    "failed",
                    "Restore this item before you edit it.",
                ));
            }
            let t = current.item_type;
            let attached: BTreeSet<String> = files_of(&current).into_iter().map(|f| f.id).collect();
            if draft.remove_files.iter().any(|f| !attached.contains(f)) {
                return Err(not_found());
            }
            let kept = attached.len() - draft.remove_files.len();
            let drafted: BTreeSet<&str> = draft
                .fields
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(f, _)| f.as_str())
                .collect();
            let cleared: BTreeSet<&str> = draft.clear.iter().map(String::as_str).collect();
            let retained = current
                .fields
                .keys()
                .filter(|f| {
                    !is_file_field(f)
                        && is_secret_field(f)
                        && !drafted.contains(f.as_str())
                        && !cleared.contains(f.as_str())
                })
                .count();
            let wanted = Self::check_draft(t, &draft, kept, draft.add_files.len(), retained)?;
            let mut new_files = Vec::new();
            for file in Self::take_staged(inner, &draft.add_files)? {
                new_files.extend(docfile::fields_of(file)?);
            }
            let mut ops: Vec<(String, Option<Zeroizing<String>>)> = Vec::new();
            for (f, value) in &wanted {
                if current.fields.get(f).map(|v| v.as_str()) != Some(value.as_str()) {
                    ops.push((f.clone(), Some(Zeroizing::new(value.clone()))));
                }
            }
            for f in current.fields.keys() {
                if !is_file_field(f)
                    && !drafted.contains(f.as_str())
                    && (!is_secret_field(f) || cleared.contains(f.as_str()))
                {
                    ops.push((f.clone(), None));
                }
            }
            for fid in &draft.remove_files {
                for leaf in ["name", "mime", "data"] {
                    let f = format!("file/{fid}/{leaf}");
                    if current.fields.contains_key(&f) {
                        ops.push((f, None));
                    }
                }
            }
            ops.extend(new_files.into_iter().map(|(f, v)| (f, Some(v))));
            Self::apply(inner, &current.id, ops)
        })
    }

    /// Write the field changes of one item and commit them together.
    pub(super) fn apply(
        inner: &mut Inner,
        item: &ItemId,
        ops: Vec<(String, Option<Zeroizing<String>>)>,
    ) -> CmdRes<()> {
        let now = now_ms();
        Self::save(inner, |s| {
            for (f, value) in &ops {
                s.vault
                    .set_field(item, f, value.as_ref().map(|v| v.as_str()), now)
                    .map_err(Fail::from)?;
            }
            Ok(())
        })?;
        inner.staged.clear();
        Ok(())
    }

    /// Change the vault and commit, as one step. If either part fails, the changes that were not written are
    /// discarded and the session stays open, so the person can fix the problem (a full disk, a Drive error that
    /// goes away) and save again without a half-applied change riding along.
    pub(super) fn save<T>(
        inner: &mut Inner,
        change: impl FnOnce(&mut figlock_session::Session) -> Result<T, Fail>,
    ) -> CmdRes<T> {
        let session = inner.session.as_mut().ok_or_else(locked)?;
        match change(&mut *session).and_then(|v| session.commit().map(|()| v)) {
            Ok(v) => {
                inner.cache = None;
                Ok(v)
            }
            Err(_) if session.is_uncertain() => {
                inner.drop_session();
                Err(CmdError::new(
                    "save_uncertain",
                    "The last save may or may not have been written, so the vault was locked. Unlock it again to see what was saved.",
                ))
            }
            Err(_) => {
                session.discard_pending();
                inner.cache = None;
                Err(CmdError::new(
                    "save_failed",
                    "Could not save. Nothing was changed; try again.",
                ))
            }
        }
    }

    pub fn delete_item(&self, id: &str) -> CmdRes<()> {
        self.change(id, |s, item, now| {
            s.vault.delete_item(item, now).map_err(Fail::from)
        })
    }

    pub fn restore_item(&self, id: &str) -> CmdRes<()> {
        self.change(id, |s, item, now| {
            s.vault.restore_item(item, now).map_err(Fail::from)
        })
    }

    fn change(
        &self,
        id: &str,
        f: impl FnOnce(&mut figlock_session::Session, &ItemId, u64) -> Result<(), Fail>,
    ) -> CmdRes<()> {
        let item = parse_id(id)?;
        self.with_session(|inner| {
            inner
                .session
                .as_mut()
                .ok_or_else(locked)?
                .vault
                .item(&item)
                .map_err(|_| not_found())?;
            Self::save(inner, |s| f(s, &item, now_ms()))
        })
    }

    // ---- files ----

    /// Read the files at `paths` for a document item and hold them until Save. A file that cannot be used is
    /// reported and does not stop the others.
    pub fn stage_files(&self, paths: &[String]) -> CmdRes<Staged> {
        self.with_session(|inner| {
            let mut files = Vec::new();
            let mut refused = Vec::new();
            for path in paths {
                match docfile::load(std::path::Path::new(path)) {
                    Ok(file) => {
                        let mut raw = [0u8; 8];
                        getrandom::fill(&mut raw)
                            .map_err(|_| CmdError::new("failed", "No randomness is available."))?;
                        let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
                        files.push(StagedFile {
                            token: token.clone(),
                            name: file.name.clone(),
                            size: file.size,
                        });
                        inner.staged.insert(token, StagedData { file });
                    }
                    Err(f) => refused.push(Refused {
                        name: std::path::Path::new(path)
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                        message: f.0,
                    }),
                }
            }
            Ok(Staged { files, refused })
        })
    }

    /// Forget a chosen file the person removed before saving.
    pub fn unstage_file(&self, token: &str) {
        self.inner().staged.remove(token);
    }

    /// Forget every chosen file, when the editor closes.
    pub fn clear_staged(&self) {
        self.inner().staged.clear();
    }

    /// A file of an item, to be written where the person chooses.
    pub fn file_bytes(&self, id: &str, file_id: &str) -> CmdRes<(String, Zeroizing<Vec<u8>>)> {
        self.with_session(|inner| {
            let v = Self::view(inner, id)?;
            let name = files_of(&v)
                .into_iter()
                .find(|f| f.id == file_id)
                .map(|f| f.name)
                .ok_or_else(not_found)?;
            let bytes = docfile::read(&v, file_id)?;
            Ok((name, bytes))
        })
    }
}
