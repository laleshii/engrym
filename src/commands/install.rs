//! `engrym install` — install the agent skills, or link this binary onto PATH.
//!
//! Two targets, both independent of `init` (which scaffolds a fresh repo):
//!
//!   * `skills` — (re)install the engrym skills into a chosen agent's skill
//!     directory. Use it to refresh the skill text after upgrading the CLI, or
//!     to add skills to a repo/agent you didn't pick at `init` time.
//!   * `bin` — symlink (or `--copy`) the running executable into a PATH
//!     directory, so a locally-built engrym is usable from any repo. The dev
//!     convenience that replaces `cargo install --path .`.
//!
//! The agent/skill machinery is shared with `init` — see [`super::agents`].

use super::agents;
use crate::config;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub enum Target {
    /// Install the engrym skills for an agent (`agent` = `--agent <bin>`).
    /// `local` routes to a repo-free skill location where the agent needs one.
    Skills { agent: Option<String>, local: bool },
    /// Link this binary onto PATH at `dir` (default chosen), copying if `copy`.
    Bin { dir: Option<PathBuf>, copy: bool },
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
        Target::Skills { agent, local } => {
            install_skills(&args.root, agent.as_deref(), local, args.json)
        }
        Target::Bin { dir, copy } => install_bin(dir, copy, args.json),
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

fn install_bin(dir: Option<PathBuf>, copy: bool, json: bool) -> Result<()> {
    let exe = std::env::current_exe().context("locating the running engrym binary")?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);

    let dir = match dir {
        Some(d) => d,
        None => agents::default_bin_dir()?,
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let dest = dir.join("engrym");

    if dest == exe {
        bail!("the running binary is already installed at {}", dest.display());
    }
    // Replace any existing engrym (a stale link or an older copy).
    if dest.symlink_metadata().is_ok() {
        std::fs::remove_file(&dest)
            .with_context(|| format!("removing existing {}", dest.display()))?;
    }

    let method = if copy {
        std::fs::copy(&exe, &dest).with_context(|| format!("copying to {}", dest.display()))?;
        "copied"
    } else {
        symlink(&exe, &dest).with_context(|| format!("linking {}", dest.display()))?;
        "linked"
    };

    let on_path = agents::dir_on_path(&dir);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "method": method,
                "source": exe.to_string_lossy(),
                "dest": dest.to_string_lossy(),
                "on_path": on_path,
            })
        );
        return Ok(());
    }

    let verb = if method == "copied" { "Copied" } else { "Linked" };
    println!("{} engrym → {}", verb, dest.display());
    println!("  source: {}", exe.display());
    if method == "linked" {
        println!("  (rebuilds of this binary take effect immediately)");
    }
    if !on_path {
        println!("\nNote: {} isn't on your PATH. Add it to your shell profile:", dir.display());
        println!("  export PATH=\"{}:$PATH\"", dir.display());
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(not(unix))]
fn symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    // No portable symlink without elevated rights on Windows; copy instead.
    std::fs::copy(src, dst).map(|_| ())
}
