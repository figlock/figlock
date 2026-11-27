//! `figlock health`: which saved passwords are weak, reused or old, and with `--breaches` which are in a
//! known breach. The analysis is local and `figlock_core::health` never returns a password; the output names
//! items by title and id only. The breach check sends nothing but the first 5 hex characters of each
//! password's SHA-1 (see `figlock_session::breach`) and fails soft: offline, the local results still print.

use figlock_core::generate::Level;
use figlock_core::health::{self, Finding};
use figlock_core::{ItemId, ItemView};
use figlock_session::breach::{PwnedPasswords, RangeSource, breaches};
use figlock_session::error::Res;
use figlock_session::session::{Session, id_hex, now_ms, title_of};
use serde_json::{Value, json};

use crate::render::clean;
use crate::{HealthArgs, prompt};

const DAY_MS: u64 = 86_400_000;
/// Names a different range service (a mirror, or a stand-in in tests); the public one when unset.
const RANGE_URL_ENV: &str = "FIGLOCK_PWNED_RANGE_URL";

struct Row {
    id: String,
    kind: &'static str,
    title: String,
    weak_bits: Option<u32>,
    reused_with: usize,
    old_days: Option<u64>,
    breached: Option<u64>,
}

impl Row {
    fn of(item: &ItemView, f: Option<&Finding>) -> Self {
        Row {
            id: id_hex(&item.id),
            kind: item.item_type.as_str(),
            title: title_of(item).to_owned(),
            weak_bits: f
                .and_then(|f| f.weak)
                .filter(|s| s.level == Level::Weak)
                .map(|s| s.bits),
            reused_with: f.map_or(0, |f| f.reused_with),
            old_days: f.and_then(|f| f.old_ms).map(|ms| ms / DAY_MS),
            breached: None,
        }
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "type": self.kind,
            "title": self.title,
            "weak_bits": self.weak_bits,
            "reused_with": self.reused_with,
            "old_days": self.old_days,
            "breached": self.breached,
        })
    }

    fn reasons(&self) -> String {
        let mut r = Vec::new();
        if let Some(b) = self.weak_bits {
            r.push(format!("weak ({b} bits)"));
        }
        match self.reused_with {
            0 => {}
            1 => r.push("reused with 1 other item".to_owned()),
            n => r.push(format!("reused with {n} other items")),
        }
        if let Some(d) = self.old_days {
            r.push(format!("unchanged for {d} days"));
        }
        if let Some(n) = self.breached {
            r.push(format!("in {n} known breaches"));
        }
        r.join(", ")
    }
}

/// Runs the check. The exit code is 1 when items need attention, unless `--no-fail`.
pub fn run(a: HealthArgs, json: bool) -> Res<i32> {
    crate::sync_cmd::pull_first();
    let mut s = Session::open(prompt::terminal())?;
    let items = s.items(false);
    let report = health::analyze_with(&items, now_ms(), a.old_days.saturating_mul(DAY_MS));
    let find = |id: &ItemId| items.iter().find(|i| &i.id == id);

    let mut rows: Vec<Row> = report
        .findings
        .iter()
        .filter_map(|f| find(&f.id).map(|i| Row::of(i, Some(f))))
        .collect();

    let (mut breach_checked, mut breach_failed) = (false, false);
    if a.breaches {
        let queries: Vec<(String, _)> = items
            .iter()
            .filter_map(|i| {
                let pw = i.fields.get("password")?;
                (!pw.is_empty()).then(|| (id_hex(&i.id), health::pwned_query(pw)))
            })
            .collect();
        let source = match std::env::var(RANGE_URL_ENV) {
            Ok(base) if !base.is_empty() => PwnedPasswords::with_base(&base),
            _ => PwnedPasswords::new(),
        };
        match breaches(&source as &dyn RangeSource, &queries) {
            Ok(found) => {
                breach_checked = true;
                let mut only = Vec::new();
                for (id, n) in found {
                    match rows.iter_mut().find(|r| r.id == id) {
                        Some(r) => r.breached = Some(n),
                        None => only.push((id, n)),
                    }
                }
                only.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
                for (id, n) in only {
                    if let Some(i) = items.iter().find(|i| id_hex(&i.id) == id) {
                        let mut r = Row::of(i, None);
                        r.breached = Some(n);
                        rows.push(r);
                    }
                }
            }
            Err(_) => {
                breach_failed = true;
                eprintln!(
                    "warning: could not reach the breach service; only weak, reused and old passwords were checked"
                );
            }
        }
    }

    if json {
        crate::commands::print_json(&json!({
            "checked": report.checked,
            "problems": rows.len(),
            "old_after_days": a.old_days,
            "breach_checked": breach_checked,
            "breach_failed": breach_failed,
            "items": rows.iter().map(Row::json).collect::<Vec<_>>(),
        }))?;
    } else {
        for r in &rows {
            println!(
                "{}  {:<6}  {}  {}",
                &r.id[..8],
                r.kind,
                clean(&r.title),
                r.reasons()
            );
        }
        match rows.len() {
            0 => eprintln!("checked {} passwords: no problems found", report.checked),
            n => eprintln!("checked {} passwords: {n} need attention", report.checked),
        }
    }
    Ok(i32::from(!rows.is_empty() && !a.no_fail))
}
