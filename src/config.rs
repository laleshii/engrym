//! Configuration: locating the repo and parsing `engrym.toml`.
//!
//! The binary is global; all state is anchored to a "state root" — the directory
//! that holds `engrym.toml`, under which the docs (`<root>/<docs.root>`) and the
//! derived index (`<root>/.engrym/index.db`) live. There are two arrangements:
//!
//!   * **in-repo** (the default): the state root *is* the repo. We find it by
//!     walking up from the cwd to an `engrym.toml`.
//!   * **local** (`init --local`): the state root is an external folder under
//!     `~/.engrym/projects/<key>/`, keyed by the repo, so engrym never writes
//!     into the repo. Discovery recomputes the key from the current repo.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAME: &str = "engrym.toml";
pub const INDEX_DIR: &str = ".engrym";
pub const INDEX_FILE: &str = "index.db";
pub const PROJECTS_SUBDIR: &str = "projects";

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub docs: DocsConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub search: SearchConfig,
    #[serde(default)]
    pub lint: LintConfig,
    #[serde(default)]
    pub daemon: DaemonConfig,

    /// Absolute path to the state root (the dir holding `engrym.toml`) — the
    /// repo itself in-repo, or the external project folder in local mode.
    /// Populated at load time, not read from the file.
    #[serde(skip)]
    pub repo_root: PathBuf,

    /// In local mode, the code repo this KB is bound to (the state lives
    /// elsewhere). `None` for an in-repo KB, where `repo_root` *is* the repo.
    #[serde(skip)]
    pub source_repo: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct DocsConfig {
    #[serde(default = "default_docs_root")]
    pub root: String,
    /// How `new`/`relocate` place files on disk. `id` is the identity regardless,
    /// so this is purely for human review.
    #[serde(default)]
    pub layout: Layout,
}

/// On-disk file placement for documents. `id` is the identity regardless, so
/// this only affects how files browse in a tree / editor / diff. The filesystem
/// is a single tree, so it can mirror only one of engrym's axes — pick the one
/// you most want to see when eyeballing files; query the others with the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[value(rename_all = "kebab-case")]
pub enum Layout {
    /// Everything flat under the docs root: `<id>.md`.
    #[default]
    Flat,
    /// Mirror the first topic path: `<topic/path>/<id>.md`.
    Topic,
    /// Overviews (altitude 0) at the root; deeper docs in `1/`, `2/`, `3/`.
    Altitude,
}

// `provider`/`api_key_env`/`rrf_k` are read by the semantic layer (Phase 2/3);
// kept here so configs validate today and behavior is forward-compatible.
#[derive(Debug, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_provider")]
    #[allow(dead_code)]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub api_key_env: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_rrf_k")]
    pub rrf_k: u32,
}

#[derive(Debug, Deserialize)]
pub struct LintConfig {
    #[serde(default)]
    pub strict: bool,
}

/// The warm embedding daemon. Auto-spawned by `search` on the first semantic
/// query and self-terminating after `idle_secs` of inactivity.
#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    #[serde(default = "default_daemon_enabled")]
    pub enabled: bool,
    #[serde(default = "default_idle_secs")]
    pub idle_secs: u64,
}

fn default_docs_root() -> String {
    "docs".to_string()
}
fn default_provider() -> String {
    "local".to_string()
}
fn default_model() -> String {
    "bge-small-en-v1.5".to_string()
}
fn default_rrf_k() -> u32 {
    60
}
fn default_daemon_enabled() -> bool {
    true
}
fn default_idle_secs() -> u64 {
    300
}

impl Default for DocsConfig {
    fn default() -> Self {
        Self {
            root: default_docs_root(),
            layout: Layout::default(),
        }
    }
}
impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            api_key_env: None,
        }
    }
}
impl Default for SearchConfig {
    fn default() -> Self {
        Self { rrf_k: default_rrf_k() }
    }
}
impl Default for LintConfig {
    fn default() -> Self {
        Self { strict: false }
    }
}
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            enabled: default_daemon_enabled(),
            idle_secs: default_idle_secs(),
        }
    }
}

impl Config {
    /// Discover and load the config. An in-repo `engrym.toml` (walking up from
    /// `start`) is canonical when present; otherwise we look for a local KB
    /// bound to this repo under `~/.engrym/projects/<key>/`.
    pub fn discover(start: &Path) -> Result<Config> {
        if let Some(config_path) = find_config(start) {
            return Self::load(&config_path, None);
        }
        let anchor = repo_anchor(start);
        if let Some(dir) = local_project_dir(&anchor) {
            let config_path = dir.join(CONFIG_FILENAME);
            if config_path.is_file() {
                return Self::load(&config_path, Some(anchor));
            }
        }
        bail!(
            "no {} found in {} or any parent, and no local engrym KB is bound to this repo — \
             run `engrym init` (in-repo) or `engrym init --local` (external)",
            CONFIG_FILENAME,
            start.display()
        );
    }

    fn load(config_path: &Path, source_repo: Option<PathBuf>) -> Result<Config> {
        let text = std::fs::read_to_string(config_path)
            .with_context(|| format!("reading {}", config_path.display()))?;
        let mut config: Config = toml::from_str(&text)
            .with_context(|| format!("parsing {}", config_path.display()))?;

        config.repo_root = config_path
            .parent()
            .expect("config path always has a parent")
            .to_path_buf();
        config.source_repo = source_repo;
        Ok(config)
    }

    /// Whether this KB is stored externally (local mode) rather than in the repo.
    pub fn is_local(&self) -> bool {
        self.source_repo.is_some()
    }

    /// Absolute path to the docs root directory.
    pub fn docs_root(&self) -> PathBuf {
        self.repo_root.join(&self.docs.root)
    }

    /// Absolute path to the SQLite index file.
    pub fn index_path(&self) -> PathBuf {
        self.repo_root.join(INDEX_DIR).join(INDEX_FILE)
    }

    /// Unix socket the warm embedding daemon listens on (per repo).
    pub fn socket_path(&self) -> PathBuf {
        self.repo_root.join(INDEX_DIR).join("engrym.sock")
    }

    /// Whether the warm daemon may be used (config + `ENGRYM_NO_DAEMON` env).
    pub fn daemon_enabled(&self) -> bool {
        self.daemon.enabled && std::env::var_os("ENGRYM_NO_DAEMON").is_none()
    }
}

/// Walk up from `start` looking for an `engrym.toml`.
fn find_config(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start)
    };
    loop {
        let candidate = dir.join(CONFIG_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Root of engrym's user-global store (`$ENGRYM_HOME`, else `~/.engrym`).
pub fn engrym_home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("ENGRYM_HOME") {
        return Some(PathBuf::from(h));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".engrym"))
}

/// Where local (external) project KBs live: `<engrym_home>/projects`.
pub fn projects_root() -> Option<PathBuf> {
    engrym_home().map(|h| h.join(PROJECTS_SUBDIR))
}

/// The local KB directory bound to `anchor`, if the store root resolves.
pub fn local_project_dir(anchor: &Path) -> Option<PathBuf> {
    projects_root().map(|r| r.join(project_key(anchor)))
}

/// The stable anchor a repo is keyed by: its git top-level if present, else the
/// directory itself. Canonicalized so symlinks / relative paths yield one key.
pub fn repo_anchor(start: &Path) -> PathBuf {
    let abs = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().map(|c| c.join(start)).unwrap_or_else(|_| start.to_path_buf())
    };
    let mut dir = abs.clone();
    loop {
        if dir.join(".git").exists() {
            return std::fs::canonicalize(&dir).unwrap_or(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    std::fs::canonicalize(&abs).unwrap_or(abs)
}

/// A filesystem-safe, human-readable, collision-resistant key for a repo:
/// `<basename>-<8 hex chars of sha256(absolute path)>`.
pub fn project_key(anchor: &Path) -> String {
    let name = anchor
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "repo" } else { slug };

    let mut hasher = Sha256::new();
    hasher.update(anchor.to_string_lossy().as_bytes());
    let hex: String = hasher.finalize().iter().take(4).map(|b| format!("{:02x}", b)).collect();
    format!("{slug}-{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_key_is_stable_readable_and_path_specific() {
        let a = project_key(Path::new("/Users/me/Projects/engrym"));
        // Deterministic and prefixed by the readable basename.
        assert_eq!(a, project_key(Path::new("/Users/me/Projects/engrym")));
        assert!(a.starts_with("engrym-"), "got {a}");
        // Different absolute paths with the same basename don't collide.
        let b = project_key(Path::new("/tmp/elsewhere/engrym"));
        assert!(b.starts_with("engrym-"));
        assert_ne!(a, b);
    }
}
