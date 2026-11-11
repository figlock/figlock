//! Output helpers. Everything stored in a vault is untrusted text once it has synced from elsewhere, so
//! terminal control characters are replaced before display.

use std::borrow::Cow;
use std::io::IsTerminal;

use figlock_core::{ItemView, is_secret_field};
use serde_json::{Map, Value, json};

use figlock_session::session::{id_hex, title_of};

pub use figlock_session::clean;

/// Text for stdout where a script may read it raw: cleaned only when a person is looking at a terminal.
pub fn raw_or_clean(s: &str) -> Cow<'_, str> {
    if std::io::stdout().is_terminal() {
        clean(s)
    } else {
        Cow::Borrowed(s)
    }
}

/// One line for `find`: short id, type, title, username and hosts. Never a secret value.
pub fn summary_line(item: &ItemView) -> String {
    let mut line = format!(
        "{}  {:<6}  {}",
        &id_hex(&item.id)[..8],
        item.item_type.as_str(),
        clean(title_of(item))
    );
    if let Some(u) = item.fields.get("username") {
        line.push_str(&format!("  {}", clean(u)));
    }
    for (name, v) in &item.fields {
        if name.starts_with("url/") || name.starts_with("allowed_hosts/") {
            line.push_str(&format!("  {}", clean(v)));
        }
    }
    line
}

pub fn summary_json(item: &ItemView) -> Value {
    let non_secret = |prefix: &str| -> Vec<&str> {
        item.fields
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(_, v)| v.as_str())
            .collect()
    };
    json!({
        "id": id_hex(&item.id),
        "type": item.item_type.as_str(),
        "title": title_of(item),
        "username": item.fields.get("username").map(|u| u.as_str()),
        "urls": non_secret("url/"),
        "allowed_hosts": non_secret("allowed_hosts/"),
        "updated_ms": item.updated.wall,
    })
}

/// Full item as JSON. Secret values are included only with `reveal`.
pub fn item_json(item: &ItemView, reveal: bool) -> Value {
    let mut fields = Map::new();
    let mut secret_fields = Vec::new();
    for (name, v) in &item.fields {
        if is_secret_field(name) {
            secret_fields.push(name.clone());
            if !reveal {
                continue;
            }
        }
        fields.insert(name.clone(), Value::String(v.to_string()));
    }
    json!({
        "id": id_hex(&item.id),
        "type": item.item_type.as_str(),
        "vault": item.vault,
        "deleted": item.deleted,
        "updated_ms": item.updated.wall,
        "fields": fields,
        "secret_fields": secret_fields,
    })
}
