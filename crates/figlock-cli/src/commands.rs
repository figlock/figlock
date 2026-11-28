//! One function per subcommand.

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use figlock_core::generate;
use figlock_core::{Index, ItemType, ItemView, KdfParams, Vault, is_secret_field, validate_field};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::render::{clean, item_json, raw_or_clean, summary_json, summary_line};
use crate::{
    AddArgs, CopyArgs, EditArgs, ExportArgs, FindArgs, GenerateArgs, GetArgs, InitArgs, KdfProfile,
    SeparatorArg, prompt, quick,
};
use figlock_session::config::{self, Config, Place, Remote};
use figlock_session::error::{Fail, Res};
use figlock_session::session;
use figlock_session::session::{Session, id_hex, now_ms, title_of};
use figlock_session::{clipboard, docfile};

pub(crate) use figlock_session::session::VAULT_NAME;

pub(crate) fn print_json(v: &Value) -> Res<()> {
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
    // Checked before anything is made: iCloud Drive must be on, and a `FigLock` folder that is still filling
    // with another device's vault must not get a second one.
    let mut remotes = a.remotes.resolve(false)?;
    crate::remotes::refuse_partial_icloud_folder(&remotes)?;
    if a.remotes.icloud {
        remotes = a.remotes.resolve(true)?;
    }
    let mut place = Place {
        local: match &a.store {
            Some(path) => path.clone(),
            None => config::default_local_dir()?,
        },
        remotes,
    };
    // Only a definite "not there" lets init continue: an unreachable or signed-out store must not look like
    // an empty one, or a second vault could be created next to the first. Nothing is created before this.
    if figlock_session::store::vault_exists(&place)? {
        return Err(Fail::new(
            "a vault already exists there; run `figlock join <code>` to add this device, or `figlock open` to open it with your Secret Key",
        ));
    }
    std::fs::create_dir_all(&place.local)?;
    place.local = std::fs::canonicalize(&place.local)?;
    let store = figlock_session::store::open(&place)?;
    let params = match a.kdf {
        KdfProfile::Default => KdfParams::DEFAULT,
        KdfProfile::Floor => KdfParams::FLOOR,
    };
    let choice = match (a.quick_unlock, a.no_quick_unlock) {
        (true, _) => quick::InitChoice::Yes,
        (_, true) => quick::InitChoice::No,
        _ => quick::InitChoice::Ask,
    };
    let password = prompt::new_password()?;
    let (mut vault, created) = Vault::create(&password, &a.device_name, params, now_ms())?;
    // The Recovery Key is made and checked before anything is written, so a vault is never left behind a code
    // nobody saved (`pairing-recovery.md` 7).
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let want_recovery = match (a.recovery_key, a.no_recovery_key) {
        (true, _) => true,
        (_, true) => false,
        _ if interactive => prompt::confirm_default_yes(
            "Make a Recovery Key? It is the only way back in if you forget your password. [Y/n] ",
        )?,
        _ => false,
    };
    let mut vault_json = created.vault_json.clone();
    let mut recovery_code = None;
    if want_recovery {
        let key = figlock_core::RecoveryKey::random()
            .map_err(|_| Fail::new("could not get randomness"))?;
        let code = key.format();
        if interactive {
            println!("Recovery Key: {}", code.as_str());
            crate::recovery::confirm_saved(&code, "Recovery Key", false)?;
        }
        vault_json = vault
            .set_recovery_with(&key, &password, &created.secret_key)?
            .vault_json;
        recovery_code = Some(code);
    }
    let cfg = Config {
        place,
        device_name: a.device_name,
        autolock_secs: None,
    };
    config::save(&cfg, vault.device_keys())?;
    let device_path = format!("devices/{}.json", hex::encode(vault.device_id()));
    let mut created_paths: Vec<&str> = Vec::new();
    let written = store.put_new("vault.json", &vault_json).and_then(|()| {
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
            "recovery_key": recovery_code.as_ref().map(|c| c.as_str()),
        }))?;
    } else {
        println!("Vault created in {}", cfg.place.local.display());
        for r in &cfg.place.remotes {
            match r {
                Remote::Drive => println!("Synced to your Google Drive (hidden app folder)"),
                Remote::Folder(p) if figlock_session::icloud::is_icloud_path(p) => println!(
                    "Synced to iCloud Drive ({}). Leave that folder alone; with Optimize Mac Storage on, iCloud \
                     may keep files only in the cloud and FigLock downloads them when needed.",
                    p.display()
                ),
                Remote::Folder(p) => println!("Synced to the folder {}", p.display()),
            }
        }
        println!("Secret Key: {}", secret_key.as_str());
        if let Some(code) = recovery_code.as_ref().filter(|_| !interactive) {
            println!("Recovery Key: {}", code.as_str());
        }
    }
    if recovery_code.is_some() {
        eprintln!(
            "Write the Recovery Key down too, apart from your Secret Key and password. Anyone with it can open the vault; with it `figlock forgot` resets a forgotten password."
        );
    } else {
        eprintln!(
            "There is no Recovery Key, so a forgotten password cannot be recovered. `figlock recovery-key enable` makes one."
        );
    }
    eprintln!(
        "Write the Secret Key down now. It is shown once and cannot be reset. This device keeps it for you; \
         you need the written copy to recover the vault or to add a device when no other one is at hand."
    );
    keep_secret_key(&created);
    if cfg.place.syncs()
        && let Err(e) = figlock_session::sync::run(&cfg, false)
    {
        eprintln!(
            "The vault is saved on this device, but the first sync failed: {e}. Run `figlock sync` to retry."
        );
    }
    let unlock_key = || {
        Vault::unlock_key(&created.vault_json, &password, &created.secret_key)
            .map_err(|_| Fail::new("could not derive the unlock key"))
    };
    match session::vault_id_of(&created.vault_json) {
        Ok(vault) => offer_quick_unlock(choice, json, &vault, unlock_key),
        Err(e) => eprintln!("warning: quick unlock was not turned on: {e}"),
    }
    Ok(())
}

/// Keep the Secret Key on this device (ADR-0027). The vault and the key are already out, so a failure only warns.
fn keep_secret_key(created: &figlock_core::Created) {
    let keys = match figlock_session::secret_key_store::system() {
        Ok(k) if k.available() => k,
        // Nothing to keep it in on this machine (FIG-105): the key is typed when needed, as before.
        _ => return,
    };
    let kept = session::vault_id_of(&created.vault_json).and_then(|vault| {
        // The key goes where no prompt is needed to unlock, whether or not quick unlock is turned on (ADR-0037).
        keys.put(&vault, &created.secret_key, 1)
    });
    if let Err(e) = kept {
        eprintln!("warning: the Secret Key was not kept on this device: {e}");
    }
}

/// The last step of `init`, `join` and `open`: offer quick unlock (`docs/adr/0025`, amendment FIG-79). The vault
/// and the Secret Key are already out, so nothing here may fail the command. `auk` derives the unlock key and
/// only runs once enrolment is going ahead.
pub(crate) fn offer_quick_unlock(
    choice: quick::InitChoice,
    json: bool,
    vault: &str,
    auk: impl FnOnce() -> Res<figlock_core::Key>,
) {
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let attempt = || -> Res<quick::InitOutcome> {
        let quick = quick::system()?;
        let ask = || {
            prompt::confirm_default_yes(
                "Unlock this device with Touch ID instead of your password?",
            )
        };
        Ok(quick.offer_at_init(choice, interactive, ask, vault, auk, now_ms() / 1000))
    };
    let retry = "run `figlock quick-unlock enable` to try again";
    match attempt() {
        Ok(quick::InitOutcome::On) => {
            eprintln!("Quick unlock is on. `figlock quick-unlock disable` turns it off.");
        }
        Ok(quick::InitOutcome::Unavailable) if interactive || choice == quick::InitChoice::Yes => {
            eprintln!("Quick unlock is not available on this machine.");
        }
        Ok(quick::InitOutcome::Failed(e)) => {
            eprintln!("warning: quick unlock was not turned on: {e}; {retry}")
        }
        Err(e) => eprintln!("warning: quick unlock was not turned on: {e}; {retry}"),
        Ok(_) => {}
    }
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

fn file_fields(t: ItemType, files: &[PathBuf]) -> Res<Vec<(String, Zeroizing<String>)>> {
    if !files.is_empty() && t != ItemType::Document {
        return Err(Fail::new("--file only works on doc items"));
    }
    let mut out = Vec::new();
    for path in files {
        out.extend(docfile::fields_for(path)?);
    }
    Ok(out)
}

pub fn add(a: AddArgs, json: bool) -> Res<()> {
    let mut fields = collect_fields(a.item_type, a.set, a.secret)?;
    fields.extend(file_fields(a.item_type, &a.files)?);
    if fields.iter().any(|(n, _)| n == "title") {
        return Err(Fail::new("use --title for the title"));
    }
    fields.push(("title".to_owned(), Zeroizing::new(a.title)));
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
    s.ensure_writable()?;
    let refs: Vec<(&str, &str)> = fields
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let id = s.vault.add_item(a.item_type, VAULT_NAME, &refs, now_ms())?;
    s.commit()?;
    let pushed = crate::sync_cmd::push_after_write();
    if json {
        print_json(&pushed.attach(json!({ "id": id_hex(&id) })))
    } else {
        println!("{}", id_hex(&id));
        pushed.announce();
        Ok(())
    }
}

/// Whole days from `then_ms` to `now`.
fn days_since(then_ms: u64, now: u64) -> u64 {
    now.saturating_sub(then_ms) / 86_400_000
}

/// "today", "1 day ago" or "N days ago" for something that happened at `then_ms`.
fn age_text(then_ms: u64, now: u64) -> String {
    match days_since(then_ms, now) {
        0 => "today".to_owned(),
        1 => "1 day ago".to_owned(),
        n => format!("{n} days ago"),
    }
}

/// `edit --restore-all`: bring every trashed item back, after saying how many and asking.
fn restore_all(yes: bool, json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
    s.ensure_writable()?;
    let ids: Vec<_> = s
        .items(true)
        .into_iter()
        .filter(|i| i.deleted)
        .map(|i| i.id)
        .collect();
    let count = ids.len();
    if count == 0 {
        if json {
            return print_json(&json!({ "restored": 0 }));
        }
        eprintln!("nothing in the trash");
        return Ok(());
    }
    let noun = if count == 1 { "item" } else { "items" };
    if !yes {
        if !std::io::stdin().is_terminal() {
            return Err(Fail::new(format!(
                "this is not a terminal, so nothing was restored; pass --yes to restore all {count} {noun}"
            )));
        }
        if !prompt::confirm_default_no(&format!("Restore {count} {noun} from the trash?"))? {
            return Err(Fail::new("nothing was restored"));
        }
    }
    let now = now_ms();
    for id in &ids {
        s.vault.restore_item(id, now)?;
    }
    s.commit()?;
    let pushed = crate::sync_cmd::push_after_write();
    if json {
        print_json(&pushed.attach(json!({ "restored": count })))
    } else {
        println!("Restored {count} {noun}.");
        pushed.announce();
        Ok(())
    }
}

pub fn edit(a: EditArgs, json: bool) -> Res<()> {
    if a.restore_all {
        return restore_all(a.yes, json);
    }
    let Some(reference) = a.item.as_deref() else {
        return Err(Fail::new("give the item to edit"));
    };
    if a.set.is_empty()
        && a.secret.is_empty()
        && a.unset.is_empty()
        && a.files.is_empty()
        && a.remove_files.is_empty()
        && !a.delete
        && !a.restore
    {
        return Err(Fail::new(
            "nothing to do: give --set, --secret, --unset, --file, --remove-file, --delete or --restore",
        ));
    }
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
    s.ensure_writable()?;
    let item = s.resolve(reference, a.restore)?;
    let id = item.id;
    let mut fields = collect_fields(item.item_type, a.set, a.secret)?;
    fields.extend(file_fields(item.item_type, &a.files)?);
    let mut unset = a.unset;
    for name in &a.remove_files {
        let fid = docfile::find(&item, name)?;
        unset.extend(["name", "mime", "data"].map(|leaf| format!("file/{fid}/{leaf}")));
    }
    for name in &unset {
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
    for name in &unset {
        // A file's mime or data may be absent; removing a field the item never had is not an error.
        if item.fields.contains_key(name) {
            s.vault.set_field(&id, name, None, now)?;
        }
    }
    if a.delete {
        s.vault.delete_item(&id, now)?;
    }
    if a.restore {
        s.vault.restore_item(&id, now)?;
    }
    s.commit()?;
    let pushed = crate::sync_cmd::push_after_write();
    if json {
        print_json(&pushed.attach(json!({ "id": id_hex(&id) })))
    } else {
        println!("{}", id_hex(&id));
        pushed.announce();
        Ok(())
    }
}

pub fn find(a: FindArgs, json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
    let mut items = s.items(a.deleted);
    if a.deleted {
        items.retain(|i| i.deleted);
    }
    if let Some(t) = a.item_type {
        items.retain(|i| i.item_type == t);
    }
    let index = if a.deleted {
        Index::build_trashed(items.iter())
    } else {
        Index::build(items.iter())
    };
    let now = now_ms();
    let hits = index.search(&a.query.join(" "), now, a.limit);
    let found: Vec<&ItemView> = hits
        .iter()
        .filter_map(|h| items.iter().find(|i| i.id == h.id))
        .collect();
    if json {
        print_json(&Value::Array(
            found
                .iter()
                .map(|i| {
                    let mut v = summary_json(i);
                    if a.deleted {
                        v["deleted"] = json!(true);
                        v["deleted_ms"] = json!(i.updated.wall);
                        v["deleted_at"] = json!(session::civil_date(i.updated.wall));
                        v["deleted_days_ago"] = json!(days_since(i.updated.wall, now));
                    }
                    v
                })
                .collect(),
        ))
    } else {
        for i in &found {
            if a.deleted {
                println!(
                    "{}  deleted {} ({})",
                    summary_line(i),
                    session::civil_date(i.updated.wall),
                    age_text(i.updated.wall, now)
                );
            } else {
                println!("{}", summary_line(i));
            }
        }
        if found.is_empty() {
            eprintln!(
                "{}",
                if a.deleted {
                    "nothing in the trash matches"
                } else {
                    "no matches"
                }
            );
        }
        Ok(())
    }
}

pub fn get(a: GetArgs, json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
    let item = s.resolve(&a.item, false)?;
    if let (Some(name), Some(out)) = (&a.file, &a.out) {
        let id = docfile::find(&item, name)?;
        let bytes = docfile::read(&item, &id)?;
        docfile::write_out(&bytes, out, a.force)?;
        if out != "-" {
            eprintln!("Wrote {} bytes to {out} (owner-only).", bytes.len());
        }
        return Ok(());
    }
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
        if name.starts_with("file/") && name.ends_with("/data") {
            println!(
                "{name}: {} bytes (save it with --file NAME --out PATH)",
                docfile::read(&item, &name["file/".len()..name.len() - "/data".len()])
                    .map_or(0, |b| b.len())
            );
        } else if is_secret_field(name) && !a.reveal {
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
        ItemType::Document => Some("password"),
        ItemType::EnvBundle => None,
    }
}

pub fn copy(a: CopyArgs, json: bool) -> Res<()> {
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
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
        match clipboard::clear_if_unchanged(&value)? {
            clipboard::Cleared::Yes => {}
            clipboard::Cleared::Changed => {
                eprintln!("The clipboard changed in the meantime; left alone.");
            }
            clipboard::Cleared::Unreadable => {
                eprintln!("Cannot read the clipboard, so it was not cleared.");
            }
        }
    }
    if json {
        print_json(&json!({ "copied": field, "clear_after": a.clear_after }))?;
    }
    Ok(())
}

pub fn generate(a: GenerateArgs, json: bool) -> Res<()> {
    let (key, secret) = if a.passphrase {
        let (min, max) = (generate::MIN_WORDS, generate::MAX_WORDS);
        if !(min..=max).contains(&a.words) {
            return Err(Fail::new(format!("words must be between {min} and {max}")));
        }
        let separator = match a.separator {
            SeparatorArg::Hyphen => generate::Separator::Hyphen,
            SeparatorArg::Dot => generate::Separator::Dot,
            SeparatorArg::Space => generate::Separator::Space,
            SeparatorArg::None => generate::Separator::None,
        };
        let p = generate::passphrase(&generate::PassphraseOptions {
            words: a.words,
            separator,
            capitalize: a.capitalize,
            ..Default::default()
        })
        .map_err(|_| Fail::new("could not generate a passphrase"))?;
        ("passphrase", p)
    } else {
        let (min, max) = (generate::MIN_LEN, generate::MAX_LEN);
        if !(min..=max).contains(&a.length) {
            return Err(Fail::new(format!("length must be between {min} and {max}")));
        }
        if a.no_lower && a.no_upper && a.no_digits && a.no_symbols {
            return Err(Fail::new(
                "at least one character type must stay on (lowercase, uppercase, digits or symbols)",
            ));
        }
        let p = generate::password(&generate::PasswordOptions {
            length: a.length,
            lower: !a.no_lower,
            upper: !a.no_upper,
            digits: !a.no_digits,
            symbols: !a.no_symbols,
            ..Default::default()
        })
        .map_err(|_| Fail::new("could not generate a password"))?;
        ("password", p)
    };
    if json {
        print_json(&json!({ key: secret.as_str() }))
    } else {
        println!("{}", secret.as_str());
        Ok(())
    }
}

pub fn export(a: ExportArgs) -> Res<()> {
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
    let items = s.items(a.include_deleted);
    if a.include_secrets {
        eprintln!("warning: this output contains secrets in plain text");
    }
    let text = figlock_core::export_json(&items, a.include_secrets)
        .ok_or_else(|| Fail::new("cannot format output"))?;
    println!("{}", text.as_str());
    Ok(())
}
