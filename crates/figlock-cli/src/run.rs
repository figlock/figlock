//! `figlock run`: start a program with secrets in its environment, after the broker's grant, and scrub the
//! secret values from what the program prints (`docs/spec/broker.md` 3.3, ADR-0003).
//!
//! This is the second-best way for an agent to use a secret (broker HTTP is the first): the program itself
//! receives the value, so output scrubbing stops accidental leaks only, not a program that means to leak.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::json;
use zeroize::Zeroizing;

use crate::broker::client::{call, expect_ok};
use crate::broker::server;
use crate::error::{Fail, Res};
use crate::scrub::{MIN_LEN, Scrubber};

/// What to run and which secrets it gets.
pub struct RunSpec {
    /// The item's title.
    pub item: String,
    /// `NAME=field` pairs. Empty means the item's own defaults (a token's `env_var`, a bundle's `env/*`).
    pub vars: Vec<(String, String)>,
    pub ttl: String,
    pub reason: Option<String>,
    pub command: Vec<String>,
}

/// The exit status a shell would report for a program that ended by signal `n`.
const SIGNAL_BASE: i32 = 128;

pub fn run(spec: &RunSpec) -> Res<i32> {
    execute(
        &server::socket_path()?,
        spec,
        std::io::stdout(),
        std::io::stderr(),
    )
}

/// Ask for approval, fetch the variables, start the program and copy its output through the scrubber.
/// Returns the program's exit code.
pub fn execute(
    socket: &Path,
    spec: &RunSpec,
    out: impl Write + Send,
    err: impl Write + Send,
) -> Res<i32> {
    let program = spec
        .command
        .first()
        .ok_or_else(|| Fail::new("no program to run"))?;
    let label = Path::new(program).file_name().map_or_else(
        || "program".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    let mut request = json!({
        "scope": {"name": spec.item}, "mode": "run", "ttl": spec.ttl,
        "label": format!("figlock run {label}"),
    });
    if let (Some(o), Some(r)) = (request.as_object_mut(), &spec.reason) {
        o.insert("reason".into(), json!(r));
    }
    let granted = expect_ok(call(socket, "grant.request", request)?)?;
    let token = granted["grant"]["token"]
        .as_str()
        .ok_or_else(|| Fail::new("the broker's answer was not valid"))?;
    let item = granted["grant"]["scope"]["item"]
        .as_str()
        .ok_or_else(|| Fail::new("the broker's answer was not valid"))?;

    let mut members = json!({"token": token, "item": item});
    if !spec.vars.is_empty() {
        let vars: serde_json::Map<String, serde_json::Value> = spec
            .vars
            .iter()
            .map(|(n, f)| (n.clone(), json!(f)))
            .collect();
        members["vars"] = vars.into();
    }
    let got = expect_ok(call(socket, "run.env", members)?)?;
    let env: Vec<(String, Zeroizing<String>)> = got["env"]
        .as_object()
        .ok_or_else(|| Fail::new("the broker's answer was not valid"))?
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), Zeroizing::new(v.as_str()?.to_owned()))))
        .collect();
    drop(got);
    for (name, value) in &env {
        if value.len() < MIN_LEN {
            eprintln!("note: {name} is too short to scrub from the output");
        }
    }

    let mut child = Command::new(program)
        .args(&spec.command[1..])
        .envs(env.iter().map(|(k, v)| (k, v.as_str())))
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Fail::new(format!("could not start {label}: {}", e.kind())))?;
    let child_out = child.stdout.take();
    let child_err = child.stderr.take();

    let (mut out, mut err) = (out, err);
    std::thread::scope(|s| {
        let a = s.spawn(|| pump(&env, child_out, &mut out));
        let b = s.spawn(|| pump(&env, child_err, &mut err));
        let _ = a.join();
        let _ = b.join();
    });

    let status = child
        .wait()
        .map_err(|e| Fail::new(format!("could not wait for {label}: {}", e.kind())))?;
    Ok(exit_code(&status))
}

/// Copy `from` to `to` with the secrets in `env` removed. If `to` stops accepting data, keep draining
/// `from` so the program is not stuck on a full pipe.
fn pump(env: &[(String, Zeroizing<String>)], from: Option<impl Read>, to: &mut impl Write) {
    let Some(mut from) = from else { return };
    let mut scrub = Scrubber::new(env);
    let mut buf = [0u8; 8192];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if to
            .write_all(&scrub.feed(&buf[..n]))
            .and_then(|()| to.flush())
            .is_err()
        {
            while from.read(&mut buf).is_ok_and(|n| n > 0) {}
            return;
        }
    }
    let _ = to.write_all(&scrub.finish());
    let _ = to.flush();
}

#[cfg(unix)]
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .or_else(|| status.signal().map(|s| SIGNAL_BASE + s))
        .unwrap_or(1)
}
