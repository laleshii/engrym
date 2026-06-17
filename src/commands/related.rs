//! `engrym related <id>` — the typed graph neighborhood of a document.
//!
//! Shows outbound edges (this doc → others) and inbound edges (others → this
//! doc), grouped by relation type. Inbound `refines`/`part_of` answers "what
//! elaborates this overview?"; that's the abstract→specific drill-down axis.

use crate::config::Config;
use crate::db;
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};

pub fn run(config: &Config, id: &str, json: bool) -> Result<()> {
    let conn = db::open_existing(&config.index_path())?;

    let title: Option<String> = conn
        .query_row("SELECT title FROM docs WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .optional()?;
    if title.is_none() {
        bail!("no document with id `{}`", id);
    }

    // Outbound: edge type + target, with the target's title when it resolves.
    let mut out_stmt = conn.prepare(
        "SELECT e.type, e.dst, d.title
         FROM edges e LEFT JOIN docs d ON d.id = e.dst
         WHERE e.src = ?1
         ORDER BY e.type, e.dst",
    )?;
    let outbound: Vec<Edge> = out_stmt
        .query_map(params![id], map_edge)?
        .collect::<rusqlite::Result<_>>()?;

    // Inbound: who points at this doc?
    let mut in_stmt = conn.prepare(
        "SELECT e.type, e.src, d.title
         FROM edges e LEFT JOIN docs d ON d.id = e.src
         WHERE e.dst = ?1
         ORDER BY e.type, e.src",
    )?;
    let inbound: Vec<Edge> = in_stmt
        .query_map(params![id], map_edge)?
        .collect::<rusqlite::Result<_>>()?;

    if json {
        let out = serde_json::json!({
            "id": id,
            "title": title,
            "outbound": edges_json(&outbound),
            "inbound": edges_json(&inbound),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("\x1b[1m{}\x1b[0m — {}", id, title.unwrap_or_default());
        print_group("Outbound (this → target)", &outbound, true);
        print_group("Inbound (source → this)", &inbound, false);
    }
    Ok(())
}

struct Edge {
    edge_type: String,
    other: String,
    other_title: Option<String>,
}

fn map_edge(row: &rusqlite::Row) -> rusqlite::Result<Edge> {
    Ok(Edge {
        edge_type: row.get(0)?,
        other: row.get(1)?,
        other_title: row.get(2)?,
    })
}

fn edges_json(edges: &[Edge]) -> Vec<serde_json::Value> {
    edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "type": e.edge_type,
                "id": e.other,
                "title": e.other_title,
                "dangling": e.other_title.is_none(),
            })
        })
        .collect()
}

fn print_group(label: &str, edges: &[Edge], _outbound: bool) {
    if edges.is_empty() {
        return;
    }
    println!("  {}:", label);
    for e in edges {
        match &e.other_title {
            Some(t) => println!("    {:<12} {} — {}", e.edge_type, e.other, t),
            None => println!("    {:<12} {} \x1b[2m(dangling)\x1b[0m", e.edge_type, e.other),
        }
    }
}
