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

    // Where config / docs / index live: the repo itself, or an external folder
    // keyed by the repo (local mode, which never writes into the repo).
    let (state_dir, anchor) = if args.local {
        let anchor = config::repo_anchor(repo);
        let dir = config::local_project_dir(&anchor)
            .context("cannot resolve the engrym store (set $HOME or $ENGRYM_HOME)")?;
        (dir, Some(anchor))
    } else {
        (repo.clone(), None)
    };

    let config_path = state_dir.join(config::CONFIG_FILENAME);
    if config_path.exists() && !args.force {
        bail!(
            "{} already exists — this repo is already initialized (use --force to re-scaffold)",
            config_path.display()
        );
    }

    let mut created: Vec<String> = Vec::new();

    // Where the docs live, relative to the state root. In-repo init asks (the
    // repo may already use `docs/` for something else); local mode keeps `docs`
    // inside its own store. `--docs` overrides non-interactively.
    let docs_root = resolve_docs_root(&args)?;

    // Config: substitute the chosen docs root into the template, then (local
    // mode) prepend a header recording the binding.
    let base = CONFIG_TEMPLATE.replace("root = \"docs\"", &format!("root = \"{}\"", docs_root));
    let config_content = match &anchor {
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

    // Pick the agent, then install the skills into *its* directory only.
    let choice =
        agents::resolve_choice(args.agent_cmd.as_deref(), args.agent.as_deref(), args.json)?;
    if let Choice::Known(a) = &choice {
        created.extend(agents::install_skills_for(a, repo, args.local)?);

        // Record the repo in the agent's global memory (a user-global file — it
        // doesn't touch the repo) so the agent knows this repo has a KB. This
        // matters most in local mode (no in-repo cue at all), but it's done for
        // in-repo KBs too. Key it by the bound repo, not the external state dir.
        if a.has_memory() {
            // Key by the same anchor the standalone `install memory` and
            // discovery use, so entries match across commands.
            let (mem_file, added) = agents::add_memory_entry(a, &config::repo_anchor(repo))?;
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
                "local": args.local,
                "agent": choice.describe(),
                "created": created,
            })
        );
        return Ok(());
    }

    if args.local {
        println!("Initialized a local engrym KB for {}", repo.display());
        println!("  stored in {} (the repo is untouched)", state_dir.display());
    } else {
        println!("Initialized engrym in {}", repo.display());
    }
    for c in &created {
        println!("  + {}", c);
    }
    println!();

    act_on_choice(&choice, repo)
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
