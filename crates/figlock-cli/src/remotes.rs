//! Turning `--drive`, `--icloud` and `--sync-folder` into the remotes a device syncs to (ADR-0043), and the
//! iCloud-specific care around them: Apple-only, never create the folder before the other device's has arrived,
//! and wait for a folder that is still downloading instead of calling it empty.

use std::path::Path;
use std::time::{Duration, Instant};

use figlock_session::config::{Place, Remote};
use figlock_session::error::{Fail, FailKind, Res};
use figlock_session::sync::SyncError;
use figlock_session::{icloud, sync};

use crate::RemoteArgs;

const POLL: Duration = Duration::from_secs(2);

impl RemoteArgs {
    pub fn is_empty(&self) -> bool {
        !self.drive && !self.icloud && self.sync_folder.is_empty()
    }

    /// The remotes asked for, in the order Drive, iCloud, other folders. `create` makes `iCloud Drive/FigLock`
    /// (and its README) when it is not there; without it a missing folder is left missing.
    pub fn resolve(&self, create: bool) -> Res<Vec<Remote>> {
        let mut out = Vec::new();
        if self.drive {
            out.push(Remote::Drive);
        }
        if self.icloud {
            let folder = if create {
                icloud::prepare()?
            } else {
                icloud::folder()?
            };
            out.push(Remote::Folder(folder));
        }
        for p in &self.sync_folder {
            let full = std::path::absolute(p)
                .map_err(|_| Fail::new(format!("not a usable folder path: {}", p.display())))?;
            let r = Remote::Folder(full);
            if !out.contains(&r) {
                out.push(r);
            }
        }
        Ok(out)
    }
}

/// How the person's words for a remote read: "Google Drive", "iCloud Drive", or the folder.
pub fn label(remote: &Remote) -> String {
    match remote {
        Remote::Drive => "Google Drive".to_owned(),
        Remote::Folder(p) if icloud::is_icloud_path(p) => "iCloud Drive".to_owned(),
        Remote::Folder(p) => p.display().to_string(),
    }
}

/// `init --icloud` into a folder that is still receiving another device's vault (see
/// `icloud::refuse_partial_folder`).
pub fn refuse_partial_icloud_folder(remotes: &[Remote]) -> Res<()> {
    for r in remotes {
        if let Remote::Folder(p) = r {
            icloud::refuse_partial_folder(p)?;
        }
    }
    Ok(())
}

/// Before `join` or `open` from iCloud Drive: wait for the `FigLock` folder and its `vault.json` to show up.
/// A folder another device just made reaches this Mac in pieces, and "no vault found" would be wrong for it.
/// Folders outside iCloud Drive are not waited for.
pub fn wait_for_icloud(remotes: &[Remote], secs: u64) -> Res<()> {
    let folders: Vec<&Path> = remotes
        .iter()
        .filter_map(|r| match r {
            Remote::Folder(p) if icloud::is_icloud_path(p) => Some(p.as_path()),
            _ => None,
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut said = false;
    for folder in folders {
        loop {
            match icloud::check_arrived(folder) {
                Ok(()) => break,
                // Still arriving: wait, and say so once.
                Err(Fail(msg, FailKind::StorageSyncing)) => {
                    if Instant::now() >= deadline {
                        return Err(Fail::new(format!(
                            "{msg}. Run this again when it has finished, or use --wait-for-sync <seconds> to wait longer"
                        )));
                    }
                    if !said {
                        eprintln!(
                            "Waiting for iCloud to finish syncing the vault to this Mac. Press Ctrl-C to stop."
                        );
                        said = true;
                    }
                    std::thread::sleep(POLL);
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

/// `sync::open_from_remote` that waits while a file is listed but not downloaded yet (iCloud Drive on a Mac that
/// evicted it), instead of failing at the first try.
pub fn open_from_remote_waiting(place: &Place, secs: u64) -> Res<()> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut said = false;
    loop {
        match sync::open_from_remote(place) {
            Err(SyncError::NotDownloaded) if Instant::now() < deadline => {
                if !said {
                    eprintln!(
                        "Waiting for iCloud to finish syncing the vault to this Mac. Press Ctrl-C to stop."
                    );
                    said = true;
                }
                std::thread::sleep(POLL);
            }
            Err(SyncError::NotDownloaded) => {
                return Err(Fail::new(
                    "iCloud has not finished downloading the vault yet. Open iCloud Drive in Finder to see its \
                     progress, then run this again",
                ));
            }
            other => return other.map_err(Into::into),
        }
    }
}
