//! The `figlock` command line tool that ships inside the app (FIG-110). The person can link it onto their PATH
//! from Settings. This only ever creates or removes one symlink the person asked for, in `~/.local/bin`: it
//! never edits a shell profile or PATH, never needs admin rights, and never touches a file or link it did not
//! make. The functions take their paths as arguments so the rules are tested without a real app bundle.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{CmdError, CmdRes};

const NAME: &str = "figlock";

/// Where the link goes, relative to the home folder.
const LINK_DIR: &str = ".local/bin";

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// This build has no command line tool inside it (a development build).
    Unavailable,
    NotInstalled,
    Installed,
    /// The link is ours but points at another copy of the app, for example after the app was moved.
    Moved,
    /// Something else is at the link's path; it is left alone.
    Conflict,
    /// A `figlock` that is not from this app is already installed elsewhere (Homebrew, the install script), so
    /// a second copy is not added.
    External,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CliToolView {
    pub state: LinkState,
    /// The link's path as the person would type it.
    pub link: String,
    /// The folder to put on PATH if `figlock` is not found in a new Terminal window.
    pub dir: String,
    /// Where the other `figlock` is, when `state` is `External`.
    pub external: Option<String>,
}

/// The tool next to the app's own executable, or none in a build that does not bundle it.
pub fn bundled_cli(exe: &Path) -> Option<PathBuf> {
    let candidate = exe.parent()?.join(NAME);
    // On a case-insensitive disk an executable named `FigLock` would be found here; it is not the tool.
    let same = fs::canonicalize(&candidate).ok()? == fs::canonicalize(exe).ok()?;
    (candidate.is_file() && !same).then_some(candidate)
}

pub fn link_path(home: &Path) -> PathBuf {
    home.join(LINK_DIR).join(NAME)
}

/// A path that looks like the tool inside an app bundle: `<name>.app/Contents/MacOS/figlock`.
fn is_app_copy(target: &Path) -> bool {
    target.file_name().is_some_and(|n| n == NAME)
        && target
            .parent()
            .is_some_and(|p| p.ends_with("Contents/MacOS"))
        && target
            .components()
            .any(|c| c.as_os_str().to_string_lossy().ends_with(".app"))
}

/// Folders a `figlock` from another install is likely in: the ones on this process's PATH (short when the app is
/// opened from Finder) and the usual places Homebrew, the install script and cargo put it.
pub fn search_dirs(home: &Path, path_env: Option<&OsStr>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = path_env
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .collect();
    dirs.extend([
        home.join(LINK_DIR),
        home.join("bin"),
        home.join(".cargo/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ]);
    dirs
}

/// A `figlock` in one of `dirs` that this app did not put there and that is not the bundled tool itself.
pub fn external(bundled: &Path, link: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    let bundled = fs::canonicalize(bundled).ok();
    dirs.iter()
        .filter(|d| d.is_absolute())
        .map(|d| d.join(NAME))
        .filter(|p| p != link && p.is_file())
        .find(|p| {
            let ours = fs::read_link(p).is_ok_and(|t| is_app_copy(&t))
                || (bundled.is_some() && fs::canonicalize(p).ok() == bundled);
            !ours
        })
}

pub fn state(bundled: Option<&Path>, link: &Path, search: &[PathBuf]) -> LinkState {
    let Some(bundled) = bundled else {
        return LinkState::Unavailable;
    };
    match fs::symlink_metadata(link) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if external(bundled, link, search).is_some() {
                LinkState::External
            } else {
                LinkState::NotInstalled
            }
        }
        Err(_) => LinkState::Conflict,
        Ok(m) if m.file_type().is_symlink() => match fs::read_link(link) {
            Ok(t) if t == bundled => LinkState::Installed,
            Ok(t) if is_app_copy(&t) => LinkState::Moved,
            _ => LinkState::Conflict,
        },
        Ok(_) => LinkState::Conflict,
    }
}

fn conflict(link: &Path) -> CmdError {
    CmdError::new(
        "conflict",
        format!(
            "There is already a different file at {} that FigLock did not put there, so it was left as it is. Remove it yourself if you want FigLock to link its own command line tool.",
            link.display()
        ),
    )
}

#[cfg(unix)]
fn make_link(bundled: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(bundled, link)
}

#[cfg(not(unix))]
fn make_link(_bundled: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Link the bundled tool at `link`. Repeating it is fine; a link of ours to a moved app is repaired.
pub fn install(bundled: Option<&Path>, link: &Path, search: &[PathBuf]) -> CmdRes<LinkState> {
    let Some(bundled) = bundled else {
        return Err(CmdError::new(
            "unavailable",
            "This build of FigLock does not include the command line tool.",
        ));
    };
    match state(Some(bundled), link, search) {
        LinkState::Installed => return Ok(LinkState::Installed),
        LinkState::Conflict => return Err(conflict(link)),
        LinkState::External => {
            let other = external(bundled, link, search).unwrap_or_default();
            return Err(CmdError::new(
                "external",
                format!(
                    "FigLock's command line tool is already installed at {}, so a second copy was not added.",
                    other.display()
                ),
            ));
        }
        LinkState::Moved => fs::remove_file(link).map_err(failed)?,
        LinkState::NotInstalled | LinkState::Unavailable => {}
    }
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir).map_err(failed)?;
    }
    match make_link(bundled, link) {
        Ok(()) => Ok(LinkState::Installed),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(conflict(link)),
        Err(e) => Err(failed(e)),
    }
}

/// Remove the link, but only one that points into an app copy; anything else is not ours to delete.
pub fn remove(link: &Path) -> CmdRes<()> {
    match fs::symlink_metadata(link) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(m)
            if m.file_type().is_symlink() && fs::read_link(link).is_ok_and(|t| is_app_copy(&t)) =>
        {
            fs::remove_file(link).map_err(failed)
        }
        _ => Err(conflict(link)),
    }
}

fn failed(e: io::Error) -> CmdError {
    CmdError::new(
        "failed",
        format!("The command line tool could not be changed: {e}."),
    )
}

/// How the person sees a path under their home folder.
fn tilde(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map_or_else(
        |_| path.display().to_string(),
        |rest| format!("~/{}", rest.display()),
    )
}

pub fn view(bundled: Option<&Path>, home: &Path, search: &[PathBuf]) -> CliToolView {
    let link = link_path(home);
    let state = state(bundled, &link, search);
    let external = match (state, bundled) {
        (LinkState::External, Some(b)) => external(b, &link, search).map(|p| tilde(&p, home)),
        _ => None,
    };
    CliToolView {
        state,
        link: tilde(&link, home),
        dir: tilde(&home.join(LINK_DIR), home),
        external,
    }
}

fn home() -> CmdRes<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| h.is_absolute())
        .ok_or_else(|| CmdError::new("failed", "This account has no home folder."))
}

fn bundled() -> Option<PathBuf> {
    bundled_cli(&std::env::current_exe().ok()?)
}

fn search() -> CmdRes<Vec<PathBuf>> {
    Ok(search_dirs(&home()?, std::env::var_os("PATH").as_deref()))
}

pub fn status() -> CmdRes<CliToolView> {
    Ok(view(bundled().as_deref(), &home()?, &search()?))
}

pub fn install_here() -> CmdRes<CliToolView> {
    let home = home()?;
    install(bundled().as_deref(), &link_path(&home), &search()?)?;
    status()
}

pub fn remove_here() -> CmdRes<CliToolView> {
    remove(&link_path(&home()?))?;
    status()
}
