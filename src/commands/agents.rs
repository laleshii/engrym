//! Shared agent + skill machinery (not a command itself).
//!
//! Which agent CLIs engrym knows, where each reads its skills in its native
//! convention, and how to install the engrym skills into one. Used by both
//! `init` (scaffold + pick an agent + launch it) and `install skills` (refresh
//! the skills for an agent on demand, decoupled from scaffolding).

use anyhow::{bail, Context, Result};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

const BOOTSTRAP_SKILL: &str = include_str!("../../assets/skills/engrym-bootstrap.md");
const WORKING_SKILL: &str = include_str!("../../assets/skills/engrym.md");

/// The skills engrym installs for an agent: (skill name, content). The name is
/// also the installed directory, so it's deliberately not "engrym-init" — that
/// reads like the `init` command and nudges agents to re-run it.
pub const SKILLS: &[(&str, &str)] =
    &[("engrym-bootstrap", BOOTSTRAP_SKILL), ("engrym", WORKING_SKILL)];

/// Where an agent reads skills, in its native convention. Skills install as
/// `<base>/<name>/SKILL.md`.
pub enum SkillLoc {
    /// Relative to the repo root — committed, travels with the repo (Claude).
    Project(&'static str),
    /// Relative to `$HOME` — user-global agent config (Codex).
    Home(&'static str),
}

impl SkillLoc {
    /// The directory skills install under, or `None` if it can't be resolved
    /// (e.g. a `Home` location with `$HOME` unset).
    pub fn base(&self, root: &Path) -> Option<PathBuf> {
        match self {
            SkillLoc::Project(p) => Some(root.join(p)),
            SkillLoc::Home(p) => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(p)),
        }
    }

    /// Short human description of where skills land, for menus and messages.
    pub fn describe(&self) -> String {
        match self {
            SkillLoc::Project(p) => format!("{} (committed to this repo)", p),
            SkillLoc::Home(p) => format!("~/{} (user-global)", p),
        }
    }
}

/// A known agent CLI: how to launch it with a prompt, and where its skills live.
/// Launch invocations are best-effort — a failed launch falls back to printed
/// instructions, so a wrong guess never strands the user.
pub struct KnownAgent {
    pub label: &'static str,
    pub bin: &'static str,
    pub prompt_args: &'static [&'static str],
    /// Where skills install in the default in-repo mode.
    pub skills: Option<SkillLoc>,
    /// Where skills install in local mode, when it must differ to avoid writing
    /// into the repo. `None` means the in-repo location already doesn't touch
    /// the repo (e.g. a user-global one), so it's reused unchanged.
    pub skills_local: Option<SkillLoc>,
    /// The agent's user-global memory/instructions file, `$HOME`-relative, where
    /// `install memory` records repos that have an engrym KB. `None` if unknown.
    pub memory: Option<&'static str>,
}

pub const KNOWN_AGENTS: &[KnownAgent] = &[
    KnownAgent {
        label: "Claude Code",
        bin: "claude",
        prompt_args: &["{prompt}"],
        // Project-level by default (committed); user-global in local mode so the
        // repo stays untouched.
        skills: Some(SkillLoc::Project(".claude/skills")),
        skills_local: Some(SkillLoc::Home(".claude/skills")),
        memory: Some(".claude/CLAUDE.md"),
    },
    KnownAgent {
        label: "OpenAI Codex",
        bin: "codex",
        prompt_args: &["{prompt}"],
        skills: Some(SkillLoc::Home(".codex/skills")),
        skills_local: None, // already user-global — never touches the repo
        memory: Some(".codex/AGENTS.md"), // CODEX_HOME defaults to ~/.codex
    },
    KnownAgent { label: "Gemini CLI", bin: "gemini", prompt_args: &["-i", "{prompt}"], skills: None, skills_local: None, memory: None },
    KnownAgent { label: "Aider", bin: "aider", prompt_args: &["--message", "{prompt}"], skills: None, skills_local: None, memory: None },
    KnownAgent { label: "opencode", bin: "opencode", prompt_args: &["run", "{prompt}"], skills: None, skills_local: None, memory: None },
    KnownAgent { label: "Amp", bin: "amp", prompt_args: &["{prompt}"], skills: None, skills_local: None, memory: None },
    KnownAgent { label: "Goose", bin: "goose", prompt_args: &["run", "-t", "{prompt}"], skills: None, skills_local: None, memory: None },
];

impl KnownAgent {
    /// Where this agent reads skills in the given mode. In local mode prefer a
    /// repo-free location, falling back to the in-repo one (which for already
    /// user-global agents never touched the repo anyway).
    pub fn skills_for(&self, local: bool) -> Option<&SkillLoc> {
        if local {
            self.skills_local.as_ref().or(self.skills.as_ref())
        } else {
            self.skills.as_ref()
        }
    }

    /// Whether engrym can auto-install skills for this agent in any mode.
    pub fn has_skills(&self) -> bool {
        self.skills.is_some() || self.skills_local.is_some()
    }

    /// Whether engrym knows this agent's global memory file.
    pub fn has_memory(&self) -> bool {
        self.memory.is_some()
    }

    /// The resolved path to the agent's global memory file (`None` if unknown or
    /// `$HOME` is unset).
    pub fn memory_file(&self) -> Option<PathBuf> {
        let rel = self.memory?;
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rel))
    }
}

/// The resolved agent decision for `init`.
pub enum Choice {
    Known(&'static KnownAgent),
    Custom(String),
    Skip,
}

impl Choice {
    pub fn describe(&self) -> String {
        match self {
            Choice::Known(a) => a.bin.to_string(),
            Choice::Custom(_) => "custom".to_string(),
            Choice::Skip => "none".to_string(),
        }
    }
}

/// Resolve the agent decision from flags, or interactively when on a terminal.
/// `cmd` is `--agent-cmd`, `name` is `--agent`.
pub fn resolve_choice(cmd: Option<&str>, name: Option<&str>, json: bool) -> Result<Choice> {
    if let Some(cmd) = cmd {
        return Ok(Choice::Custom(cmd.to_string()));
    }
    if let Some(name) = name {
        let name = name.trim();
        if matches!(name.to_lowercase().as_str(), "none" | "skip" | "") {
            return Ok(Choice::Skip);
        }
        if let Some(a) = KNOWN_AGENTS.iter().find(|a| a.bin.eq_ignore_ascii_case(name)) {
            return Ok(Choice::Known(a));
        }
        return Ok(Choice::Custom(name.to_string()));
    }
    if !json && std::io::stdin().is_terminal() {
        return interactive_select();
    }
    Ok(Choice::Skip)
}

fn interactive_select() -> Result<Choice> {
    let installed: Vec<&'static KnownAgent> =
        KNOWN_AGENTS.iter().filter(|a| on_path(a.bin)).collect();

    println!("Which agent should build the knowledge base?");
    for (i, a) in installed.iter().enumerate() {
        let note = if a.has_skills() { "" } else { " — no skill auto-install" };
        println!("  {}. {} ({}){}", i + 1, a.label, a.bin, note);
    }
    if installed.is_empty() {
        println!("  (no known agent CLIs detected on PATH)");
    }
    let custom_n = installed.len() + 1;
    let skip_n = installed.len() + 2;
    println!("  {}. Enter a custom command", custom_n);
    println!("  {}. Skip — just scaffold", skip_n);

    let choice = prompt(&format!("Select [1-{}]: ", skip_n), &skip_n.to_string())?;
    match choice.parse::<usize>() {
        Ok(n) if n >= 1 && n <= installed.len() => Ok(Choice::Known(installed[n - 1])),
        Ok(n) if n == custom_n => {
            let cmd = prompt("Command (use {prompt} for the task, e.g. `myagent chat`): ", "")?;
            if cmd.is_empty() {
                Ok(Choice::Skip)
            } else {
                Ok(Choice::Custom(cmd))
            }
        }
        _ => Ok(Choice::Skip),
    }
}

/// Resolve which known *skill-capable* agent to act on. `name` is an optional
/// `--agent <bin>`; `local` selects the local-mode location for menu display;
/// `action` ("Install" / "Remove") labels the interactive prompt. Returns `None`
/// when nothing is chosen (skip / no terminal). Errors if a named agent is
/// unknown or has no skill location.
pub fn resolve_skill_agent(
    name: Option<&str>,
    local: bool,
    action: &str,
    json: bool,
) -> Result<Option<&'static KnownAgent>> {
    if let Some(name) = name {
        let name = name.trim();
        if matches!(name.to_lowercase().as_str(), "none" | "skip" | "") {
            return Ok(None);
        }
        return match KNOWN_AGENTS.iter().find(|a| a.bin.eq_ignore_ascii_case(name)) {
            Some(a) if a.has_skills() => Ok(Some(a)),
            Some(a) => bail!(
                "{} has no engrym skill directory (it uses the CLI directly)",
                a.label
            ),
            None => bail!("unknown agent `{}` (skill-capable: {})", name, skill_capable_bins()),
        };
    }
    if !json && std::io::stdin().is_terminal() {
        return interactive_skill_select(local, action);
    }
    Ok(None)
}

fn interactive_skill_select(local: bool, action: &str) -> Result<Option<&'static KnownAgent>> {
    let capable: Vec<&'static KnownAgent> =
        KNOWN_AGENTS.iter().filter(|a| a.has_skills()).collect();

    println!("{} engrym skills for which agent?", action);
    for (i, a) in capable.iter().enumerate() {
        // `skills_for` is Some by construction of `capable`.
        let dest = a.skills_for(local).map(|l| l.describe()).unwrap_or_default();
        println!("  {}. {} → {}", i + 1, a.label, dest);
    }
    let cancel_n = capable.len() + 1;
    println!("  {}. Cancel", cancel_n);

    let choice = prompt(&format!("Select [1-{}]: ", cancel_n), &cancel_n.to_string())?;
    match choice.parse::<usize>() {
        Ok(n) if n >= 1 && n <= capable.len() => Ok(Some(capable[n - 1])),
        _ => Ok(None),
    }
}

/// Install the engrym skills for one agent, into its native skill directory for
/// the given mode (`local` routes to a repo-free location where one exists).
/// Returns the installed files' display paths (empty if the agent has no skill
/// location, or `$HOME` is unset for a user-global one).
pub fn install_skills_for(agent: &KnownAgent, root: &Path, local: bool) -> Result<Vec<String>> {
    let Some(loc) = agent.skills_for(local) else {
        return Ok(vec![]);
    };
    let Some(base) = loc.base(root) else {
        return Ok(vec![]); // e.g. $HOME unset
    };
    let mut written = Vec::new();
    for (name, content) in SKILLS {
        let path = base.join(name).join("SKILL.md");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&path, *content).with_context(|| format!("writing {}", path.display()))?;
        written.push(display_path(&path, root));
    }
    Ok(written)
}

/// Remove the engrym skills for one agent from its skill directory (the inverse
/// of [`install_skills_for`]). Returns the removed directories' display paths
/// (empty if none were present, or the agent has no skill location).
pub fn remove_skills_for(agent: &KnownAgent, root: &Path, local: bool) -> Result<Vec<String>> {
    let Some(loc) = agent.skills_for(local) else {
        return Ok(vec![]);
    };
    let Some(base) = loc.base(root) else {
        return Ok(vec![]);
    };
    let mut removed = Vec::new();
    for (name, _content) in SKILLS {
        let dir = base.join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
            removed.push(display_path(&dir, root));
        }
    }
    Ok(removed)
}

/// A PATH directory to link engrym into (or remove it from). Prefer one already
/// on PATH so the link is usable immediately; otherwise fall back to the
/// conventional `~/.local/bin`.
pub fn default_bin_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("$HOME is not set; pass --dir to choose the engrym bin directory")?;
    let local = home.join(".local/bin");
    let cargo = home.join(".cargo/bin");
    if dir_on_path(&local) {
        return Ok(local);
    }
    if dir_on_path(&cargo) {
        return Ok(cargo);
    }
    Ok(local)
}

fn skill_capable_bins() -> String {
    KNOWN_AGENTS
        .iter()
        .filter(|a| a.has_skills())
        .map(|a| a.bin)
        .collect::<Vec<_>>()
        .join(", ")
}

// --- Global memory note ---------------------------------------------------
//
// `install memory` records a repo in the agent's user-global memory file, so the
// agent learns "this repo has an engrym KB" without anything being written into
// the repo itself. engrym owns a marker-delimited block; everything outside it is
// left untouched.

const MEM_BEGIN: &str = "<!-- engrym:begin (managed by `engrym install memory`) -->";
const MEM_END: &str = "<!-- engrym:end -->";
const MEM_HEADING: &str = "## engrym knowledge bases";
const MEM_PROSE: &str = "The repositories listed below have an engrym knowledge base, queryable with the \
`engrym` CLI (or the `engrym` skill). When working inside one, retrieve relevant knowledge before a \
task and capture durable, non-obvious findings after — especially for a local KB, where there is no \
engrym.toml in the repo to signal that engrym is available.";

/// Resolve which known *memory-capable* agent to act on, mirroring
/// [`resolve_skill_agent`]. `action` ("Add" / "Remove") labels the prompt.
pub fn resolve_memory_agent(
    name: Option<&str>,
    action: &str,
    json: bool,
) -> Result<Option<&'static KnownAgent>> {
    if let Some(name) = name {
        let name = name.trim();
        if matches!(name.to_lowercase().as_str(), "none" | "skip" | "") {
            return Ok(None);
        }
        return match KNOWN_AGENTS.iter().find(|a| a.bin.eq_ignore_ascii_case(name)) {
            Some(a) if a.has_memory() => Ok(Some(a)),
            Some(a) => bail!("engrym doesn't know {}'s global memory file", a.label),
            None => bail!("unknown agent `{}` (memory-capable: {})", name, memory_capable_bins()),
        };
    }
    if !json && std::io::stdin().is_terminal() {
        let capable: Vec<&'static KnownAgent> =
            KNOWN_AGENTS.iter().filter(|a| a.has_memory()).collect();
        println!("{} the engrym memory note in which agent's memory?", action);
        for (i, a) in capable.iter().enumerate() {
            println!("  {}. {} → ~/{}", i + 1, a.label, a.memory.unwrap());
        }
        let cancel = capable.len() + 1;
        println!("  {}. Cancel", cancel);
        let choice = prompt(&format!("Select [1-{}]: ", cancel), &cancel.to_string())?;
        return Ok(match choice.parse::<usize>() {
            Ok(n) if n >= 1 && n <= capable.len() => Some(capable[n - 1]),
            _ => None,
        });
    }
    Ok(None)
}

/// Record `repo` in the agent's memory file. Returns (file, added?) — `added` is
/// false when it was already listed.
pub fn add_memory_entry(agent: &KnownAgent, repo: &Path) -> Result<(PathBuf, bool)> {
    let file = agent.memory_file().context("$HOME is not set")?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let key = repo.to_string_lossy().into_owned();

    let mut paths = parse_memory_block(&existing);
    if paths.iter().any(|p| p == &key) {
        return Ok((file, false));
    }
    paths.push(key);
    paths.sort();
    paths.dedup();
    write_memory(&file, &render_memory_block(&existing, &paths))?;
    Ok((file, true))
}

/// Remove `repo` from the agent's memory file. Returns (file, removed?).
pub fn remove_memory_entry(agent: &KnownAgent, repo: &Path) -> Result<(PathBuf, bool)> {
    let file = agent.memory_file().context("$HOME is not set")?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let key = repo.to_string_lossy().into_owned();

    let mut paths = parse_memory_block(&existing);
    let before = paths.len();
    paths.retain(|p| p != &key);
    if paths.len() == before {
        return Ok((file, false));
    }
    write_memory(&file, &render_memory_block(&existing, &paths))?;
    Ok((file, true))
}

fn write_memory(file: &Path, content: &str) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(file, content).with_context(|| format!("writing {}", file.display()))
}

/// The repo paths currently listed in the managed block (empty if absent).
fn parse_memory_block(content: &str) -> Vec<String> {
    let Some(start) = content.find(MEM_BEGIN) else {
        return vec![];
    };
    let Some(end) = content[start..].find(MEM_END).map(|e| start + e) else {
        return vec![];
    };
    content[start..end]
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Splice the managed block (set to `paths`) into `content`, replacing any
/// existing block, appending if absent, or removing it entirely when empty.
fn render_memory_block(content: &str, paths: &[String]) -> String {
    let block = if paths.is_empty() {
        String::new()
    } else {
        let bullets: String = paths.iter().map(|p| format!("- {p}\n")).collect();
        format!("{MEM_BEGIN}\n{MEM_HEADING}\n\n{MEM_PROSE}\n\n{bullets}{MEM_END}")
    };

    if let Some(start) = content.find(MEM_BEGIN) {
        let end = content[start..]
            .find(MEM_END)
            .map(|e| start + e + MEM_END.len())
            .unwrap_or(content.len());
        let head = content[..start].trim_end();
        let tail = content[end..].trim_start_matches('\n');
        let mut out = String::new();
        if !head.is_empty() {
            out.push_str(head);
            out.push_str("\n\n");
        }
        if !block.is_empty() {
            out.push_str(&block);
            if !tail.is_empty() {
                out.push_str("\n\n");
            }
        }
        out.push_str(tail);
        return normalize_trailing(&out);
    }

    // No existing block.
    if block.is_empty() {
        return content.to_string();
    }
    if content.trim().is_empty() {
        return format!("{block}\n");
    }
    normalize_trailing(&format!("{}\n\n{}", content.trim_end(), block))
}

fn normalize_trailing(s: &str) -> String {
    let t = s.trim_end();
    if t.is_empty() {
        String::new()
    } else {
        format!("{t}\n")
    }
}

fn memory_capable_bins() -> String {
    KNOWN_AGENTS
        .iter()
        .filter(|a| a.has_memory())
        .map(|a| a.bin)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `bin` is found on `PATH`.
pub fn on_path(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file())
}

/// Whether `dir` is one of the entries in `$PATH` (compared directly and via
/// canonicalization, so symlinked PATH entries still match).
pub fn dir_on_path(dir: &Path) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let target = std::fs::canonicalize(dir).ok();
    std::env::split_paths(&paths).any(|p| {
        p == *dir || (target.is_some() && std::fs::canonicalize(&p).ok() == target)
    })
}

pub fn prompt(message: &str, default: &str) -> Result<String> {
    print!("{}", message);
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).context("reading input")?;
    let trimmed = line.trim();
    Ok(if trimmed.is_empty() { default.to_string() } else { trimmed.to_string() })
}

/// Display a written path relative to the repo, or `~`-relative, else absolute.
pub fn display_path(path: &Path, root: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(root) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    if let Some(home) = std::env::var_os("HOME") {
        if let Ok(rel) = path.strip_prefix(PathBuf::from(home)) {
            return format!("~/{}", rel.to_string_lossy().replace('\\', "/"));
        }
    }
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{parse_memory_block, render_memory_block};

    #[test]
    fn adds_block_to_empty_then_parses_it_back() {
        let out = render_memory_block("", &["/a".into(), "/b".into()]);
        assert!(out.contains(super::MEM_BEGIN) && out.contains(super::MEM_END));
        assert_eq!(parse_memory_block(&out), vec!["/a", "/b"]);
    }

    #[test]
    fn preserves_surrounding_content_when_splicing() {
        let original = "# My notes\n\nkeep me\n";
        let out = render_memory_block(original, &["/repo".into()]);
        assert!(out.starts_with("# My notes\n\nkeep me"), "lost prose: {out}");
        assert_eq!(parse_memory_block(&out), vec!["/repo"]);

        // Re-rendering with a new set replaces the block, not the prose.
        // (render preserves the given order; callers sort before rendering.)
        let out2 = render_memory_block(&out, &["/repo".into(), "/other".into()]);
        assert!(out2.starts_with("# My notes\n\nkeep me"), "{out2}");
        assert_eq!(parse_memory_block(&out2), vec!["/repo", "/other"]);
    }

    #[test]
    fn empty_paths_removes_block_but_keeps_prose() {
        let original = "# Notes\n\nkeep me\n";
        let with = render_memory_block(original, &["/repo".into()]);
        let without = render_memory_block(&with, &[]);
        assert!(!without.contains(super::MEM_BEGIN), "block should be gone: {without}");
        assert!(without.contains("keep me"));
        assert!(parse_memory_block(&without).is_empty());
    }
}
