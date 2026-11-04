//! Local or synced folder adapter (`sync.md` 2).
//!
//! Works for a plain directory, an iCloud Drive folder, Dropbox or Syncthing. Writes go to a temporary file
//! in the same directory and are published with an atomic rename (replace) or an atomic no-clobber rename
//! (create-only), so readers never see a partial blob and two writers cannot both create the same path.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::path::{validate_path, validate_prefix};
use crate::{BlobStore, Entry, MAX_BLOB_BYTES, Result, StoreError};

/// A store rooted at a directory.
#[derive(Clone, Debug)]
pub struct LocalFolder {
    root: PathBuf,
}

impl LocalFolder {
    /// Open (creating if needed) a store at `root`. The root itself is resolved once; everything below it
    /// is checked on every operation.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(LocalFolder {
            root: fs::canonicalize(root.as_ref())?,
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
        walk(&start, dir_rel, 0, &mut out)?;
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

fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<Entry>) -> Result<()> {
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
                walk(&entry.path(), &join(name), depth + 1, out)?;
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
                out.push(Entry {
                    path: join(name),
                    size: meta.len(),
                    mtime: meta.modified().ok(),
                    available: true,
                });
            }
        }
    }
    Ok(())
}
