//! `engrym uninstall` — the inverse of `install`.
//!
//!   * `skills` — remove the engrym skills from a chosen agent's skill directory.
//!   * `bin` — remove the engrym binary linked onto PATH by `install bin`.
//!
//! Neither needs an `engrym.toml`; both run before config discovery. To wipe a
//! KB's *content* (docs + index), use `engrym reset` instead.

use super::agents;
use crate::config;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub enum Target {
    /// Remove the engrym skills for an agent (`agent` = `--agent <bin>`).
    Skills { agent: Option<String>, local: bool },
    /// Remove the engrym binary at `dir` (default: the install default).
    Bin { dir: Option<PathBuf> },
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
        Target::Bin { dir } => uninstall_bin(dir, args.json),
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

fn uninstall_bin(dir: Option<PathBuf>, json: bool) -> Result<()> {
    let dir = match dir {
        Some(d) => d,
        None => agents::default_bin_dir()?,
    };
    let dest = dir.join("engrym");

    let meta = dest.symlink_metadata().ok();
    let existed = meta.is_some();
    let is_symlink = meta.map(|m| m.file_type().is_symlink()).unwrap_or(false);

    // Guard: refuse to delete the actual build output (a real file that *is* the
    // running binary). A symlink to it, or a `--copy`, is safe to remove.
    if existed && !is_symlink {
        let exe = std::env::current_exe().ok().and_then(|p| std::fs::canonicalize(p).ok());
        let dest_canon = std::fs::canonicalize(&dest).ok();
        if dest_canon.is_some() && dest_canon == exe {
            bail!(
                "{} is the running binary itself, not an installed link — refusing to delete it",
                dest.display()
            );
        }
    }

    if existed {
        std::fs::remove_file(&dest).with_context(|| format!("removing {}", dest.display()))?;
    }

    if json {
        println!(
            "{}",
            serde_json::json!({ "removed": existed, "path": dest.to_string_lossy() })
        );
    } else if existed {
        println!("Removed {}", dest.display());
    } else {
        println!("No engrym binary at {} — nothing to remove.", dest.display());
    }
    Ok(())
}
