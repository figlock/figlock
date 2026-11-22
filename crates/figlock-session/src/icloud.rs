//! iCloud Drive as a plain synced folder (ADR-0043), shared by the CLI and the desktop app.
//!
//! There is no iCloud API here and nothing to sign in to: iCloud Drive is a folder on a Mac, `iCloud Drive` in
//! Finder, and the system syncs whatever is put in it. FigLock keeps its vault in a folder called `FigLock`
//! there and treats it like any other folder remote. This module knows where that folder is, whether iCloud
//! Drive is on, and what to say when it is not.
//!
//! `FIGLOCK_ICLOUD_DIR` stands in for the iCloud Drive folder, for tests and for trying the flow on a machine
//! without iCloud.

use std::path::{Path, PathBuf};

use crate::error::{Fail, FailKind, Res};

/// The folder FigLock uses inside iCloud Drive.
pub const FOLDER: &str = "FigLock";

/// Where macOS puts iCloud Drive, below the home folder.
const ROOT_BELOW_HOME: &str = "Library/Mobile Documents/com~apple~CloudDocs";

/// Written next to the vault so a person browsing iCloud Drive knows what the folder is.
const README: &str = "\
This folder holds your FigLock vault, so iCloud Drive can keep it in step on your devices.

Everything in it is encrypted. Please do not open, rename, move or delete these files, and do not put
anything else in this folder. To stop using it, turn iCloud Drive sync off in FigLock first.

https://figlock.com
";

/// Why iCloud Drive cannot be used here, in words for the person.
const NOT_APPLE: &str = "iCloud Drive is only on Apple devices, and this computer has none. Use Google Drive \
(`figlock drive login`), or `--sync-folder <path>` for another synced folder such as Dropbox or Syncthing.";

const TURN_ON: &str = "iCloud Drive is turned off or not signed in on this Mac. Open System Settings, click \
your name, choose iCloud, then Drive, and turn on \"Sync this Mac\". Then try again.";

/// What this machine can say about iCloud Drive without touching it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// This is an Apple computer, or `FIGLOCK_ICLOUD_DIR` is set.
    pub apple: bool,
    /// iCloud Drive is on and signed in: its folder is there.
    pub ready: bool,
    /// Where `iCloud Drive/FigLock` is (or would be), when iCloud Drive is ready.
    pub folder: Option<PathBuf>,
    /// The `FigLock` folder already exists.
    pub folder_exists: bool,
}

/// The iCloud Drive folder, whether or not it is there.
fn root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FIGLOCK_ICLOUD_DIR").filter(|v| !v.is_empty()) {
        return Some(dir.into());
    }
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME").map(|h| PathBuf::from(h).join(ROOT_BELOW_HOME));
    }
    None
}

pub fn status() -> Status {
    status_of(root())
}

fn status_of(root: Option<PathBuf>) -> Status {
    let Some(root) = root else {
        return Status {
            apple: false,
            ready: false,
            folder: None,
            folder_exists: false,
        };
    };
    // iCloud Drive off or signed out: the folder is gone (or empty of its marker folders and unreadable).
    let ready = root.is_dir() && std::fs::read_dir(&root).is_ok();
    let folder = ready.then(|| root.join(FOLDER));
    let folder_exists = folder.as_deref().is_some_and(Path::is_dir);
    Status {
        apple: true,
        ready,
        folder,
        folder_exists,
    }
}

/// The `FigLock` folder in iCloud Drive, or the reason iCloud Drive cannot be used.
pub fn folder() -> Res<PathBuf> {
    folder_of(status())
}

fn folder_of(s: Status) -> Res<PathBuf> {
    if !s.apple {
        return Err(Fail::new(NOT_APPLE));
    }
    s.folder.ok_or_else(|| Fail::new(TURN_ON))
}

/// [`folder`], made: creates `iCloud Drive/FigLock` and its README.txt when they are not there yet.
pub fn prepare() -> Res<PathBuf> {
    prepare_in(folder()?)
}

fn prepare_in(folder: PathBuf) -> Res<PathBuf> {
    std::fs::create_dir_all(&folder)
        .map_err(|e| Fail::new(format!("could not create {}: {e}", folder.display())))?;
    // Only when absent: a README another device already put there is left alone, so two devices do not
    // fight over a file iCloud would keep a conflict copy of.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(folder.join("README.txt"))
    {
        Ok(mut f) => {
            let _ = std::io::Write::write_all(&mut f, README.as_bytes());
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        // A folder that cannot hold a README is still a usable vault folder.
        Err(_) => {}
    }
    Ok(folder)
}

/// Whether `path` is inside iCloud Drive (so it may still be downloading, and must never be created by us
/// before the other device's copy of the same folder has arrived).
pub fn is_icloud_path(path: &Path) -> bool {
    is_icloud_path_of(root().as_deref(), path)
}

fn is_icloud_path_of(root: Option<&Path>, path: &Path) -> bool {
    if let Some(root) = root
        && path.starts_with(root)
    {
        return true;
    }
    path.components()
        .any(|c| c.as_os_str() == "com~apple~CloudDocs")
}

/// What an iCloud remote that has gone missing means, in words: iCloud Drive off, or the folder not synced yet.
pub fn missing_message(folder: &Path) -> String {
    missing_message_of(status(), folder)
}

fn missing_message_of(s: Status, folder: &Path) -> String {
    if !s.apple {
        NOT_APPLE.to_owned()
    } else if !s.ready {
        TURN_ON.to_owned()
    } else {
        format!(
            "the iCloud Drive folder {} is not there. It may still be syncing from another device; wait for iCloud \
             to finish and try again",
            folder.display()
        )
    }
}

/// A `FigLock` folder that holds files but no `vault.json` is a vault still arriving from another device, not an
/// empty folder: creating a second vault there would split the two. Fails for such a folder.
pub fn refuse_partial_folder(folder: &Path) -> Res<()> {
    if !is_icloud_path(folder) || !folder.is_dir() {
        return Ok(());
    }
    let mut has_vault = false;
    let mut other = false;
    if let Ok(rd) = std::fs::read_dir(folder) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            match name.as_str() {
                "vault.json" | ".vault.json.icloud" => has_vault = true,
                "README.txt" | ".DS_Store" | ".localized" => {}
                n if n.starts_with('.') && !n.ends_with(".icloud") => {}
                _ => other = true,
            }
        }
    }
    if other && !has_vault {
        return Err(Fail::new(format!(
            "{} already holds FigLock files but no vault yet. They may still be arriving from another device \
             through iCloud. Wait for iCloud to finish, then join or open that vault instead of creating a \
             second one",
            folder.display()
        )));
    }
    Ok(())
}

/// Whether `vault.json` (or iCloud's placeholder for it) is in `folder` yet.
pub fn vault_has_arrived(folder: &Path) -> bool {
    folder.join("vault.json").exists() || folder.join(".vault.json.icloud").exists()
}

/// Before opening or joining a vault from an iCloud `folder`: it must be there with its `vault.json`. Not yet is
/// [`FailKind::StorageSyncing`] (iCloud is still delivering it: wait and ask again), never "no vault found".
/// iCloud Drive off or missing is a plain failure that says what to switch on.
pub fn check_arrived(folder: &Path) -> Res<()> {
    let s = status();
    if !s.apple || !s.ready {
        return Err(Fail::new(missing_message(folder)));
    }
    if folder.is_dir() && vault_has_arrived(folder) {
        return Ok(());
    }
    Err(Fail::of(
        FailKind::StorageSyncing,
        format!(
            "the vault has not arrived from iCloud Drive yet (looked in {}). iCloud is still syncing it to this \
             Mac; open iCloud Drive in Finder to see its progress",
            folder.display()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icloud_drive_off_says_what_to_switch_on() {
        let d = tempfile::tempdir().unwrap();
        let s = status_of(Some(d.path().join("no-icloud")));
        assert!(s.apple && !s.ready && s.folder.is_none());
        let Err(Fail(m, _)) = folder_of(s.clone()) else {
            panic!("must not resolve")
        };
        assert!(m.contains("System Settings") && m.contains("iCloud"), "{m}");
        assert_eq!(missing_message_of(s, Path::new("/x")), m);
    }

    #[test]
    fn a_computer_without_icloud_says_so() {
        let s = status_of(None);
        assert!(!s.apple && !s.ready);
        let Err(Fail(m, _)) = folder_of(s) else {
            panic!("must not resolve")
        };
        assert!(
            m.contains("only on Apple devices") && m.contains("--sync-folder"),
            "{m}"
        );
    }

    #[test]
    fn prepare_makes_the_folder_and_a_readme_once() {
        let d = tempfile::tempdir().unwrap();
        let s = status_of(Some(d.path().to_path_buf()));
        assert!(s.ready && !s.folder_exists);
        let f = prepare_in(folder_of(s).unwrap()).unwrap();
        assert_eq!(f, d.path().join("FigLock"));
        assert!(status_of(Some(d.path().to_path_buf())).folder_exists);
        let readme = std::fs::read_to_string(f.join("README.txt")).unwrap();
        assert!(readme.contains("do not open, rename, move or delete"));
        // A README that is already there is left as it is.
        std::fs::write(f.join("README.txt"), "other").unwrap();
        prepare_in(f.clone()).unwrap();
        assert_eq!(
            std::fs::read_to_string(f.join("README.txt")).unwrap(),
            "other"
        );
    }

    #[test]
    fn a_path_in_icloud_drive_is_recognised() {
        let d = tempfile::tempdir().unwrap();
        assert!(is_icloud_path_of(Some(d.path()), &d.path().join("FigLock")));
        assert!(!is_icloud_path_of(
            Some(d.path()),
            Path::new("/somewhere/else")
        ));
        assert!(is_icloud_path_of(
            None,
            Path::new("/Users/x/Library/Mobile Documents/com~apple~CloudDocs/FigLock")
        ));
    }
}
