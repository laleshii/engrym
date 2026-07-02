//! `engrym install` — install the agent skills, or record the repo in memory.
//!
//! Two targets, both independent of `init` (which scaffolds a fresh repo):
//!
//!   * `skills` — (re)install the engrym skills into a chosen agent's skill
//!     directory. Use it to refresh the skill text after upgrading the CLI, or
//!     to add skills to a repo/agent you didn't pick at `init` time.
//!   * `memory` — record this repo in an agent's global memory so it learns the
//!     repo has an engrym KB (no repo footprint — see `agents::add_memory_entry`).
//!
//! The agent/skill machinery is shared with `init` — see [`super::agents`].

use super::agents;
use crate::config;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub enum Target {
    /// Install the engrym skills for an agent (`agent` = `--agent <bin>`).
    /// `local` routes to a repo-free skill location where the agent needs one.
    Skills { agent: Option<String>, local: bool, refresh: bool },
    /// Record this repo in an agent's global memory so it learns the repo has an
    /// engrym KB (no repo footprint — see `agents::add_memory_entry`).
    Memory { agent: Option<String> },
}

pub struct InstallArgs {
    /// Repo root for project-level skills (defaults to the cwd).
    pub root: PathBuf,
    pub target: Target,
    pub json: bool,
}

pub fn run(args: InstallArgs) -> Result<()> {
    match args.target {
        Target::Skills { agent, local, refresh } => {
            if refresh {
                refresh_skills(&args.root, args.json)
            } else {
                install_skills(&args.root, agent.as_deref(), local, args.json)
            }
        }
        Target::Memory { agent } => install_memory(&args.root, agent.as_deref(), args.json),
    }
}

fn install_memory(root: &Path, agent: Option<&str>, json: bool) -> Result<()> {
    let Some(a) = agents::resolve_memory_agent(agent, "Add", json)? else {
        if json {
            println!("{}", serde_json::json!({ "agent": null, "added": false }));
        } else {
            println!("No agent selected — nothing changed.");
        }
        return Ok(());
    };

    let repo = config::repo_anchor(root);
    let (file, added) = agents::add_memory_entry(a, &repo)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "agent": a.bin,
                "added": added,
                "repo": repo.to_string_lossy(),
                "memory_file": file.to_string_lossy(),
            })
        );
    } else if added {
        println!("Recorded {} in {}'s memory ({}).", repo.display(), a.label, file.display());
    } else {
        println!("{} is already in {}'s memory ({}).", repo.display(), a.label, file.display());
    }
    Ok(())
}

fn install_skills(root: &Path, agent: Option<&str>, local: bool, json: bool) -> Result<()> {
    let Some(a) = agents::resolve_skill_agent(agent, local, "Install", json)? else {
        if json {
            println!("{}", serde_json::json!({ "agent": null, "installed": [] }));
        } else {
            println!("No agent selected — nothing installed.");
        }
        return Ok(());
    };

    let written = agents::install_skills_for(a, root, local)?;
    if written.is_empty() {
        // `resolve_skill_agent` guarantees a skill location, so this only
        // happens for a user-global location with $HOME unset.
        bail!("couldn't resolve {}'s skill directory (is $HOME set?)", a.label);
    }

    if json {
        println!("{}", serde_json::json!({ "agent": a.bin, "installed": written }));
    } else {
        println!("Installed engrym skills for {}:", a.label);
        for w in &written {
            println!("  + {}", w);
        }
    }
    Ok(())
}

/// `install skills --refresh` — update every already-installed location to the
/// running binary's version (use after upgrading engrym). No agent prompt: it
/// refreshes wherever engrym skills already exist, project and user-global.
fn refresh_skills(root: &Path, json: bool) -> Result<()> {
    let refreshed = agents::refresh_installed_skills(root)?;
    if json {
        println!(
            "{}",
            serde_json::json!({ "version": agents::skill_version(), "refreshed": refreshed })
        );
    } else if refreshed.is_empty() {
        println!("No installed engrym skills found to refresh.");
        println!("  Install them with `engrym install skills` (or `engrym init`).");
    } else {
        println!("Refreshed engrym skills to v{}:", agents::skill_version());
        for w in &refreshed {
            println!("  ~ {}", w);
        }
    }
    Ok(())
}

