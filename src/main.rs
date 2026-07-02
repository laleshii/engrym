//! engrym — a fast, AI-first knowledge base layered over Markdown.
//!
//! The Markdown files (with YAML frontmatter) are the source of truth; the
//! SQLite index under `.engrym/` is a disposable, rebuildable cache. This binary
//! is the query surface — point it at any repo containing an `engrym.toml`.

mod commands;
mod config;
mod daemon;
mod db;
mod embed;
mod model;
mod parse;
mod registry;
mod vector;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "engrym",
    version,
    about = "Fast, AI-first knowledge base over Markdown — typed relations, topic hierarchies, hybrid search.",
    propagate_version = true
)]
struct Cli {
    /// Emit machine-readable JSON instead of human-readable output.
    #[arg(long, global = true)]
    json: bool,

    /// Start the repo search from this directory (defaults to the cwd).
    #[arg(long, global = true, value_name = "DIR")]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// (Re)build the index from the Markdown docs.
    Index {
        /// Rebuild structure only; skip (re)embedding passages.
        #[arg(long)]
        no_embed: bool,
    },

    /// Hybrid search over passages (keyword BM25 + semantic, fused via RRF).
    Search {
        /// The search query.
        query: Vec<String>,
        /// Maximum number of passages to return.
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Restrict to a single altitude level (0=overview … 3=detail).
        #[arg(short, long)]
        altitude: Option<i64>,
        /// Keyword-only ranking (BM25); skips loading the embedding model.
        #[arg(long, conflicts_with = "semantic")]
        keyword: bool,
        /// Semantic-only ranking (vector cosine).
        #[arg(long)]
        semantic: bool,
    },

    /// List documents at or below a topic subtree.
    Topic {
        /// Topic path, e.g. `backend/auth`.
        path: String,
    },

    /// Show the typed graph neighborhood of a document.
    Related {
        /// Document id.
        id: String,
    },

    /// Print a document (raw Markdown, or `--json` for structured form).
    Show {
        /// Document id.
        id: String,
    },

    /// Scaffold a repo for engrym and hand off to an agent to build the KB.
    Init {
        /// Known agent to launch by binary name (e.g. `claude`), or `none`.
        #[arg(long)]
        agent: Option<String>,
        /// Custom launch command (e.g. "myagent chat"); `{prompt}` is substituted.
        #[arg(long)]
        agent_cmd: Option<String>,
        /// Store the KB externally under ~/.engrym/projects/ — never touch the repo.
        #[arg(long)]
        local: bool,
        /// Docs directory relative to the repo (default `docs`; prompted if interactive).
        #[arg(long)]
        docs: Option<String>,
        /// Re-scaffold even if `engrym.toml` already exists.
        #[arg(long)]
        force: bool,
    },

    /// Install the agent skills, or record this repo in an agent's memory.
    Install {
        #[command(subcommand)]
        target: InstallTarget,
    },

    /// Remove the agent skills or memory entry (inverse of `install`).
    Uninstall {
        #[command(subcommand)]
        target: UninstallTarget,
    },

    /// Delete all documents and the index (keeps `engrym.toml`).
    Reset {
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },

    /// Completely remove engrym from this repo (inverse of `init`).
    Deinit {
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },

    /// Report whether a KB is reachable here (a fast gate for skills/agents).
    /// Exits non-zero when none is found.
    Where,

    /// List the local (external) KB stores and how they're shared across clones.
    List,

    /// Share this checkout's KB with another clone/worktree of the same repo.
    Link {
        /// A store key (`engrym list`) or a path to another checkout of the repo.
        target: String,
    },

    /// Detach this checkout from a shared KB (reverting to its own store).
    Unlink,

    /// Serve a local web UI to read and navigate the KB.
    Browse {
        /// Port to bind on localhost (0 = pick a free one).
        #[arg(long, default_value_t = 7345)]
        port: u16,
        /// Open the URL in your browser.
        #[arg(long)]
        open: bool,
    },

    /// Move documents on disk to match the docs layout.
    Relocate {
        /// Only relocate this document (default: all).
        id: Option<String>,
        /// Layout to use (default: `[docs] layout` from config).
        #[arg(long, value_enum)]
        layout: Option<config::Layout>,
        /// Show what would move without moving anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Create a new document with valid frontmatter.
    #[command(visible_alias = "create")]
    New {
        /// Stable kebab-case id (also the default filename).
        id: String,
        #[arg(long)]
        title: String,
        /// Altitude: 0=overview … 3=implementation detail.
        #[arg(short, long)]
        altitude: i64,
        /// Topic path (repeatable), e.g. `backend/auth/oauth`.
        #[arg(short, long = "topic")]
        topics: Vec<String>,
        /// Typed relation `type:target` (repeatable).
        #[arg(short, long = "relation")]
        relations: Vec<String>,
        #[arg(long)]
        summary: Option<String>,
        /// File path relative to docs root (default `<id>.md`).
        #[arg(long)]
        path: Option<String>,
        /// Body text (otherwise a minimal scaffold, or `--stdin`).
        #[arg(long)]
        body: Option<String>,
        /// Read the body from stdin.
        #[arg(long)]
        stdin: bool,
        /// Overwrite an existing file at the target path.
        #[arg(long)]
        force: bool,
    },

    /// Update an existing document's frontmatter (and optionally its body).
    #[command(visible_alias = "update")]
    Set {
        /// Document id.
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(short, long)]
        altitude: Option<i64>,
        /// Set the summary (empty string clears it).
        #[arg(long)]
        summary: Option<String>,
        #[arg(long = "add-topic")]
        add_topics: Vec<String>,
        #[arg(long = "remove-topic")]
        remove_topics: Vec<String>,
        #[arg(long = "add-relation")]
        add_relations: Vec<String>,
        #[arg(long = "remove-relation")]
        remove_relations: Vec<String>,
        /// Replace the body with stdin.
        #[arg(long = "body-stdin")]
        body_stdin: bool,
    },

    /// Delete a document.
    #[command(visible_alias = "delete")]
    Rm {
        /// Document id.
        id: String,
        /// Delete even if other documents reference it.
        #[arg(long)]
        force: bool,
    },

    /// Validate the KB against the frontmatter contract.
    Lint {
        /// Treat warnings as errors (use in CI).
        #[arg(long)]
        strict: bool,
    },

    /// Run the warm embedding daemon. Usually auto-spawned by `search`; run it
    /// manually to pre-warm, or with `--stop` to shut a running one down.
    Serve {
        /// Seconds of inactivity before the daemon self-terminates.
        #[arg(long, default_value_t = 300)]
        idle: u64,
        /// Stop a running daemon for this repo instead of starting one.
        #[arg(long)]
        stop: bool,
    },
}

#[derive(Subcommand)]
enum InstallTarget {
    /// (Re)install the engrym agent skills into an agent's skill directory.
    Skills {
        /// Agent to install for (e.g. `claude`, `codex`); prompts if omitted.
        #[arg(long)]
        agent: Option<String>,
        /// Use the local-mode (repo-free) skill location where the agent needs one.
        #[arg(long)]
        local: bool,
        /// Refresh every already-installed location (project + user-global) to the
        /// running binary's version. Use after upgrading engrym. Ignores --agent.
        #[arg(long)]
        refresh: bool,
    },

    /// Record this repo in an agent's global memory (so it knows the repo has a KB).
    Memory {
        /// Agent whose memory to update (e.g. `claude`, `codex`); prompts if omitted.
        #[arg(long)]
        agent: Option<String>,
    },
}

#[derive(Subcommand)]
enum UninstallTarget {
    /// Remove the engrym agent skills from an agent's skill directory.
    Skills {
        /// Agent to remove from (e.g. `claude`, `codex`); prompts if omitted.
        #[arg(long)]
        agent: Option<String>,
        /// Target the local-mode (repo-free) skill location.
        #[arg(long)]
        local: bool,
    },

    /// Remove this repo from an agent's global memory (inverse of `install memory`).
    Memory {
        /// Agent whose memory to update (e.g. `claude`, `codex`); prompts if omitted.
        #[arg(long)]
        agent: Option<String>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("\x1b[31merror:\x1b[0m {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let start = cli
        .repo
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // `init` and `install` run *before* a config exists, so they can't (and
    // needn't) discover one: `init` scaffolds it, `install` only touches an
    // agent's skill dir or memory.
    if let Command::Init { agent, agent_cmd, local, docs, force } = cli.command {
        return commands::init::run(commands::init::InitArgs {
            root: start,
            agent,
            agent_cmd,
            local,
            docs,
            force,
            json: cli.json,
        });
    }
    if let Command::Install { target } = cli.command {
        let target = match target {
            InstallTarget::Skills { agent, local, refresh } => {
                commands::install::Target::Skills { agent, local, refresh }
            }
            InstallTarget::Memory { agent } => commands::install::Target::Memory { agent },
        };
        return commands::install::run(commands::install::InstallArgs {
            root: start,
            target,
            json: cli.json,
        });
    }
    if let Command::Uninstall { target } = cli.command {
        let target = match target {
            UninstallTarget::Skills { agent, local } => {
                commands::uninstall::Target::Skills { agent, local }
            }
            UninstallTarget::Memory { agent } => commands::uninstall::Target::Memory { agent },
        };
        return commands::uninstall::run(commands::uninstall::UninstallArgs {
            root: start,
            target,
            json: cli.json,
        });
    }
    // `deinit` does its own (optional) discovery — it must work even when the
    // config is already gone, so it can't go through the discover-or-fail path.
    if let Command::Deinit { yes } = cli.command {
        return commands::deinit::run(&start, yes, cli.json);
    }

    // KB discovery/linking runs before config discovery: `where` must answer
    // "no KB" gracefully (not error), and link/unlink/list operate on the
    // registry regardless of whether a config is present here.
    match cli.command {
        Command::Where => {
            let present = commands::kb::where_(&start, cli.json)?;
            std::process::exit(if present { 0 } else { 1 });
        }
        Command::List => return commands::kb::list(cli.json),
        Command::Link { ref target } => return commands::kb::link(&start, target, cli.json),
        Command::Unlink => return commands::kb::unlink(&start, cli.json),
        _ => {}
    }

    let config = Config::discover(&start)?;

    match cli.command {
        Command::Init { .. }
        | Command::Install { .. }
        | Command::Uninstall { .. }
        | Command::Deinit { .. }
        | Command::Where
        | Command::List
        | Command::Link { .. }
        | Command::Unlink => unreachable!("handled above"),
        Command::Reset { yes } => commands::reset::run(&config, yes, cli.json),
        Command::Browse { port, open } => commands::browse::run(&config, port, open),
        Command::Index { no_embed } => commands::index::run(&config, no_embed, cli.json),
        Command::Search {
            query,
            limit,
            altitude,
            keyword,
            semantic,
        } => {
            let mode = if keyword {
                commands::search::Mode::Keyword
            } else if semantic {
                commands::search::Mode::Semantic
            } else {
                commands::search::Mode::Hybrid
            };
            let args = commands::search::Args {
                query: query.join(" "),
                limit,
                altitude,
                mode,
            };
            commands::search::run(&config, &args, cli.json)
        }
        Command::Topic { path } => commands::topic::run(&config, &path, cli.json),
        Command::Related { id } => commands::related::run(&config, &id, cli.json),
        Command::Show { id } => commands::show::run(&config, &id, cli.json),
        Command::New {
            id,
            title,
            altitude,
            topics,
            relations,
            summary,
            path,
            body,
            stdin,
            force,
        } => commands::author::new(
            &config,
            commands::author::NewArgs {
                id,
                title,
                altitude,
                topics,
                relations,
                summary,
                path,
                body,
                stdin,
                force,
            },
            cli.json,
        ),
        Command::Set {
            id,
            title,
            altitude,
            summary,
            add_topics,
            remove_topics,
            add_relations,
            remove_relations,
            body_stdin,
        } => commands::author::set(
            &config,
            commands::author::SetArgs {
                id,
                title,
                altitude,
                summary,
                add_topics,
                remove_topics,
                add_relations,
                remove_relations,
                body_stdin,
            },
            cli.json,
        ),
        Command::Rm { id, force } => commands::author::rm(&config, &id, force, cli.json),
        Command::Relocate { id, layout, dry_run } => {
            commands::author::relocate(&config, id.as_deref(), layout, dry_run, cli.json)
        }
        Command::Lint { strict } => {
            // `--strict` flag OR `[lint] strict = true` in config.
            let strict = strict || config.lint.strict;
            let passed = commands::lint::run(&config, strict, cli.json)?;
            if !passed {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Serve { idle, stop } => {
            if stop {
                let stopped = daemon::stop(&config)?;
                if cli.json {
                    println!("{}", serde_json::json!({ "stopped": stopped }));
                } else if stopped {
                    println!("Stopped the engrym daemon.");
                } else {
                    println!("No running daemon for this repo.");
                }
                Ok(())
            } else {
                daemon::serve(&config, idle)
            }
        }
    }
}
