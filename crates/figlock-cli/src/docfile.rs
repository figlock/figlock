//! Files on document items (`docs/adr/0024-document-items.md`): reading one from disk into the three
//! `file/<id>/...` fields, and writing one back out with owner-only permissions.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use figlock_core::{ItemView, MAX_FILE_BYTES, decode_file, encode_file};
use zeroize::Zeroizing;

use figlock_session::config::create_private_file;
use figlock_session::error::{Fail, Res};

const MAX_NAME_CHARS: usize = 200;

/// The three fields for one file, in the order they are written. `id` is a fresh random suffix.
pub fn fields_for(path: &Path) -> Res<Vec<(String, Zeroizing<String>)>> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME_CHARS)
        .filter(|n| !n.chars().any(char::is_control))
        .ok_or_else(|| Fail::new("the file name must be 1 to 200 printable characters"))?
        .to_owned();
    let meta =
        fs::metadata(path).map_err(|e| Fail::new(format!("cannot read {name}: {}", e.kind())))?;
    if !meta.is_file() {
        return Err(Fail::new(format!("{name} is not a regular file")));
    }
    if meta.len() == 0 || meta.len() > MAX_FILE_BYTES as u64 {
        return Err(Fail::new(format!(
            "{name} must be between 1 byte and {} KiB",
            MAX_FILE_BYTES / 1024
        )));
    }
    // Read through a limit so a file that grows after the check cannot exceed the cap.
    let mut bytes = Zeroizing::new(Vec::with_capacity(meta.len() as usize));
    fs::File::open(path)?
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let data =
        encode_file(&bytes).map_err(|_| Fail::new(format!("{name} is too large or empty")))?;
    let id = random_id()?;
    Ok(vec![
        (format!("file/{id}/name"), Zeroizing::new(name.clone())),
        (
            format!("file/{id}/mime"),
            Zeroizing::new(mime_for(&name).to_owned()),
        ),
        (format!("file/{id}/data"), data),
    ])
}

fn random_id() -> Res<String> {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).map_err(|_| Fail::new("no randomness available"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("p12" | "pfx") => "application/x-pkcs12",
        Some("p8" | "key" | "pem" | "crt" | "cer") => "application/x-pem-file",
        Some("json") => "application/json",
        Some("txt" | "env") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// The files of an item as (id, name, mime), by id.
pub fn list(item: &ItemView) -> Vec<(String, String, String)> {
    let mut by_id: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    for (field, value) in &item.fields {
        let Some(rest) = field.strip_prefix("file/") else {
            continue;
        };
        let Some((id, leaf)) = rest.split_once('/') else {
            continue;
        };
        let e = by_id.entry(id).or_default();
        match leaf {
            "name" => e.0 = value.as_str(),
            "mime" => e.1 = value.as_str(),
            _ => {}
        }
    }
    by_id
        .into_iter()
        .map(|(id, (n, m))| (id.to_owned(), n.to_owned(), m.to_owned()))
        .collect()
}

/// The id of the file called `name` (or with that id) on `item`.
pub fn find(item: &ItemView, name: &str) -> Res<String> {
    let files = list(item);
    let hits: Vec<&(String, String, String)> = files
        .iter()
        .filter(|(id, n, _)| n == name || id == name)
        .collect();
    match hits.as_slice() {
        [one] => Ok(one.0.clone()),
        [] => Err(Fail::new(format!("the item has no file called {name}"))),
        _ => Err(Fail::new(format!(
            "more than one file is called {name}: use its id (see `figlock get`)"
        ))),
    }
}

/// The decoded contents of file `id`.
pub fn read(item: &ItemView, id: &str) -> Res<Zeroizing<Vec<u8>>> {
    let text = item
        .fields
        .get(&format!("file/{id}/data"))
        .ok_or_else(|| Fail::new("the file has no contents"))?;
    decode_file(text).map_err(|_| Fail::new("the stored file is damaged"))
}

/// Write `bytes` to `out` (`-` is stdout). A real path is created owner-only and never overwritten unless
/// `force` is set.
pub fn write_out(bytes: &[u8], out: &str, force: bool) -> Res<()> {
    if out == "-" {
        std::io::stdout().write_all(bytes)?;
        return Ok(());
    }
    let path = Path::new(out);
    if force && path.is_file() {
        fs::remove_file(path)?;
    }
    let mut f = create_private_file(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => Fail::new(format!(
            "{out} already exists: choose another path or give --force"
        )),
        k => Fail::new(format!("cannot write {out}: {k}")),
    })?;
    f.write_all(bytes)?;
    Ok(())
}
