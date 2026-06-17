//! `engrym deinit` — completely remove engrym from a repository (inverse of
//! `init`).
//!
//! It removes engrym's **per-repo** footprint:
//!   * the KB — in-repo files (`engrym.toml`, `docs/`, `.engrym/`, the
//!     `.gitignore` entry), or the external local-mode store folder;
//!   * the repo's **project-level** skills (e.g. `.claude/skills/engrym*`);
//!   * the repo's entry in each agent's global memory.
//!
//! It deliberately leaves *shared* infrastructure alone — **user-global** skills
//! (used by every other repo) and the installed binary; those are removed with
//! `engrym uninstall`. Deleting docs is irreversible, so it confirms by default.

use super::agents;
use crate::config::{self, Config, INDEX_DIR};
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

const GITIGNORE_COMMENT: &str = "# engrym derived index (rebuildable)";
const GITIGNORE_ENTRY: &str = ".engrym/";

enum Op {
    RemoveDir(PathBuf),
    RemoveFile(PathBuf),
    CleanGitignore(PathBuf),
    RemoveMemory(&'static agents::KnownAgent),
}

pub fn run(start: &Path, yes: bool, json: bool) -> Result<()> {
    let anchor = config::repo_anchor(start);
    let cfg = Config::discover(start).ok();

    let mut ops: Vec<Op> = Vec::new();
    let mut docs_deleted = 0usize;

    // 1. The KB itself.
    if let Some(cfg) = &cfg {
        if cfg.is_local() {
            docs_deleted = count_md(&cfg.docs_root());
            ops.push(Op::RemoveDir(cfg.repo_root.clone())); // the external store
        } else {
            // Safety: never delete the repo root (e.g. a stray docs.root = ".").
            let repo_canon = fs::canonicalize(&cfg.repo_root).ok();
            let docs_canon = fs::canonicalize(cfg.docs_root()).ok();
            if cfg.docs_root().exists() && docs_canon.is_some() && docs_canon == repo_canon {
                bail!(
                    "docs root resolves to the repo root ({}) — refusing to delete it; \
                     check `docs.root` in engrym.toml",
                    cfg.repo_root.display()
                );
            }
            docs_deleted = count_md(&cfg.docs_root());
            ops.push(Op::RemoveFile(cfg.repo_root.join(config::CONFIG_FILENAME)));
            if cfg.docs_root().exists() {
                ops.push(Op::RemoveDir(cfg.docs_root()));
            }
            let idx = cfg.repo_root.join(INDEX_DIR);
            if idx.exists() {
                ops.push(Op::RemoveDir(idx));
            }
            let gi = cfg.repo_root.join(".gitignore");
            if gitignore_has_entry(&gi) {
                ops.push(Op::CleanGitignore(gi));
            }
        }
    }

    // 2. Project-level (in-repo) skills only — never shared user-global ones.
    for agent in agents::KNOWN_AGENTS {
        if let Some(agents::SkillLoc::Project(p)) = &agent.skills {
            for (name, _) in agents::SKILLS {
                let dir = start.join(p).join(name);
                if dir.exists() {
                    ops.push(Op::RemoveDir(dir));
                }
            }
        }
    }

    // 3. This repo's entry in each agent's global memory.
    let anchor_str = anchor.to_string_lossy().into_owned();
    for agent in agents::KNOWN_AGENTS.iter().filter(|a| a.has_memory()) {
        if let Some(file) = agent.memory_file() {
            if fs::read_to_string(&file).map(|b| b.contains(&anchor_str)).unwrap_or(false) {
                ops.push(Op::RemoveMemory(agent));
            }
        }
    }

    if ops.is_empty() {
        if json {
            println!("{}", serde_json::json!({ "deinitialized": false, "reason": "nothing found" }));
        } else {
            println!("No engrym footprint found for this repo — nothing to remove.");
        }
        return Ok(());
    }

    // Confirm before deleting authored content.
    if !yes {
        if json || !std::io::stdin().is_terminal() {
            bail!(
                "refusing to remove engrym ({} doc(s) and more) without confirmation — pass --yes",
                docs_deleted
            );
        }
        println!("This removes engrym from this repo:");
        for op in &ops {
            println!("  - {}", label(op, start));
        }
        if docs_deleted > 0 {
            println!("({docs_deleted} document(s) will be deleted — this cannot be undone.)");
        }
        print!("Type 'remove' to confirm: ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).context("reading confirmation")?;
        if line.trim() != "remove" {
            println!("Aborted.");
            return Ok(());
        }
    }

    let mut removed: Vec<String> = Vec::new();
    for op in &ops {
        let what = label(op, start);
        execute(op, &anchor)?;
        removed.push(what);
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "deinitialized": true,
                "repo": anchor.to_string_lossy(),
                "docs_deleted": docs_deleted,
                "removed": removed,
            })
        );
    } else {
        println!("Removed engrym from {}:", anchor.display());
        for r in &removed {
            println!("  - {}", r);
        }
    }
    Ok(())
}

fn execute(op: &Op, anchor: &Path) -> Result<()> {
    match op {
        Op::RemoveDir(p) => {
            fs::remove_dir_all(p).with_context(|| format!("removing {}", p.display()))
        }
        Op::RemoveFile(p) => {
            fs::remove_file(p).with_context(|| format!("removing {}", p.display()))
        }
        Op::CleanGitignore(p) => clean_gitignore(p),
        Op::RemoveMemory(agent) => {
            agents::remove_memory_entry(agent, anchor).map(|_| ())
        }
    }
}

fn label(op: &Op, root: &Path) -> String {
    match op {
        Op::RemoveDir(p) | Op::RemoveFile(p) => agents::display_path(p, root),
        Op::CleanGitignore(p) => format!("{} ({} entry)", agents::display_path(p, root), GITIGNORE_ENTRY),
        Op::RemoveMemory(agent) => {
            let f = agent.memory_file().map(|p| agents::display_path(&p, root)).unwrap_or_default();
            format!("{} memory entry ({})", agent.label, f)
        }
    }
}

fn count_md(docs_root: &Path) -> usize {
    if !docs_root.is_dir() {
        return 0;
    }
    walkdir::WalkDir::new(docs_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().and_then(|x| x.to_str()) == Some("md")
        })
        .count()
}

fn gitignore_has_entry(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|b| b.lines().any(|l| l.trim() == GITIGNORE_ENTRY))
        .unwrap_or(false)
}

/// Strip engrym's lines from `.gitignore`, removing the file if nothing remains.
fn clean_gitignore(path: &Path) -> Result<()> {
    let body = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let kept: Vec<&str> = body
        .lines()
        .filter(|l| {
            let t = l.trim();
            t != GITIGNORE_ENTRY && t != GITIGNORE_COMMENT
        })
        .collect();
    let rebuilt = kept.join("\n");
    if rebuilt.trim().is_empty() {
        fs::remove_file(path).with_context(|| format!("removing {}", path.display()))
    } else {
        fs::write(path, format!("{}\n", rebuilt.trim_end()))
            .with_context(|| format!("writing {}", path.display()))
    }
}
