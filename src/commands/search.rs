//! `engrym search <query>` — hybrid passage retrieval.
//!
//! Keyword (BM25 over FTS5) and semantic (brute-force cosine over chunk vectors)
//! rankings are fused with reciprocal rank fusion. Keyword nails exact
//! identifiers like `OAuth2RefreshToken`; vectors catch "how do we keep sessions
//! alive". `--keyword`/`--semantic` force a single ranker; hybrid gracefully
//! degrades to keyword-only when the index has no embeddings.

use crate::config::Config;
use crate::daemon;
use crate::db;
use crate::embed::Embedder;
use crate::vector;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Hybrid,
    Keyword,
    Semantic,
}

pub struct Args {
    pub query: String,
    pub limit: usize,
    pub altitude: Option<i64>,
    pub mode: Mode,
}

pub fn run(config: &Config, args: &Args, json: bool) -> Result<()> {
    let conn = db::open_existing(&config.index_path())?;
    let pool = (args.limit * 5).max(50);
    let has_vectors = embedded_dim(&conn).unwrap_or(0) > 0;

    // Resolve the effective ranker set, degrading semantic→keyword when the
    // index isn't embedded.
    let (use_keyword, use_vector) = match args.mode {
        Mode::Keyword => (true, false),
        Mode::Semantic => {
            if !has_vectors {
                eprintln!(
                    "\x1b[33mwarning:\x1b[0m index has no embeddings; falling back to keyword search. \
                     Run `engrym index` to enable semantic search."
                );
                (true, false)
            } else {
                (false, true)
            }
        }
        Mode::Hybrid => (true, has_vectors),
    };

    let mut ranked_lists: Vec<Vec<i64>> = Vec::new();
    if use_keyword {
        ranked_lists.push(keyword_rank(&conn, &args.query, pool)?);
    }
    if use_vector {
        match vector_rank(&conn, config, &args.query, pool) {
            Ok(list) => ranked_lists.push(list),
            Err(e) => eprintln!(
                "\x1b[33mwarning:\x1b[0m semantic ranking skipped ({:#}); using keyword results.",
                e
            ),
        }
    }

    let fused = rrf_fuse(&ranked_lists, config.search.rrf_k);
    let hits = materialize(&conn, &fused, args.limit, args.altitude)?;

    if json {
        let arr: Vec<_> = hits
            .iter()
            .map(|h| {
                serde_json::json!({
                    "id": h.id,
                    "title": h.title,
                    "heading": h.heading,
                    "altitude": h.altitude,
                    "score": h.score,
                    "passage": h.text,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else if hits.is_empty() {
        println!("No matches for \"{}\".", args.query);
    } else {
        for h in &hits {
            let loc = match &h.heading {
                Some(hd) if !hd.is_empty() => format!("{} › {}", h.id, hd),
                _ => h.id.clone(),
            };
            println!("\x1b[1m{}\x1b[0m  (alt {})", loc, h.altitude);
            println!("  {}", snippet(&h.text, 220));
            println!();
        }
    }
    Ok(())
}

/// BM25 ranking → chunk ids, best first.
fn keyword_rank(conn: &rusqlite::Connection, query: &str, pool: usize) -> Result<Vec<i64>> {
    let match_expr = build_match(query);
    let mut stmt = conn.prepare(
        "SELECT fts.rowid FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts) LIMIT ?2",
    )?;
    let ids = stmt
        .query_map(params![match_expr, pool as i64], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids)
}

/// Cosine ranking → chunk ids, best first. Brute-force over all embedded chunks
/// (microsecond-fast for a repo's worth of passages).
fn vector_rank(
    conn: &rusqlite::Connection,
    config: &Config,
    query: &str,
    pool: usize,
) -> Result<Vec<i64>> {
    let qvec = query_embedding(config, query)?;

    let mut stmt = conn.prepare("SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL")?;
    let mut scored: Vec<(i64, f32)> = stmt
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            let bytes: Vec<u8> = r.get(1)?;
            Ok((id, bytes))
        })?
        .filter_map(|res| res.ok())
        .map(|(id, bytes)| {
            let v = vector::from_bytes(&bytes);
            (id, vector::dot(&qvec, &v))
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(pool);
    Ok(scored.into_iter().map(|(id, _)| id).collect())
}

/// Embed the query via the warm daemon (auto-spawned, self-reaping). Falls back
/// to a one-shot in-process load if the daemon is disabled or unavailable, so a
/// search never fails on the daemon's account.
fn query_embedding(config: &Config, query: &str) -> Result<Vec<f32>> {
    if config.daemon_enabled() {
        if let Ok(v) = daemon::query_embedding(config, query) {
            return Ok(v);
        }
    }
    let mut embedder = Embedder::load(&config.embedding.model, false)?;
    embedder.embed_query(query)
}

/// Reciprocal rank fusion: score(d) = Σ_rankers 1 / (k + rank). Higher is better.
/// A single list still produces a valid monotonic ranking.
fn rrf_fuse(lists: &[Vec<i64>], k: u32) -> Vec<(i64, f64)> {
    let k = k as f64;
    let mut scores: HashMap<i64, f64> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(*id).or_insert(0.0) += 1.0 / (k + (rank as f64) + 1.0);
        }
    }
    let mut fused: Vec<(i64, f64)> = scores.into_iter().collect();
    // Stable, deterministic ordering: score desc, then chunk id asc.
    fused.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    fused
}

/// Fetch display rows for the top fused chunk ids, applying the altitude filter,
/// preserving fused order, and stopping at `limit`.
fn materialize(
    conn: &rusqlite::Connection,
    fused: &[(i64, f64)],
    limit: usize,
    altitude: Option<i64>,
) -> Result<Vec<Hit>> {
    let mut row_stmt = conn.prepare(
        "SELECT d.id, d.title, c.heading, c.text, d.altitude
         FROM chunks c JOIN docs d ON d.id = c.doc_id
         WHERE c.id = ?1",
    )?;

    let mut hits = Vec::with_capacity(limit);
    for (chunk_id, score) in fused {
        if hits.len() >= limit {
            break;
        }
        let row = row_stmt
            .query_row(params![chunk_id], |r| {
                Ok(Hit {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    heading: r.get(2)?,
                    text: r.get(3)?,
                    altitude: r.get(4)?,
                    score: *score,
                })
            })
            .ok();
        if let Some(hit) = row {
            if altitude.map(|a| a == hit.altitude).unwrap_or(true) {
                hits.push(hit);
            }
        }
    }
    Ok(hits)
}

fn embedded_dim(conn: &rusqlite::Connection) -> Option<i64> {
    conn.query_row("SELECT value FROM meta WHERE key = 'embed_dim'", [], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .and_then(|s| s.parse().ok())
}

struct Hit {
    id: String,
    title: String,
    heading: Option<String>,
    text: String,
    altitude: i64,
    score: f64,
}

/// Build a safe FTS5 MATCH expression: quote each alphanumeric token as a phrase
/// and AND them together. Keeps identifiers like `OAuth2RefreshToken` intact and
/// avoids FTS5 syntax injection from punctuation.
fn build_match(query: &str) -> String {
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t))
        .collect();
    if tokens.is_empty() {
        "\"\"".to_string()
    } else {
        tokens.join(" ")
    }
}

fn snippet(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let cut: String = t.chars().take(max).collect();
        format!("{}…", cut.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::rrf_fuse;

    #[test]
    fn rrf_rewards_consensus_and_breaks_ties_by_id() {
        // Doc 1 ranks top in both lists; doc 2 is top of only one; doc 3 mid.
        let keyword = vec![1, 2, 3];
        let vector = vec![1, 3, 2];
        let fused = rrf_fuse(&[keyword, vector], 60);

        // Appearing #1 in both beats anything ranked once → doc 1 wins.
        assert_eq!(fused[0].0, 1);
        // Every id surfaces exactly once, scores strictly descending.
        assert_eq!(fused.len(), 3);
        for w in fused.windows(2) {
            assert!(w[0].1 >= w[1].1, "scores must be sorted desc: {fused:?}");
        }
    }

    #[test]
    fn rrf_tie_breaks_on_lower_id() {
        // Two ids tie on score (each appears once at rank 0) → lower id first.
        let fused = rrf_fuse(&[vec![7], vec![3]], 60);
        assert_eq!(fused.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![3, 7]);
    }
}
