//! Item types and their field schemas (`docs/prd.md` 7.1, `sync.md` 6).
//!
//! A field id is an ASCII name. Multi-valued data uses one field per value with a unique suffix, so
//! concurrent additions on two devices merge instead of colliding (`url/<id>`, `env/<NAME>`, ...).

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemType {
    Login,
    Token,
    Note,
    EnvBundle,
    SshKey,
}

impl ItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemType::Login => "login",
            ItemType::Token => "token",
            ItemType::Note => "note",
            ItemType::EnvBundle => "env",
            ItemType::SshKey => "ssh",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "login" => ItemType::Login,
            "token" => ItemType::Token,
            "note" => ItemType::Note,
            "env" => ItemType::EnvBundle,
            "ssh" => ItemType::SshKey,
            _ => return Err(Error),
        })
    }
}

/// Reserved fields written by the vault itself, never by callers.
pub(crate) const FIELD_TYPE: &str = "_type";
pub(crate) const FIELD_VAULT: &str = "_vault";
pub(crate) const FIELD_IK: &str = "_ik";
pub(crate) const FIELD_DELETED: &str = "_deleted";

const MAX_FIELD_ID: usize = 64;
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;

fn token(s: &str, max: usize, ok: impl Fn(u8) -> bool) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(ok)
}

fn suffix_id(s: &str) -> bool {
    token(s, 16, |b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn env_name(s: &str) -> bool {
    s.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_uppercase() || b == b'_')
        && token(s, 64, |b| {
            b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'
        })
}

fn tag(s: &str) -> bool {
    token(s, 40, |b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
    })
}

fn common(field: &str) -> bool {
    if matches!(field, "title" | "notes") {
        return true;
    }
    if let Some(t) = field.strip_prefix("tags/") {
        return tag(t);
    }
    if let Some(rest) = field.strip_prefix("custom/") {
        return rest
            .rsplit_once('/')
            .is_some_and(|(id, leaf)| suffix_id(id) && matches!(leaf, "name" | "value"));
    }
    false
}

/// Whether `field` is allowed on an item of type `t`. Reserved `_` fields are not.
pub fn validate_field(t: ItemType, field: &str) -> Result<()> {
    if field.len() > MAX_FIELD_ID || !field.is_ascii() || field.starts_with('_') {
        return Err(Error);
    }
    let ok = common(field)
        || match t {
            ItemType::Login => {
                matches!(field, "username" | "password" | "totp")
                    || field.strip_prefix("url/").is_some_and(suffix_id)
            }
            ItemType::Token => {
                matches!(
                    field,
                    "provider" | "token" | "env_var" | "scopes" | "expires_at"
                ) || field.strip_prefix("allowed_hosts/").is_some_and(suffix_id)
            }
            ItemType::Note => field == "body",
            ItemType::EnvBundle => field.strip_prefix("env/").is_some_and(env_name),
            ItemType::SshKey => matches!(field, "private_key" | "public_key" | "passphrase"),
        };
    if ok { Ok(()) } else { Err(Error) }
}

/// Secret fields are never indexed for search or shown without an explicit reveal.
pub fn is_secret_field(field: &str) -> bool {
    matches!(
        field,
        "password" | "totp" | "token" | "private_key" | "passphrase"
    ) || field.starts_with("env/")
        || (field.starts_with("custom/") && field.ends_with("/value"))
}
