//! Who is on the other end of the socket (`docs/spec/broker.md` 2): user id and process id from the
//! operating system, and the program path the OS reports for that process. A name the caller claims is a
//! separate, untrusted field.

use std::io;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use super::grants::Env;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerId {
    pub uid: u32,
    pub pid: Option<u32>,
    pub exe: Option<String>,
    /// Programs that started this one, nearest first (best effort, display only). Through the `figlock`
    /// command the peer is always `figlock`; this is what names the agent behind it.
    pub parents: Vec<String>,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn credentials(stream: &UnixStream) -> io::Result<(u32, Option<u32>)> {
    use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
    let c = getsockopt(stream, PeerCredentials).map_err(io::Error::from)?;
    Ok((c.uid(), u32::try_from(c.pid()).ok().filter(|p| *p != 0)))
}

#[cfg(target_vendor = "apple")]
fn credentials(stream: &UnixStream) -> io::Result<(u32, Option<u32>)> {
    use nix::sys::socket::{
        getsockopt,
        sockopt::{LocalPeerCred, LocalPeerPid},
    };
    let uid = getsockopt(stream, LocalPeerCred)
        .map_err(io::Error::from)?
        .uid();
    let pid = getsockopt(stream, LocalPeerPid)
        .ok()
        .and_then(|p| u32::try_from(p).ok())
        .filter(|p| *p != 0);
    Ok((uid, pid))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn credentials(_: &UnixStream) -> io::Result<(u32, Option<u32>)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer credentials are not available on this system",
    ))
}

/// The program the OS says `pid` is running, if it can be found. Best effort and display only.
fn exe_of(pid: u32) -> Option<String> {
    let linux = std::fs::read_link(format!("/proc/{pid}/exe")).ok();
    let path = linux
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| {
            let out = std::process::Command::new("/bin/ps")
                .args(["-p", &pid.to_string(), "-o", "comm="])
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        })?;
    (!path.is_empty()).then_some(path)
}

/// The parent of `pid`, if the OS says. Display only.
fn parent_of(pid: u32) -> Option<u32> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "ppid="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let ppid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    (ppid > 1).then_some(ppid)
}

/// At most this many ancestors are looked up for the prompt.
const MAX_PARENTS: usize = 3;

fn parents_of(pid: u32) -> Vec<String> {
    let mut found = Vec::new();
    let mut at = pid;
    while found.len() < MAX_PARENTS {
        let Some(parent) = parent_of(at) else { break };
        let Some(exe) = exe_of(parent) else { break };
        found.push(exe);
        at = parent;
    }
    found
}

/// Read the peer's identity. Fails closed: if the OS cannot say who it is, the connection is refused.
pub fn identify(stream: &UnixStream) -> io::Result<PeerId> {
    let (uid, pid) = credentials(stream)?;
    Ok(PeerId {
        uid,
        pid,
        exe: pid.and_then(exe_of),
        parents: pid.map(parents_of).unwrap_or_default(),
    })
}

/// The effective user id of this process.
pub fn my_uid() -> u32 {
    nix::unistd::geteuid().as_raw()
}

pub struct SystemEnv {
    start: Instant,
}

impl SystemEnv {
    pub fn new() -> Self {
        SystemEnv {
            start: Instant::now(),
        }
    }
}

impl Env for SystemEnv {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn unix_ms(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    fn pid_alive(&self, pid: u32) -> bool {
        use nix::errno::Errno;
        use nix::sys::signal::kill;
        use nix::unistd::Pid;
        let Ok(raw) = i32::try_from(pid) else {
            return false;
        };
        // Signal 0 only checks that the process exists; EPERM means it exists but is not ours.
        match kill(Pid::from_raw(raw), None) {
            Ok(()) | Err(Errno::EPERM) => true,
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn a_connected_peer_is_identified_as_this_process() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s");
        let listener = UnixListener::bind(&path).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        let peer = identify(&server_side).unwrap();
        assert_eq!(peer.uid, my_uid());
        assert_eq!(peer.pid, Some(std::process::id()));
        drop(client);
    }

    #[test]
    fn liveness_of_pids() {
        let env = SystemEnv::new();
        assert!(env.pid_alive(std::process::id()));
        assert!(!env.pid_alive(u32::MAX), "not a valid pid");
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("0")
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!env.pid_alive(pid), "a reaped process is gone");
    }
}
