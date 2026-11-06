//! One function per subcommand.

use std::collections::BTreeSet;
use std::thread::sleep;
use std::time::Duration;

use figlock_core::{Index, ItemType, ItemView, KdfParams, Vault, is_secret_field, validate_field};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::config::{self, Config, StoreSpec};
use crate::error::{Fail, Res};
use crate::render::{clean, item_json, raw_or_clean, summary_json, summary_line};
use crate::session::{Session, id_hex, now_ms, title_of};
use crate::{
    AddArgs, CopyArgs, EditArgs, ExportArgs, FindArgs, GenerateArgs, GetArgs, InitArgs, KdfProfile,
    clipboard, generate, prompt,
};

pub(crate) const VAULT_NAME: &str = "personal";

fn print_json(v: &Value) -> Res<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).map_err(|_| Fail::new("cannot format output"))?
    );
    Ok(())
}

pub fn init(a: InitArgs, json: bool) -> Res<()> {
    if config::is_initialised()? {
        return Err(Fail::new("this device already has a vault set up"));
    }
    let spec = match &a.store {
        Some(path) => {
            std::fs::create_dir_all(path)?;
            StoreSpec::Folder(std::fs::canonicalize(path)?)
        }
        None => StoreSpec::Drive,
    };
    let store = crate::store::open(&spec)?;
    // Only a definite "not there" lets init continue: an unreachable or signed-out store must not look like
    // an empty one, or a second vault could be created next to the first.
    match store.get("vault.json") {
        Err(figlock_store::StoreError::NotFound) => {}
        Ok(_) => {
            return Err(Fail::new(
                "a vault already exists there; use the device pairing flow to add this device",
            ));
        }
        Err(e) => return Err(e.into()),
    }
    let params = match a.kdf {
        KdfProfile::Default => KdfParams::DEFAULT,
        KdfProfile::Floor => KdfParams::FLOOR,
    };
    let password = prompt::new_password()?;
    let (vault, created) = Vault::create(&password, &a.device_name, params, now_ms())?;
    let cfg = Config {
        store: spec,
        device_name: a.device_name,
    };
    config::save(&cfg, vault.device_keys())?;
    let device_path = format!("devices/{}.json", hex::encode(vault.device_id()));
    let mut created_paths: Vec<&str> = Vec::new();
    let written = store
        .put_new("vault.json", &created.vault_json)
        .and_then(|()| {
            created_paths.push("vault.json");
            store.put_new(&device_path, &created.device_record)
        });
    if let Err(e) = written {
        config::forget();
        for path in created_paths {
            let _ = store.delete(path);
        }
        return Err(e.into());
    }
    let secret_key = created.secret_key.format();
    if json {
        print_json(&json!({
            "vault_id": hex::encode(vault.vault_id()),
            "device_id": hex::encode(vault.device_id()),
            "secret_key": secret_key.as_str(),
        }))?;
    } else {
        match &cfg.store {
            StoreSpec::Folder(path) => println!("Vault created in {}", path.display()),
            StoreSpec::Drive => println!("Vault created in your Google Drive (hidden app folder)"),
        }
        println!("Secret Key: {}", secret_key.as_str());
    }
    eprintln!(
        "Write the Secret Key down now. It is shown once, is needed on every new device and for recovery, \
         and cannot be reset."
    );
    Ok(())
}

/// Validated `(field, value)` pairs from `--set` and `--secret`; secret values are read here.
fn collect_fields(
    t: ItemType,
    set: Vec<(String, String)>,
    secret: Vec<String>,
) -> Res<Vec<(String, Zeroizing<String>)>> {
    let mut out: Vec<(String, Zeroizing<String>)> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut claim = |name: &str| -> Res<()> {
        validate_field(t, name)
            .map_err(|_| Fail::new(format!("{name} is not a field of a {} item", t.as_str())))?;
        if !seen.insert(name.to_owned()) {
            return Err(Fail::new(format!("{name} given more than once")));
        }
        Ok(())
    };
    for (name, value) in set {
        claim(&name)?;
        if is_secret_field(&name) {
            return Err(Fail::new(format!(
                "{name} is a secret field: use --secret {name} so the value stays off the command line"
            )));
        }
        out.push((name, Zeroizing::new(value)));
    }
    for name in secret {
        claim(&name)?;
        if !is_secret_field(&name) {
            return Err(Fail::new(format!(
                "{name} is not a secret field: use --set"
            )));
        }
        let value = prompt::secret_value(&name)?;
        out.push((name, value));
    }
    Ok(out)
}

pub fn add(a: AddArgs, json: bool) -> Res<()> {
    let mut fields = collect_fields(a.item_type, a.set, a.secret)?;
    if fields.iter().any(|(n, _)| n == "title") {
        return Err(Fail::new("use --title for the title"));
    }
    fields.push(("title".to_owned(), Zeroizing::new(a.title)));
    let mut s = Session::open()?;
    let refs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let id = s.vault.add_item(a.item_type, VAULT_NAME, &refs, now_ms())?;
    s.commit()?;
    if json {
        print_json(&json!({ "id": id_hex(&id) }))
    } else {
        println!("{}", id_hex(&id));
        Ok(())
    }
}

pub fn edit(a: EditArgs, json: bool) -> Res<()> {
    if a.set.is_empty() && a.secret.is_empty() && a.unset.is_empty() && !a.delete && !a.restore {
        return Err(Fail::new(
            "nothing to do: give --set, --secret, --unset, --delete or --restore",
        ));
    }
    let mut s = Session::open()?;
    let item = s.resolve(&a.item, a.restore)?;
    let id = item.id;
    let fields = collect_fields(item.item_type, a.set, a.secret)?;
    for name in &a.unset {
        validate_field(item.item_type, name).map_err(|_| {
            Fail::new(format!(
                "{name} is not a field of a {} item",
                item.item_type.as_str()
            ))
        })?;
    }
    let now = now_ms();
    for (name, value) in &fields {
        s.vault.set_field(&id, name, Some(value), now)?;
    }
    for name in &a.unset {
        s.vault.set_field(&id, name, None, now)?;
    }
    if a.delete {
        s.vault.delete_item(&id, now)?;
    }
    if a.restore {
        s.vault.restore_item(&id, now)?;
    }
    s.commit()?;
    if json {
        print_json(&json!({ "id": id_hex(&id) }))
    } else {
        println!("{}", id_hex(&id));
        Ok(())
    }
}

pub fn find(a: FindArgs, json: bool) -> Res<()> {
    let mut s = Session::open()?;
    let mut items = s.items(false);
    if let Some(t) = a.item_type {
        items.retain(|i| i.item_type == t);
    }
    let index = Index::build(items.iter());
    let hits = index.search(&a.query.join(" "), now_ms(), a.limit);
    let found: Vec<&ItemView> = hits
        .iter()
        .filter_map(|h| items.iter().find(|i| i.id == h.id))
        .collect();
    if json {
        print_json(&Value::Array(
            found.iter().map(|i| summary_json(i)).collect(),
        ))
    } else {
        for i in &found {
            println!("{}", summary_line(i));
        }
        if found.is_empty() {
            eprintln!("no matches");
        }
        Ok(())
    }
}

pub fn get(a: GetArgs, json: bool) -> Res<()> {
    let mut s = Session::open()?;
    let item = s.resolve(&a.item, false)?;
    if let Some(field) = a.field {
        let value = item
            .fields
            .get(&field)
            .ok_or_else(|| Fail::new(format!("the item has no field {field}")))?;
        if is_secret_field(&field) && !a.reveal {
            return Err(Fail::new(format!(
                "{field} is a secret: use `figlock copy` to put it on the clipboard, or --reveal to print it"
            )));
        }
        if json {
            return print_json(&json!({ "field": field, "value": value.as_str() }));
        }
        println!("{}", raw_or_clean(value));
        return Ok(());
    }
    if json {
        return print_json(&item_json(&item, a.reveal));
    }
    println!("{}", clean(title_of(&item)));
    println!("id:   {}", id_hex(&item.id));
    println!("type: {}", item.item_type.as_str());
    for (name, value) in &item.fields {
        if name == "title" {
            continue;
        }
        if is_secret_field(name) && !a.reveal {
            println!("{name}: ******** (secret)");
        } else {
            println!("{name}: {}", clean(value));
        }
    }
    Ok(())
}

fn default_secret_field(t: ItemType) -> Option<&'static str> {
    match t {
        ItemType::Login => Some("password"),
        ItemType::Token => Some("token"),
        ItemType::Note => Some("body"),
        ItemType::SshKey => Some("private_key"),
        ItemType::EnvBundle => None,
    }
}

pub fn copy(a: CopyArgs, json: bool) -> Res<()> {
    let mut s = Session::open()?;
    let item = s.resolve(&a.item, false)?;
    let field = match a.field {
        Some(f) => f,
        None => default_secret_field(item.item_type)
            .ok_or_else(|| Fail::new("this item has no default field; give --field"))?
            .to_owned(),
    };
    let value = item
        .fields
        .get(&field)
        .ok_or_else(|| Fail::new(format!("the item has no field {field}")))?
        .clone();
    clipboard::copy(&value)?;
    eprintln!("Copied to the clipboard: {}", clean(title_of(&item)));
    if a.clear_after > 0 {
        eprintln!(
            "The clipboard is cleared in {}s (Ctrl-C to keep it).",
            a.clear_after
        );
        sleep(Duration::from_secs(a.clear_after));
        match clipboard::paste() {
            Some(now) if *now == *value || now.trim_end() == value.as_str() => clipboard::copy("")?,
            Some(_) => eprintln!("The clipboard changed in the meantime; left alone."),
            None => eprintln!("Cannot read the clipboard, so it was not cleared."),
        }
    }
    if json {
        print_json(&json!({ "copied": field, "clear_after": a.clear_after }))?;
    }
    Ok(())
}

pub fn generate(a: GenerateArgs, json: bool) -> Res<()> {
    let p = generate::password(a.length, !a.no_symbols)?;
    if json {
        print_json(&json!({ "password": p.as_str() }))
    } else {
        println!("{}", p.as_str());
        Ok(())
    }
}

pub fn export(a: ExportArgs) -> Res<()> {
    let mut s = Session::open()?;
    let items = s.items(a.include_deleted);
    let list: Vec<Value> = items
        .iter()
        .map(|i| item_json(i, a.include_secrets))
        .collect();
    if a.include_secrets {
        eprintln!("warning: this output contains secrets in plain text");
    }
    print_json(&json!({
        "format": "figlock-export-1",
        "secrets_included": a.include_secrets,
        "items": list,
    }))
}
