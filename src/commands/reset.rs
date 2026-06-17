//! `engrym reset` — wipe a KB's *content*: every document and the derived index.
//!
//! It keeps `engrym.toml` (and, in local mode, the external project folder), so
//! the KB stays initialized and ready to rebuild from scratch — this is "empty
//! it", not "uninstall it". Deleting authored docs is irreversible, so it
//! confirms by default (override with `--yes`).

use crate::config::{Config, INDEX_DIR};
use anyhow::{bail, Context, Result};
use std::io::{IsTerminal, Write};
use walkdir::WalkDir;

pub fn run(config: &Config, yes: bool, json: bool) -> Result<()> {
    let docs_root = config.docs_root();
    let index_dir = config.repo_root.join(INDEX_DIR);

    // Safety: never delete the state root itself (e.g. a stray `docs.root = "."`).
    let repo_canon = std::fs::canonicalize(&config.repo_root).ok();
    let docs_canon = std::fs::canonicalize(&docs_root).ok();
    if docs_root.exists() && docs_canon.is_some() && docs_canon == repo_canon {
        bail!(
            "docs root resolves to the state root ({}) — refusing to delete it; \
             check `docs.root` in engrym.toml",
            config.repo_root.display()
        );
    }

    let doc_count = if docs_root.is_dir() {
        WalkDir::new(&docs_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path().extension().and_then(|x| x.to_str()) == Some("md")
            })
            .count()
    } else {
        0
    };
    let index_exists = index_dir.exists();

    if !docs_root.exists() && !index_exists {
        if json {
            println!("{}", serde_json::json!({ "reset": false, "reason": "nothing to delete" }));
        } else {
            println!("Nothing to reset — no docs or index found.");
        }
        return Ok(());
    }

    // Confirm before deleting authored content.
    if !yes {
        if json || !std::io::stdin().is_terminal() {
            bail!(
                "refusing to delete {} doc(s) and the index without confirmation — pass --yes",
                doc_count
            );
        }
        println!("This deletes ALL engrym content for this KB:");
        println!("  docs:  {} ({} markdown file(s))", docs_root.display(), doc_count);
        if index_exists {
            println!("  index: {}", index_dir.display());
        }
        println!("engrym.toml is kept. This cannot be undone.");
        print!("Type 'reset' to confirm: ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).context("reading confirmation")?;
        if line.trim() != "reset" {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Wipe docs (recreating an empty root) and the index directory.
    if docs_root.exists() {
        std::fs::remove_dir_all(&docs_root)
            .with_context(|| format!("removing {}", docs_root.display()))?;
    }
    std::fs::create_dir_all(&docs_root)
        .with_context(|| format!("recreating {}", docs_root.display()))?;
    if index_exists {
        std::fs::remove_dir_all(&index_dir)
            .with_context(|| format!("removing {}", index_dir.display()))?;
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "reset": true,
                "docs_root": docs_root.to_string_lossy(),
                "docs_deleted": doc_count,
                "index_removed": index_exists,
            })
        );
    } else {
        println!("Reset complete: deleted {} doc(s) and the index.", doc_count);
        println!("The KB is empty but still initialized ({}).", docs_root.display());
    }
    Ok(())
}
