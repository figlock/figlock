//! Secrets typed or piped by the user. Nothing secret is ever taken from a command-line argument, where it
//! would show in `ps` and shell history. `FIGLOCK_PASSWORD` and `FIGLOCK_SECRET_KEY` exist for scripts and
//! tests; prefer the prompt.

use std::env;
use std::io::{self, BufRead, IsTerminal, Write};

use figlock_core::SecretKey;
use zeroize::Zeroizing;

use crate::error::{Fail, Res};

fn hidden(label: &str) -> Res<Zeroizing<String>> {
    let mut err = io::stderr();
    err.write_all(label.as_bytes())?;
    err.flush()?;
    let line = rpassword::read_password().map_err(|_| {
        Fail::new("cannot read from the terminal; use the environment variable for scripts")
    })?;
    Ok(Zeroizing::new(line))
}

fn from_env(name: &str) -> Option<Zeroizing<String>> {
    env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .map(Zeroizing::new)
}

pub fn password() -> Res<Zeroizing<String>> {
    if let Some(p) = from_env("FIGLOCK_PASSWORD") {
        return Ok(p);
    }
    hidden("Master password: ")
}

/// A new master password, typed twice.
pub fn new_password() -> Res<Zeroizing<String>> {
    if let Some(p) = from_env("FIGLOCK_PASSWORD") {
        return Ok(p);
    }
    let first = hidden("New master password: ")?;
    if first.is_empty() {
        return Err(Fail::new("the master password cannot be empty"));
    }
    let again = hidden("Repeat master password: ")?;
    if *first != *again {
        return Err(Fail::new("the passwords do not match"));
    }
    Ok(first)
}

pub fn secret_key() -> Res<SecretKey> {
    let text = match from_env("FIGLOCK_SECRET_KEY") {
        Some(t) => t,
        None => hidden("Secret Key (FL1-...): ")?,
    };
    SecretKey::parse(text.trim())
        .map_err(|_| Fail::new("that Secret Key is not valid; check it for typos"))
}

/// The value of a secret field: hidden prompt on a terminal, otherwise one line from stdin.
pub fn secret_value(field: &str) -> Res<Zeroizing<String>> {
    if io::stdin().is_terminal() {
        return hidden(&format!("Value for {field}: "));
    }
    let mut line = Zeroizing::new(String::new());
    let n = io::stdin().lock().read_line(&mut line)?;
    if n == 0 {
        return Err(Fail::new(format!("no value on stdin for {field}")));
    }
    let trimmed = line.trim_end_matches(['\n', '\r']).len();
    line.truncate(trimmed);
    Ok(line)
}
