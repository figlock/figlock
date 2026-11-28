//! The vault export document. One implementation behind `figlock export`, the desktop app and the iOS
//! app, so all three produce the same JSON (`format: figlock-export-1`).

use serde_json::{Map, Value, json};
use zeroize::Zeroizing;

use crate::{ItemView, is_secret_field};

/// The `format` value of an export document.
pub const EXPORT_FORMAT: &str = "figlock-export-1";

/// One item as JSON. Secret values are included only with `reveal`; the names of the secret fields are
/// always listed so a reader can see what was left out.
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
        "id": hex::encode(item.id),
        "type": item.item_type.as_str(),
        "vault": item.vault,
        "deleted": item.deleted,
        "updated_ms": item.updated.wall,
        "fields": fields,
        "secret_fields": secret_fields,
    })
}

/// The export document as a JSON value.
pub fn export_value(items: &[ItemView], include_secrets: bool) -> Value {
    json!({
        "format": EXPORT_FORMAT,
        "secrets_included": include_secrets,
        "items": items.iter().map(|i| item_json(i, include_secrets)).collect::<Vec<_>>(),
    })
}

/// The export document as pretty-printed JSON text. The text zeroizes on drop, because with
/// `include_secrets` it holds every secret in plain text. `None` only if serialization fails.
pub fn export_json(items: &[ItemView], include_secrets: bool) -> Option<Zeroizing<String>> {
    serde_json::to_string_pretty(&export_value(items, include_secrets))
        .ok()
        .map(Zeroizing::new)
}
