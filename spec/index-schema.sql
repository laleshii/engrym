-- engrym index schema (SQLite, rebuildable from Markdown)
-- Lives at .engrym/index.db (gitignored). Never hand-edited.
-- Conventions: ids are doc frontmatter ids; content_hash drives incremental
-- re-embedding (only changed files are re-chunked and re-embedded).

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- One row per Markdown document --------------------------------------------
CREATE TABLE docs (
    id            TEXT PRIMARY KEY,         -- frontmatter id
    path          TEXT NOT NULL UNIQUE,     -- path relative to docs root
    title         TEXT NOT NULL,
    altitude      INTEGER NOT NULL CHECK (altitude BETWEEN 0 AND 3),
    summary       TEXT,
    content_hash  TEXT NOT NULL,            -- hash of raw file; skip re-embed if unchanged
    indexed_at    INTEGER NOT NULL          -- unix seconds
);

-- Topic assignments. Hierarchy is implicit in the path; we also store depth
-- and the materialized ancestor prefixes for fast subtree queries.
CREATE TABLE topics (
    doc_id  TEXT NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
    path    TEXT NOT NULL,                  -- e.g. 'backend/auth/oauth'
    depth   INTEGER NOT NULL,               -- number of segments
    PRIMARY KEY (doc_id, path)
);
-- Prefix index for `topic backend/auth` -> everything at or below.
CREATE INDEX idx_topics_path ON topics(path);

-- Typed edges between documents --------------------------------------------
CREATE TABLE edges (
    src    TEXT NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
    dst    TEXT NOT NULL,                   -- target id (may dangle; lint flags it)
    type   TEXT NOT NULL CHECK (type IN
              ('refines','part_of','depends_on','references','supersedes')),
    PRIMARY KEY (src, dst, type)
);
CREATE INDEX idx_edges_dst ON edges(dst);   -- inbound lookups (who refines X?)

-- Passage-level chunks for semantic search. One doc -> many chunks (by heading).
CREATE TABLE chunks (
    id        INTEGER PRIMARY KEY,
    doc_id    TEXT NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
    heading   TEXT,                         -- nearest section heading
    ord       INTEGER NOT NULL,             -- order within doc
    text      TEXT NOT NULL,
    embedding BLOB                          -- f32[dim], dim from brain.toml; NULL until embedded
);
CREATE INDEX idx_chunks_doc ON chunks(doc_id);

-- Full-text search over chunk text + titles (BM25). Half of hybrid retrieval;
-- vector cosine over chunks.embedding is the other half, fused via RRF in code.
CREATE VIRTUAL TABLE fts USING fts5(
    text,
    title,
    content='',                             -- contentless; we manage rows explicitly
    tokenize = 'porter unicode61'
);
-- fts rowid is kept equal to chunks.id by the indexer.

-- Persistent embedding cache, keyed by chunk-text hash. Survives the full
-- structural rebuild that every `index` performs, so only genuinely changed
-- passages are re-embedded (embedding is the one slow step). Cleared wholesale
-- when the embedding model changes, since vectors across models are
-- incomparable.
CREATE TABLE embed_cache (
    text_hash  TEXT PRIMARY KEY,  -- sha256 of the chunk text
    dim        INTEGER NOT NULL,
    vec        BLOB NOT NULL      -- normalized f32[dim], little-endian
);

-- Bookkeeping: model + dims so a config change forces a clean re-embed.
CREATE TABLE meta (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
-- expected keys: schema_version, embed_model, embed_dim
