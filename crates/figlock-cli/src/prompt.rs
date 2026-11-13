//! Secrets typed or piped by the user. Nothing secret is ever taken from a command-line argument, where it
//! would show in `ps` and shell history. `FIGLOCK_PASSWORD` and `FIGLOCK_SECRET_KEY` exist for scripts and
//! tests; prefer the prompt.

use std::env;
use std::io::{self, BufRead, IsTerminal, Write};

use figlock_core::SecretKey;
use zeroize::Zeroizing;

use figlock_session::error::{Fail, Res};

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

/// The replacement for a master password that is being changed, typed twice. `FIGLOCK_NEW_PASSWORD` is for
/// scripts and tests.
pub fn changed_password() -> Res<Zeroizing<String>> {
    if let Some(p) = from_env("FIGLOCK_NEW_PASSWORD") {
        return Ok(p);
    }
    let first = hidden("New master password: ")?;
    if first.is_empty() {
        return Err(Fail::new("the master password cannot be empty"));
    }
    let again = hidden("Repeat new master password: ")?;
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

/// A yes/no question on the terminal where Enter means Yes. Anything but y/yes counts as No.
pub fn confirm_default_yes(question: &str) -> Res<bool> {
    let mut err = io::stderr();
    err.write_all(format!("{question} [Y/n] ").as_bytes())?;
    err.flush()?;
    let mut line = String::new();
    // A closed input is not an Enter: only a typed newline accepts the default.
    if io::stdin().lock().read_line(&mut line)? == 0 {
        return Ok(false);
    }
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}

/// A yes/no question on the terminal where Enter means No, for approvals. A closed input is a No.
pub fn confirm_default_no(question: &str) -> Res<bool> {
    let mut err = io::stderr();
    err.write_all(format!("{question} [y/N] ").as_bytes())?;
    err.flush()?;
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line)? == 0 {
        return Ok(false);
    }
    let answer = line.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

/// Credentials already in hand (just typed for a join or an open), so the unlock that follows does not ask
/// again.
struct Known {
    password: Zeroizing<String>,
    secret_key: SecretKey,
}

impl figlock_session::Ui for Known {
    fn password(&self) -> Res<Zeroizing<String>> {
        Ok(self.password.clone())
    }
    fn secret_key(&self) -> Res<SecretKey> {
        Ok(SecretKey::from_bytes(*self.secret_key.expose()))
    }
    fn notice(&self, message: &str) {
        eprintln!("{message}");
    }
    fn warn(&self, message: &str) {
        eprintln!("warning: {message}");
    }
}

pub fn known(password: &str, secret_key: SecretKey) -> Box<dyn figlock_session::Ui> {
    Box::new(Known {
        password: Zeroizing::new(password.to_owned()),
        secret_key,
    })
}

/// The terminal as the session's way of asking and reporting.
struct Terminal;

impl figlock_session::Ui for Terminal {
    fn password(&self) -> Res<Zeroizing<String>> {
        password()
    }
    fn secret_key(&self) -> Res<SecretKey> {
        secret_key()
    }
    fn notice(&self, message: &str) {
        eprintln!("{message}");
    }
    fn warn(&self, message: &str) {
        eprintln!("warning: {message}");
    }
}

pub fn terminal() -> Box<dyn figlock_session::Ui> {
    Box::new(Terminal)
}
