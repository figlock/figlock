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

/// The program the OS says `pid` is running, as an absolute path, if it can be found (FIG-190).
fn exe_of(pid: u32) -> Option<String> {
    figlock_procinfo::exe_path(pid)
}

/// The parent of `pid`, if the OS says.
fn parent_of(pid: u32) -> Option<u32> {
    figlock_procinfo::parent_pid(pid)
}

/// At most this many ancestors are looked up. Enough to see the program behind several shells (FIG-191); the
/// prompt shows the nearest few.
const MAX_PARENTS: usize = 8;

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

impl Default for SystemEnv {
    fn default() -> Self {
        Self::new()
    }
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
        // What the system says, never the name the process was started under (FIG-190).
        assert!(
            peer.exe.as_deref().is_some_and(|e| e.starts_with('/')),
            "{:?}",
            peer.exe
        );
        assert!(
            !peer.parents.is_empty() && peer.parents.iter().all(|p| p.starts_with('/')),
            "{:?}",
            peer.parents
        );
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
