//! `figlock skill install` as a person meets it: a fresh home, a re-run, a modified copy, a project install.
//! No vault is needed, so nothing here prompts.

use std::fs;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const SHIPPED: &str = include_str!("../../../skill/SKILL.md");

fn figlock(home: &TempDir, cwd: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_figlock"))
        .args(args)
        .env("HOME", home.path())
        .env_remove("CLAUDE_CONFIG_DIR")
        .current_dir(cwd.path())
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn install_writes_the_shipped_text_and_is_idempotent() {
    let (home, cwd) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let target = home.path().join(".claude/skills/figlock/SKILL.md");

    let out = figlock(&home, &cwd, &["skill", "install"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).starts_with("installed: "));
    assert_eq!(fs::read_to_string(&target).unwrap(), SHIPPED);

    let again = figlock(&home, &cwd, &["--json", "skill", "install"]);
    assert!(again.status.success());
    let v: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(v["status"], "unchanged");
}

#[test]
fn install_refuses_to_overwrite_a_modified_copy_without_force() {
    let (home, cwd) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let target = home.path().join(".claude/skills/figlock/SKILL.md");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "my own edits").unwrap();

    let out = figlock(&home, &cwd, &["skill", "install"]);
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("--force"));
    assert_eq!(fs::read_to_string(&target).unwrap(), "my own edits");

    let forced = figlock(&home, &cwd, &["skill", "install", "--force"]);
    assert!(forced.status.success(), "{}", text(&forced.stderr));
    assert!(text(&forced.stdout).starts_with("replaced: "));
    assert_eq!(fs::read_to_string(&target).unwrap(), SHIPPED);
}

#[test]
fn project_install_goes_under_the_working_folder_only() {
    let (home, cwd) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let out = figlock(&home, &cwd, &["skill", "install", "--project"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(
        fs::read_to_string(cwd.path().join(".claude/skills/figlock/SKILL.md")).unwrap(),
        SHIPPED
    );
    assert!(!home.path().join(".claude").exists());
}

#[test]
fn show_prints_the_shipped_text() {
    let (home, cwd) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let out = figlock(&home, &cwd, &["skill", "show"]);
    assert!(out.status.success());
    assert_eq!(text(&out.stdout), SHIPPED);
}

/// G11: the skill must steer agents to `http` and `run` and away from every command that returns a value.
#[test]
fn skill_steers_to_the_broker_and_away_from_reveal() {
    assert!(SHIPPED.starts_with("---\nname: figlock\ndescription: "));
    assert!(SHIPPED.contains("figlock http"));
    assert!(SHIPPED.contains("figlock run"));
    for line in SHIPPED
        .lines()
        .filter(|l| l.trim_start().starts_with("figlock "))
    {
        for banned in ["--reveal", "--include-secrets", " copy ", " export"] {
            assert!(
                !line.contains(banned),
                "example command uses {banned}: {line}"
            );
        }
    }
    assert!(!SHIPPED.contains('\u{2014}'), "no em dash");
}
