//! Workspace resolution — one knowledge base, or several side by side.
//!
//! engrym is anchored to a repo, but a working machine is usually a *folder of
//! clones*: `~/Projects/{api,web,jobs}`, each its own repo with its own KB. From
//! that parent folder, "search" should mean "search all of them" — cross-service
//! answers are exactly what you're standing there to find.
//!
//! Resolution is ordered so it never surprises you:
//!
//!   * a KB reachable by walking *up* from the cwd wins, exactly as before;
//!   * only when there is none do we look *down*, one level, for directories
//!     carrying their own KB;
//!   * `--all` asks for the fan-out explicitly from inside a repo — the
//!     workspace is then the folder holding that repo, so its siblings join in.
//!
//! Scanning never climbs from a candidate (that's what [`Config::discover_here`]
//! enforces): a plain subdirectory of a repo is not a member of anything. It
//! goes one level down by default and as many as `--depth` asks for, but it
//! stops descending the moment a directory turns out to be a git checkout —
//! sibling repos live *beside* repos, never inside one — so widening the depth
//! reaches nested groupings like `~/Projects/<org>/<repo>` without ever walking
//! into a repo's `node_modules` or `target`. Depth only ever widens the scan
//! *downward*: the root is the directory you're standing in, or — from inside a
//! repo — the one folder holding it, never something inferred further up. Point
//! at a different root explicitly with `--repo <dir>`. Commands that mutate a KB
//! go through [`Workspace::only`], so fanning out is always a read, never a
//! write.

use crate::config::Config;
use crate::db;
use anyhow::{bail, Result};
use rusqlite::OptionalExtension;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// One KB in scope: the checkout it belongs to, its loaded config, and the short
/// name it is labelled by and addressed with (`<name>:<doc-id>`).
pub struct Member {
    pub name: String,
    pub repo: PathBuf,
    pub config: Config,
}

impl Member {
    /// Whether this KB's index contains `id`. Used to resolve an unqualified
    /// document reference across repos; a member with no index yet simply can't
    /// claim it.
    pub fn has_doc(&self, id: &str) -> bool {
        let Ok(conn) = db::open_existing(&self.config.index_path()) else {
            return false;
        };
        conn.query_row("SELECT 1 FROM docs WHERE id = ?1", [id], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }
}

/// The set of KBs a command operates on.
pub struct Workspace {
    /// The folder the members were found under (the repo itself, single-KB).
    pub root: PathBuf,
    pub members: Vec<Member>,
    /// Whether we're spanning repos, which drives repo-labelled output. True
    /// even for a one-member scan: if we had to look sideways to find a KB, you
    /// should be told which repo answered.
    pub spans_repos: bool,
    /// The depth this workspace was scanned at, so a follow-up command can be
    /// suggested with the same reach.
    pub depth: usize,
}

impl Workspace {
    /// Resolve the KBs in scope from `start`. `all` forces the sibling fan-out
    /// even when this repo has a KB of its own; `depth` is how many levels below
    /// the workspace root to look for members.
    pub fn resolve(start: &Path, all: bool, depth: usize) -> Result<Workspace> {
        let own = Config::discover(start);

        // With a KB of our own, `--all` means "and my siblings too", so the
        // workspace is the folder holding this repo.
        if let Ok(config) = own {
            if !all {
                return Ok(Workspace::single(config));
            }
            let repo = config.repo().to_path_buf();
            let root = repo.parent().map(Path::to_path_buf).unwrap_or_else(|| repo.clone());
            let mut collector = Collector::new(&root);
            collector.scan(&root, depth);
            // Belt and braces: our own KB is normally found by the scan, but keep
            // it in scope even if the layout hides it (repo at the filesystem
            // root, a symlinked checkout, an unreadable parent).
            collector.add_config(&repo, config);
            return Ok(Workspace { root, members: collector.finish(), spans_repos: true, depth });
        }

        // No KB anywhere above us — look outward before giving up. The original
        // error is the better message when that finds nothing either, so it's
        // kept rather than replaced.
        let err = own.unwrap_err();
        let (root, members) = scan_outward(start, depth);
        if members.is_empty() {
            return Err(err);
        }
        Ok(Workspace { root, members, spans_repos: true, depth })
    }

    /// Every KB a workspace search from `start` would cover, without failing
    /// when there are none. Used by `engrym list` to show the reach of `--all`.
    pub fn survey(start: &Path, depth: usize) -> Workspace {
        let (root, members) = match Config::discover(start) {
            Ok(config) => {
                let repo = config.repo();
                let root =
                    repo.parent().map(Path::to_path_buf).unwrap_or_else(|| repo.to_path_buf());
                let members = scan(&root, depth);
                (root, members)
            }
            Err(_) => scan_outward(start, depth),
        };
        Workspace { root, members, spans_repos: true, depth }
    }

    fn single(config: Config) -> Workspace {
        let repo = config.repo().to_path_buf();
        let name = basename(&repo);
        Workspace {
            root: repo.clone(),
            members: vec![Member { name, repo, config }],
            spans_repos: false,
            depth: 1,
        }
    }

    /// The one KB this command operates on. Authoring, `reset`, `lint`, `browse`
    /// and friends go through here: silently fanning a mutation (or a
    /// repo-specific server) across several repos would be a surprise, not a
    /// convenience. A workspace that resolved to exactly one KB is unambiguous,
    /// so it's allowed.
    pub fn only(&self) -> Result<&Config> {
        match self.members.as_slice() {
            [m] => Ok(&m.config),
            members => bail!(
                "{} knowledge bases are in scope under {} ({}) — this command works on one at a \
                 time; cd into a repo, or point at one with `--repo <dir>`",
                members.len(),
                self.root.display(),
                self.names()
            ),
        }
    }

    /// The flags a follow-up command needs to reach the same members. Empty at
    /// the default depth, where discovery is automatic.
    pub fn reach_flags(&self) -> String {
        if self.depth > 1 {
            format!(" --depth {}", self.depth)
        } else {
            String::new()
        }
    }

    /// Comma-separated member names, for error messages.
    pub fn names(&self) -> String {
        self.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
    }

    /// Resolve a document reference to the member holding it. `<repo>:<id>`
    /// names one outright; a bare id is fine while it's unambiguous, and when
    /// several repos define it we ask for the qualifier instead of guessing.
    pub fn resolve_doc(&self, reference: &str) -> Result<(&Member, String)> {
        if let Some((prefix, id)) = reference.split_once(':') {
            if let Some(member) = self.members.iter().find(|m| m.name == prefix) {
                return Ok((member, id.to_string()));
            }
            // Only insist on a known repo when we're actually spanning several;
            // otherwise fall through and treat the whole string as an id.
            if self.spans_repos {
                // `org-a/api` names a repo a level deeper than we looked — the
                // likely fix is reach, not spelling.
                let nesting = prefix.matches('/').count();
                let hint = if nesting > 0 && self.depth <= nesting {
                    format!(" — try `--depth {}`", nesting + 1)
                } else {
                    String::new()
                };
                bail!("no repo `{}` in scope ({}){}", prefix, self.names(), hint);
            }
        }
        if let [m] = self.members.as_slice() {
            return Ok((m, reference.to_string()));
        }
        let matches: Vec<&Member> = self.members.iter().filter(|m| m.has_doc(reference)).collect();
        match matches.as_slice() {
            [m] => Ok((m, reference.to_string())),
            [] => bail!(
                "no document with id `{}` in any KB in scope ({})",
                reference,
                self.names()
            ),
            many => bail!(
                "`{}` exists in {} repos ({}) — qualify it, e.g. `{}:{}`",
                reference,
                many.len(),
                many.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", "),
                many[0].name,
                reference
            ),
        }
    }

    /// JSON description of the workspace, shared by every `--json` renderer.
    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "root": self.root.to_string_lossy(),
            "depth": self.depth,
            "repos": self.members.iter().map(|m| serde_json::json!({
                "name": m.name,
                "path": m.repo.to_string_lossy(),
                "store": m.config.repo_root.to_string_lossy(),
                "mode": if m.config.is_local() { "local" } else { "in-repo" },
            })).collect::<Vec<_>>(),
        })
    }
}

/// Accumulates members while keeping names unique and stores de-duplicated.
struct Collector {
    root: PathBuf,
    members: Vec<Member>,
    stores: HashSet<PathBuf>,
    names: HashSet<String>,
}

impl Collector {
    fn new(root: &Path) -> Collector {
        Collector {
            root: root.to_path_buf(),
            members: Vec::new(),
            stores: HashSet::new(),
            names: HashSet::new(),
        }
    }

    /// Add `root`, then walk up to `depth` levels of subdirectories below it.
    fn scan(&mut self, root: &Path, depth: usize) {
        self.add(root);
        self.descend(root, depth);
    }

    fn descend(&mut self, dir: &Path, remaining: usize) {
        if remaining == 0 {
            return;
        }
        let mut children: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            // `is_dir` follows symlinks, so a symlinked checkout still counts.
            .filter(|p| p.is_dir() && !is_hidden(p))
            .collect();
        children.sort();
        for child in &children {
            // Stop at any git checkout, KB or not. Repos sit *beside* each
            // other, never inside one another, so a repo's interior holds no
            // members — only the `node_modules` / `target` trees that would make
            // a deeper scan expensive for nothing.
            let is_member = self.add(child);
            if is_member || is_checkout(child) {
                continue;
            }
            self.descend(child, remaining - 1);
        }
    }

    /// Whether `dir` carried a KB and became a member.
    fn add(&mut self, dir: &Path) -> bool {
        match Config::discover_here(dir) {
            Some(config) => self.add_config(dir, config),
            None => false,
        }
    }

    fn add_config(&mut self, dir: &Path, config: Config) -> bool {
        // Worktrees and linked clones share one store; keeping both would make
        // the same KB answer twice under two names. It's still a member as far
        // as the walk is concerned — we just don't list it twice.
        if !self.stores.insert(config.repo_root.clone()) {
            return true;
        }
        let name = self.unique_name(dir);
        self.members.push(Member { name, repo: dir.to_path_buf(), config });
        true
    }

    /// A member's name is its path relative to the workspace root — `api` when
    /// clones sit side by side, `org-a/api` when they're grouped a level deeper.
    /// That keeps the qualifier readable *and* unique by construction, which
    /// plain basenames stop being as soon as two orgs both have an `api`.
    fn unique_name(&mut self, dir: &Path) -> String {
        let base = dir
            .strip_prefix(&self.root)
            .ok()
            .map(|rel| {
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| basename(dir));

        let mut name = base.clone();
        let mut n = 2;
        while !self.names.insert(name.clone()) {
            name = format!("{base}-{n}");
            n += 1;
        }
        name
    }

    fn finish(mut self) -> Vec<Member> {
        self.members.sort_by(|a, b| a.name.cmp(&b.name));
        self.members
    }
}

/// Find members from `start`, widening once if it comes up empty.
///
/// The first look is downward from `start` itself — the folder-of-clones case.
/// The fallback covers the other common spot to be standing in: a git checkout
/// that simply doesn't use engrym. Its own tree holds nothing, but the folder
/// holding *it* is where its siblings live, and those are the repos you meant.
/// That's the same one-level-up root `--all` already uses, so we still never
/// infer a root further up than that.
fn scan_outward(start: &Path, depth: usize) -> (PathBuf, Vec<Member>) {
    let root = absolute(start);
    let members = scan(&root, depth);
    if !members.is_empty() {
        return (root, members);
    }
    let Some(repo) = enclosing_checkout(&root) else { return (root, Vec::new()) };
    let Some(parent) = repo.parent().map(Path::to_path_buf).filter(|p| p != &root) else {
        return (root, Vec::new());
    };

    let mut members = scan(&parent, depth);
    // A different *clone of this same repo* sitting alongside us is not a
    // sibling service — it's our own KB under another path. Adopting it is
    // `engrym link`'s deliberate job (see [`crate::registry`]), never a scan's
    // side effect, so drop it and let the link-candidate flow speak instead.
    if let Some(identity) = crate::registry::repo_identity(&repo) {
        members.retain(|m| {
            crate::registry::repo_identity(&m.repo).as_deref() != Some(identity.as_str())
        });
    }
    if members.is_empty() {
        return (root, Vec::new());
    }
    (parent, members)
}

/// The nearest git checkout at or above `dir`, if any. Deliberately *not*
/// [`crate::config::repo_anchor`]: that folds a linked worktree back to its main
/// root, and here we want the checkout physically in front of us — its siblings
/// are the ones sitting beside it on disk.
fn enclosing_checkout(dir: &Path) -> Option<PathBuf> {
    let mut dir = dir.to_path_buf();
    loop {
        if is_checkout(&dir) {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn scan(root: &Path, depth: usize) -> Vec<Member> {
    let mut collector = Collector::new(root);
    collector.scan(root, depth);
    collector.finish()
}

/// Whether `dir` is a git checkout (a `.git` directory, or a worktree's `.git`
/// file). Cheap: one `stat`.
fn is_checkout(dir: &Path) -> bool {
    dir.join(".git").exists()
}

fn basename(dir: &Path) -> String {
    dir.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string())
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

fn absolute(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map(|c| c.join(path)).unwrap_or_else(|_| path.to_path_buf())
    };
    std::fs::canonicalize(&abs).unwrap_or(abs)
}
