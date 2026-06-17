//! Authoring commands: `new` (create), `set` (update), `rm` (delete).
//!
//! These operate on the Markdown files — the source of truth — not the index.
//! Docs are located by scanning for their frontmatter `id`, so authoring works
//! regardless of whether the index is fresh. After authoring, rebuild with
//! `engrym index`.
//!
//! The point of `new`/`set` is that frontmatter is *generated*, never
//! hand-written: an agent supplies fields as flags and gets a guaranteed-valid,
//! reviewable document back.

use crate::config::{Config, Layout};
use crate::model::{Frontmatter, Relation, EDGE_TYPES, MAX_ALTITUDE};
use crate::parse::{self, ParsedDoc};
use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Where a document's file lives under the docs root, per the configured layout.
/// `id` is the identity regardless, so this only affects on-disk organization.
pub fn doc_rel_path(layout: Layout, id: &str, altitude: Option<i64>, topics: &[String]) -> String {
    let topic_seg = topics
        .first()
        .map(|t| t.trim_matches('/'))
        .filter(|s| !s.is_empty());

    let mut parts: Vec<String> = Vec::new();
    match layout {
        Layout::Flat => {}
        Layout::Topic => {
            if let Some(t) = topic_seg {
                parts.push(t.to_string());
            }
        }
        // Overviews (altitude 0, or unset) stay at the root; 1/2/3 get a folder.
        Layout::Altitude => {
            if let Some(n) = altitude {
                if n >= 1 {
                    parts.push(n.to_string());
                }
            }
        }
    }
    parts.push(format!("{}.md", id));
    parts.join("/")
}

// --- new -------------------------------------------------------------------

pub struct NewArgs {
    pub id: String,
    pub title: String,
    pub altitude: i64,
    pub topics: Vec<String>,
    pub relations: Vec<String>, // raw "type:target"
    pub summary: Option<String>,
    pub path: Option<String>,
    pub body: Option<String>,
    pub stdin: bool,
    pub force: bool,
}

pub fn new(config: &Config, args: NewArgs, json: bool) -> Result<()> {
    validate_id(&args.id)?;
    if !(0..=MAX_ALTITUDE).contains(&args.altitude) {
        bail!("altitude {} out of range 0..={}", args.altitude, MAX_ALTITUDE);
    }
    if args.topics.is_empty() {
        bail!("at least one --topic is required");
    }
    let relations = parse_relations(&args.relations)?;

    let docs_root = config.docs_root();
    let rel_path = args.path.clone().unwrap_or_else(|| {
        doc_rel_path(config.docs.layout, &args.id, Some(args.altitude), &args.topics)
    });
    let abs_path = docs_root.join(&rel_path);
    if abs_path.exists() && !args.force {
        bail!("{} already exists (use --force to overwrite)", rel_path);
    }
    // An id must be unique across the KB.
    if let Some(existing) = find_doc_by_id(&docs_root, &args.id)? {
        if existing != abs_path {
            bail!(
                "id `{}` is already used by {}",
                args.id,
                rel_path_str(&docs_root, &existing)
            );
        }
    }

    let body = resolve_body(&args)?;
    let fm = Frontmatter {
        id: Some(args.id.clone()),
        title: Some(args.title.clone()),
        altitude: Some(args.altitude),
        topics: args.topics.clone(),
        relations,
        summary: args.summary.clone(),
    };
    let content = parse::render_doc(&fm, &body)?;

    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&abs_path, content).with_context(|| format!("writing {}", abs_path.display()))?;

    if json {
        println!(
            "{}",
            serde_json::json!({ "created": args.id, "path": rel_path })
        );
    } else {
        println!("Created {} → {}", args.id, rel_path);
        println!("Run `engrym index` to update the index.");
    }
    Ok(())
}

fn resolve_body(args: &NewArgs) -> Result<String> {
    if args.stdin {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .context("reading body from stdin")?;
        Ok(s)
    } else if let Some(b) = &args.body {
        Ok(b.clone())
    } else {
        // Minimal scaffold; the author fills in the prose.
        Ok(format!("# {}\n", args.title))
    }
}

// --- set -------------------------------------------------------------------

pub struct SetArgs {
    pub id: String,
    pub title: Option<String>,
    pub altitude: Option<i64>,
    pub summary: Option<String>,
    pub add_topics: Vec<String>,
    pub remove_topics: Vec<String>,
    pub add_relations: Vec<String>,
    pub remove_relations: Vec<String>,
    pub body_stdin: bool,
}

pub fn set(config: &Config, args: SetArgs, json: bool) -> Result<()> {
    let docs_root = config.docs_root();
    let path = find_doc_by_id(&docs_root, &args.id)?
        .ok_or_else(|| anyhow!("no document with id `{}`", args.id))?;
    let rel_path = rel_path_str(&docs_root, &path);
    let parsed = parse::parse_file(&path, &rel_path)?
        .ok_or_else(|| anyhow!("{} has no frontmatter", rel_path))?;
    let mut fm = parsed.frontmatter;

    if let Some(t) = args.title {
        fm.title = Some(t);
    }
    if let Some(a) = args.altitude {
        if !(0..=MAX_ALTITUDE).contains(&a) {
            bail!("altitude {} out of range 0..={}", a, MAX_ALTITUDE);
        }
        fm.altitude = Some(a);
    }
    if let Some(s) = args.summary {
        fm.summary = if s.is_empty() { None } else { Some(s) };
    }

    for t in &args.remove_topics {
        fm.topics.retain(|x| x != t);
    }
    for t in args.add_topics {
        if !fm.topics.contains(&t) {
            fm.topics.push(t);
        }
    }
    if fm.topics.is_empty() {
        bail!("a document must keep at least one topic");
    }

    for raw in parse_relations(&args.remove_relations)? {
        fm.relations
            .retain(|r| !(r.edge_type == raw.edge_type && r.target == raw.target));
    }
    for r in parse_relations(&args.add_relations)? {
        let dup = fm
            .relations
            .iter()
            .any(|x| x.edge_type == r.edge_type && x.target == r.target);
        if !dup {
            fm.relations.push(r);
        }
    }

    let body = if args.body_stdin {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .context("reading body from stdin")?;
        s
    } else {
        parsed.body
    };

    let content = parse::render_doc(&fm, &body)?;
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;

    if json {
        println!("{}", serde_json::json!({ "updated": args.id, "path": rel_path }));
    } else {
        println!("Updated {} → {}", args.id, rel_path);
        println!("Run `engrym index` to update the index.");
    }
    Ok(())
}

// --- rm --------------------------------------------------------------------

pub fn rm(config: &Config, id: &str, force: bool, json: bool) -> Result<()> {
    let docs_root = config.docs_root();
    let path = find_doc_by_id(&docs_root, id)?
        .ok_or_else(|| anyhow!("no document with id `{}`", id))?;
    let rel_path = rel_path_str(&docs_root, &path);

    // Find docs that point at this one — those edges will dangle after removal.
    let inbound = inbound_referrers(&docs_root, id)?;
    if !inbound.is_empty() && !force {
        let list = inbound.join(", ");
        bail!(
            "{} document(s) reference `{}` ({}). \
             Their relations/links would dangle — re-point them or pass --force.",
            inbound.len(),
            id,
            list
        );
    }

    std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;

    if json {
        println!(
            "{}",
            serde_json::json!({ "deleted": id, "path": rel_path, "now_dangling_from": inbound })
        );
    } else {
        println!("Deleted {} ({})", id, rel_path);
        if !inbound.is_empty() {
            println!(
                "warning: {} document(s) now have dangling references: {}",
                inbound.len(),
                inbound.join(", ")
            );
        }
        println!("Run `engrym index` to update the index.");
    }
    Ok(())
}

// --- relocate --------------------------------------------------------------

/// Move documents to the paths implied by the configured `[docs] layout`
/// (e.g. adopt topic-mirrored folders, or flatten back). `id` is unaffected, so
/// this is safe — relations and the index are keyed by id, not path.
pub fn relocate(
    config: &Config,
    only_id: Option<&str>,
    layout_override: Option<Layout>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let docs_root = config.docs_root();
    if !docs_root.is_dir() {
        bail!("docs root {} does not exist", docs_root.display());
    }
    let layout = layout_override.unwrap_or(config.docs.layout);

    let mut moves: Vec<(String, String, String)> = Vec::new(); // id, from, to
    for (path, parsed) in collect_docs(&docs_root)? {
        let id = match &parsed.frontmatter.id {
            Some(i) => i.clone(),
            None => continue,
        };
        if let Some(want) = only_id {
            if want != id {
                continue;
            }
        }
        let from = rel_path_str(&docs_root, &path);
        let to = doc_rel_path(layout, &id, parsed.frontmatter.altitude, &parsed.frontmatter.topics);
        if from != to {
            moves.push((id, from, to));
        }
    }
    moves.sort();

    if !dry_run {
        for (_, from, to) in &moves {
            let src = docs_root.join(from);
            let dst = docs_root.join(to);
            if dst.exists() {
                bail!("cannot move {} → {}: target already exists", from, to);
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::rename(&src, &dst)
                .with_context(|| format!("moving {} → {}", from, to))?;
        }
        prune_empty_dirs(&docs_root)?;
    }

    if json {
        let arr: Vec<_> = moves
            .iter()
            .map(|(id, from, to)| serde_json::json!({ "id": id, "from": from, "to": to }))
            .collect();
        println!(
            "{}",
            serde_json::json!({ "moved": arr, "dry_run": dry_run, "count": moves.len() })
        );
    } else if moves.is_empty() {
        println!("All documents already match the `{:?}` layout.", layout);
    } else {
        let verb = if dry_run { "Would move" } else { "Moved" };
        for (_, from, to) in &moves {
            println!("{} {} → {}", verb, from, to);
        }
        if !dry_run {
            println!("\nRun `engrym index` to update the index.");
            if layout_override.is_some() && layout_override != Some(config.docs.layout) {
                println!(
                    "Note: set `[docs] layout = \"{}\"` in engrym.toml so `new` places files the same way.",
                    format!("{:?}", layout).to_lowercase()
                );
            }
        }
    }
    Ok(())
}

/// Remove empty directories under `root` (left behind after moves), bottom-up.
fn prune_empty_dirs(root: &Path) -> Result<()> {
    let mut dirs: Vec<PathBuf> = WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.path() != root)
        .map(|e| e.path().to_path_buf())
        .collect();
    // Deepest first so parents become empty after children are removed.
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for dir in dirs {
        if std::fs::read_dir(&dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
            std::fs::remove_dir(&dir).ok();
        }
    }
    Ok(())
}

// --- shared helpers --------------------------------------------------------

/// Walk the docs root and return (absolute path, parsed doc) for every file
/// that has a frontmatter block.
fn collect_docs(docs_root: &Path) -> Result<Vec<(PathBuf, ParsedDoc)>> {
    let mut out = Vec::new();
    for entry in WalkDir::new(docs_root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = rel_path_str(docs_root, path);
        if let Some(parsed) = parse::parse_file(path, &rel)? {
            out.push((path.to_path_buf(), parsed));
        }
    }
    Ok(out)
}

fn find_doc_by_id(docs_root: &Path, id: &str) -> Result<Option<PathBuf>> {
    if !docs_root.is_dir() {
        return Ok(None);
    }
    for (path, parsed) in collect_docs(docs_root)? {
        if parsed.frontmatter.id.as_deref() == Some(id) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Ids of documents whose relations or wikilinks target `id`.
fn inbound_referrers(docs_root: &Path, id: &str) -> Result<Vec<String>> {
    let mut refs = Vec::new();
    for (_, parsed) in collect_docs(docs_root)? {
        let self_id = parsed.frontmatter.id.clone().unwrap_or_default();
        if self_id == id {
            continue;
        }
        let hits = parsed.frontmatter.relations.iter().any(|r| r.target == id)
            || parsed.wikilinks.iter().any(|w| w == id);
        if hits && !refs.contains(&self_id) {
            refs.push(self_id);
        }
    }
    refs.sort();
    Ok(refs)
}

fn parse_relations(raw: &[String]) -> Result<Vec<Relation>> {
    let mut out = Vec::with_capacity(raw.len());
    for r in raw {
        let (edge_type, target) = r
            .split_once(':')
            .ok_or_else(|| anyhow!("relation `{}` must be in the form `type:target`", r))?;
        let edge_type = edge_type.trim().to_string();
        let target = target.trim().to_string();
        if !EDGE_TYPES.contains(&edge_type.as_str()) {
            bail!(
                "unknown relation type `{}` (expected one of: {})",
                edge_type,
                EDGE_TYPES.join(", ")
            );
        }
        if target.is_empty() {
            bail!("relation `{}` has an empty target", r);
        }
        out.push(Relation { edge_type, target });
    }
    Ok(out)
}

/// Ids are stable identifiers: lowercase alphanumerics and hyphens.
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() {
        bail!("id cannot be empty");
    }
    let ok = id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok || id.starts_with('-') || id.ends_with('-') {
        bail!("id `{}` must be kebab-case (lowercase letters, digits, hyphens)", id);
    }
    Ok(())
}

fn rel_path_str(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_place_files_correctly() {
        let topics = vec!["search/hybrid".to_string(), "embedding".to_string()];
        let alt = Some(3);
        assert_eq!(doc_rel_path(Layout::Flat, "rrf-fusion", alt, &topics), "rrf-fusion.md");
        assert_eq!(
            doc_rel_path(Layout::Topic, "rrf-fusion", alt, &topics),
            "search/hybrid/rrf-fusion.md"
        );
        assert_eq!(doc_rel_path(Layout::Altitude, "rrf-fusion", alt, &topics), "3/rrf-fusion.md");
        // Altitude 0 (and unset) stay at the root.
        assert_eq!(doc_rel_path(Layout::Altitude, "engrym-overview", Some(0), &topics), "engrym-overview.md");
        assert_eq!(doc_rel_path(Layout::Altitude, "x", None, &[]), "x.md");
        // No topics → falls back gracefully.
        assert_eq!(doc_rel_path(Layout::Topic, "x", Some(0), &[]), "x.md");
    }
}
