//! Who a process is, asked of the operating system (FIG-190). The broker shows the person which program is
//! asking and which programs started it, and uses the same answers to tell one program from another, so they
//! must be what the system says: an absolute path, never the name a process was started under.

/// The absolute path of the program `pid` is running, or `None` when the system will not say (the process is
/// gone, belongs to someone else, or this platform has no way to ask).
pub fn exe_path(pid: u32) -> Option<String> {
    let path = imp::exe_path(pid)?;
    path.starts_with('/').then_some(path)
}

/// The parent of `pid`. Never `1` or `0`: those are the system, not a program that started this one.
pub fn parent_pid(pid: u32) -> Option<u32> {
    imp::parent_pid(pid).filter(|p| *p > 1)
}

#[cfg(target_vendor = "apple")]
#[allow(
    unsafe_code,
    reason = "calls proc_pidpath and proc_pidinfo, which fill buffers this code sizes"
)]
mod imp {
    use std::mem::size_of;

    pub fn exe_path(pid: u32) -> Option<String> {
        let pid = i32::try_from(pid).ok()?;
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let size = u32::try_from(buf.len()).ok()?;
        // SAFETY: `buf` is valid for `size` bytes and the call writes at most that many.
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), size) };
        let n = usize::try_from(n).ok().filter(|n| *n > 0)?;
        buf.truncate(n);
        String::from_utf8(buf).ok()
    }

    pub fn parent_pid(pid: u32) -> Option<u32> {
        let pid = i32::try_from(pid).ok()?;
        // SAFETY: an all-zero `proc_bsdinfo` is a valid value for this plain C struct.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let want = i32::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
        // SAFETY: `info` is valid for `want` bytes and the call writes at most that many.
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                std::ptr::addr_of_mut!(info).cast(),
                want,
            )
        };
        (got == want).then_some(info.pbi_ppid)
    }
}

#[cfg(all(unix, not(target_vendor = "apple")))]
mod imp {
    pub fn exe_path(pid: u32) -> Option<String> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    pub fn parent_pid(pid: u32) -> Option<u32> {
        // `/proc/<pid>/stat` is `pid (comm) state ppid ...`; the name may hold spaces and parentheses, so
        // read from the last closing one.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after = stat.get(stat.rfind(')')? + 1..)?;
        after.split_whitespace().nth(1)?.parse().ok()
    }
}

#[cfg(not(unix))]
mod imp {
    pub fn exe_path(_: u32) -> Option<String> {
        None
    }

    pub fn parent_pid(_: u32) -> Option<u32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_an_absolute_path_and_its_parent_is_found() {
        let me = std::process::id();
        let path = exe_path(me).expect("the system names this process's program");
        assert!(path.starts_with('/'), "{path}");
        assert!(
            std::env::current_exe().unwrap().canonicalize().unwrap()
                == std::path::Path::new(&path).canonicalize().unwrap(),
            "{path}"
        );
        let parent = parent_pid(me).expect("a test runs under a parent");
        assert!(parent > 1 && parent != me);
    }

    #[test]
    fn a_process_that_does_not_exist_has_no_path_and_no_parent() {
        assert_eq!(exe_path(u32::MAX), None);
        assert_eq!(parent_pid(u32::MAX), None);
    }

    #[test]
    fn a_child_started_by_a_relative_path_still_reports_an_absolute_one() {
        // The case that showed a relative path on the approval card: the program was started as `./true`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy("/bin/sleep", dir.path().join("napper")).unwrap();
        let mut child = std::process::Command::new("./napper")
            .arg("5")
            .current_dir(dir.path())
            .spawn()
            .unwrap();
        let path = exe_path(child.id()).expect("the system names the child's program");
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(path.starts_with('/'), "{path}");
        assert!(path.ends_with("/napper"), "{path}");
    }
}
