//! The `figlock broker ...` and `figlock audit` commands.

use crate::prompt;
use std::fs;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use figlock_broker::approve::{Approver, BiometricApprover, DenyAll, TerminalApprover};
use figlock_broker::audit::{self, AuditWriter};
use figlock_broker::client::{call, expect_ok};
use figlock_broker::peer::{SystemEnv, my_uid};
use figlock_broker::{Broker, server, syncer};
use figlock_core::SecretKey;
use figlock_session::error::{Fail, Res};
use figlock_session::session::{Session, Ui};

fn print_json(v: &Value) -> Res<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(v).map_err(|_| Fail::new("cannot format output"))?
    );
    Ok(())
}

/// A broker locks itself after this many minutes without an approved request, unless told otherwise.
const DEFAULT_IDLE_MINUTES: u64 = 30;
/// Sets the idle timeout for a broker that `figlock http` or `figlock run` starts in the background.
const IDLE_ENV: &str = "FIGLOCK_BROKER_IDLE_MINUTES";

/// What a broker started for an agent has instead of a keyboard: it can use quick unlock, and it can never ask
/// for the password.
struct NoPrompt;

impl Ui for NoPrompt {
    fn password(&self) -> Res<Zeroizing<String>> {
        Err(Fail::new(
            "a background broker cannot ask for the password; open FigLock and unlock it, or run `figlock broker start` in a terminal",
        ))
    }

    fn secret_key(&self) -> Res<SecretKey> {
        Err(Fail::new(
            "a background broker cannot ask for the Secret Key; open FigLock and unlock it, or run `figlock broker start` in a terminal",
        ))
    }

    fn notice(&self, message: &str) {
        eprintln!("{message}");
    }

    fn warn(&self, message: &str) {
        eprintln!("warning: {message}");
    }
}

/// Unlock the vault once, then serve until someone runs `figlock broker lock`, the idle timeout passes with no
/// approved request (FIG-291), or the Mac sleeps or its screen locks. In the background, only with quick unlock
/// and only if something can ask for approval.
pub fn start(deny_all: bool, background: bool, idle_minutes: Option<u64>) -> Res<()> {
    let idle_minutes = match idle_minutes {
        Some(m) => m,
        None => match std::env::var(IDLE_ENV) {
            Ok(v) => v.trim().parse().map_err(|_| {
                Fail::new(format!(
                    "{IDLE_ENV} must be a whole number of minutes, 0 for never"
                ))
            })?,
            Err(_) => DEFAULT_IDLE_MINUTES,
        },
    };
    // A broker nobody can approve for is refused before anything is unlocked, so Touch ID is not spent on it.
    let biometric = BiometricApprover::open();
    if background && !deny_all && biometric.is_none() {
        return Err(Fail::new(
            "this Mac cannot show a Touch ID prompt and there is no terminal to ask on, so a background broker could approve nothing; run `figlock broker start` in a terminal",
        ));
    }
    crate::sync_cmd::pull_first();
    let session = if background {
        Session::open(Box::new(NoPrompt))?
    } else {
        Session::open(prompt::terminal())?
    };
    let log = audit::path_for(&session.vault)?;
    let writer = AuditWriter::open(&log, &session.vault)?;
    let socket = server::socket_path()?;
    let listener = server::bind(&socket)?;
    let approver: Box<dyn Approver> = if deny_all {
        Box::new(DenyAll)
    } else if let Some(b) = biometric {
        eprintln!("approvals are asked with Touch ID or your device password, every time");
        if std::env::var("FIGLOCK_DEVICE_KEYS").is_ok_and(|v| v == "file") {
            // That setting only moves where the device keys live; there is no setting that skips the prompt.
            eprintln!(
                "device keys are kept in a file (FIGLOCK_DEVICE_KEYS=file); approvals are still asked"
            );
        }
        Box::new(b)
    } else if let Some(t) = TerminalApprover::open() {
        eprintln!("no biometric prompt on this machine, approvals are asked on this terminal");
        Box::new(t)
    } else {
        eprintln!("warning: not asking anyone, so every request will be denied");
        Box::new(DenyAll)
    };
    let broker = Broker::start(
        session,
        writer,
        approver,
        Arc::new(SystemEnv::new()),
        my_uid(),
    )
    .map_err(|_| Fail::new("could not write to the audit log"))?;
    eprintln!("broker listening on {}", socket.display());
    let broker = Arc::new(broker);
    let chosen = (idle_minutes > 0).then(|| Duration::from_secs(idle_minutes.saturating_mul(60)));
    // `FIGLOCK_BROKER_IDLE_SECS` can only shorten the timeout, for tests (like `FIGLOCK_AUTOLOCK_SECS`).
    let shortened = std::env::var("FIGLOCK_BROKER_IDLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|d| chosen.is_some_and(|c| *d < c));
    if let Some(limit) = shortened.or(chosen) {
        broker.set_idle_limit(Some(limit));
        eprintln!(
            "it locks after {} without an approved request",
            span(limit.as_secs())
        );
    } else {
        eprintln!(
            "warning: the idle timeout is off, so it stays unlocked until `figlock broker lock`"
        );
    }
    syncer::spawn(broker.clone());
    // The Mac going to sleep or locking its screen ends the broker at once, with the grants (ADR-0030).
    let sleeping = broker.clone();
    figlock_biometric::power::watch_sleep(move || sleeping.end("sleep"));
    let served = server::serve(&listener, &broker);
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
    let idle = match (
        r["idle_timeout_secs"].as_u64(),
        r["idle_left_secs"].as_u64(),
    ) {
        (None, _) => "no idle timeout".to_owned(),
        (Some(_), Some(left)) => format!("locks in {} if unused", span(left)),
        (Some(limit), None) => format!("idle timeout {}", span(limit)),
    };
    println!(
        "{}, {} grant(s), {idle}, audit record {} (head {})",
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

/// `1h 5m`, `12m 3s` or `45s`: how long is left, for people.
fn span(secs: u64) -> String {
    match (secs / 3600, secs % 3600 / 60, secs % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s}s"),
        (h, m, _) => format!("{h}h {m}m"),
    }
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
    crate::sync_cmd::pull_first();
    let Session { vault, .. } = Session::open(prompt::terminal())?;
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
        return Err(Fail::new(format!(
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
    use super::{span, when};

    #[test]
    fn time_left_reads_naturally() {
        assert_eq!(span(45), "45s");
        assert_eq!(span(723), "12m 3s");
        assert_eq!(span(3900), "1h 5m");
        assert_eq!(span(0), "0s");
    }

    #[test]
    fn dates_are_formatted_in_utc() {
        assert_eq!(when(0), "1970-01-01 00:00:00Z");
        assert_eq!(when(1_700_000_000_000), "2023-11-14 22:13:20Z");
        assert_eq!(when(951_782_400_000), "2000-02-29 00:00:00Z");
        assert_eq!(when(1_709_251_199_000), "2024-02-29 23:59:59Z");
    }
}
