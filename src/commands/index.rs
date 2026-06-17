//! `engrym index` — (re)build the SQLite index from the Markdown KB.
//!
//! Each run does a full *structural* rebuild (docs/topics/edges/chunks/FTS) but
//! re-embeds only genuinely changed passages: chunk vectors are cached by
//! text hash in `embed_cache`, which survives the rebuild. Embedding is the one
//! slow step, so it never repeats needlessly. Files without a frontmatter block
//! are plain Markdown and are skipped (reported), not errors.

use crate::config::Config;
use crate::db;
use crate::embed::Embedder;
use crate::model::{Frontmatter, MAX_ALTITUDE};
use crate::parse::{self, ParsedDoc};
use crate::vector;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub fn run(config: &Config, no_embed: bool, json: bool) -> Result<()> {
    let docs_root = config.docs_root();
    if !docs_root.is_dir() {
        bail!(
            "docs root {} does not exist (configured as docs.root = \"{}\")",
            docs_root.display(),
            config.docs.root
        );
    }

    let mut conn = db::open_for_index(&config.index_path())?;

    // If the configured model changed, cached vectors are incomparable — drop
    // them so everything re-embeds against the new model.
    let prev_model: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'embed_model'", [], |r| {
            r.get(0)
        })
        .ok();
    if prev_model.as_deref() != Some(config.embedding.model.as_str()) {
        conn.execute("DELETE FROM embed_cache", [])?;
    }

    // --- Structural rebuild (fast: pure SQLite inserts) ---------------------
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM docs", [])?; // cascades to topics/edges/chunks
    tx.execute("INSERT INTO fts(fts) VALUES('delete-all')", [])?;

    let mut seen_ids: HashMap<String, String> = HashMap::new();
    let mut stats = Stats::default();

    for entry in WalkDir::new(&docs_root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel_path = rel_path_str(&docs_root, path);

        let parsed = match parse::parse_file(path, &rel_path)? {
            Some(p) => p,
            None => {
                stats.skipped.push(rel_path);
                continue;
            }
        };

        let doc = validate(&parsed, &rel_path)?;
        if let Some(prev) = seen_ids.insert(doc.id.clone(), rel_path.clone()) {
            bail!("duplicate id `{}` in {} and {}", doc.id, prev, rel_path);
        }

        insert_doc(&tx, &doc, &parsed)?;
        stats.indexed += 1;
        stats.chunks += parsed.chunks.len();
    }

    tx.commit()?;

    // --- Embedding pass (the slow step; incremental via embed_cache) --------
    if no_embed {
        // Leave vectors NULL and mark the index unembedded so search stays
        // keyword-only until the next embed.
        set_meta(&conn, "embed_dim", "0")?;
        set_meta(&conn, "embed_model", &config.embedding.model)?;
    } else if let Err(e) = embed_pass(&mut conn, config, &mut stats) {
        // Structure is already committed; degrade to keyword-only rather than
        // losing the whole index when the model can't load (e.g. offline).
        eprintln!(
            "\x1b[33mwarning:\x1b[0m embedding skipped ({:#}). \
             Search will use keyword-only until `engrym index` succeeds with the model available.",
            e
        );
        set_meta(&conn, "embed_dim", "0")?;
    }

    report(&stats, config, no_embed, json);
    Ok(())
}

/// Embed every chunk, reusing cached vectors for unchanged passage text.
fn embed_pass(conn: &mut Connection, config: &Config, stats: &mut Stats) -> Result<()> {
    // All chunks (a fresh structural rebuild leaves every embedding NULL).
    let chunks: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, text FROM chunks")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    if chunks.is_empty() {
        set_meta(conn, "embed_dim", "0")?;
        set_meta(conn, "embed_model", &config.embedding.model)?;
        return Ok(());
    }

    // Partition into cache hits and misses by text hash.
    let mut hashes: Vec<String> = Vec::with_capacity(chunks.len());
    let mut cached: Vec<(i64, Vec<u8>)> = Vec::new();
    let mut misses: Vec<(i64, String, String)> = Vec::new(); // (chunk_id, text, hash)
    let mut seen_hashes: HashSet<String> = HashSet::new();

    {
        let mut lookup = conn.prepare("SELECT vec FROM embed_cache WHERE text_hash = ?1")?;
        for (id, text) in &chunks {
            let hash = hex(&Sha256::digest(text.as_bytes()));
            seen_hashes.insert(hash.clone());
            hashes.push(hash.clone());
            let hit: Option<Vec<u8>> = lookup
                .query_row(params![hash], |r| r.get(0))
                .ok();
            match hit {
                Some(bytes) => cached.push((*id, bytes)),
                None => misses.push((*id, text.clone(), hash)),
            }
        }
    }
    stats.embedded_cached = cached.len();
    stats.embedded_new = misses.len();

    // Embed the misses (loading the model only if there's actually work).
    let mut new_vecs: Vec<(i64, String, Vec<u8>)> = Vec::new();
    let dim;
    if misses.is_empty() {
        // Dim is known from any cached entry (all share the current model).
        dim = cached
            .first()
            .map(|(_, b)| b.len() / 4)
            .unwrap_or(0);
    } else {
        let mut embedder = Embedder::load(&config.embedding.model, true)?;
        dim = embedder.dim();
        let texts: Vec<String> = misses.iter().map(|(_, t, _)| t.clone()).collect();
        let vecs = embedder.embed_passages(&texts)?;
        if vecs.len() != misses.len() {
            bail!("embedder returned {} vectors for {} passages", vecs.len(), misses.len());
        }
        for ((id, _text, hash), v) in misses.into_iter().zip(vecs) {
            new_vecs.push((id, hash, vector::to_bytes(&v)));
        }
    }

    // Write everything in one transaction: chunk vectors, new cache entries,
    // and a cache prune to keep it bounded to live passages.
    let tx = conn.transaction()?;
    {
        let mut upd = tx.prepare("UPDATE chunks SET embedding = ?2 WHERE id = ?1")?;
        for (id, bytes) in &cached {
            upd.execute(params![id, bytes])?;
        }
        let mut ins =
            tx.prepare("INSERT OR REPLACE INTO embed_cache (text_hash, dim, vec) VALUES (?1, ?2, ?3)")?;
        for (id, hash, bytes) in &new_vecs {
            upd.execute(params![id, bytes])?;
            ins.execute(params![hash, dim as i64, bytes])?;
        }
    }
    prune_cache(&tx, &seen_hashes)?;
    tx.commit()?;

    set_meta(conn, "embed_dim", &dim.to_string())?;
    set_meta(conn, "embed_model", &config.embedding.model)?;
    Ok(())
}

/// Delete cache entries no longer referenced by any live chunk.
fn prune_cache(conn: &Connection, live: &HashSet<String>) -> Result<()> {
    conn.execute("CREATE TEMP TABLE live_hashes (h TEXT PRIMARY KEY)", [])?;
    {
        let mut ins = conn.prepare("INSERT OR IGNORE INTO live_hashes (h) VALUES (?1)")?;
        for h in live {
            ins.execute(params![h])?;
        }
    }
    conn.execute(
        "DELETE FROM embed_cache WHERE text_hash NOT IN (SELECT h FROM live_hashes)",
        [],
    )?;
    conn.execute("DROP TABLE live_hashes", [])?;
    Ok(())
}

/// A fully-validated document, ready to persist.
struct ValidDoc {
    id: String,
    title: String,
    altitude: i64,
    summary: Option<String>,
    topics: Vec<String>,
    content_hash: String,
}

fn validate(parsed: &ParsedDoc, rel_path: &str) -> Result<ValidDoc> {
    let fm: &Frontmatter = &parsed.frontmatter;
    let id = fm
        .id
        .clone()
        .ok_or_else(|| anyhow!("{}: missing required field `id`", rel_path))?;
    let title = fm
        .title
        .clone()
        .ok_or_else(|| anyhow!("{}: missing required field `title`", rel_path))?;
    let altitude = fm
        .altitude
        .ok_or_else(|| anyhow!("{}: missing required field `altitude`", rel_path))?;
    if !(0..=MAX_ALTITUDE).contains(&altitude) {
        bail!("{}: altitude {} out of range 0..={}", rel_path, altitude, MAX_ALTITUDE);
    }
    if fm.topics.is_empty() {
        bail!("{}: at least one topic is required", rel_path);
    }
    for rel in &fm.relations {
        if !rel.is_valid_type() {
            bail!("{}: unknown relation type `{}`", rel_path, rel.edge_type);
        }
    }

    Ok(ValidDoc {
        id,
        title,
        altitude,
        summary: fm.summary.clone(),
        topics: fm.topics.clone(),
        content_hash: parsed.content_hash.clone(),
    })
}

fn insert_doc(conn: &Connection, doc: &ValidDoc, parsed: &ParsedDoc) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO docs (id, path, title, altitude, summary, content_hash, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            doc.id,
            parsed.rel_path,
            doc.title,
            doc.altitude,
            doc.summary,
            doc.content_hash,
            now
        ],
    )?;

    // Topics: store the full path; subtree queries use a LIKE prefix.
    for topic in &doc.topics {
        let depth = topic.split('/').filter(|s| !s.is_empty()).count() as i64;
        conn.execute(
            "INSERT OR IGNORE INTO topics (doc_id, path, depth) VALUES (?1, ?2, ?3)",
            params![doc.id, topic, depth],
        )?;
    }

    // Authored relations.
    for rel in &parsed.frontmatter.relations {
        conn.execute(
            "INSERT OR IGNORE INTO edges (src, dst, type) VALUES (?1, ?2, ?3)",
            params![doc.id, rel.target, rel.edge_type],
        )?;
    }
    // `[[wikilinks]]` synthesize `references` edges for free.
    for target in &parsed.wikilinks {
        conn.execute(
            "INSERT OR IGNORE INTO edges (src, dst, type) VALUES (?1, ?2, 'references')",
            params![doc.id, target],
        )?;
    }

    // Passage chunks + FTS rows (rowid kept equal to chunks.id). Embeddings are
    // filled in by the embedding pass.
    let mut chunk_stmt = conn.prepare_cached(
        "INSERT INTO chunks (doc_id, heading, ord, text, embedding)
         VALUES (?1, ?2, ?3, ?4, NULL)",
    )?;
    let mut fts_stmt =
        conn.prepare_cached("INSERT INTO fts (rowid, text, title) VALUES (?1, ?2, ?3)")?;
    for chunk in &parsed.chunks {
        chunk_stmt.execute(params![doc.id, chunk.heading, chunk.ord as i64, chunk.text])?;
        let rowid = conn.last_insert_rowid();
        fts_stmt.execute(params![rowid, chunk.text, doc.title])?;
    }

    Ok(())
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

fn rel_path_str(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[derive(Default)]
struct Stats {
    indexed: usize,
    chunks: usize,
    embedded_new: usize,
    embedded_cached: usize,
    skipped: Vec<String>,
}

fn report(stats: &Stats, config: &Config, no_embed: bool, json: bool) {
    if json {
        let out = serde_json::json!({
            "indexed": stats.indexed,
            "chunks": stats.chunks,
            "embedded_new": stats.embedded_new,
            "embedded_cached": stats.embedded_cached,
            "embeddings": !no_embed,
            "skipped": stats.skipped,
            "index_path": config.index_path().to_string_lossy(),
            "local": config.is_local(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        println!(
            "Indexed {} document(s), {} chunk(s) → {}",
            stats.indexed,
            stats.chunks,
            config.index_path().display()
        );
        if config.is_local() {
            if let Some(repo) = &config.source_repo {
                println!("Local KB (stored outside the repo, bound to {}).", repo.display());
            }
        }
        if no_embed {
            println!("Embeddings skipped (--no-embed); search will be keyword-only.");
        } else {
            println!(
                "Embedded {} new, {} reused from cache.",
                stats.embedded_new, stats.embedded_cached
            );
        }
        if !stats.skipped.is_empty() {
            println!("Skipped {} file(s) without frontmatter:", stats.skipped.len());
            for s in &stats.skipped {
                println!("  - {}", s);
            }
        }
    }
}
