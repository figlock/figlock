//! The `figlock broker ...` and `figlock audit` commands.

use std::fs;
use std::sync::Arc;

use serde_json::{Value, json};

use super::approve::{Approver, DenyAll, TerminalApprover};
use super::audit::{self, AuditWriter};
use super::client::{call, expect_ok};
use super::peer::{SystemEnv, my_uid};
use super::{Broker, server};
use crate::error::{Fail, Res};
use crate::session::Session;

fn print_json(v: &Value) -> Res<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).map_err(|_| Fail::new("cannot format output"))?
    );
    Ok(())
}

/// Unlock the vault once, then serve until someone runs `figlock broker lock`.
pub fn start(deny_all: bool) -> Res<()> {
    let Session { vault, .. } = Session::open()?;
    let log = audit::path_for(&vault)?;
    let writer = AuditWriter::open(&log, &vault)?;
    let socket = server::socket_path()?;
    let listener = server::bind(&socket)?;
    let terminal = if deny_all {
        None
    } else {
        TerminalApprover::open()
    };
    let approver: Box<dyn Approver> = match terminal {
        Some(t) => Box::new(t),
        None => {
            eprintln!("warning: not asking anyone, so every request will be denied");
            Box::new(DenyAll)
        }
    };
    let broker = Broker::start(
        vault,
        writer,
        approver,
        Arc::new(SystemEnv::new()),
        my_uid(),
    )
    .map_err(|_| Fail::new("could not write to the audit log"))?;
    eprintln!("broker listening on {}", socket.display());
    let served = server::serve(&listener, &Arc::new(broker));
    drop(listener);
    let _ = fs::remove_file(&socket);
    eprintln!("broker locked");
    served
}

fn ask(op: &str, members: Value) -> Res<Value> {
    expect_ok(call(&server::socket_path()?, op, members)?)
}

pub fn status(json_out: bool) -> Res<()> {
    let r = ask("status", json!({}))?;
    if json_out {
        return print_json(&r);
    }
    println!(
        "{}, {} grant(s), audit record {} (head {})",
        if r["locked"] == true {
            "locked"
        } else {
            "unlocked"
        },
        r["grants"],
        r["audit_seq"],
        r["audit_head"]
            .as_str()
            .unwrap_or("")
            .get(..16)
            .unwrap_or(""),
    );
    Ok(())
}

pub fn grants(json_out: bool) -> Res<()> {
    let r = ask("grant.list", json!({}))?;
    if json_out {
        return print_json(&r["grants"]);
    }
    let list = r["grants"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        eprintln!("no active grants");
    }
    for g in list {
        let left = g["expires_in"]
            .as_u64()
            .map_or("until lock".to_owned(), |s| format!("{s}s left"));
        println!(
            "{}  {:<6} {:<10} {}  {}  {}",
            g["id"].as_str().unwrap_or(""),
            g["mode"].as_str().unwrap_or(""),
            g["ttl"].as_str().unwrap_or(""),
            left,
            g["scope"],
            crate::render::clean(g["peer"]["exe"].as_str().unwrap_or("(unknown program)")),
        );
    }
    Ok(())
}

pub fn revoke(id: &str, json_out: bool) -> Res<()> {
    let r = ask("grant.revoke", json!({"grant": id}))?;
    if json_out {
        return print_json(&r);
    }
    eprintln!("revoked {id}");
    Ok(())
}

pub fn lock(json_out: bool) -> Res<()> {
    let r = ask("lock", json!({}))?;
    if json_out {
        return print_json(&r);
    }
    eprintln!("broker locked");
    Ok(())
}

/// Civil date and time from Unix milliseconds (UTC), without a date library.
fn when(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's days-to-civil algorithm.
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Verify and print the audit log. Stops at the first record that fails, and says so.
pub fn show_audit(tail: Option<usize>, json_out: bool) -> Res<()> {
    let Session { vault, .. } = Session::open()?;
    let path = audit::path_for(&vault)?;
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    let (chain, damaged_tail) = vault.read_audit_log(&bytes);
    let skip = tail.map_or(0, |n| chain.records.len().saturating_sub(n));
    let shown = &chain.records[skip..];
    if json_out {
        let rows: Vec<Value> = shown
            .iter()
            .map(|r| json!({"seq": r.seq, "t": r.t, "event": r.ev, "detail": r.d}))
            .collect();
        print_json(&Value::Array(rows))?;
    } else {
        for r in shown {
            let detail = serde_json::to_string(&r.d).unwrap_or_default();
            println!(
                "{:>6}  {}  {:<16} {}",
                r.seq,
                when(r.t),
                r.ev,
                crate::render::clean(&detail)
            );
        }
        if chain.records.is_empty() {
            eprintln!("the audit log is empty");
        }
    }
    if let Some(seq) = chain.first_bad {
        return Err(Fail(format!(
            "the audit log failed verification at record {seq}; records after it are not shown"
        )));
    }
    if damaged_tail {
        eprintln!("warning: the end of the audit log is incomplete");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::when;

    #[test]
    fn dates_are_formatted_in_utc() {
        assert_eq!(when(0), "1970-01-01 00:00:00Z");
        assert_eq!(when(1_700_000_000_000), "2023-11-14 22:13:20Z");
        assert_eq!(when(951_782_400_000), "2000-02-29 00:00:00Z");
        assert_eq!(when(1_709_251_199_000), "2024-02-29 23:59:59Z");
    }
}
