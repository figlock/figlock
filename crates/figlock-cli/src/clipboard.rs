//! System clipboard through the platform's own tools, so no extra dependency and no unsafe code.
//!
//! `FIGLOCK_CLIPBOARD_COPY` and `FIGLOCK_CLIPBOARD_PASTE` override the tool (a program and plain
//! space-separated arguments), for remote sessions or tests.

use std::env;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

use zeroize::Zeroizing;

use crate::error::{Fail, Res};

fn candidates(var: &str, defaults: &[&str]) -> Vec<Vec<String>> {
    let split = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    match env::var(var) {
        Ok(v) if !v.trim().is_empty() => vec![split(&v)],
        _ => defaults.iter().map(|d| split(d)).collect(),
    }
}

fn copy_tools() -> Vec<Vec<String>> {
    let d: &[&str] = if cfg!(target_os = "macos") {
        &["pbcopy"]
    } else if cfg!(windows) {
        &["clip"]
    } else {
        &[
            "wl-copy",
            "xclip -selection clipboard",
            "xsel --clipboard --input",
        ]
    };
    candidates("FIGLOCK_CLIPBOARD_COPY", d)
}

fn paste_tools() -> Vec<Vec<String>> {
    let d: &[&str] = if cfg!(target_os = "macos") {
        &["pbpaste"]
    } else if cfg!(windows) {
        &["powershell -NoProfile -Command Get-Clipboard"]
    } else {
        &[
            "wl-paste --no-newline",
            "xclip -selection clipboard -o",
            "xsel --clipboard --output",
        ]
    };
    candidates("FIGLOCK_CLIPBOARD_PASTE", d)
}

pub fn copy(text: &str) -> Res<()> {
    for tool in copy_tools() {
        let Some((prog, args)) = tool.split_first() else {
            continue;
        };
        let Ok(mut child) = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }
        if child.wait()?.success() {
            return Ok(());
        }
    }
    Err(Fail::new(
        "no clipboard tool found (install wl-clipboard, xclip or xsel, or set FIGLOCK_CLIPBOARD_COPY)",
    ))
}

/// Current clipboard text, or `None` when it cannot be read.
pub fn paste() -> Option<Zeroizing<String>> {
    for tool in paste_tools() {
        let Some((prog, args)) = tool.split_first() else {
            continue;
        };
        let Ok(mut child) = Command::new(prog)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let mut out = Zeroizing::new(Vec::new());
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_end(&mut out);
        }
        if child.wait().is_ok_and(|s| s.success()) {
            return Some(Zeroizing::new(String::from_utf8_lossy(&out).into_owned()));
        }
    }
    None
}
