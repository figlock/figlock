//! Asking the user (`docs/spec/broker.md` 3.2). The broker only knows the [`Approver`] trait; the biometric
//! prompt (FIG-35) will be another implementation. Until then the daemon asks on its own terminal.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

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

/// Asks on the terminal the broker was started from. A temporary stand-in for the biometric prompt.
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
            "{}\n(This is a plain terminal question until the Touch ID prompt exists.)\nAllow? [y/N] ",
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
    fn deny_all_denies() {
        assert_eq!(
            DenyAll.approve(&req("a", "b"), Duration::from_secs(1)),
            Decision::Deny
        );
    }
}
