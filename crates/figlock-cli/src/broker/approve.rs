//! Asking the user (`docs/spec/broker.md` 3.2). The broker only knows the [`Approver`] trait. On a Mac the
//! daemon asks with Touch ID ([`BiometricApprover`]); anywhere else it asks on its own terminal.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use figlock_biometric::Outcome;
use figlock_core::audit_clean_text;

use super::peer::PeerId;
use super::proto::{Mode, Ttl};

/// Nobody answering within this long counts as no.
pub const APPROVAL_WAIT: Duration = Duration::from_secs(60);

pub struct ApprovalRequest {
    /// Verified by the operating system.
    pub peer: PeerId,
    /// Claimed by the caller. Untrusted.
    pub label: Option<String>,
    pub reason: Option<String>,
    /// The scope in plain words, written by the broker from the vault, not by the caller.
    pub scope_words: String,
    pub mode: Mode,
    pub ttl: Ttl,
    /// Extra loud wording for the dangerous cases (reveal, the whole vault).
    pub loud: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

pub trait Approver: Send + Sync {
    /// Block until the user decides or `wait` passes (then deny).
    fn approve(&self, req: &ApprovalRequest, wait: Duration) -> Decision;
}

/// Denies everything. Used when there is no terminal to ask on.
pub struct DenyAll;

impl Approver for DenyAll {
    fn approve(&self, _: &ApprovalRequest, _: Duration) -> Decision {
        Decision::Deny
    }
}

fn how(mode: Mode) -> &'static str {
    match mode {
        Mode::Http => {
            "http: figlock makes the request and adds the secret; the program never sees it"
        }
        Mode::Run => "run: the secret is put in the environment of a program figlock starts",
        Mode::Reveal => "reveal: the program is given the secret value itself",
    }
}

fn duration(ttl: Ttl) -> &'static str {
    match ttl {
        Ttl::Once => "one use",
        Ttl::Min15 => "15 minutes",
        Ttl::Session => "until that program exits (at most 12 hours)",
        Ttl::Day => "1 day",
        Ttl::UntilLock => "until the broker is locked",
    }
}

/// Program names, nearest first, as `claude < zsh`.
fn chain(parents: &[String], max: usize) -> String {
    let names: Vec<String> = parents
        .iter()
        .map(|p| {
            let name = p.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or(p);
            audit_clean_text(name, 40)
        })
        .collect();
    audit_clean_text(&names.join(" < "), max)
}

/// The text of the prompt. What the OS verified comes first and is marked; what the caller claimed is
/// quoted and marked unverified.
pub fn describe(req: &ApprovalRequest) -> String {
    let pid = req.peer.pid.map_or("unknown".to_owned(), |p| p.to_string());
    let exe = req.peer.exe.as_deref().unwrap_or("(program not found)");
    let mut s = String::from("\nfiglock: a program asks to use your secrets\n");
    s.push_str(&format!(
        "  program   {}  (pid {pid}, user {})  [checked by the system]\n",
        audit_clean_text(exe, 300),
        req.peer.uid
    ));
    if !req.peer.parents.is_empty() {
        s.push_str(&format!(
            "  started by {}  [checked by the system]\n",
            chain(&req.peer.parents, 300)
        ));
    }
    if let Some(l) = &req.label {
        s.push_str(&format!(
            "  it says   \"{}\"  [not checked]\n",
            audit_clean_text(l, 100)
        ));
    }
    if let Some(r) = &req.reason {
        s.push_str(&format!(
            "  reason    \"{}\"  [not checked]\n",
            audit_clean_text(r, 200)
        ));
    }
    s.push_str(&format!("  wants     {}\n", req.scope_words));
    s.push_str(&format!("  how       {}\n", how(req.mode)));
    s.push_str(&format!("  for       {}\n", duration(req.ttl)));
    for warning in &req.loud {
        s.push_str(&format!("  !! {warning}\n"));
    }
    s
}

/// The one line the system prompt shows (Touch ID has room for little more). It names the program the
/// operating system verified, never the caller's own label, and keeps the dangerous cases loud.
pub fn prompt_line(req: &ApprovalRequest) -> String {
    let exe = req.peer.exe.as_deref().unwrap_or("an unknown program");
    let name = exe
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(exe);
    let pid = req
        .peer
        .pid
        .map_or(String::new(), |p| format!(" (pid {p})"));
    let mut s = String::new();
    for warning in &req.loud {
        s.push_str(&format!("{warning}. "));
    }
    let by = if req.peer.parents.is_empty() {
        String::new()
    } else {
        format!(", started by {},", chain(&req.peer.parents, 120))
    };
    s.push_str(&format!(
        "Let {}{pid}{by} use {} for {}",
        audit_clean_text(name, 40),
        audit_clean_text(&req.scope_words, 120),
        duration(req.ttl)
    ));
    if req.mode == Mode::Reveal {
        s.push_str(" - it will SEE the secret");
    }
    audit_clean_text(&s, 400)
}

/// Asks with the system's biometric prompt. `confirm` is the prompt itself, injected so tests need no screen.
pub struct BiometricApprover {
    confirm: fn(&str, Duration) -> Outcome,
}

impl BiometricApprover {
    /// `None` when this machine cannot show a biometric prompt.
    pub fn open() -> Option<BiometricApprover> {
        figlock_biometric::available().then_some(BiometricApprover {
            confirm: figlock_biometric::confirm,
        })
    }

    #[cfg(test)]
    pub fn with(confirm: fn(&str, Duration) -> Outcome) -> BiometricApprover {
        BiometricApprover { confirm }
    }
}

impl Approver for BiometricApprover {
    fn approve(&self, req: &ApprovalRequest, wait: Duration) -> Decision {
        match (self.confirm)(&prompt_line(req), wait) {
            Outcome::Confirmed => Decision::Allow,
            Outcome::Declined | Outcome::TimedOut | Outcome::Unavailable => Decision::Deny,
        }
    }
}

/// Asks on the terminal the broker was started from.
pub struct TerminalApprover {
    out: Mutex<File>,
    answers: Mutex<Receiver<String>>,
}

impl TerminalApprover {
    /// `None` when there is no controlling terminal.
    pub fn open() -> Option<TerminalApprover> {
        let read = OpenOptions::new().read(true).open("/dev/tty").ok()?;
        let out = OpenOptions::new().write(true).open("/dev/tty").ok()?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(read).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Some(TerminalApprover {
            out: Mutex::new(out),
            answers: Mutex::new(rx),
        })
    }
}

impl Approver for TerminalApprover {
    fn approve(&self, req: &ApprovalRequest, wait: Duration) -> Decision {
        let (Ok(mut out), Ok(answers)) = (self.out.lock(), self.answers.lock()) else {
            return Decision::Deny;
        };
        // Anything typed before this prompt appeared is not an answer to it.
        while answers.try_recv().is_ok() {}
        let text = format!(
            "{}\n(This Mac has no biometric prompt, so this is a plain terminal question.)\nAllow? [y/N] ",
            describe(req)
        );
        if out
            .write_all(text.as_bytes())
            .and_then(|()| out.flush())
            .is_err()
        {
            return Decision::Deny;
        }
        match answers.recv_timeout(wait) {
            Ok(line) if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") => {
                Decision::Allow
            }
            Err(RecvTimeoutError::Timeout) => {
                let _ = writeln!(out, "no answer, denied");
                Decision::Deny
            }
            _ => Decision::Deny,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(label: &str, reason: &str) -> ApprovalRequest {
        ApprovalRequest {
            peer: PeerId {
                uid: 501,
                pid: Some(4242),
                exe: Some("/usr/bin/real-agent".into()),
                parents: vec![],
            },
            label: Some(label.into()),
            reason: Some(reason.into()),
            scope_words: "the item \"GitHub\"".into(),
            mode: Mode::Http,
            ttl: Ttl::Min15,
            loud: vec![],
        }
    }

    #[test]
    fn the_verified_identity_comes_first_and_a_spoofed_label_is_marked_unchecked() {
        let text = describe(&req("Anthropic Claude Code (trusted)", "to deploy"));
        let program = text.find("/usr/bin/real-agent").unwrap();
        let claim = text.find("Anthropic Claude Code (trusted)").unwrap();
        assert!(program < claim);
        assert!(text.contains("[checked by the system]"));
        assert!(text.contains("\"Anthropic Claude Code (trusted)\"  [not checked]"));
        assert!(text.contains("pid 4242"));
        assert!(text.contains("15 minutes"));
    }

    #[test]
    fn control_characters_cannot_rewrite_the_prompt() {
        let text = describe(&req(
            "x\u{1b}[2J\r  program   /bin/fake",
            "ok\nwants   everything",
        ));
        assert!(!text.contains('\u{1b}') && !text.contains('\r'));
        // The reason's newline became a space, so it cannot start a fake line.
        assert_eq!(text.matches("\n  wants ").count(), 1);
        assert_eq!(text.matches("\n  program ").count(), 1);
    }

    #[test]
    fn dangerous_requests_carry_loud_wording() {
        let mut r = req("a", "b");
        r.mode = Mode::Reveal;
        r.loud = vec!["THE PROGRAM WILL SEE THE SECRET"];
        let text = describe(&r);
        assert!(text.contains("!! THE PROGRAM WILL SEE THE SECRET"));
        assert!(text.contains("given the secret value itself"));
    }

    #[test]
    fn the_prompt_line_names_the_verified_program_not_the_claim() {
        let line = prompt_line(&req("Anthropic Claude Code (trusted)", "to deploy"));
        assert!(line.contains("real-agent"));
        assert!(line.contains("pid 4242"));
        assert!(line.contains("the item \"GitHub\""));
        assert!(line.contains("15 minutes"));
        assert!(!line.contains("Anthropic"));
        assert!(!line.contains("/usr/bin/"));
    }

    #[test]
    fn the_prompt_names_who_started_the_asking_program() {
        let mut r = req("a", "b");
        r.peer.exe = Some("/usr/local/bin/figlock".into());
        r.peer.parents = vec!["/opt/claude/claude".into(), "/bin/zsh".into()];
        let line = prompt_line(&r);
        assert!(line.contains("figlock (pid 4242), started by claude < zsh, use"));
        let text = describe(&r);
        assert!(text.contains("started by claude < zsh  [checked by the system]"));
    }

    #[test]
    fn the_prompt_line_is_one_clean_line_and_keeps_warnings() {
        let mut r = req("a", "b");
        r.peer.exe = Some("/bin/x\u{1b}[2J\ny".into());
        r.mode = Mode::Reveal;
        r.loud = vec!["THE WHOLE VAULT"];
        let line = prompt_line(&r);
        assert!(!line.chars().any(char::is_control));
        assert!(line.contains("SEE the secret") && line.contains("THE WHOLE VAULT"));
        assert!(line.len() <= 400);
    }

    #[test]
    fn a_long_tag_scope_cannot_push_the_warnings_out() {
        let mut r = req("a", "b");
        r.mode = Mode::Reveal;
        r.loud = vec!["THIS COVERS EVERY ITEM IN YOUR VAULT"];
        let titles: Vec<String> = (0..5)
            .map(|i| format!("\"{i}{}\"", "t".repeat(79)))
            .collect();
        r.scope_words = format!("every item tagged x: {}", titles.join(", "));
        let line = prompt_line(&r);
        assert!(line.contains("THIS COVERS EVERY ITEM IN YOUR VAULT"));
        assert!(line.contains("SEE the secret"));
        assert!(line.contains("15 minutes"));
        assert!(line.contains("real-agent"));
    }

    #[test]
    fn only_a_confirmed_prompt_allows() {
        let r = req("a", "b");
        let w = Duration::from_secs(1);
        let allow = BiometricApprover::with(|_, _| Outcome::Confirmed);
        assert_eq!(allow.approve(&r, w), Decision::Allow);
        for outcome in [Outcome::Declined, Outcome::TimedOut, Outcome::Unavailable] {
            let b = match outcome {
                Outcome::Declined => BiometricApprover::with(|_, _| Outcome::Declined),
                Outcome::TimedOut => BiometricApprover::with(|_, _| Outcome::TimedOut),
                _ => BiometricApprover::with(|_, _| Outcome::Unavailable),
            };
            assert_eq!(b.approve(&r, w), Decision::Deny, "{outcome:?}");
        }
    }

    #[test]
    fn deny_all_denies() {
        assert_eq!(
            DenyAll.approve(&req("a", "b"), Duration::from_secs(1)),
            Decision::Deny
        );
    }
}
