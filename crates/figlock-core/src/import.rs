//! Import of other password managers' CSV exports and of a raw notes dump (`docs/prd.md` 7.1).
//!
//! Pure and in memory: bytes in, [`Draft`] items out, nothing touches storage. The caller adds the drafts to
//! a vault. Every field name in a draft already passes [`validate_field`] and every value fits
//! [`MAX_VALUE_BYTES`], so adding a draft cannot fail on schema grounds.
//!
//! The input is hostile: it is size-capped, parsed with fixed limits, and errors are fixed strings that never
//! contain any part of it. A record that cannot be imported is skipped and counted, not echoed.
//!
//! A notes dump is split into notes at lines made only of `---` or `===`. Well-known API tokens found in a
//! note (`ghp_`, `sk-ant-`, `AKIA`, ...) are filed as token items and replaced in the note body by a pointer,
//! so a secret does not stay in a plain note that anything may display.

use std::fmt;

use zeroize::Zeroizing;

use crate::{ItemType, max_value_bytes, validate_field};

/// Larger inputs are refused before any parsing.
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
/// Records (CSV rows or notes) beyond this are refused.
pub const MAX_RECORDS: usize = 50_000;
const MAX_COLUMNS: usize = 64;
const MAX_TITLE_CHARS: usize = 200;
const MAX_URLS: usize = 20;
const MAX_CUSTOM: usize = 20;
const MAX_TAGS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Chrome,
    Apple,
    Bitwarden,
    OnePassword,
    /// A plain text dump of notes.
    Notes,
}

impl Source {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "chrome" => Source::Chrome,
            "apple" => Source::Apple,
            "bitwarden" => Source::Bitwarden,
            "1password" => Source::OnePassword,
            "notes" => Source::Notes,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Source::Chrome => "chrome",
            Source::Apple => "apple",
            Source::Bitwarden => "bitwarden",
            Source::OnePassword => "1password",
            Source::Notes => "notes",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    TooLarge,
    NotUtf8,
    /// Unterminated quote or too many columns.
    Malformed,
    /// The header row is not the one this source writes.
    WrongFormat,
    TooManyRecords,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ImportError::TooLarge => "the file is too large to import (limit 16 MiB)",
            ImportError::NotUtf8 => "the file is not valid UTF-8 text",
            ImportError::Malformed => "the file is not well-formed CSV",
            ImportError::WrongFormat => "the file does not look like an export from that source",
            ImportError::TooManyRecords => "the file has too many records to import (limit 50000)",
        })
    }
}

impl std::error::Error for ImportError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// Nothing to import in the record.
    Empty,
    /// A value is larger than an item field may be.
    FieldTooLarge,
    /// A record type this import does not handle (cards, identities).
    UnsupportedType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// 1-based record number; for a CSV the header is record 1.
    pub record: usize,
    pub reason: SkipReason,
}

/// One item ready for `Vault::add_item`. Values are zeroized on drop; there is deliberately no `Debug`.
pub struct Draft {
    pub item_type: ItemType,
    pub fields: Vec<(String, Zeroizing<String>)>,
}

#[derive(Default)]
pub struct Report {
    pub items: Vec<Draft>,
    pub skipped: Vec<Skipped>,
    /// How many token items came from detection in notes.
    pub tokens_detected: usize,
}

/// Parse `bytes` as an export of `source`.
pub fn parse(source: Source, bytes: &[u8]) -> Result<Report, ImportError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ImportError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ImportError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match source {
        Source::Notes => parse_notes(text),
        _ => parse_csv_export(source, text),
    }
}

// ---- CSV ----

type Row = Vec<Zeroizing<String>>;

/// Move the finished `row` into `rows`; the header counts as a record.
fn finish_row(row: &mut Row, rows: &mut Vec<Row>) -> Result<(), ImportError> {
    if rows.len() > MAX_RECORDS {
        return Err(ImportError::TooManyRecords);
    }
    rows.push(std::mem::take(row));
    Ok(())
}

/// RFC 4180 with the leniency browsers have: a quote inside an unquoted field is literal, `\r\n`, `\n` and
/// a lone `\r` all end a record, blank lines are skipped.
fn read_csv(text: &str) -> Result<Vec<Row>, ImportError> {
    let mut rows: Vec<Row> = Vec::new();
    let mut row: Row = Vec::new();
    let mut field = Zeroizing::new(String::new());
    let mut quoted = false;
    let mut field_started = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if !field_started => {
                quoted = true;
                field_started = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                field_started = false;
                if row.len() > MAX_COLUMNS {
                    return Err(ImportError::Malformed);
                }
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if field_started || !row.is_empty() || !field.is_empty() {
                    row.push(std::mem::take(&mut field));
                    finish_row(&mut row, &mut rows)?;
                }
                field_started = false;
            }
            _ => {
                field.push(c);
                field_started = true;
            }
        }
    }
    if quoted {
        return Err(ImportError::Malformed);
    }
    if field_started || !row.is_empty() || !field.is_empty() {
        row.push(field);
        finish_row(&mut row, &mut rows)?;
    }
    Ok(rows)
}

struct Columns {
    names: Vec<String>,
}

impl Columns {
    fn new(header: &Row) -> Self {
        Columns {
            names: header
                .iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .collect(),
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }
}

/// The header columns each source must have, then the optional ones read when present.
fn required(source: Source) -> &'static [&'static str] {
    match source {
        Source::Chrome => &["name", "url", "username", "password"],
        Source::Apple => &["title", "url", "username", "password"],
        Source::Bitwarden => &[
            "type",
            "name",
            "login_uri",
            "login_username",
            "login_password",
        ],
        Source::OnePassword => &["title", "url", "username", "password"],
        Source::Notes => &[],
    }
}

/// Column names for title, url, username, password, totp, notes, tags, in that order.
fn names(source: Source) -> [&'static str; 7] {
    match source {
        Source::Chrome => ["name", "url", "username", "password", "", "note", ""],
        Source::Apple => [
            "title", "url", "username", "password", "otpauth", "notes", "",
        ],
        Source::Bitwarden => [
            "name",
            "login_uri",
            "login_username",
            "login_password",
            "login_totp",
            "notes",
            "folder",
        ],
        Source::OnePassword => [
            "title", "url", "username", "password", "otpauth", "notes", "tags",
        ],
        Source::Notes => [""; 7],
    }
}

/// One CSV record with the source's column lookups applied.
struct Record<'a> {
    row: &'a Row,
    cols: [Option<usize>; 7],
}

impl Record<'_> {
    fn get(&self, i: usize) -> &str {
        self.cols[i]
            .and_then(|c| self.row.get(c))
            .map_or("", |v| v.as_str())
    }
}

fn parse_csv_export(source: Source, text: &str) -> Result<Report, ImportError> {
    let rows = read_csv(text)?;
    let header = rows.first().ok_or(ImportError::WrongFormat)?;
    let columns = Columns::new(header);
    if required(source).iter().any(|r| columns.find(r).is_none()) {
        return Err(ImportError::WrongFormat);
    }
    let n = names(source);
    let cols = n.map(|name| {
        if name.is_empty() {
            None
        } else {
            columns.find(name)
        }
    });
    let type_col = (source == Source::Bitwarden)
        .then(|| columns.find("type"))
        .flatten();
    let fields_col = (source == Source::Bitwarden)
        .then(|| columns.find("fields"))
        .flatten();
    let mut report = Report::default();
    for (i, row) in rows.iter().enumerate().skip(1) {
        let record = i + 1;
        let rec = Record { row, cols };
        let kind = type_col
            .and_then(|c| row.get(c))
            .map_or("login", |t| t.trim());
        let built = match kind.to_ascii_lowercase().as_str() {
            "login" => login(source, &rec, fields_col.and_then(|c| row.get(c))),
            "note" | "securenote" => note(&rec),
            _ => Err(SkipReason::UnsupportedType),
        };
        match built {
            Ok(d) => report.items.push(d),
            Err(reason) => report.skipped.push(Skipped { record, reason }),
        }
    }
    Ok(report)
}

type Fields = Vec<(String, Zeroizing<String>)>;

fn push(fields: &mut Fields, name: &str, value: &str) {
    if !value.is_empty() {
        fields.push((name.to_owned(), Zeroizing::new(value.to_owned())));
    }
}

/// Fail the record when any value is larger than a field may be.
fn check_sizes(fields: &Fields) -> Result<(), SkipReason> {
    if fields.iter().any(|(n, v)| v.len() > max_value_bytes(n)) {
        Err(SkipReason::FieldTooLarge)
    } else {
        Ok(())
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Host of a URL for a fallback title: no scheme, credentials, port, path or leading `www.`.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    (!host.is_empty()).then(|| host.to_owned())
}

fn login(
    source: Source,
    rec: &Record<'_>,
    custom: Option<&Zeroizing<String>>,
) -> Result<Draft, SkipReason> {
    let (title, url, username, password, totp, notes, tags) = (
        rec.get(0).trim(),
        rec.get(1).trim(),
        rec.get(2).trim(),
        rec.get(3),
        rec.get(4).trim(),
        rec.get(5),
        rec.get(6),
    );
    if [title, url, username, password, totp, notes]
        .iter()
        .all(|v| v.is_empty())
    {
        return Err(SkipReason::Empty);
    }
    let urls: Vec<&str> = if source == Source::Bitwarden {
        url.split(['\n', '\r']).map(str::trim).collect()
    } else {
        vec![url]
    };
    let urls: Vec<&str> = urls
        .into_iter()
        .filter(|u| !u.is_empty())
        .take(MAX_URLS)
        .collect();
    let title = if title.is_empty() {
        urls.first()
            .and_then(|u| host_of(u))
            .unwrap_or_else(|| "(untitled)".to_owned())
    } else {
        truncate_chars(title, MAX_TITLE_CHARS)
    };
    let mut fields: Fields = Vec::new();
    push(&mut fields, "title", &title);
    push(&mut fields, "username", username);
    push(&mut fields, "password", password);
    push(&mut fields, "totp", totp);
    for (n, u) in urls.iter().enumerate() {
        push(&mut fields, &format!("url/{}", n + 1), u);
    }
    push(&mut fields, "notes", notes);
    match source {
        Source::OnePassword => add_tags(&mut fields, tags.split(',')),
        Source::Bitwarden => add_tags(&mut fields, std::iter::once(tags)),
        _ => {}
    }
    if let Some(custom) = custom {
        add_custom(&mut fields, custom);
    }
    check_sizes(&fields)?;
    debug_assert!(
        fields
            .iter()
            .all(|(n, _)| validate_field(ItemType::Login, n).is_ok())
    );
    Ok(Draft {
        item_type: ItemType::Login,
        fields,
    })
}

fn note(rec: &Record<'_>) -> Result<Draft, SkipReason> {
    let (title, body) = (rec.get(0).trim(), rec.get(5));
    if title.is_empty() && body.is_empty() {
        return Err(SkipReason::Empty);
    }
    let mut fields: Fields = Vec::new();
    let title = if title.is_empty() {
        "(untitled)".to_owned()
    } else {
        truncate_chars(title, MAX_TITLE_CHARS)
    };
    push(&mut fields, "title", &title);
    push(&mut fields, "body", body);
    check_sizes(&fields)?;
    Ok(Draft {
        item_type: ItemType::Note,
        fields,
    })
}

/// A tag field id is lowercase letters, digits, `-` and `_`, up to 40; anything else is made to fit or
/// dropped.
fn add_tags<'a>(fields: &mut Fields, tags: impl Iterator<Item = &'a str>) {
    let mut added = 0;
    for t in tags {
        let id: String = t
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .take(40)
            .collect();
        let id = id.trim_matches('-').to_owned();
        let name = format!("tags/{id}");
        if id.is_empty()
            || added >= MAX_TAGS
            || fields.iter().any(|(n, _)| *n == name)
            || validate_field(ItemType::Login, &name).is_err()
        {
            continue;
        }
        // A tag carries no value of its own; `1` marks it present.
        fields.push((name, Zeroizing::new("1".to_owned())));
        added += 1;
    }
}

/// Bitwarden's `fields` column holds `name: value` lines.
fn add_custom(fields: &mut Fields, custom: &str) {
    let mut n = 0;
    for line in custom.lines() {
        let Some((name, value)) = line.split_once(": ") else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() || value.is_empty() || n >= MAX_CUSTOM {
            continue;
        }
        n += 1;
        push(fields, &format!("custom/{n}/name"), name);
        push(fields, &format!("custom/{n}/value"), value);
    }
}

// ---- notes and token detection ----

/// A token found in text, as byte offsets into it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detected {
    pub provider: &'static str,
    pub label: &'static str,
    pub start: usize,
    pub end: usize,
    /// The variable it was assigned to (`GITHUB_TOKEN=...`), when the text names one.
    pub env_var: Option<String>,
}

struct Rule {
    prefix: &'static str,
    provider: &'static str,
    label: &'static str,
    charset: fn(u8) -> bool,
    /// Length bounds of what follows the prefix.
    min: usize,
    max: usize,
}

fn alnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}
fn alnum_dash(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}
fn alnum_under(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
fn upper_digit(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}
fn alnum_hyphen(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

/// More specific prefixes come before the ones they start with (`sk-ant-` before `sk-`).
const RULES: &[Rule] = &[
    Rule {
        prefix: "github_pat_",
        provider: "github",
        label: "GitHub",
        charset: alnum_under,
        min: 70,
        max: 100,
    },
    Rule {
        prefix: "ghp_",
        provider: "github",
        label: "GitHub",
        charset: alnum,
        min: 36,
        max: 255,
    },
    Rule {
        prefix: "gho_",
        provider: "github",
        label: "GitHub",
        charset: alnum,
        min: 36,
        max: 255,
    },
    Rule {
        prefix: "ghu_",
        provider: "github",
        label: "GitHub",
        charset: alnum,
        min: 36,
        max: 255,
    },
    Rule {
        prefix: "ghs_",
        provider: "github",
        label: "GitHub",
        charset: alnum,
        min: 36,
        max: 255,
    },
    Rule {
        prefix: "ghr_",
        provider: "github",
        label: "GitHub",
        charset: alnum,
        min: 36,
        max: 255,
    },
    Rule {
        prefix: "glpat-",
        provider: "gitlab",
        label: "GitLab",
        charset: alnum_dash,
        min: 20,
        max: 50,
    },
    Rule {
        prefix: "sk-ant-",
        provider: "anthropic",
        label: "Anthropic",
        charset: alnum_dash,
        min: 30,
        max: 300,
    },
    Rule {
        prefix: "sk-proj-",
        provider: "openai",
        label: "OpenAI",
        charset: alnum_dash,
        min: 40,
        max: 300,
    },
    Rule {
        prefix: "sk-",
        provider: "openai",
        label: "OpenAI",
        charset: alnum,
        min: 48,
        max: 48,
    },
    Rule {
        prefix: "AKIA",
        provider: "aws",
        label: "AWS access key",
        charset: upper_digit,
        min: 16,
        max: 16,
    },
    Rule {
        prefix: "ASIA",
        provider: "aws",
        label: "AWS access key",
        charset: upper_digit,
        min: 16,
        max: 16,
    },
    Rule {
        prefix: "xoxb-",
        provider: "slack",
        label: "Slack",
        charset: alnum_hyphen,
        min: 20,
        max: 200,
    },
    Rule {
        prefix: "xoxp-",
        provider: "slack",
        label: "Slack",
        charset: alnum_hyphen,
        min: 20,
        max: 200,
    },
    Rule {
        prefix: "xoxa-",
        provider: "slack",
        label: "Slack",
        charset: alnum_hyphen,
        min: 20,
        max: 200,
    },
    Rule {
        prefix: "xoxr-",
        provider: "slack",
        label: "Slack",
        charset: alnum_hyphen,
        min: 20,
        max: 200,
    },
    Rule {
        prefix: "xoxs-",
        provider: "slack",
        label: "Slack",
        charset: alnum_hyphen,
        min: 20,
        max: 200,
    },
    Rule {
        prefix: "sk_live_",
        provider: "stripe",
        label: "Stripe",
        charset: alnum,
        min: 24,
        max: 99,
    },
    Rule {
        prefix: "rk_live_",
        provider: "stripe",
        label: "Stripe",
        charset: alnum,
        min: 24,
        max: 99,
    },
    Rule {
        prefix: "npm_",
        provider: "npm",
        label: "npm",
        charset: alnum,
        min: 36,
        max: 36,
    },
    Rule {
        prefix: "AIza",
        provider: "google",
        label: "Google API",
        charset: alnum_dash,
        min: 35,
        max: 35,
    },
];

/// Find well-known API tokens in `text`, in order. A token must start at a word boundary and be exactly
/// the length that provider issues, so ordinary words and longer random strings are not reported.
pub fn detect_tokens(text: &str) -> Vec<Detected> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    let mut prev_end = 0;
    while i < bytes.len() {
        // Also a boundary right after another token, so two glued together are both found.
        let boundary = i == 0 || i == prev_end || !alnum_dash(bytes[i - 1]);
        let hit = boundary.then(|| match_rule(bytes, i)).flatten();
        if let Some((rule, end)) = hit {
            found.push(Detected {
                provider: rule.provider,
                label: rule.label,
                start: i,
                end,
                env_var: env_var_before(bytes, i),
            });
            i = end;
            prev_end = end;
        } else {
            i += 1;
        }
    }
    found
}

fn match_rule(bytes: &[u8], at: usize) -> Option<(&'static Rule, usize)> {
    let rest = &bytes[at..];
    for rule in RULES {
        let Some(after) = rest.strip_prefix(rule.prefix.as_bytes()) else {
            continue;
        };
        let run = after.iter().take_while(|b| (rule.charset)(**b)).count();
        if run >= rule.min && run <= rule.max {
            return Some((rule, at + rule.prefix.len() + run));
        }
    }
    None
}

/// `NAME=` or `NAME:` (optionally quoted or spaced) immediately before a token.
fn env_var_before(bytes: &[u8], start: usize) -> Option<String> {
    let mut i = start;
    if i > 0 && matches!(bytes[i - 1], b'"' | b'\'') {
        i -= 1;
    }
    while i > 0 && bytes[i - 1] == b' ' {
        i -= 1;
    }
    if i == 0 || !matches!(bytes[i - 1], b'=' | b':') {
        return None;
    }
    i -= 1;
    while i > 0 && bytes[i - 1] == b' ' {
        i -= 1;
    }
    let end = i;
    while i > 0
        && (bytes[i - 1].is_ascii_uppercase()
            || bytes[i - 1].is_ascii_digit()
            || bytes[i - 1] == b'_')
    {
        i -= 1;
    }
    let name = std::str::from_utf8(&bytes[i..end]).ok()?;
    let name = name.trim_start_matches(|c: char| c.is_ascii_digit());
    (!name.is_empty() && validate_field(ItemType::EnvBundle, &format!("env/{name}")).is_ok())
        .then(|| name.to_owned())
}

fn is_separator(line: &str) -> bool {
    let l = line.trim();
    l.len() >= 3 && (l.bytes().all(|b| b == b'-') || l.bytes().all(|b| b == b'='))
}

/// Safety net for text that is stored as plain note text: anything that still looks like a token (for
/// example a long run cut to a valid length by truncation) is masked, so a note never keeps one.
fn scrub(text: String) -> String {
    let mut text = text;
    for _ in 0..4 {
        let found = detect_tokens(&text);
        if found.is_empty() {
            return text;
        }
        text = replace_spans(&text, 0, &found, |_| "[token]".to_owned());
    }
    "[redacted]".to_owned()
}

/// `text` is `section[base..base + text.len()]`; every detected span inside it is replaced by `with(span)`.
fn replace_spans(
    text: &str,
    base: usize,
    found: &[Detected],
    with: impl Fn(&Detected) -> String,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for d in found {
        if d.start < base || d.end > base + text.len() {
            continue;
        }
        out.push_str(&text[last..d.start - base]);
        out.push_str(&with(d));
        last = d.end - base;
    }
    out.push_str(&text[last..]);
    out
}

fn parse_notes(text: &str) -> Result<Report, ImportError> {
    let mut sections: Vec<String> = vec![String::new()];
    for line in text.lines() {
        if is_separator(line) {
            sections.push(String::new());
        } else if let Some(s) = sections.last_mut() {
            s.push_str(line);
            s.push('\n');
        }
        if sections.len() > MAX_RECORDS {
            return Err(ImportError::TooManyRecords);
        }
    }
    let mut report = Report::default();
    for (i, section) in sections.iter().enumerate() {
        let section = section.trim();
        let record = i + 1;
        if section.is_empty() {
            if sections.len() > 1 {
                report.skipped.push(Skipped {
                    record,
                    reason: SkipReason::Empty,
                });
            }
            continue;
        }
        // Tokens are found once, in the whole note, so the title and the body are both redacted.
        let found = detect_tokens(section);
        let first_end = section.find('\n').unwrap_or(section.len());
        let title_line = replace_spans(&section[..first_end], 0, &found, |_| "[token]".to_owned());
        let title = title_line.trim().trim_start_matches('#').trim();
        let title = if title.is_empty() {
            "(untitled)".to_owned()
        } else {
            scrub(truncate_chars(title, MAX_TITLE_CHARS))
        };
        // The text after the first line, or the single line itself when there is nothing else.
        let after = section.get(first_end + 1..).unwrap_or("");
        let (body_base, body) = if after.trim().is_empty() {
            (0, section)
        } else {
            (
                first_end + 1 + (after.len() - after.trim_start().len()),
                after.trim(),
            )
        };
        let body_out = Zeroizing::new(scrub(replace_spans(body, body_base, &found, |d| {
            format!("[token filed as: {} token ({title})]", d.label)
        })));
        let mut tokens: Vec<(Detected, Zeroizing<String>)> = Vec::new();
        for d in &found {
            let value = Zeroizing::new(section[d.start..d.end].to_owned());
            if !tokens.iter().any(|(_, v)| **v == *value) {
                tokens.push((d.clone(), value));
            }
        }

        let mut fields: Fields = Vec::new();
        push(&mut fields, "title", &title);
        push(&mut fields, "body", &body_out);
        if check_sizes(&fields).is_err() {
            report.skipped.push(Skipped {
                record,
                reason: SkipReason::FieldTooLarge,
            });
            continue;
        }
        report.items.push(Draft {
            item_type: ItemType::Note,
            fields,
        });
        for (d, value) in tokens {
            let mut fields: Fields = Vec::new();
            push(
                &mut fields,
                "title",
                &format!("{} token ({title})", d.label),
            );
            push(&mut fields, "provider", d.provider);
            push(&mut fields, "token", &value);
            if let Some(name) = &d.env_var {
                push(&mut fields, "env_var", name);
            }
            push(
                &mut fields,
                "notes",
                &format!("Imported from the note \"{title}\""),
            );
            push(&mut fields, "tags/imported", "1");
            report.items.push(Draft {
                item_type: ItemType::Token,
                fields,
            });
            report.tokens_detected += 1;
        }
    }
    Ok(report)
}
