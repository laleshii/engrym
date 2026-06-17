//! SQLite index lifecycle. The schema is the single source of truth in
//! `docs/schema.sql`, embedded at compile time so the binary is self-contained.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

/// Bump when `schema.sql` changes shape; a mismatch forces a rebuild.
pub const SCHEMA_VERSION: &str = "2";

const SCHEMA_SQL: &str = include_str!("../spec/index-schema.sql");

/// Open an existing index read-only-ish (still a writable handle, but we don't
/// create the schema). Errors clearly if the index hasn't been built yet.
pub fn open_existing(index_path: &Path) -> Result<Connection> {
    if !index_path.exists() {
        anyhow::bail!(
            "no index at {} — run `engrym index` first",
            index_path.display()
        );
    }
    let conn = Connection::open(index_path)
        .with_context(|| format!("opening index {}", index_path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Open the index for a (re)build. Reuses an existing, schema-compatible file so
/// the persistent `embed_cache` survives — `index` does a full *structural*
/// rebuild but only re-embeds genuinely changed passages. A missing file or a
/// schema-version mismatch triggers a clean recreate.
pub fn open_for_index(index_path: &Path) -> Result<Connection> {
    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating index dir {}", parent.display()))?;
    }

    if index_path.exists() {
        let conn = Connection::open(index_path)
            .with_context(|| format!("opening index {}", index_path.display()))?;
        // `.ok()` collapses a missing `meta` table (old/foreign schema) to None.
        let version: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .ok();
        if version.as_deref() == Some(SCHEMA_VERSION) {
            conn.pragma_update(None, "foreign_keys", "ON")?;
            return Ok(conn);
        }
        // Incompatible: drop it and rebuild from scratch.
        drop(conn);
    }

    for suffix in ["", "-wal", "-shm"] {
        let p = with_suffix(index_path, suffix);
        if p.exists() {
            std::fs::remove_file(&p).ok();
        }
    }
    let conn = Connection::open(index_path)
        .with_context(|| format!("creating index {}", index_path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.execute_batch(SCHEMA_SQL).context("applying schema.sql")?;
    // Stamp the version so the next `index` recognizes this file as compatible
    // and preserves the embedding cache instead of rebuilding from scratch.
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION],
    )?;
    Ok(conn)
}

fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    if suffix.is_empty() {
        return path.to_path_buf();
    }
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    std::path::PathBuf::from(s)
}
