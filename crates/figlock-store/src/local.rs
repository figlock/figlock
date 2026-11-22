//! Local or synced folder adapter (`sync.md` 2).
//!
//! Works for a plain directory, an iCloud Drive folder, Dropbox or Syncthing. Writes go to a temporary file
//! in the same directory and are published with an atomic rename (replace) or an atomic no-clobber rename
//! (create-only), so readers never see a partial blob and two writers cannot both create the same path.

use std::fmt::Debug;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::path::{validate_path, validate_prefix};
use crate::{BlobStore, Entry, MAX_BLOB_BYTES, Result, StoreError};

/// Whether a file's bytes are on this device, and how to ask the OS to bring them (FIG-195).
///
/// Since macOS 14 iCloud Drive runs on File Provider: an evicted file keeps its real name and only carries the
/// `SF_DATALESS` flag in `st_flags`, so the hidden `.name.icloud` stub is gone there. Such a file is "not
/// downloaded yet", never missing. A trait so tests can fake the flag on any platform.
pub trait Materialize: Send + Sync + Debug {
    /// The file at `path` exists but its bytes are not on this device.
    fn is_dataless(&self, path: &Path, meta: &fs::Metadata) -> bool;
    /// Ask the OS to download `path` in the background. Best effort and never blocks on the transfer.
    fn request_download(&self, path: &Path);
}

/// The real thing: `SF_DATALESS` on macOS, nothing is ever dataless elsewhere.
#[derive(Debug, Default)]
struct Os;

/// `SF_DATALESS` from `<sys/stat.h>`.
#[cfg(target_os = "macos")]
const SF_DATALESS: u32 = 0x4000_0000;

impl Materialize for Os {
    #[cfg(target_os = "macos")]
    fn is_dataless(&self, _path: &Path, meta: &fs::Metadata) -> bool {
        use std::os::macos::fs::MetadataExt;
        meta.st_flags() & SF_DATALESS != 0
    }

    #[cfg(not(target_os = "macos"))]
    fn is_dataless(&self, _path: &Path, _meta: &fs::Metadata) -> bool {
        false
    }

    /// Foundation's `startDownloadingUbiquitousItem` works from an unsigned binary (`brctl download` was
    /// removed in macOS 14). It fails for a file that is not in an iCloud container, which is fine: then
    /// there is nothing to download and the file stays "not downloaded yet".
    #[cfg(target_os = "macos")]
    fn request_download(&self, path: &Path) {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        let Some(path) = path.to_str() else { return };
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let _ = NSFileManager::defaultManager().startDownloadingUbiquitousItemAtURL_error(&url);
    }

    #[cfg(not(target_os = "macos"))]
    fn request_download(&self, _path: &Path) {}
}

/// A store rooted at a directory.
#[derive(Clone, Debug)]
pub struct LocalFolder {
    root: PathBuf,
    files: Arc<dyn Materialize>,
}

impl LocalFolder {
    /// Open (creating if needed) a store at `root`. The root itself is resolved once; everything below it
    /// is checked on every operation.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(root, Arc::new(Os))
    }

    /// [`LocalFolder::open`] with its own idea of which files are not downloaded (tests).
    pub fn open_with(root: impl AsRef<Path>, files: Arc<dyn Materialize>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(LocalFolder {
            root: fs::canonicalize(root.as_ref())?,
            files,
        })
    }

    /// Resolve `rel` under the root, refusing any symlink on the way (`rel` is already validated).
    fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let mut p = self.root.clone();
        for seg in rel.split('/') {
            p.push(seg);
            match fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_symlink() => return Err(StoreError::InvalidPath),
                Ok(_) | Err(_) => {}
            }
        }
        Ok(p)
    }

    fn placeholder(path: &Path) -> Option<PathBuf> {
        let name = path.file_name()?.to_str()?;
        Some(path.with_file_name(format!(".{name}.icloud")))
    }

    fn write_temp(dir: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
        let mut tmp = tempfile::Builder::new().prefix(".tmp-").tempfile_in(dir)?;
        tmp.write_all(bytes)?;
        tmp.as_file().sync_all()?;
        Ok(tmp)
    }

    fn sync_dir(dir: &Path) {
        // Best effort durability of the rename; not available or needed on every platform.
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }

    fn prepare_parent(&self, rel: &str) -> Result<(PathBuf, PathBuf)> {
        validate_path(rel)?;
        let dest = self.resolve(rel)?;
        let dir = dest.parent().ok_or(StoreError::InvalidPath)?.to_path_buf();
        fs::create_dir_all(&dir)?;
        // Re-check after creating directories: a racing symlink must not redirect the write.
        if fs::canonicalize(&dir)?.strip_prefix(&self.root).is_err() {
            return Err(StoreError::InvalidPath);
        }
        Ok((dest, dir))
    }
}

impl BlobStore for LocalFolder {
    fn get(&self, path: &str) -> Result<Vec<u8>> {
        validate_path(path)?;
        let p = self.resolve(path)?;
        match fs::symlink_metadata(&p) {
            Ok(m) if m.is_file() => {
                if self.files.is_dataless(&p, &m) {
                    // Reading it would block on a download, or fail. Ask for the bytes and say "not yet".
                    self.files.request_download(&p);
                    return Err(StoreError::NotYetAvailable);
                }
                if m.len() > MAX_BLOB_BYTES {
                    return Err(StoreError::TooLarge);
                }
            }
            Ok(_) => return Err(StoreError::InvalidPath),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(match Self::placeholder(&p) {
                    Some(ph) if ph.exists() => StoreError::NotYetAvailable,
                    _ => StoreError::NotFound,
                });
            }
            Err(e) => return Err(e.into()),
        }
        let mut buf = Vec::new();
        fs::File::open(&p)?
            .take(MAX_BLOB_BYTES + 1)
            .read_to_end(&mut buf)?;
        if buf.len() as u64 > MAX_BLOB_BYTES {
            return Err(StoreError::TooLarge);
        }
        Ok(buf)
    }

    fn put_new(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() as u64 > MAX_BLOB_BYTES {
            return Err(StoreError::TooLarge);
        }
        let (dest, dir) = self.prepare_parent(path)?;
        // A placeholder means the blob exists in the cloud but is not here yet.
        if Self::placeholder(&dest).is_some_and(|ph| ph.exists()) {
            return Err(StoreError::Exists);
        }
        let tmp = Self::write_temp(&dir, bytes)?;
        match tmp.persist_noclobber(&dest) {
            Ok(_) => {
                Self::sync_dir(&dir);
                Ok(())
            }
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(StoreError::Exists)
            }
            Err(e) => Err(e.error.into()),
        }
    }

    fn put_replace(&self, path: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() as u64 > MAX_BLOB_BYTES {
            return Err(StoreError::TooLarge);
        }
        let (dest, dir) = self.prepare_parent(path)?;
        if fs::symlink_metadata(&dest).is_ok_and(|m| !m.is_file()) {
            return Err(StoreError::InvalidPath);
        }
        Self::write_temp(&dir, bytes)?
            .persist(&dest)
            .map_err(|e| StoreError::from(e.error))?;
        Self::sync_dir(&dir);
        Ok(())
    }

    fn list(&self, prefix: &str) -> Result<Vec<Entry>> {
        validate_prefix(prefix)?;
        let (dir_rel, _) = prefix.rsplit_once('/').unwrap_or(("", prefix));
        let start = if dir_rel.is_empty() {
            self.root.clone()
        } else {
            self.resolve(dir_rel)?
        };
        let mut out = Vec::new();
        walk(&start, dir_rel, 0, self.files.as_ref(), &mut out)?;
        out.retain(|e| e.path.starts_with(prefix));
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn delete(&self, path: &str) -> Result<()> {
        validate_path(path)?;
        let p = self.resolve(path)?;
        for target in [Some(p.clone()), Self::placeholder(&p)]
            .into_iter()
            .flatten()
        {
            match fs::symlink_metadata(&target) {
                Ok(m) if m.is_file() => fs::remove_file(&target)?,
                Ok(_) => return Err(StoreError::InvalidPath),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

fn walk(
    dir: &Path,
    rel: &str,
    depth: usize,
    files: &dyn Materialize,
    out: &mut Vec<Entry>,
) -> Result<()> {
    if depth > 4 {
        return Ok(());
    }
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in read {
        let entry = match entry {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let meta = match fs::symlink_metadata(entry.path()) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let join = |n: &str| {
            if rel.is_empty() {
                n.to_owned()
            } else {
                format!("{rel}/{n}")
            }
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if !name.starts_with('.') && validate_path(&join(name)).is_ok() {
                walk(&entry.path(), &join(name), depth + 1, files, out)?;
            }
        } else if meta.is_file() {
            if let Some(real) = name
                .strip_prefix('.')
                .and_then(|n| n.strip_suffix(".icloud"))
            {
                // iCloud Drive placeholder for a file that is not downloaded yet.
                if validate_path(&join(real)).is_ok() {
                    out.push(Entry {
                        path: join(real),
                        size: 0,
                        mtime: None,
                        available: false,
                    });
                }
            } else if !name.starts_with('.') && validate_path(&join(name)).is_ok() {
                // macOS 14+: an evicted file keeps its name and only carries the dataless flag. Listed, so it
                // is never a gap, but not available until the OS has downloaded it.
                let available = !files.is_dataless(&entry.path(), &meta);
                if !available {
                    files.request_download(&entry.path());
                }
                out.push(Entry {
                    path: join(name),
                    size: meta.len(),
                    mtime: meta.modified().ok(),
                    available,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    use super::*;

    /// Files named in `evicted` count as dataless, and every download request is recorded.
    #[derive(Debug, Default)]
    struct Fake {
        evicted: Mutex<BTreeSet<String>>,
        asked: Mutex<Vec<String>>,
    }

    impl Fake {
        fn name(path: &Path) -> String {
            path.file_name().unwrap().to_string_lossy().into_owned()
        }
    }

    impl Materialize for Fake {
        fn is_dataless(&self, path: &Path, _meta: &fs::Metadata) -> bool {
            self.evicted.lock().unwrap().contains(&Self::name(path))
        }
        fn request_download(&self, path: &Path) {
            self.asked.lock().unwrap().push(Self::name(path));
        }
    }

    fn folder() -> (tempfile::TempDir, LocalFolder, Arc<Fake>) {
        let d = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let s = LocalFolder::open_with(d.path(), fake.clone()).unwrap();
        (d, s, fake)
    }

    fn evict(fake: &Fake, name: &str) {
        fake.evicted.lock().unwrap().insert(name.to_owned());
    }

    #[test]
    fn a_dataless_file_is_not_downloaded_yet_never_missing() {
        let (_d, s, fake) = folder();
        s.put_replace("log/aa/1", b"op").unwrap();
        evict(&fake, "1");
        assert_eq!(s.get("log/aa/1"), Err(StoreError::NotYetAvailable));
        assert_ne!(s.get("log/aa/1"), Err(StoreError::NotFound));
        assert_eq!(*fake.asked.lock().unwrap(), ["1", "1"]);
    }

    #[test]
    fn a_dataless_file_is_listed_as_unavailable_and_a_download_is_requested() {
        let (_d, s, fake) = folder();
        s.put_replace("log/aa/1", b"op").unwrap();
        s.put_replace("log/aa/2", b"op2").unwrap();
        evict(&fake, "1");
        let l = s.list("log/").unwrap();
        let by = |p: &str| l.iter().find(|e| e.path == p).unwrap();
        assert!(!by("log/aa/1").available);
        assert_eq!(by("log/aa/1").size, 2);
        assert!(by("log/aa/2").available);
        assert_eq!(*fake.asked.lock().unwrap(), ["1"]);
    }

    #[test]
    fn a_file_that_arrives_reads_normally() {
        let (_d, s, fake) = folder();
        s.put_replace("log/aa/1", b"op").unwrap();
        evict(&fake, "1");
        assert!(s.get("log/aa/1").is_err());
        fake.evicted.lock().unwrap().clear();
        assert_eq!(s.get("log/aa/1").unwrap(), b"op");
    }

    #[test]
    fn create_only_never_overwrites_a_dataless_file() {
        let (_d, s, fake) = folder();
        s.put_replace("log/aa/1", b"op").unwrap();
        evict(&fake, "1");
        assert_eq!(s.put_new("log/aa/1", b"other"), Err(StoreError::Exists));
    }

    #[test]
    fn a_missing_file_is_still_missing() {
        let (_d, s, fake) = folder();
        evict(&fake, "2");
        assert_eq!(s.get("log/aa/2"), Err(StoreError::NotFound));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_os_source_sees_an_ordinary_file_as_present() {
        let d = tempfile::tempdir().unwrap();
        let s = LocalFolder::open(d.path()).unwrap();
        s.put_replace("log/aa/1", b"op").unwrap();
        assert_eq!(s.get("log/aa/1").unwrap(), b"op");
        assert!(s.list("log/").unwrap()[0].available);
    }
}
