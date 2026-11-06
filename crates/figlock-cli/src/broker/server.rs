//! The broker's Unix socket (`docs/spec/broker.md` 2): a private directory, a `0600` socket, the peer's
//! user id checked on every connection, and small bounded reads.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use super::Broker;
use super::peer;
use super::proto::{Code, MAX_LINE, err};
use crate::config;
use crate::error::{Fail, Res};

const MAX_CONNECTIONS: usize = 8;
const IDLE: Duration = Duration::from_secs(30);
/// `sun_path` is 104 bytes on macOS and the BSDs, 108 on Linux; stay under the smaller.
const MAX_SOCKET_PATH: usize = 100;

/// The directory holding the socket. An explicit `FIGLOCK_HOME` wins (tests and portable setups), then
/// `$XDG_RUNTIME_DIR/figlock`, then `<home>/run`.
pub fn run_dir() -> Res<PathBuf> {
    use std::env::var_os;
    if var_os("FIGLOCK_HOME").is_some_and(|v| !v.is_empty()) {
        return Ok(config::home()?.join("run"));
    }
    if let Some(x) = var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(x).join("figlock"));
    }
    Ok(config::home()?.join("run"))
}

pub fn socket_path() -> Res<PathBuf> {
    Ok(run_dir()?.join("broker.sock"))
}

/// The broker's folder must belong to this user and be closed to everyone else.
fn dir_is_private(owner: u32, mode: u32, me: u32) -> bool {
    owner == me && mode & 0o077 == 0
}

/// Create the socket: refuse an unsafe directory, replace a dead socket, never replace a live one.
pub fn bind(path: &Path) -> Res<UnixListener> {
    if path.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(Fail::new(
            "the socket path is too long; set FIGLOCK_HOME to a shorter folder",
        ));
    }
    let dir = path.parent().ok_or_else(|| Fail::new("bad socket path"))?;
    config::private_dir(dir)?;
    let meta = fs::metadata(dir)?;
    if !dir_is_private(meta.uid(), meta.permissions().mode(), peer::my_uid()) {
        return Err(Fail::new(
            "the broker's folder is not private to you; fix it or set FIGLOCK_HOME",
        ));
    }
    if let Ok(existing) = fs::symlink_metadata(path) {
        if !existing.file_type().is_socket() {
            return Err(Fail::new("something else is at the broker's socket path"));
        }
        match UnixStream::connect(path) {
            Ok(_) => return Err(Fail::new("a broker is already running")),
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => fs::remove_file(path)?,
            Err(e) => return Err(e.into()),
        }
    }
    // The folder was just checked to be private to this user, so nobody else can reach the socket in the
    // moment before it is tightened. No process-wide umask change: it would hand other threads files and
    // folders without their owner's execute bit.
    let bound = UnixListener::bind(path);
    let listener = bound?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    if fs::metadata(path)?.permissions().mode() & 0o777 != 0o600 {
        let _ = fs::remove_file(path);
        return Err(Fail::new("could not make the socket private"));
    }
    Ok(listener)
}

pub enum Line {
    Eof,
    Text(String),
    /// Longer than the limit, or not UTF-8: the stream cannot be trusted to resynchronise.
    Bad,
}

/// One newline-terminated line of at most `max` bytes, without reading past the limit.
pub fn read_line(r: &mut impl BufRead, max: usize) -> io::Result<Line> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(max as u64 + 2)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(Line::Eof);
    }
    if buf.last() != Some(&b'\n') {
        // No newline within the limit (or the peer closed mid-line).
        return Ok(if buf.len() > max {
            Line::Bad
        } else {
            Line::Eof
        });
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    if buf.len() > max {
        return Ok(Line::Bad);
    }
    Ok(String::from_utf8(buf).map_or(Line::Bad, Line::Text))
}

fn write_line(w: &mut impl Write, line: &str) -> io::Result<()> {
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

fn connection(stream: UnixStream, who: peer::PeerId, broker: &Broker, stop: &AtomicBool) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut writer = stream;
    let _ = writer.set_write_timeout(Some(IDLE));
    let mut reader = BufReader::new(read_half);
    loop {
        match read_line(&mut reader, MAX_LINE) {
            Ok(Line::Text(line)) => {
                let reply = broker.handle_line(&who, &line);
                if write_line(&mut writer, &reply.line).is_err() {
                    return;
                }
                if reply.exit {
                    stop.store(true, Ordering::SeqCst);
                    return;
                }
            }
            Ok(Line::Bad) => {
                let _ = write_line(&mut writer, &err("", Code::BadRequest));
                return;
            }
            Ok(Line::Eof) | Err(_) => return,
        }
    }
}

/// Serve until a client locks the broker. Each connection gets a thread; at most [`MAX_CONNECTIONS`].
pub fn serve(listener: &UnixListener, broker: &Arc<Broker>) -> Res<()> {
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::SeqCst) {
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        // Accepted sockets can inherit the listener's non-blocking mode.
        if stream.set_nonblocking(false).is_err() {
            continue;
        }
        // If the system cannot say who this is, they are not served.
        let Ok(who) = peer::identify(&stream) else {
            continue;
        };
        if !broker.admit(&who) {
            continue;
        }
        let _ = stream.set_read_timeout(Some(IDLE));
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            let mut s = stream;
            let _ = write_line(&mut s, &err("", Code::Busy));
            continue;
        }
        let (broker, stop, active) = (broker.clone(), stop.clone(), active.clone());
        std::thread::spawn(move || {
            connection(stream, who, &broker, &stop);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn short_dir() -> tempfile::TempDir {
        // Unix socket paths are short; the default temp folder on macOS is not.
        tempfile::Builder::new()
            .prefix("fl")
            .tempdir_in("/tmp")
            .unwrap()
    }

    #[test]
    fn the_socket_is_private_and_so_is_its_folder() {
        let tmp = short_dir();
        let path = tmp.path().join("run").join("b.sock");
        let _l = bind(&path).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn a_loose_folder_is_tightened() {
        let tmp = short_dir();
        let dir = tmp.path().join("run");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
        let _l = bind(&dir.join("b.sock")).unwrap();
        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn a_folder_of_another_user_or_open_to_others_is_not_acceptable() {
        // A second user cannot be created in CI, so the rule is checked on its inputs.
        assert!(dir_is_private(501, 0o700, 501));
        assert!(dir_is_private(501, 0o500, 501));
        assert!(!dir_is_private(0, 0o700, 501), "owned by someone else");
        assert!(!dir_is_private(501, 0o750, 501), "group can enter");
        assert!(!dir_is_private(501, 0o705, 501), "others can enter");
        assert!(!dir_is_private(501, 0o777, 501));
    }

    #[test]
    fn a_stale_socket_is_replaced_a_live_one_and_a_stranger_are_not() {
        let tmp = short_dir();
        let path = tmp.path().join("run").join("b.sock");
        let first = bind(&path).unwrap();
        // A live broker answers: refuse.
        let e = bind(&path).err().unwrap();
        assert!(e.0.contains("already running"), "{e}");
        // Dead: replaced.
        drop(first);
        let _again = bind(&path).unwrap();
        // Something that is not a socket is never deleted.
        let other = tmp.path().join("run").join("file");
        fs::write(&other, b"x").unwrap();
        assert!(bind(&other).is_err());
        assert_eq!(fs::read(&other).unwrap(), b"x");
    }

    #[test]
    fn long_paths_are_refused_with_a_hint() {
        let long = PathBuf::from(format!("/tmp/{}/b.sock", "x".repeat(120)));
        assert!(bind(&long).err().unwrap().0.contains("too long"));
    }

    #[test]
    fn lines_are_bounded_and_strict() {
        let mut ok = Cursor::new(b"hello\nworld\r\n".to_vec());
        assert!(matches!(read_line(&mut ok, 100).unwrap(), Line::Text(t) if t == "hello"));
        assert!(matches!(read_line(&mut ok, 100).unwrap(), Line::Text(t) if t == "world"));
        assert!(matches!(read_line(&mut ok, 100).unwrap(), Line::Eof));
        let mut long = Cursor::new(vec![b'a'; 500]);
        assert!(
            matches!(read_line(&mut long, 100).unwrap(), Line::Bad),
            "no newline within the limit"
        );
        let mut long = Cursor::new([vec![b'a'; 101], b"\n".to_vec()].concat());
        assert!(matches!(read_line(&mut long, 100).unwrap(), Line::Bad));
        let mut exact = Cursor::new([vec![b'a'; 100], b"\n".to_vec()].concat());
        assert!(matches!(read_line(&mut exact, 100).unwrap(), Line::Text(t) if t.len() == 100));
        let mut utf = Cursor::new(vec![0xff, 0xfe, b'\n']);
        assert!(matches!(read_line(&mut utf, 100).unwrap(), Line::Bad));
        let mut cut = Cursor::new(b"no newline".to_vec());
        assert!(matches!(read_line(&mut cut, 100).unwrap(), Line::Eof));
    }
}
