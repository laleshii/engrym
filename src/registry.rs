//! The local-KB registry: `~/.engrym/registry.json`.
//!
//! In local mode a KB lives in an external store keyed by the repo. A single
//! repo, though, surfaces at many filesystem paths — git worktrees (already
//! folded to one anchor by [`crate::config::repo_anchor`]) and separate *clones*
//! of the same remote. The registry lets several anchors share one store:
//!
//!   * `key` is the `projects/<key>/` directory name — the canonical KB;
//!   * `identity` is the repo's normalized `origin` URL, the signal we use to
//!     recognize two clones as "the same repo";
//!   * `anchors` are the resolved paths that map to this store.
//!
//! Resolution ([`crate::config::local_key`]) is a pure lookup: an anchor maps to
//! a store only via an explicit entry. *Establishing* a link (adopting a
//! same-identity store for a new clone) is deliberate — `init --local` (prompted)
//! or `engrym link` — so cross-clone sharing is never a silent surprise.

use crate::config;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const REGISTRY_FILE: &str = "registry.json";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub repos: Vec<Repo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    /// Store directory name under `projects/` — the canonical KB key.
    pub key: String,
    /// Normalized `origin` URL, when the repo has one. The dedupe signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Resolved anchor paths (canonical, absolute) that map to this store.
    #[serde(default)]
    pub anchors: Vec<String>,
}

fn registry_path() -> Option<PathBuf> {
    config::engrym_home().map(|h| h.join(REGISTRY_FILE))
}

fn anchor_str(anchor: &Path) -> String {
    anchor.to_string_lossy().into_owned()
}

impl Registry {
    /// Load the registry, tolerating a missing or corrupt file (either yields an
    /// empty registry — the store on disk is the real source of truth).
    pub fn load() -> Registry {
        let Some(path) = registry_path() else { return Registry::default() };
        let Ok(text) = std::fs::read_to_string(&path) else { return Registry::default() };
        serde_json::from_str(&text).unwrap_or_default()
    }

    /// Atomically persist the registry (temp file + rename), creating
    /// `~/.engrym/` if needed.
    pub fn save(&self) -> Result<()> {
        let path = registry_path().context("cannot resolve $HOME / $ENGRYM_HOME for the registry")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self).context("serializing registry")?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    /// The store key an anchor is explicitly mapped to, if any.
    pub fn key_for_anchor(&self, anchor: &Path) -> Option<String> {
        let a = anchor_str(anchor);
        self.repos.iter().find(|r| r.anchors.iter().any(|x| x == &a)).map(|r| r.key.clone())
    }

    /// The entry for a repo identity (a same-repo store under another clone).
    pub fn find_by_identity(&self, identity: &str) -> Option<&Repo> {
        self.repos.iter().find(|r| r.identity.as_deref() == Some(identity))
    }

    /// Record `anchor -> key` (and the repo's `identity`), so this checkout
    /// shares the store. Idempotent; returns whether anything changed. An anchor
    /// maps to exactly one store, so it's first detached from any other entry.
    pub fn link(&mut self, anchor: &Path, key: &str, identity: Option<String>) -> bool {
        let a = anchor_str(anchor);
        let mut changed = false;

        // Detach the anchor from any other store.
        for r in &mut self.repos {
            if r.key != key {
                let before = r.anchors.len();
                r.anchors.retain(|x| x != &a);
                changed |= r.anchors.len() != before;
            }
        }

        let entry = match self.repos.iter_mut().find(|r| r.key == key) {
            Some(e) => e,
            None => {
                self.repos.push(Repo { key: key.to_string(), identity: None, anchors: Vec::new() });
                changed = true;
                self.repos.last_mut().unwrap()
            }
        };
        if !entry.anchors.iter().any(|x| x == &a) {
            entry.anchors.push(a);
            changed = true;
        }
        if entry.identity.is_none() && identity.is_some() {
            entry.identity = identity;
            changed = true;
        }
        changed
    }

    /// Remove an anchor's mapping. Returns the key it was linked to, if any.
    pub fn unlink(&mut self, anchor: &Path) -> Option<String> {
        let a = anchor_str(anchor);
        let mut hit = None;
        for r in &mut self.repos {
            if r.anchors.iter().any(|x| x == &a) {
                r.anchors.retain(|x| x != &a);
                hit = Some(r.key.clone());
            }
        }
        // Drop entries that now have no anchors and no store on disk.
        self.repos.retain(|r| !r.anchors.is_empty() || store_exists(&r.key));
        hit
    }

    /// Drop anchors whose path no longer exists (ephemeral worktrees torn down by
    /// e.g. `/remove-ticket`), and entries with neither anchors nor a live store.
    pub fn prune(&mut self) -> bool {
        let mut changed = false;
        for r in &mut self.repos {
            let before = r.anchors.len();
            r.anchors.retain(|a| Path::new(a).exists());
            changed |= r.anchors.len() != before;
        }
        let before = self.repos.len();
        self.repos.retain(|r| !r.anchors.is_empty() || store_exists(&r.key));
        changed |= self.repos.len() != before;
        changed
    }
}

/// Whether a store directory (`projects/<key>/engrym.toml`) exists.
pub fn store_exists(key: &str) -> bool {
    config::projects_root()
        .map(|r| r.join(key).join(config::CONFIG_FILENAME).is_file())
        .unwrap_or(false)
}

/// Teach the registry an `anchor -> key` mapping discovered at load time (so a
/// pre-existing store, created before the registry, becomes linkable by
/// identity). Writes only when something actually changed — safe on the hot
/// read path. Best-effort: registry write failures are swallowed.
pub fn learn(anchor: &Path, key: &str) {
    let mut reg = Registry::load();
    // Already mapped to this key with an identity recorded? Nothing to do.
    if reg.repos.iter().any(|r| {
        r.key == key && r.identity.is_some() && r.anchors.iter().any(|a| Path::new(a) == anchor)
    }) {
        return;
    }
    if reg.link(anchor, key, repo_identity(anchor)) {
        let _ = reg.save();
    }
}

/// The repo's identity for dedupe: its normalized `origin` remote URL, read from
/// `<anchor>/.git/config` with no `git` subprocess. `None` when there's no git
/// config or no `origin` — callers then fall back to path-based keying.
pub fn repo_identity(anchor: &Path) -> Option<String> {
    let cfg = std::fs::read_to_string(anchor.join(".git").join("config")).ok()?;
    parse_origin_url(&cfg).map(|u| normalize_git_url(&u))
}

/// Extract `[remote "origin"] url = …` from a git config file.
fn parse_origin_url(config_text: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config_text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_origin = t == "[remote \"origin\"]";
            continue;
        }
        if in_origin {
            if let Some(rest) = t.strip_prefix("url") {
                if let Some(v) = rest.trim_start().strip_prefix('=') {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Normalize a git remote URL to a scheme/user-agnostic identity so scp-style
/// and https forms of the same remote collapse to one string, e.g.
/// `git@github.com:org/repo.git` and `https://github.com/org/repo` →
/// `github.com/org/repo`.
pub fn normalize_git_url(url: &str) -> String {
    let mut s = url.trim().to_string();
    for scheme in ["ssh://", "git+ssh://", "https://", "http://", "git://"] {
        if let Some(rest) = s.strip_prefix(scheme) {
            s = rest.to_string();
            break;
        }
    }
    // Drop leading `user@` (only when it precedes the path).
    if let Some(at) = s.find('@') {
        if at < s.find('/').unwrap_or(s.len()) {
            s = s[at + 1..].to_string();
        }
    }
    // scp-style `host:path` → `host/path` (colon before any slash).
    if let Some(colon) = s.find(':') {
        if colon < s.find('/').unwrap_or(s.len()) {
            s.replace_range(colon..colon + 1, "/");
        }
    }
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let s = s.trim_end_matches('/');
    // Lowercase the host; leave the path case intact.
    match s.split_once('/') {
        Some((host, path)) => format!("{}/{}", host.to_lowercase(), path),
        None => s.to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scp_and_https_urls_normalize_to_one_identity() {
        let want = "github.com/uplisting/uplisting-api";
        for url in [
            "git@github.com:uplisting/uplisting-api.git",
            "https://github.com/uplisting/uplisting-api.git",
            "https://github.com/uplisting/uplisting-api",
            "ssh://git@github.com/uplisting/uplisting-api.git",
            "git@GitHub.com:uplisting/uplisting-api.git",
        ] {
            assert_eq!(normalize_git_url(url), want, "for {url}");
        }
        // Different repos stay distinct.
        assert_ne!(
            normalize_git_url("git@github.com:uplisting/uplisting-api.git"),
            normalize_git_url("git@github.com:uplisting/uplisting-frontend.git"),
        );
    }

    #[test]
    fn parses_origin_url_ignoring_other_remotes() {
        let cfg = "\
[core]
\trepositoryformatversion = 0
[remote \"upstream\"]
\turl = git@github.com:other/fork.git
[remote \"origin\"]
\turl = git@github.com:uplisting/uplisting-api.git
\tfetch = +refs/heads/*:refs/remotes/origin/*
";
        assert_eq!(
            parse_origin_url(cfg).as_deref(),
            Some("git@github.com:uplisting/uplisting-api.git")
        );
    }

    #[test]
    fn link_shares_a_store_and_unlink_reverts() {
        let mut reg = Registry::default();
        let a = Path::new("/tmp/clone-a");
        let b = Path::new("/tmp/clone-b");
        let id = Some("github.com/org/repo".to_string());

        assert!(reg.link(a, "repo-key", id.clone()));
        assert!(reg.link(b, "repo-key", id.clone())); // second clone shares it
        assert_eq!(reg.key_for_anchor(a).as_deref(), Some("repo-key"));
        assert_eq!(reg.key_for_anchor(b).as_deref(), Some("repo-key"));
        assert_eq!(reg.find_by_identity("github.com/org/repo").unwrap().anchors.len(), 2);

        assert_eq!(reg.unlink(b).as_deref(), Some("repo-key"));
        assert_eq!(reg.key_for_anchor(b), None);
        assert_eq!(reg.key_for_anchor(a).as_deref(), Some("repo-key"));
    }
}
