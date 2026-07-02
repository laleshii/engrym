//! `engrym where` / `list` / `link` / `unlink` — discovery and cross-clone
//! linking for local-mode knowledge bases.
//!
//! These run *before* config discovery: `where` is the cheap gate a skill runs
//! to decide whether engrym applies here at all, and it must answer "no KB"
//! gracefully rather than erroring. `link`/`unlink` edit the [`crate::registry`]
//! so separate clones (or a worktree's main root) share one store.

use super::agents;
use crate::config::{self, Config};
use crate::registry::{self, Registry};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// `engrym where` — report whether a KB is reachable from `start`, and how.
/// Returns `true` when one is present (the caller maps that to the exit code, so
/// `engrym where` can gate a skill with a plain `if`).
pub fn where_(start: &Path, json: bool) -> Result<bool> {
    // Reuse the real resolver: Ok means a KB is reachable (in-repo or local),
    // Err is precisely the "nothing here" case.
    match Config::discover(start) {
        Ok(cfg) => {
            let anchor = config::repo_anchor(start);
            // A local KB is "shared" when more than one checkout (worktree or
            // clone) is registered against its store.
            let shared = cfg.is_local()
                && cfg
                    .repo_root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|key| {
                        Registry::load().repos.into_iter().find(|r| r.key == key)
                    })
                    .map(|r| r.anchors.len() > 1)
                    .unwrap_or(false);
            let mode = if cfg.is_local() { "local" } else { "in-repo" };
            // A stale installed skill (after a CLI upgrade) is worth surfacing on
            // the gate the skill itself runs, so it can prompt a refresh.
            let skill_outdated = agents::any_installed_skill_outdated(start);
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "kb": true,
                        "mode": mode,
                        "shared": shared,
                        "store": cfg.repo_root.to_string_lossy(),
                        "identity": registry::repo_identity(&anchor),
                        "skill_outdated": skill_outdated,
                    })
                );
            } else {
                let tag = if shared { ", shared across checkouts" } else { "" };
                println!("engrym KB: yes ({mode}{tag})");
                println!("  store: {}", cfg.repo_root.display());
                if skill_outdated {
                    println!("  note: installed skill is stale — `engrym install skills --refresh`");
                }
            }
            Ok(true)
        }
        Err(_) => {
            // No KB here — but a same-identity KB under another clone is a link
            // away, which is exactly what the caller wants to know.
            let anchor = config::repo_anchor(start);
            let identity = registry::repo_identity(&anchor);
            let hint = identity.as_deref().and_then(|id| {
                let reg = Registry::load_migrated();
                let entry = reg.find_by_identity(id)?;
                registry::store_exists(&entry.key).then(|| entry.key.clone())
            });
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "kb": false,
                        "identity": identity,
                        "link_candidate": hint,
                    })
                );
            } else {
                println!("engrym KB: none for this repo");
                if let Some(key) = &hint {
                    println!("  a KB for the same repo exists ({key}) — `engrym link {key}` to use it");
                }
            }
            Ok(false)
        }
    }
}

/// `engrym list` — every local KB store on disk, enriched from the registry.
/// Prunes dead anchors first (self-healing after worktree teardown).
pub fn list(json: bool) -> Result<()> {
    let mut reg = Registry::load_migrated(); // one-time backfill if never built
    if reg.prune() {
        // drop dead worktree anchors
        let _ = reg.save();
    }

    let Some(root) = config::projects_root() else {
        anyhow::bail!("cannot resolve the engrym store (set $HOME or $ENGRYM_HOME)");
    };

    // Source of truth is the stores on disk; the registry adds identity/anchors.
    let mut stores: Vec<StoreView> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            if !e.path().join(config::CONFIG_FILENAME).is_file() {
                continue;
            }
            let key = e.file_name().to_string_lossy().into_owned();
            let entry = reg.repos.iter().find(|r| r.key == key);
            stores.push(StoreView {
                key,
                identity: entry.and_then(|r| r.identity.clone()),
                anchors: entry.map(|r| r.anchors.clone()).unwrap_or_default(),
            });
        }
    }
    stores.sort_by(|a, b| a.key.cmp(&b.key));

    if json {
        println!(
            "{}",
            serde_json::json!({
                "stores": stores.iter().map(|s| serde_json::json!({
                    "key": s.key,
                    "identity": s.identity,
                    "anchors": s.anchors,
                })).collect::<Vec<_>>(),
            })
        );
        return Ok(());
    }

    if stores.is_empty() {
        println!("No local engrym KBs.");
        return Ok(());
    }
    println!("Local engrym KBs ({}):", stores.len());
    for s in &stores {
        println!("  {}", s.key);
        if let Some(id) = &s.identity {
            println!("    identity: {id}");
        }
        match s.anchors.len() {
            0 => println!("    anchors:  (none recorded)"),
            _ => {
                for a in &s.anchors {
                    println!("    anchor:   {a}");
                }
            }
        }
    }
    Ok(())
}

struct StoreView {
    key: String,
    identity: Option<String>,
    anchors: Vec<String>,
}

/// `engrym link <target>` — make this checkout share `target`'s KB. `target` is
/// either an existing store key (`projects/<key>/`) or a path to another
/// checkout of the same repo.
pub fn link(start: &Path, target: &str, json: bool) -> Result<()> {
    let anchor = config::repo_anchor(start);
    let key = resolve_target_key(target)?;
    let identity = registry::repo_identity(&anchor);

    let mut reg = Registry::load();
    let changed = reg.link(&anchor, &key, identity);
    reg.save().context("saving the registry")?;

    if json {
        println!(
            "{}",
            serde_json::json!({ "linked": changed, "anchor": anchor.to_string_lossy(), "key": key })
        );
    } else if changed {
        println!("Linked {} → KB `{}`.", anchor.display(), key);
    } else {
        println!("{} was already linked to `{}`.", anchor.display(), key);
    }
    Ok(())
}

/// `engrym unlink` — detach this checkout from a shared KB (reverting to its own
/// path-derived store).
pub fn unlink(start: &Path, json: bool) -> Result<()> {
    let anchor = config::repo_anchor(start);
    let mut reg = Registry::load();
    let key = reg.unlink(&anchor);
    if key.is_some() {
        reg.save().context("saving the registry")?;
    }
    if json {
        println!(
            "{}",
            serde_json::json!({ "unlinked": key.is_some(), "anchor": anchor.to_string_lossy(), "key": key })
        );
    } else if let Some(k) = key {
        println!("Unlinked {} from `{}`.", anchor.display(), k);
    } else {
        println!("{} wasn't linked to any KB.", anchor.display());
    }
    Ok(())
}

/// Resolve a `link` target to a store key. An exact store-dir match wins;
/// otherwise treat it as a path to another checkout and resolve its key.
fn resolve_target_key(target: &str) -> Result<String> {
    if registry::store_exists(target) {
        return Ok(target.to_string());
    }
    let path = PathBuf::from(target);
    if path.exists() {
        let anchor = config::repo_anchor(&path);
        let key = config::local_key(&anchor);
        if registry::store_exists(&key) {
            return Ok(key);
        }
        anyhow::bail!(
            "`{}` resolves to KB `{}`, which has no store yet — `engrym init --local` it first",
            target,
            key
        );
    }
    anyhow::bail!("`{}` is neither a known KB key nor an existing path", target)
}
