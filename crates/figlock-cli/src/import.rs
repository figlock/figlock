//! `figlock import`: read another manager's export (or a notes dump) and add its items to the vault.
//!
//! Parsing is `figlock_core::parse_import` (pure, bounded, errors never quote the file). This module only reads
//! the file, shows what was found, and writes the items. `--dry-run` stops after the summary and never asks for
//! the master password.

use std::fs::File;
use std::io::Read;

use figlock_core::{Draft, ItemType, MAX_INPUT_BYTES, Report, SkipReason, parse_import};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::ImportArgs;
use crate::commands::VAULT_NAME;
use crate::error::{Fail, Res};
use crate::session::{Session, now_ms};

/// Items per op file, so one file stays well under the store's blob limit.
const COMMIT_EVERY: usize = 100;

/// The export, read whole into memory that is wiped on drop. It holds passwords in plain text.
fn read_export(path: &std::path::Path) -> Res<Zeroizing<Vec<u8>>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Fail::new("that path is not a file"));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn count(items: &[Draft], t: ItemType) -> usize {
    items.iter().filter(|d| d.item_type == t).count()
}

fn skipped(report: &Report, reason: SkipReason) -> usize {
    report.skipped.iter().filter(|s| s.reason == reason).count()
}

fn summary_json(a: &ImportArgs, r: &Report, imported: bool) -> Value {
    json!({
        "source": a.from.source().as_str(),
        "dry_run": a.dry_run,
        "imported": imported,
        "logins": count(&r.items, ItemType::Login),
        "notes": count(&r.items, ItemType::Note),
        "tokens": count(&r.items, ItemType::Token),
        "tokens_detected": r.tokens_detected,
        "skipped": {
            "empty": skipped(r, SkipReason::Empty),
            "field_too_large": skipped(r, SkipReason::FieldTooLarge),
            "unsupported_type": skipped(r, SkipReason::UnsupportedType),
        },
    })
}

fn print_summary(r: &Report, imported: bool) {
    let verb = if imported { "Imported" } else { "Found" };
    println!(
        "{verb} {} logins, {} notes, {} API tokens.",
        count(&r.items, ItemType::Login),
        count(&r.items, ItemType::Note),
        count(&r.items, ItemType::Token),
    );
    if !r.skipped.is_empty() {
        println!(
            "Skipped {}: {} empty, {} too large, {} of an unsupported type.",
            r.skipped.len(),
            skipped(r, SkipReason::Empty),
            skipped(r, SkipReason::FieldTooLarge),
            skipped(r, SkipReason::UnsupportedType),
        );
    }
    if r.tokens_detected > 0 {
        println!("Tokens found in notes were filed as token items and removed from the note text.");
    }
}

pub fn import(a: ImportArgs, json: bool) -> Res<()> {
    let bytes = read_export(&a.file)?;
    let report = parse_import(a.from.source(), &bytes).map_err(|e| Fail::new(e.to_string()))?;
    drop(bytes);
    if a.dry_run {
        if json {
            println!("{}", summary_json(&a, &report, false));
        } else {
            print_summary(&report, false);
            eprintln!("Dry run: nothing was written.");
        }
        return Ok(());
    }
    if report.items.is_empty() {
        if json {
            println!("{}", summary_json(&a, &report, false));
        } else {
            print_summary(&report, false);
        }
        return Err(Fail::new("nothing to import"));
    }
    let mut s = Session::open()?;
    let now = now_ms();
    for (n, chunk) in report.items.chunks(COMMIT_EVERY).enumerate() {
        for d in chunk {
            let fields: Vec<(&str, &str)> = d
                .fields
                .iter()
                .map(|(name, v)| (name.as_str(), v.as_str()))
                .collect();
            s.vault
                .add_item(d.item_type, VAULT_NAME, &fields, now)
                .map_err(|_| partial(n * COMMIT_EVERY))?;
        }
        s.commit().map_err(|_| partial(n * COMMIT_EVERY))?;
    }
    if json {
        println!("{}", summary_json(&a, &report, true));
    } else {
        print_summary(&report, true);
    }
    eprintln!(
        "The export file holds your passwords in plain text. Delete it securely once you have checked the import. \
         Running the import again adds the items a second time."
    );
    Ok(())
}

fn partial(done: usize) -> Fail {
    Fail::new(format!(
        "the import stopped after {done} items were written; those are in the vault, the rest are not"
    ))
}
