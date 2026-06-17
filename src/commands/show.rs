//! `engrym show <id>` — print a document.
//!
//! Default prints the raw Markdown file (full fidelity for a human). `--json`
//! returns structured metadata + body for an agent.

use crate::config::Config;
use crate::db;
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};

pub fn run(config: &Config, id: &str, json: bool) -> Result<()> {
    let conn = db::open_existing(&config.index_path())?;

    let row = conn
        .query_row(
            "SELECT path, title, altitude, summary FROM docs WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;

    let Some((rel_path, title, altitude, summary)) = row else {
        bail!("no document with id `{}`", id);
    };

    let abs_path = config.docs_root().join(&rel_path);
    let content = std::fs::read_to_string(&abs_path).unwrap_or_default();

    if json {
        // Topics for completeness in the structured view.
        let mut tstmt = conn.prepare("SELECT path FROM topics WHERE doc_id = ?1 ORDER BY path")?;
        let topics: Vec<String> = tstmt
            .query_map(params![id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        let out = serde_json::json!({
            "id": id,
            "title": title,
            "altitude": altitude,
            "summary": summary,
            "topics": topics,
            "path": rel_path,
            "content": content,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("{}", content);
    }
    Ok(())
}
