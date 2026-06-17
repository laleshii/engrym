//! Parsing Markdown KB files: frontmatter split, heading-level chunking, and
//! `[[wikilink]]` extraction.
//!
//! A passage-level chunk is the text under a heading (the lead text before the
//! first heading becomes chunk 0). Chunks are what we embed and full-text index
//! so an agent retrieves the *relevant section*, not a whole file.

use crate::model::Frontmatter;
use anyhow::{Context, Result};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::OnceLock;

/// A document parsed from disk, ready to index or lint.
pub struct ParsedDoc {
    pub frontmatter: Frontmatter,
    /// Markdown body (everything after the frontmatter block).
    pub body: String,
    /// Path relative to the docs root, using `/` separators.
    pub rel_path: String,
    /// SHA-256 of the raw file bytes; drives incremental re-embedding.
    pub content_hash: String,
    pub chunks: Vec<Chunk>,
    /// Distinct `[[targets]]` referenced in the body.
    pub wikilinks: Vec<String>,
}

pub struct Chunk {
    pub heading: Option<String>,
    pub ord: usize,
    pub text: String,
}

/// Read and fully parse a file. Returns `Ok(None)` when the file has no
/// frontmatter block at all — i.e. it's plain Markdown, not an engrym KB doc.
pub fn parse_file(abs_path: &Path, rel_path: &str) -> Result<Option<ParsedDoc>> {
    let raw = std::fs::read_to_string(abs_path)
        .with_context(|| format!("reading {}", abs_path.display()))?;

    let Some((yaml, body)) = split_frontmatter(&raw) else {
        return Ok(None);
    };

    let frontmatter: Frontmatter = serde_yaml::from_str(yaml)
        .with_context(|| format!("parsing frontmatter in {}", abs_path.display()))?;

    let content_hash = hex(&Sha256::digest(raw.as_bytes()));
    let chunks = chunk_by_heading(body);
    let wikilinks = extract_wikilinks(body);

    Ok(Some(ParsedDoc {
        frontmatter,
        body: body.to_string(),
        rel_path: rel_path.to_string(),
        content_hash,
        chunks,
        wikilinks,
    }))
}

/// Render a document file from frontmatter + body — the inverse of
/// [`split_frontmatter`]. Used by the authoring commands so generated files
/// always have a well-formed, reviewable frontmatter block.
pub fn render_doc(frontmatter: &Frontmatter, body: &str) -> Result<String> {
    let yaml = serde_yaml::to_string(frontmatter).context("serializing frontmatter")?;
    let body = body.trim_start_matches('\n');
    let sep = if body.is_empty() || body.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Ok(format!("---\n{}---\n\n{}{}", yaml, body, sep))
}

/// Split a `---\n...\n---\n` leading YAML block from the body.
/// Returns `None` if the file doesn't open with a frontmatter fence.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    // Tolerate a leading BOM / whitespace-free start; require the very first
    // line to be exactly `---`.
    let rest = raw.strip_prefix("---\n").or_else(|| raw.strip_prefix("---\r\n"))?;

    // Find the closing fence on its own line.
    let mut search_from = 0;
    loop {
        let nl = rest[search_from..].find('\n')?;
        let line_start = search_from;
        let line_end = search_from + nl;
        let line = rest[line_start..line_end].trim_end_matches('\r');
        if line.trim() == "---" {
            let yaml = &rest[..line_start];
            let body = &rest[line_end + 1..];
            return Some((yaml, body));
        }
        search_from = line_end + 1;
        if search_from >= rest.len() {
            return None;
        }
    }
}

/// Chunk the body into passages, one per heading section.
fn chunk_by_heading(body: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut cur_heading: Option<String> = None;
    let mut cur_text = String::new();
    let mut in_heading = false;
    let mut heading_buf = String::new();
    let mut ord = 0;

    let mut flush = |heading: &Option<String>, text: &mut String, ord: &mut usize| {
        let normalized = collapse_ws(text);
        if !normalized.is_empty() || heading.is_some() {
            chunks.push(Chunk {
                heading: heading.clone(),
                ord: *ord,
                text: normalized,
            });
            *ord += 1;
        }
        text.clear();
    };

    for event in Parser::new(body) {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                // Close out the previous section before starting a new one.
                flush(&cur_heading, &mut cur_text, &mut ord);
                in_heading = true;
                heading_buf.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                cur_heading = Some(heading_buf.trim().to_string());
            }
            // Text/Code events already carry their own surrounding whitespace,
            // so concatenate verbatim — injecting spaces would mangle inline
            // runs like `[[wikilinks]]` into `[ [ wikilinks ] ]`.
            Event::Text(t) | Event::Code(t) => {
                if in_heading {
                    heading_buf.push_str(&t);
                } else {
                    cur_text.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if !in_heading {
                    cur_text.push(' ');
                }
            }
            // Separate block-level runs so adjacent paragraphs/list items don't
            // fuse into one word.
            Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Item)
            | Event::End(TagEnd::CodeBlock)
            | Event::End(TagEnd::TableCell) => {
                if !in_heading {
                    cur_text.push(' ');
                }
            }
            _ => {}
        }
    }
    flush(&cur_heading, &mut cur_text, &mut ord);
    chunks
}

/// Extract distinct `[[wikilink]]` targets, stripping any `|alias` suffix.
///
/// Only prose is scanned: `[[x]]` inside an inline code span or a fenced code
/// block is literal text (e.g. documentation *about* the wikilink syntax), not
/// a link. Non-code text is concatenated first so a `[[link]]` that pulldown
/// splits across events is still matched.
fn extract_wikilinks(body: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());

    let mut prose = String::new();
    let mut in_code_block = false;
    for event in Parser::new(body) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Text(t) if !in_code_block => prose.push_str(&t),
            Event::SoftBreak | Event::HardBreak if !in_code_block => prose.push(' '),
            // Event::Code (inline code span) is intentionally skipped.
            _ => {}
        }
    }

    let mut seen = Vec::new();
    for cap in re.captures_iter(&prose) {
        let target = cap[1].split('|').next().unwrap_or("").trim().to_string();
        if !target.is_empty() && !seen.contains(&target) {
            seen.push(target);
        }
    }
    seen
}

/// Collapse runs of whitespace to single spaces and trim the ends.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frontmatter() {
        let (yaml, body) = split_frontmatter("---\nid: x\n---\nhello\n").unwrap();
        assert_eq!(yaml, "id: x\n");
        assert_eq!(body, "hello\n");
    }

    #[test]
    fn no_frontmatter_returns_none() {
        assert!(split_frontmatter("# just markdown\n").is_none());
        // Unclosed fence is not a valid block.
        assert!(split_frontmatter("---\nid: x\nnever closed\n").is_none());
    }

    #[test]
    fn extracts_distinct_wikilinks_stripping_aliases() {
        let links = extract_wikilinks("see [[a]] and [[b|Bee]] and [[a]] again");
        assert_eq!(links, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn ignores_wikilinks_inside_code() {
        // Inline code span and fenced block are literal, not links.
        let body = "real [[link-a]] but `[[code-span]]` and\n\n```\n[[in-block]]\n```\n";
        let links = extract_wikilinks(body);
        assert_eq!(links, vec!["link-a".to_string()]);
    }

    #[test]
    fn chunks_by_heading_with_lead_and_clean_inline() {
        let body = "lead text\n\n# First\n\nlinks to [[token-store]] here\n\n## Second\n\nmore";
        let chunks = chunk_by_heading(body);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].heading, None);
        assert_eq!(chunks[0].text, "lead text");
        assert_eq!(chunks[1].heading.as_deref(), Some("First"));
        // Inline `[[wikilink]]` must survive intact (no injected spaces).
        assert!(chunks[1].text.contains("[[token-store]]"));
        assert_eq!(chunks[2].heading.as_deref(), Some("Second"));
    }
}
