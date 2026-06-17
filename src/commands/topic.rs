//! `engrym topic <path>` — list docs at or below a topic subtree.
//!
//! Topics are slash-delimited paths; a query for `backend/auth` returns docs
//! tagged `backend/auth` and anything deeper (`backend/auth/oauth`, …).

use crate::config::Config;
use crate::db;
use anyhow::Result;
use rusqlite::params;

pub fn run(config: &Config, path: &str, json: bool) -> Result<()> {
    let conn = db::open_existing(&config.index_path())?;
    let prefix = path.trim_matches('/');

    let mut stmt = conn.prepare(
        "SELECT DISTINCT d.id, d.title, d.altitude, d.summary
         FROM topics t
         JOIN docs d ON d.id = t.doc_id
         WHERE t.path = ?1 OR t.path LIKE ?1 || '/%'
         ORDER BY d.altitude, d.id",
    )?;
    let rows: Vec<Row> = stmt
        .query_map(params![prefix], |row| {
            Ok(Row {
                id: row.get(0)?,
                title: row.get(1)?,
                altitude: row.get(2)?,
                summary: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    if json {
        let arr: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "title": r.title,
                    "altitude": r.altitude,
                    "summary": r.summary,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if rows.is_empty() {
        println!("No documents under topic \"{}\".", prefix);
    } else {
        println!("Documents under \x1b[1m{}\x1b[0m:", prefix);
        for r in &rows {
            println!("  [alt {}] {} — {}", r.altitude, r.id, r.title);
            if let Some(s) = &r.summary {
                if !s.trim().is_empty() {
                    println!("           {}", s.trim());
                }
            }
        }
    }
    Ok(())
}

struct Row {
    id: String,
    title: String,
    altitude: i64,
    summary: Option<String>,
}
