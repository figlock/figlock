//! The command line tool link: what it creates, repairs and refuses, in a temporary home folder.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use super::cli_tool::*;

/// An app bundle with the tool inside, laid out like a real one, plus an empty home folder.
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let macos = dir.path().join("Apps/FigLock.app/Contents/MacOS");
    fs::create_dir_all(&macos).unwrap();
    fs::write(macos.join("figlock-desktop"), b"app").unwrap();
    fs::write(macos.join("figlock"), b"cli").unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    (dir, macos, home)
}

fn code(r: &Result<LinkState, super::CmdError>) -> &'static str {
    r.as_ref().unwrap_err().code
}

#[test]
fn the_tool_is_found_beside_the_app_executable_and_not_in_a_build_without_it() {
    let (_d, macos, _home) = setup();
    let exe = macos.join("figlock-desktop");
    assert_eq!(bundled_cli(&exe), Some(macos.join("figlock")));
    fs::remove_file(macos.join("figlock")).unwrap();
    assert_eq!(bundled_cli(&exe), None);
}

#[test]
fn the_app_executable_is_never_mistaken_for_the_tool() {
    // A case-insensitive disk resolves `figlock` to an executable named `FigLock`.
    let (_d, macos, _home) = setup();
    fs::remove_file(macos.join("figlock")).unwrap();
    fs::write(macos.join("FigLock"), b"app").unwrap();
    let exe = macos.join("FigLock");
    assert_eq!(bundled_cli(&exe), None);
}

#[test]
fn installing_links_the_bundled_tool_and_repeating_it_changes_nothing() {
    let (_d, macos, home) = setup();
    let cli = macos.join("figlock");
    let link = link_path(&home);
    assert_eq!(state(Some(&cli), &link), LinkState::NotInstalled);

    assert_eq!(install(Some(&cli), &link).unwrap(), LinkState::Installed);
    assert_eq!(fs::read_link(&link).unwrap(), cli);
    assert_eq!(fs::read(&link).unwrap(), b"cli");
    assert_eq!(install(Some(&cli), &link).unwrap(), LinkState::Installed);
}

#[test]
fn a_build_without_the_tool_cannot_install_it() {
    let (_d, _macos, home) = setup();
    let link = link_path(&home);
    assert_eq!(state(None, &link), LinkState::Unavailable);
    assert_eq!(code(&install(None, &link)), "unavailable");
    assert!(!link.exists());
}

#[test]
fn a_real_file_or_someone_elses_link_is_never_overwritten() {
    let (_d, macos, home) = setup();
    let cli = macos.join("figlock");
    let link = link_path(&home);
    fs::create_dir_all(link.parent().unwrap()).unwrap();

    fs::write(&link, b"someone else's tool").unwrap();
    assert_eq!(state(Some(&cli), &link), LinkState::Conflict);
    assert_eq!(code(&install(Some(&cli), &link)), "conflict");
    assert_eq!(fs::read(&link).unwrap(), b"someone else's tool");
    assert_eq!(code_unit(remove(&link)), "conflict");
    assert_eq!(fs::read(&link).unwrap(), b"someone else's tool");
    fs::remove_file(&link).unwrap();

    // A Homebrew install is a link too, but not into an app bundle.
    let brew = home.join("homebrew/bin/figlock");
    fs::create_dir_all(brew.parent().unwrap()).unwrap();
    fs::write(&brew, b"brew").unwrap();
    symlink(&brew, &link).unwrap();
    assert_eq!(state(Some(&cli), &link), LinkState::Conflict);
    assert_eq!(code(&install(Some(&cli), &link)), "conflict");
    assert_eq!(fs::read_link(&link).unwrap(), brew);
    assert_eq!(code_unit(remove(&link)), "conflict");
    assert!(fs::symlink_metadata(&link).is_ok());
}

fn code_unit(r: Result<(), super::CmdError>) -> &'static str {
    r.unwrap_err().code
}

#[test]
fn a_link_to_a_moved_app_is_repaired_and_a_removed_app_leaves_a_removable_link() {
    let (d, macos, home) = setup();
    let link = link_path(&home);
    install(Some(&macos.join("figlock")), &link).unwrap();

    // The app is moved: the old link dangles, and a new copy of the app is installing.
    let moved = d.path().join("Elsewhere/FigLock.app/Contents/MacOS");
    fs::create_dir_all(&moved).unwrap();
    fs::write(moved.join("figlock"), b"cli").unwrap();
    fs::remove_dir_all(d.path().join("Apps")).unwrap();
    let cli = moved.join("figlock");
    assert_eq!(state(Some(&cli), &link), LinkState::Moved);
    assert_eq!(install(Some(&cli), &link).unwrap(), LinkState::Installed);
    assert_eq!(fs::read_link(&link).unwrap(), cli);

    // Even with the app gone, its link can be taken away.
    fs::remove_dir_all(d.path().join("Elsewhere")).unwrap();
    remove(&link).unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
}

#[test]
fn removing_takes_only_our_link_and_nothing_when_there_is_none() {
    let (_d, macos, home) = setup();
    let link = link_path(&home);
    remove(&link).unwrap();
    install(Some(&macos.join("figlock")), &link).unwrap();
    remove(&link).unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
    // The tool inside the app is untouched.
    assert_eq!(fs::read(macos.join("figlock")).unwrap(), b"cli");
}

#[test]
fn the_view_shows_paths_as_the_person_types_them() {
    let (_d, macos, home) = setup();
    let v = view(Some(&macos.join("figlock")), &home);
    assert_eq!(v.state, LinkState::NotInstalled);
    assert_eq!(v.link, "~/.local/bin/figlock");
    assert_eq!(v.dir, "~/.local/bin");
}
