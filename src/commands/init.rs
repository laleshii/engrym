//! `engrym init` — scaffold a repo for engrym, then hand off to an agent.
//!
//! engrym isn't an LLM, so it doesn't analyze the repo itself. `init` does the
//! deterministic part — write `engrym.toml`, create `docs/`, ignore `.engrym/` —
//! then asks which agent to use, installs the engrym skills into *that agent's*
//! skill directory (and no other), and launches it. The skill walks the agent
//! through building a high-value initial KB. If no agent is available (or
//! there's no terminal), it prints the next steps instead.
//!
//! The agent/skill machinery is shared with `install` — see [`super::agents`].

use super::agents::{self, Choice};
use crate::config;
use crate::registry::{self, Registry};
use anyhow::{bail, Context, Result};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const CONFIG_TEMPLATE: &str = include_str!("../../assets/engrym.toml.template");

const BOOTSTRAP_PROMPT: &str =
    "engrym is already initialized for this repository — do NOT run `engrym init` \
     again. (The knowledge base may be stored outside the repo, so a missing \
     engrym.toml in the working tree is expected, not a sign it needs setup.) Use \
     the engrym-bootstrap skill to analyze the repo and build a high-value \
     initial knowledge base, authoring documents with `engrym new`.";

pub struct InitArgs {
    /// Root of the repo to initialize (defaults to the cwd).
    pub root: PathBuf,
    /// Known agent to launch by binary name (e.g. "claude"), or "none".
    pub agent: Option<String>,
    /// Custom launch command template (whitespace-split; `{prompt}` substituted,
    /// else the prompt is appended).
    pub agent_cmd: Option<String>,
    /// Store the KB externally under `~/.engrym/projects/` instead of in the repo.
    pub local: bool,
    /// Docs directory (relative to the state root). Prompted for in interactive
    /// in-repo init; defaults to `docs`.
    pub docs: Option<String>,
    /// Re-scaffold even if `engrym.toml` already exists.
    pub force: bool,
    pub json: bool,
}

pub fn run(args: InitArgs) -> Result<()> {
    let repo = &args.root;
    let anchor = config::repo_anchor(repo);

    // Dedupe FIRST — before scaffolding, skill install, or the bootstrap
    // handoff: if a KB for this *same repo* already exists (a local store matched
    // by `origin` URL), offer to link this checkout to it rather than build a
    // second, disconnected one. Linking reuses the already-built KB, so we skip
    // scaffolding and bootstrapping below — but still install the skills, so the
    // agent in *this* checkout can query the shared KB.
    let linked_dir = reuse_existing_kb(&anchor, args.json)?;
    let linked = linked_dir.is_some();

    // Where config / docs / index live: the linked store, an external folder
    // (local mode), or the repo itself (in-repo). A linked checkout has no
    // in-repo footprint, so skills go to the repo-free location too.
    let skills_local = args.local || linked;
    let (state_dir, source_anchor) = match (&linked_dir, args.local) {
        (Some(dir), _) => (dir.clone(), Some(anchor.clone())),
        (None, true) => {
            let dir = config::local_project_dir(&anchor)
                .context("cannot resolve the engrym store (set $HOME or $ENGRYM_HOME)")?;
            (dir, Some(anchor.clone()))
        }
        (None, false) => (repo.clone(), None),
    };

    let mut created: Vec<String> = Vec::new();

    // Scaffold config + docs, unless we linked to an already-built KB.
    if !linked {
        let config_path = state_dir.join(config::CONFIG_FILENAME);
        if config_path.exists() && !args.force {
            bail!(
                "{} already exists — this repo is already initialized (use --force to re-scaffold)",
                config_path.display()
            );
        }

        // Where the docs live, relative to the state root. In-repo init asks (the
        // repo may already use `docs/` for something else); local mode keeps
        // `docs` inside its own store. `--docs` overrides non-interactively.
        let docs_root = resolve_docs_root(&args)?;
        let base = CONFIG_TEMPLATE.replace("root = \"docs\"", &format!("root = \"{}\"", docs_root));
        let config_content = match &source_anchor {
            Some(a) => local_config(&base, a, &state_dir),
            None => base,
        };
        write_if_needed(&config_path, &config_content, args.force, &mut created, repo)?;

        let docs_dir = state_dir.join(&docs_root);
        if !docs_dir.exists() {
            std::fs::create_dir_all(&docs_dir)
                .with_context(|| format!("creating {}", docs_dir.display()))?;
            created.push(agents::display_path(&docs_dir, repo));
        }

        // Only an in-repo index needs gitignoring; a local one is already outside.
        if !args.local {
            ensure_gitignore(repo, &mut created)?;
        }

        // Register a newly-created local store so future worktrees/clones of this
        // repo can discover it (and dedupe against it by origin URL).
        if let Some(a) = &source_anchor {
            if let Some(key) = state_dir.file_name().and_then(|s| s.to_str()) {
                let mut reg = Registry::load();
                if reg.link(a, key, registry::repo_identity(a)) {
                    let _ = reg.save();
                }
            }
        }
    }

    // Install the skills (fresh KB or linked one — the agent needs them either
    // way) and record the repo in the agent's global memory.
    let choice =
        agents::resolve_choice(args.agent_cmd.as_deref(), args.agent.as_deref(), args.json)?;
    if let Choice::Known(a) = &choice {
        created.extend(agents::install_skills_for(a, repo, skills_local)?);
        if a.has_memory() {
            // Key by the same anchor discovery and `install memory` use.
            let (mem_file, added) = agents::add_memory_entry(a, &anchor)?;
            if added {
                created.push(agents::display_path(&mem_file, repo));
            }
        }
    }

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "initialized": state_dir.to_string_lossy(),
                "repo": repo.to_string_lossy(),
                "local": skills_local,
                "linked": linked,
                "agent": choice.describe(),
                "created": created,
            })
        );
        return Ok(());
    }

    if linked {
        println!("Linked {} to the existing KB at {}", repo.display(), state_dir.display());
        println!("  (same repo detected by origin URL — reusing its knowledge, no new store)");
    } else if args.local {
        println!("Initialized a local engrym KB for {}", repo.display());
        println!("  stored in {} (the repo is untouched)", state_dir.display());
    } else {
        println!("Initialized engrym in {}", repo.display());
    }
    for c in &created {
        println!("  + {}", c);
    }
    println!();

    // A linked KB is already populated — skip the bootstrap handoff. A fresh one
    // hands off to the agent to build it.
    if linked {
        println!("The knowledge base is ready to query (e.g. `engrym search \"…\"`).");
        Ok(())
    } else {
        act_on_choice(&choice, repo)
    }
}

/// If a local KB for the *same repo* already exists under another clone (matched
/// by normalized `origin` URL), offer to link this checkout to it instead of
/// scaffolding a disconnected second store. Returns the shared store dir when
/// linked. Only prompts on an interactive terminal — non-interactive runs never
/// link silently, so automation stays predictable.
fn reuse_existing_kb(anchor: &Path, json: bool) -> Result<Option<PathBuf>> {
    // On first use (no registry yet), backfill from disk so dedupe sees
    // same-repo stores created before the registry existed.
    let reg = Registry::load_migrated();
    // Already mapped — the normal flow will report "already initialized".
    if reg.key_for_anchor(anchor).is_some() {
        return Ok(None);
    }
    let Some(identity) = registry::repo_identity(anchor) else { return Ok(None) };
    let Some(entry) = reg.find_by_identity(&identity) else { return Ok(None) };
    if !registry::store_exists(&entry.key) {
        return Ok(None);
    }
    let Some(dir) = config::projects_root().map(|r| r.join(&entry.key)) else {
        return Ok(None);
    };

    if json || !std::io::stdin().is_terminal() {
        return Ok(None); // never link without an explicit yes
    }
    println!("An engrym KB for this repo already exists (origin {identity}):");
    println!("  {}", dir.display());
    let ans = agents::prompt("Reuse it for this checkout? [Y/n]: ", "y")?;
    if ans.trim().eq_ignore_ascii_case("n") || ans.trim().eq_ignore_ascii_case("no") {
        return Ok(None);
    }
    let key = entry.key.clone();
    let mut reg = reg;
    reg.link(anchor, &key, Some(identity));
    reg.save().context("saving the registry")?;
    Ok(Some(dir))
}

/// Decide the docs directory (relative to the state root). `--docs` wins; else
/// in-repo interactive init asks (defaulting to `docs`); else `docs`.
fn resolve_docs_root(args: &InitArgs) -> Result<String> {
    if let Some(d) = &args.docs {
        return valid_docs_root(d);
    }
    // Only prompt for an in-repo KB on a real terminal — that's where the docs
    // dir lands next to the user's code and might collide.
    if !args.local && !args.json && std::io::stdin().is_terminal() {
        let entered = agents::prompt(
            "Docs directory, relative to the repo [docs]: ",
            "docs",
        )?;
        return valid_docs_root(&entered);
    }
    Ok("docs".to_string())
}

/// Validate a docs root: a non-empty relative path inside the repo. Rejects
/// absolute paths, `.` (would alias the repo root), and `..` escapes.
fn valid_docs_root(s: &str) -> Result<String> {
    let s = s.trim().trim_end_matches('/');
    if s.is_empty() {
        return Ok("docs".to_string());
    }
    if Path::new(s).is_absolute() || s == "." || s.split('/').any(|c| c == "..") {
        bail!("docs directory must be a relative path inside the repo (got `{}`)", s);
    }
    Ok(s.to_string())
}

/// Build the config for a local KB: the shared template body, prefixed with a
/// header that records the bound repo and store location.
fn local_config(template: &str, repo: &Path, store: &Path) -> String {
    let body = template.find("[docs]").map(|i| &template[i..]).unwrap_or(template);
    format!(
        "# engrym knowledge base — stored locally, OUTSIDE the code repository.\n\
         # Bound to repo: {}\n\
         # Lives in:      {}\n\
         # The repo is never modified; delete that folder to reset this KB.\n\n{}",
        repo.display(),
        store.display(),
        body
    )
}

/// Launch the chosen agent, or print next steps.
fn act_on_choice(choice: &Choice, root: &Path) -> Result<()> {
    match choice {
        Choice::Skip => {
            println!(
                "No agent launched. Re-run with `--agent claude` or `--agent codex` to install \
                 the skills, or drive it yourself:"
            );
            print_manual_steps();
            Ok(())
        }
        Choice::Known(a) => {
            if a.skills.is_none() {
                println!(
                    "Note: no known skill location for {}, so the skills weren't installed — \
                     the engrym CLI works regardless.",
                    a.label
                );
            }
            try_launch(root, a.bin, &resolve_prompt_args(a.prompt_args))
        }
        Choice::Custom(cmd) => {
            println!("Note: skills aren't auto-installed for custom commands.");
            try_custom(root, cmd)
        }
    }
}

fn resolve_prompt_args(template: &[&str]) -> Vec<String> {
    template.iter().map(|a| a.replace("{prompt}", BOOTSTRAP_PROMPT)).collect()
}

fn try_launch(root: &Path, program: &str, args: &[String]) -> Result<()> {
    match launch(root, program, args) {
        Ok(()) => Ok(()),
        Err(_) => {
            println!("Couldn't launch `{}` (is it installed and on PATH?).", program);
            print_manual_steps();
            Ok(())
        }
    }
}

/// Run a custom command template: whitespace-split, substitute `{prompt}` (or
/// append the prompt if the placeholder is absent).
fn try_custom(root: &Path, template: &str) -> Result<()> {
    let mut parts: Vec<String> = template.split_whitespace().map(String::from).collect();
    if parts.is_empty() {
        print_manual_steps();
        return Ok(());
    }
    let had_placeholder = parts.iter().any(|p| p.contains("{prompt}"));
    for p in parts.iter_mut() {
        *p = p.replace("{prompt}", BOOTSTRAP_PROMPT);
    }
    if !had_placeholder {
        parts.push(BOOTSTRAP_PROMPT.to_string());
    }
    let (program, rest) = parts.split_first().unwrap();
    try_launch(root, program, rest)
}

fn launch(root: &Path, program: &str, args: &[String]) -> Result<()> {
    println!("Launching {} to build the knowledge base…\n", program);
    let status = Command::new(program)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| format!("launching {}", program))?;
    if !status.success() {
        bail!("{} exited with status {}", program, status);
    }
    Ok(())
}

fn print_manual_steps() {
    println!("Open this repo in your agent and prompt it with:");
    println!("  \"{}\"", BOOTSTRAP_PROMPT);
    println!("Then: engrym lint --strict && engrym index");
}

fn write_if_needed(
    path: &Path,
    content: &str,
    overwrite: bool,
    created: &mut Vec<String>,
    display_root: &Path,
) -> Result<()> {
    if path.exists() && !overwrite {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    created.push(agents::display_path(path, display_root));
    Ok(())
}

/// Ensure `.engrym/` is gitignored, appending to an existing `.gitignore` or
/// creating one.
fn ensure_gitignore(root: &Path, created: &mut Vec<String>) -> Result<()> {
    let path = root.join(".gitignore");
    let entry = ".engrym/";
    if path.exists() {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current.lines().any(|l| l.trim() == entry) {
            return Ok(());
        }
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let prefix = if current.ends_with('\n') || current.is_empty() { "" } else { "\n" };
        writeln!(f, "{}\n# engrym derived index (rebuildable)\n{}", prefix, entry)
            .context("appending to .gitignore")?;
        created.push(".gitignore (+ .engrym/)".into());
    } else {
        std::fs::write(&path, format!("# engrym derived index (rebuildable)\n{}\n", entry))
            .with_context(|| format!("writing {}", path.display()))?;
        created.push(".gitignore".into());
    }
    Ok(())
}
