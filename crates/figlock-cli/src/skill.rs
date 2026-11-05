//! `figlock skill`: the Claude skill that teaches agents to use secrets through the broker. The text is
//! compiled in from `skill/SKILL.md`, so the binary always carries the skill that matches its commands.

use std::fs;
use std::path::PathBuf;

use serde_json::json;

use crate::SkillInstallArgs;
use crate::error::{Fail, Res};

/// The skill, exactly as shipped.
pub const TEXT: &str = include_str!("../../../skill/SKILL.md");

fn skills_dir(project: bool) -> Res<PathBuf> {
    use std::env::var_os;
    let base = if project {
        std::env::current_dir()?.join(".claude")
    } else if let Some(d) = var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        d.into()
    } else {
        var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join(".claude"))
            .ok_or_else(|| Fail::new("cannot find a home folder; set CLAUDE_CONFIG_DIR"))?
    };
    Ok(base.join("skills").join("figlock"))
}

pub fn show() {
    print!("{TEXT}");
}

pub fn install(a: &SkillInstallArgs, json: bool) -> Res<()> {
    let dir = skills_dir(a.project)?;
    let file = dir.join("SKILL.md");
    let state = match fs::read_to_string(&file) {
        Ok(existing) if existing == TEXT => "unchanged",
        Ok(_) if !a.force => {
            return Err(Fail::new(
                "a different figlock skill is already installed there; run again with --force to replace it",
            ));
        }
        Ok(_) => "replaced",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "installed",
        Err(e) => return Err(e.into()),
    };
    if state != "unchanged" {
        fs::create_dir_all(&dir)?;
        fs::write(&file, TEXT)?;
    }
    if json {
        println!("{}", json!({"status": state, "path": file}));
    } else {
        println!("{state}: {}", file.display());
    }
    Ok(())
}
