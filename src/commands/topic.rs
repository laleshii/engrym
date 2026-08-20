//! `engrym topic <path>` — list docs at or below a topic subtree.
//!
//! Topics are slash-delimited paths; a query for `backend/auth` returns docs
//! tagged `backend/auth` and anything deeper (`backend/auth/oauth`, …). Across a
//! workspace the same taxonomy usually spans services, so results are grouped by
//! the repo that owns them.

use crate::config::Config;
use crate::db;
use crate::workspace::Workspace;
use anyhow::Result;
use rusqlite::params;

pub fn run(ws: &Workspace, path: &str, json: bool) -> Result<()> {
    let prefix = path.trim_matches('/');
    let mut groups: Vec<Group> = Vec::new();

    for member in &ws.members {
        match rows_for(&member.config, prefix) {
            Ok(rows) => groups.push(Group { repo: member.name.clone(), rows }),
            Err(e) => {
                if !ws.spans_repos {
                    return Err(e);
                }
                eprintln!("\x1b[33mwarning:\x1b[0m {} skipped ({:#})", member.name, e);
            }
        }
    }
    groups.retain(|g| !g.rows.is_empty());

    if ws.spans_repos {
        render_workspace(ws, prefix, &groups, json)
    } else {
        render_single(prefix, groups.first().map(|g| g.rows.as_slice()).unwrap_or(&[]), json)
    }
}

fn rows_for(config: &Config, prefix: &str) -> Result<Vec<Row>> {
    let conn = db::open_existing(&config.index_path())?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT d.id, d.title, d.altitude, d.summary
         FROM topics t
         JOIN docs d ON d.id = t.doc_id
         WHERE t.path = ?1 OR t.path LIKE ?1 || '/%'
         ORDER BY d.altitude, d.id",
    )?;
    let rows = stmt
        .query_map(params![prefix], |row| {
            Ok(Row {
                id: row.get(0)?,
                title: row.get(1)?,
                altitude: row.get(2)?,
                summary: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn render_single(prefix: &str, rows: &[Row], json: bool) -> Result<()> {
    if json {
        let arr: Vec<_> = rows.iter().map(row_json).collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if rows.is_empty() {
        println!("No documents under topic \"{}\".", prefix);
    } else {
        println!("Documents under \x1b[1m{}\x1b[0m:", prefix);
        print_rows(rows, "");
    }
    Ok(())
}

fn render_workspace(ws: &Workspace, prefix: &str, groups: &[Group], json: bool) -> Result<()> {
    if json {
        let out = serde_json::json!({
            "workspace": ws.json(),
            "topic": prefix,
            "groups": groups.iter().map(|g| serde_json::json!({
                "repo": g.repo,
                "docs": g.rows.iter().map(|r| {
                    let mut v = row_json(r);
                    v["repo"] = serde_json::json!(g.repo);
                    v["ref"] = serde_json::json!(format!("{}:{}", g.repo, r.id));
                    v
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    if groups.is_empty() {
        println!(
            "No documents under topic \"{}\" in any of the {} KB(s) under {}.",
            prefix,
            ws.members.len(),
            ws.root.display()
        );
        return Ok(());
    }
    println!("Documents under \x1b[1m{}\x1b[0m:\n", prefix);
    for g in groups {
        println!("\x1b[1;36m{}\x1b[0m \x1b[2m({} doc(s))\x1b[0m", g.repo, g.rows.len());
        print_rows(&g.rows, "  ");
    }
    Ok(())
}

fn print_rows(rows: &[Row], indent: &str) {
    for r in rows {
        println!("{indent}  [alt {}] {} — {}", r.altitude, r.id, r.title);
        if let Some(s) = &r.summary {
            if !s.trim().is_empty() {
                println!("{indent}           {}", s.trim());
            }
        }
    }
}

fn row_json(r: &Row) -> serde_json::Value {
    serde_json::json!({
        "id": r.id,
        "title": r.title,
        "altitude": r.altitude,
        "summary": r.summary,
    })
}

struct Group {
    repo: String,
    rows: Vec<Row>,
}

struct Row {
    id: String,
    title: String,
    altitude: i64,
    summary: Option<String>,
}
