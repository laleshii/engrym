//! `engrym uninstall` — the inverse of `install`.
//!
//!   * `skills` — remove the engrym skills from a chosen agent's skill directory.
//!   * `memory` — remove this repo from an agent's global memory.
//!
//! Neither needs an `engrym.toml`; both run before config discovery. To wipe a
//! KB's *content* (docs + index), use `engrym reset` instead.

use super::agents;
use crate::config;
use anyhow::Result;
use std::path::{Path, PathBuf};

pub enum Target {
    /// Remove the engrym skills for an agent (`agent` = `--agent <bin>`).
    Skills { agent: Option<String>, local: bool },
    /// Remove this repo from an agent's global memory (inverse of `install memory`).
    Memory { agent: Option<String> },
}

pub struct UninstallArgs {
    /// Repo root for project-level skills (defaults to the cwd).
    pub root: PathBuf,
    pub target: Target,
    pub json: bool,
}

pub fn run(args: UninstallArgs) -> Result<()> {
    match args.target {
        Target::Skills { agent, local } => {
            uninstall_skills(&args.root, agent.as_deref(), local, args.json)
        }
        Target::Memory { agent } => uninstall_memory(&args.root, agent.as_deref(), args.json),
    }
}

fn uninstall_memory(root: &Path, agent: Option<&str>, json: bool) -> Result<()> {
    let Some(a) = agents::resolve_memory_agent(agent, "Remove", json)? else {
        if json {
            println!("{}", serde_json::json!({ "agent": null, "removed": false }));
        } else {
            println!("No agent selected — nothing changed.");
        }
        return Ok(());
    };

    let repo = config::repo_anchor(root);
    let (file, removed) = agents::remove_memory_entry(a, &repo)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "agent": a.bin,
                "removed": removed,
                "repo": repo.to_string_lossy(),
                "memory_file": file.to_string_lossy(),
            })
        );
    } else if removed {
        println!("Removed {} from {}'s memory ({}).", repo.display(), a.label, file.display());
    } else {
        println!("{} was not in {}'s memory ({}).", repo.display(), a.label, file.display());
    }
    Ok(())
}

fn uninstall_skills(root: &Path, agent: Option<&str>, local: bool, json: bool) -> Result<()> {
    let Some(a) = agents::resolve_skill_agent(agent, local, "Remove", json)? else {
        if json {
            println!("{}", serde_json::json!({ "agent": null, "removed": [] }));
        } else {
            println!("No agent selected — nothing removed.");
        }
        return Ok(());
    };

    let removed = agents::remove_skills_for(a, root, local)?;
    if json {
        println!("{}", serde_json::json!({ "agent": a.bin, "removed": removed }));
    } else if removed.is_empty() {
        println!("No engrym skills found for {} — nothing to remove.", a.label);
    } else {
        println!("Removed engrym skills for {}:", a.label);
        for r in &removed {
            println!("  - {}", r);
        }
    }
    Ok(())
}

