//! `engrym browse` — a local web server for reading and navigating the KB.
//!
//! Server-rendered HTML: each document is rendered from its Markdown
//! (via pulldown-cmark, already a dependency), `[[wikilinks]]` become real
//! links, and a side panel surfaces the graph — typed relations (in/out),
//! same-altitude docs, and same-topic docs — all clickable. Reads source files
//! per request, so edits show on refresh. Everything else (the graph, topics)
//! comes from the SQLite index, exactly like `related`/`topic`/`search`. The
//! only script is the theme toggle, and it degrades to the OS preference.

use crate::config::Config;
use crate::db;
use crate::parse;
use anyhow::{anyhow, Result};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::sync::OnceLock;

pub fn run(config: &Config, port: u16, open: bool) -> Result<()> {
    // Fail fast with a friendly message if the index isn't built yet.
    db::open_existing(&config.index_path())?;

    let addr = format!("127.0.0.1:{}", port);
    let server = tiny_http::Server::http(&addr)
        .map_err(|e| anyhow!("couldn't start the server on {} ({})", addr, e))?;
    let shown = server
        .server_addr()
        .to_ip()
        .map(|s| s.to_string())
        .unwrap_or(addr);
    let url = format!("http://{}", shown);

    println!("engrym browse → {}  (Ctrl-C to stop)", url);
    if open {
        open_browser(&url);
    }

    for request in server.incoming_requests() {
        let (status, body) = handle(config, request.url());
        let header =
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                .unwrap();
        let response = tiny_http::Response::from_string(body)
            .with_status_code(tiny_http::StatusCode(status))
            .with_header(header);
        let _ = request.respond(response);
    }
    Ok(())
}

fn handle(config: &Config, url: &str) -> (u16, String) {
    match route(config, url) {
        Ok(Some(html)) => (200, html),
        Ok(None) => (404, page(config, None, "Not found", "<p>No such page.</p>")),
        Err(e) => (
            500,
            page(config, None, "Error", &format!("<pre>{}</pre>", esc(&format!("{e:#}")))),
        ),
    }
}

fn route(config: &Config, url: &str) -> Result<Option<String>> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let conn = db::open_existing(&config.index_path())?;

    if path == "/" {
        return Ok(Some(index_page(config, &conn)?));
    }
    if let Some(rest) = path.strip_prefix("/doc/") {
        return doc_page(config, &conn, &percent_decode(rest));
    }
    if let Some(rest) = path.strip_prefix("/topic/") {
        return Ok(Some(topic_page(config, &conn, &percent_decode(rest))?));
    }
    if path == "/search" {
        return Ok(Some(search_page(config, &conn, &param(query, "q"))?));
    }
    if path == "/favicon.ico" {
        return Ok(Some(String::new()));
    }
    Ok(None)
}

// --- Pages ----------------------------------------------------------------

fn index_page(config: &Config, conn: &Connection) -> Result<String> {
    let docs = all_docs(conn)?;
    let mut main = String::from("<h1>Knowledge base</h1>");
    for alt in 0..=3 {
        let cards: String = docs.iter().filter(|d| d.altitude == alt).map(doc_card).collect();
        if cards.is_empty() {
            continue;
        }
        main.push_str(&format!("<h2>{}</h2><ul class=cards>{}</ul>", altitude_label(alt), cards));
    }
    Ok(page(config, None, "Knowledge base", &main))
}

fn topic_page(config: &Config, conn: &Connection, prefix: &str) -> Result<String> {
    let docs = docs_under_topic(conn, prefix)?;
    let mut main = format!("<h1>Topic: <code>{}</code></h1>", esc(prefix));
    if docs.is_empty() {
        main.push_str("<p>No documents under this topic.</p>");
    } else {
        let cards: String = docs.iter().map(doc_card).collect();
        main.push_str(&format!("<p class=summary>{} document(s) at or below this topic.</p><ul class=cards>{}</ul>", docs.len(), cards));
    }
    Ok(page(config, None, &format!("topic: {}", prefix), &main))
}

fn doc_card(d: &Doc) -> String {
    format!(
        "<li><a href=\"/doc/{}\">{}</a>{}</li>",
        esc(&d.id),
        esc(&d.title),
        d.summary
            .as_deref()
            .map(|s| format!("<span class=summary>{}</span>", esc(s)))
            .unwrap_or_default()
    )
}

/// Topic breadcrumbs: each path segment links to `/topic/<prefix>` and shows the
/// count of documents at or below that prefix.
fn topics_panel(conn: &Connection, topics: &[String]) -> Result<String> {
    if topics.is_empty() {
        return Ok(String::new());
    }
    let mut rows = String::new();
    for topic in topics {
        let mut crumb = String::new();
        let mut prefix = String::new();
        for (i, seg) in topic.split('/').enumerate() {
            if i > 0 {
                prefix.push('/');
                crumb.push_str("<span class=sep>/</span>");
            }
            prefix.push_str(seg);
            crumb.push_str(&format!(
                "<a href=\"/topic/{}\">{}</a><span class=tcount>{}</span>",
                esc(&prefix),
                esc(seg),
                topic_count(conn, &prefix)?
            ));
        }
        rows.push_str(&format!("<div class=crumb>{}</div>", crumb));
    }
    Ok(format!("<div class=panel><h4>Topics</h4>{}</div>", rows))
}

fn topic_count(conn: &Connection, prefix: &str) -> Result<i64> {
    let n = conn.query_row(
        "SELECT COUNT(DISTINCT doc_id) FROM topics WHERE path = ?1 OR path LIKE ?2",
        params![prefix, format!("{}/%", prefix)],
        |r| r.get(0),
    )?;
    Ok(n)
}

fn docs_under_topic(conn: &Connection, prefix: &str) -> Result<Vec<Doc>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT d.id, d.title, d.altitude, d.summary FROM docs d
         JOIN topics t ON t.doc_id = d.id
         WHERE t.path = ?1 OR t.path LIKE ?2 ORDER BY d.altitude, d.title",
    )?;
    let docs = stmt
        .query_map(params![prefix, format!("{}/%", prefix)], |r| {
            Ok(Doc { id: r.get(0)?, title: r.get(1)?, altitude: r.get(2)?, summary: r.get(3)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(docs)
}

/// Drop a leading `# Heading` line from the body (we render the title ourselves).
fn strip_leading_h1(body: &str) -> &str {
    let trimmed = body.trim_start_matches(['\n', '\r', ' ', '\t']);
    if let Some(rest) = trimmed.strip_prefix("# ") {
        return rest.find('\n').map(|nl| &rest[nl + 1..]).unwrap_or("");
    }
    body
}

fn doc_page(config: &Config, conn: &Connection, id: &str) -> Result<Option<String>> {
    let meta = conn
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
    let Some((rel_path, title, altitude, summary)) = meta else {
        return Ok(None);
    };

    // Body: render the source Markdown (minus its own `# Title`, which we render
    // ourselves), rewriting [[wikilinks]] to /doc/ links.
    let abs = config.docs_root().join(&rel_path);
    let body_md = parse::parse_file(&abs, &rel_path)?.map(|p| p.body).unwrap_or_default();
    let body_html = render_markdown(strip_leading_h1(&body_md));

    let topics = topics_of(conn, id)?;
    let outbound = edges(conn, id, true)?;
    let inbound = edges(conn, id, false)?;
    let same_topic = same_topic(conn, id)?;

    // Title row: altitude on the right; summary beneath.
    let header = format!(
        "<header class=dochead><div class=titlerow><h1>{}</h1>\
         <span class=\"badge alt{altitude}\">{}</span></div>{}</header>",
        esc(&title),
        altitude_label(altitude),
        summary.as_deref().map(|s| format!("<p class=summary>{}</p>", esc(s))).unwrap_or_default()
    );

    let mut aside = String::new();
    aside.push_str(&topics_panel(conn, &topics)?);
    aside.push_str(&edge_section("Outbound", &outbound, true));
    aside.push_str(&edge_section("Inbound", &inbound, false));
    aside.push_str(&doc_list_section("Same topic", &same_topic));

    let main = format!(
        "<article><div class=main>{}<div class=doc>{}</div></div><aside>{}</aside></article>",
        header, body_html, aside
    );
    Ok(Some(page(config, Some(id), &title, &main)))
}

fn search_page(config: &Config, conn: &Connection, q: &str) -> Result<String> {
    let mut main = format!(
        "<h1>Search</h1><form action=/search><input name=q value=\"{}\" \
         placeholder=\"search the KB\" autofocus></form>",
        esc(q)
    );
    if !q.trim().is_empty() {
        let hits = keyword_search(conn, q)?;
        if hits.is_empty() {
            main.push_str(&format!("<p>No matches for “{}”.</p>", esc(q)));
        } else {
            let cards: String = hits.iter().map(doc_card).collect();
            main.push_str(&format!("<ul class=cards>{}</ul>", cards));
        }
    }
    Ok(page(config, None, "Search", &main))
}

// --- HTML chrome ----------------------------------------------------------

fn page(config: &Config, current: Option<&str>, title: &str, main: &str) -> String {
    let sidebar = db::open_existing(&config.index_path())
        .ok()
        .and_then(|c| sidebar(&c, current).ok())
        .unwrap_or_default();
    format!(
        "<!doctype html><html lang=en><head><meta charset=utf-8>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>{} · engrym</title><style>{}</style>{}</head><body>\
         <header class=topbar><a href=/ class=brand>engrym</a>{}\
         <form action=/search class=search><input name=q placeholder=search></form></header>\
         <div class=layout><nav>{}</nav><section class=content>{}</section></div></body></html>",
        esc(title),
        CSS,
        THEME_SCRIPT,
        THEME_BUTTON,
        sidebar,
        main
    )
}

fn sidebar(conn: &Connection, current: Option<&str>) -> Result<String> {
    let docs = all_docs(conn)?;
    let mut out = String::new();
    for alt in 0..=3 {
        let group: Vec<&Doc> = docs.iter().filter(|d| d.altitude == alt).collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(&format!("<h3>{}</h3><ul>", altitude_label(alt)));
        for d in group {
            let here = current == Some(d.id.as_str());
            out.push_str(&format!(
                "<li{}><a href=\"/doc/{}\">{}</a></li>",
                if here { " class=here" } else { "" },
                esc(&d.id),
                esc(&d.title)
            ));
        }
        out.push_str("</ul>");
    }
    Ok(out)
}

fn edge_section(label: &str, edges: &[Edge], outbound: bool) -> String {
    if edges.is_empty() {
        return String::new();
    }
    // Group consecutive edges by relation type (the query sorts by type), so the
    // type shows once as a label above its documents rather than on every row.
    let mut groups: Vec<(&str, Vec<&Edge>)> = Vec::new();
    for e in edges {
        match groups.last_mut() {
            Some((t, v)) if *t == e.edge_type => v.push(e),
            _ => groups.push((&e.edge_type, vec![e])),
        }
    }
    let body: String = groups
        .iter()
        .map(|(t, es)| {
            let items: String = es
                .iter()
                .map(|e| match &e.other_title {
                    Some(title) => {
                        format!("<li><a href=\"/doc/{}\">{}</a></li>", esc(&e.other), esc(title))
                    }
                    None => format!("<li class=dangling>{} (missing)</li>", esc(&e.other)),
                })
                .collect();
            format!(
                "<div class=egroup><span class=etype>{}</span><ul>{}</ul></div>",
                esc(edge_label(t, outbound)),
                items
            )
        })
        .collect();
    format!("<div class=panel><h4>{}</h4>{}</div>", esc(label), body)
}

/// A relation type phrased for its direction, so the label reads correctly from
/// the current document's point of view (e.g. `references` vs `referenced by`).
fn edge_label(edge_type: &str, outbound: bool) -> &str {
    match (edge_type, outbound) {
        ("refines", true) => "refines",
        ("refines", false) => "refined by",
        ("part_of", true) => "part of",
        ("part_of", false) => "has part",
        ("depends_on", true) => "depends on",
        ("depends_on", false) => "required by",
        ("references", true) => "references",
        ("references", false) => "referenced by",
        ("supersedes", true) => "supersedes",
        ("supersedes", false) => "superseded by",
        (other, _) => other,
    }
}

fn doc_list_section(label: &str, docs: &[Doc]) -> String {
    if docs.is_empty() {
        return String::new();
    }
    let items: String = docs
        .iter()
        .map(|d| format!("<li><a href=\"/doc/{}\">{}</a></li>", esc(&d.id), esc(&d.title)))
        .collect();
    format!("<div class=panel><h4>{}</h4><ul>{}</ul></div>", esc(label), items)
}

// --- Markdown rendering with wikilink rewriting ---------------------------

/// Render Markdown to HTML, first rewriting `[[id]]` / `[[id|alias]]` (outside
/// code) into ordinary `[alias](/doc/id)` links so pulldown-cmark handles them.
fn render_markdown(body: &str) -> String {
    let rewritten = rewrite_wikilinks(body);
    // GFM extensions off by default in pulldown-cmark — enable the common ones
    // (tables especially, or they render as flattened paragraphs).
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(&rewritten, opts));
    out
}

/// Turn prose `[[id]]` / `[[id|alias]]` into Markdown links. Done on the raw
/// source (pulldown splits `[[…]]` across events), skipping code spans/blocks
/// located by byte offset — a `[[x]]` shown in code is documentation, not a link.
fn rewrite_wikilinks(body: &str) -> String {
    let code = code_ranges(body);
    let mut out = String::with_capacity(body.len());
    let mut last = 0;
    for cap in wikilink_re().captures_iter(body) {
        let m = cap.get(0).unwrap();
        if code.iter().any(|(s, e)| m.start() >= *s && m.start() < *e) {
            continue; // inside code — leave literal
        }
        out.push_str(&body[last..m.start()]);
        let mut parts = cap[1].splitn(2, '|');
        let id = parts.next().unwrap_or("").trim();
        let label = parts.next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or(id);
        out.push_str(&format!("[{}](/doc/{})", label.replace(['[', ']'], ""), id));
        last = m.end();
    }
    out.push_str(&body[last..]);
    out
}

/// Byte ranges of code spans and fenced blocks in the source.
fn code_ranges(body: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut block_start: Option<usize> = None;
    for (ev, r) in Parser::new(body).into_offset_iter() {
        match ev {
            Event::Start(Tag::CodeBlock(_)) => block_start = Some(r.start),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(s) = block_start.take() {
                    ranges.push((s, r.end));
                }
            }
            Event::Code(_) => ranges.push((r.start, r.end)),
            _ => {}
        }
    }
    ranges
}

fn wikilink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap())
}

// --- Queries --------------------------------------------------------------

struct Doc {
    id: String,
    title: String,
    altitude: i64,
    summary: Option<String>,
}

struct Edge {
    edge_type: String,
    other: String,
    other_title: Option<String>,
}

fn all_docs(conn: &Connection) -> Result<Vec<Doc>> {
    let mut stmt =
        conn.prepare("SELECT id, title, altitude, summary FROM docs ORDER BY altitude, title")?;
    let docs = stmt
        .query_map([], |r| {
            Ok(Doc {
                id: r.get(0)?,
                title: r.get(1)?,
                altitude: r.get(2)?,
                summary: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(docs)
}

fn topics_of(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM topics WHERE doc_id = ?1 ORDER BY path")?;
    let t = stmt.query_map(params![id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(t)
}

fn edges(conn: &Connection, id: &str, outbound: bool) -> Result<Vec<Edge>> {
    let sql = if outbound {
        "SELECT e.type, e.dst, d.title FROM edges e LEFT JOIN docs d ON d.id = e.dst
         WHERE e.src = ?1 ORDER BY e.type, e.dst"
    } else {
        "SELECT e.type, e.src, d.title FROM edges e LEFT JOIN docs d ON d.id = e.src
         WHERE e.dst = ?1 ORDER BY e.type, e.src"
    };
    let mut stmt = conn.prepare(sql)?;
    let edges = stmt
        .query_map(params![id], |r| {
            Ok(Edge {
                edge_type: r.get(0)?,
                other: r.get(1)?,
                other_title: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(edges)
}

fn same_topic(conn: &Connection, id: &str) -> Result<Vec<Doc>> {
    // Docs that share at least one exact topic with this one.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT d.id, d.title, d.altitude, d.summary FROM docs d
         JOIN topics t ON t.doc_id = d.id
         WHERE t.path IN (SELECT path FROM topics WHERE doc_id = ?1) AND d.id != ?1
         ORDER BY d.altitude, d.title",
    )?;
    let docs = stmt
        .query_map(params![id], |r| {
            Ok(Doc { id: r.get(0)?, title: r.get(1)?, altitude: r.get(2)?, summary: r.get(3)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(docs)
}

fn keyword_search(conn: &Connection, q: &str) -> Result<Vec<Doc>> {
    let Some(match_expr) = fts_match(q) else {
        return Ok(vec![]);
    };
    // FTS5's bm25() is only usable querying the fts table directly (not inside an
    // aggregate / join). So rank chunk rowids here, then resolve docs in code.
    let mut rank_stmt =
        conn.prepare("SELECT rowid FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts) LIMIT 200")?;
    let rowids: Vec<i64> = rank_stmt
        .query_map(params![match_expr], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    // Map ranked chunks → docs, keeping the best (first) rank per doc.
    let mut seen = HashSet::new();
    let mut doc_ids = Vec::new();
    let mut doc_of = conn.prepare("SELECT doc_id FROM chunks WHERE id = ?1")?;
    for rid in rowids {
        let doc_id: Option<String> = doc_of.query_row(params![rid], |r| r.get(0)).optional()?;
        if let Some(id) = doc_id {
            if seen.insert(id.clone()) {
                doc_ids.push(id);
                if doc_ids.len() >= 30 {
                    break;
                }
            }
        }
    }

    let mut meta = conn.prepare("SELECT id, title, altitude, summary FROM docs WHERE id = ?1")?;
    let mut out = Vec::new();
    for id in doc_ids {
        if let Some(d) = meta
            .query_row(params![id], |r| {
                Ok(Doc { id: r.get(0)?, title: r.get(1)?, altitude: r.get(2)?, summary: r.get(3)? })
            })
            .optional()?
        {
            out.push(d);
        }
    }
    Ok(out)
}

/// Build a safe FTS5 MATCH expression: alphanumeric tokens, quoted, OR-joined.
fn fts_match(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

// --- Helpers --------------------------------------------------------------

fn altitude_label(alt: i64) -> &'static str {
    match alt {
        0 => "Overview (0)",
        1 => "Subsystem (1)",
        2 => "Component (2)",
        _ => "Detail (3)",
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The value of a query-string key (percent-decoded). No deps.
fn param(query: &str, key: &str) -> String {
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
            return percent_decode(v);
        }
    }
    String::new()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn open_browser(url: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(url).spawn();
}

/// Runs in `<head>`, before the body paints, so a stored choice never flashes
/// the other theme. "auto" is the absence of `data-theme`, which leaves the
/// `prefers-color-scheme` rules in charge.
const THEME_SCRIPT: &str = r#"<script>
(function(){
  var root=document.documentElement,key='engrym-theme',modes=['auto','light','dark'];
  function label(m){return 'Theme: '+m+' (click to change)'}
  function apply(m,save){
    if(m==='auto'){root.removeAttribute('data-theme')}else{root.setAttribute('data-theme',m)}
    if(save){try{m==='auto'?localStorage.removeItem(key):localStorage.setItem(key,m)}catch(e){}}
    var b=document.getElementById('theme');
    if(b){b.setAttribute('title',label(m));b.setAttribute('aria-label',label(m))}
  }
  var stored;
  try{stored=localStorage.getItem(key)}catch(e){}
  apply(modes.indexOf(stored)>0?stored:'auto',false);
  // The button doesn't exist yet at head time, so label it once it does.
  document.addEventListener('DOMContentLoaded',function(){apply(root.getAttribute('data-theme')||'auto',false)});
  document.addEventListener('click',function(e){
    if(!e.target.closest||!e.target.closest('#theme'))return;
    var cur=root.getAttribute('data-theme')||'auto';
    apply(modes[(modes.indexOf(cur)+1)%modes.length],true);
  });
})()
</script>"#;

/// All three icons ship in the markup; CSS reveals the one matching the mode.
const THEME_BUTTON: &str = concat!(
    r#"<button id=theme class=themebtn type=button title="Theme: auto (click to change)" aria-label="Theme: auto (click to change)">"#,
    r#"<svg class=i-auto viewBox="0 0 24 24" aria-hidden=true><circle cx=12 cy=12 r=8.25 /><path d="M12 3.75a8.25 8.25 0 0 0 0 16.5Z" fill=currentColor stroke=none /></svg>"#,
    r#"<svg class=i-light viewBox="0 0 24 24" aria-hidden=true><circle cx=12 cy=12 r=4.25 /><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M18.7 5.3l-1.4 1.4M6.7 17.3l-1.4 1.4"/></svg>"#,
    r#"<svg class=i-dark viewBox="0 0 24 24" aria-hidden=true><path d="M20.5 14.6A8.5 8.5 0 1 1 9.4 3.5a6.9 6.9 0 0 0 11.1 11.1Z"/></svg>"#,
    r#"</button>"#,
);

const CSS: &str = "\
:root{color-scheme:light dark;--fg:#1c1c1e;--muted:#6b6b70;--line:#e4e4e7;--bg:#fff;--accent:#3b5bdb;--soft:#f6f6f7;--danger:#b42318;--a0bg:#e7f0ff;--a0fg:#1f4fd6;--a1bg:#e9f7ee;--a1fg:#1f9254;--a2bg:#fff4e6;--a2fg:#c2750a;--a3bg:#f3eefc;--a3fg:#7a45c9}
*{box-sizing:border-box}
body{margin:0;font:15px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;color:var(--fg);background:var(--bg)}
a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
.topbar{display:flex;align-items:center;gap:1rem;padding:.6rem 1.2rem;border-bottom:1px solid var(--line);position:sticky;top:0;background:var(--bg);z-index:1}
.brand{font-weight:700;font-size:1.05rem;color:var(--fg)}
.search{margin-left:auto}.search input,form input{padding:.35rem .6rem;border:1px solid var(--line);border-radius:6px;font:inherit;min-width:16rem;background:var(--bg);color:var(--fg)}
.layout{display:grid;grid-template-columns:16rem 1fr;align-items:start}
nav{position:sticky;top:3rem;max-height:calc(100vh - 3rem);overflow:auto;padding:1rem;border-right:1px solid var(--line)}
nav h3{font-size:.72rem;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);margin:1rem 0 .3rem}
nav ul{list-style:none;margin:0 0 .6rem;padding:0}nav li{padding:.2rem 0;line-height:1.35}
nav li.here>a{font-weight:700;color:var(--fg)}
.content{padding:1.4rem 2.2rem;max-width:82rem}
article{display:grid;grid-template-columns:minmax(0,1fr) 19rem;gap:2.6rem;align-items:start}
.main{min-width:0}.doc{min-width:0}.doc h1,.doc h2,.doc h3{line-height:1.25}
.doc pre{background:var(--soft);padding:.8rem 1rem;border-radius:8px;overflow:auto}
.doc code{background:var(--soft);padding:.1em .35em;border-radius:4px;font-size:.9em}
.doc pre code{background:none;padding:0}
.doc table{border-collapse:collapse;margin:1rem 0;font-size:.92rem;display:block;overflow-x:auto}
.doc th,.doc td{border:1px solid var(--line);padding:.4rem .7rem;text-align:left;vertical-align:top}
.doc th{background:var(--soft);font-weight:600}
.dochead{border-bottom:1px solid var(--line);margin-bottom:1rem;padding-bottom:.6rem}
.titlerow{display:flex;justify-content:space-between;align-items:baseline;gap:1rem}
.titlerow h1{margin:0}
.dochead .summary{margin:.5rem 0 0;font-size:1rem}
.summary{color:var(--muted)}
.crumb{margin:.2rem 0;line-height:1.5}.crumb a{color:var(--fg)}
.sep{color:var(--muted);margin:0 .15rem}
.tcount{color:var(--muted);font-size:.7rem;margin-left:.2rem}
ul.cards{list-style:none;padding:0}ul.cards li{padding:.5rem 0;border-bottom:1px solid var(--line)}
ul.cards .summary{display:block;font-size:.9rem}
aside{position:sticky;top:4rem;font-size:.9rem;line-height:1.45}
.panel{margin-bottom:1.6rem}
.panel h4{margin:0 0 .5rem;padding-bottom:.3rem;border-bottom:1px solid var(--line);font-size:.7rem;text-transform:uppercase;letter-spacing:.05em;color:var(--muted)}
.egroup{margin:0 0 .7rem}
.etype{display:block;font-size:.68rem;text-transform:uppercase;letter-spacing:.04em;color:var(--muted);margin-bottom:.1rem}
.panel ul{list-style:none;margin:0;padding:0}.panel li{padding:.15rem 0}
.dangling{color:var(--danger)}
.meta{margin-bottom:1rem;display:flex;flex-wrap:wrap;gap:.3rem}
.badge,.topic{font-size:.72rem;padding:.1rem .5rem;border-radius:999px;background:var(--soft);color:var(--muted)}
.badge.alt0{background:var(--a0bg);color:var(--a0fg)}.badge.alt1{background:var(--a1bg);color:var(--a1fg)}
.badge.alt2{background:var(--a2bg);color:var(--a2fg)}.badge.alt3{background:var(--a3bg);color:var(--a3fg)}
/* Auto (no data-theme) follows the OS; [data-theme] is the explicit override.
   Keep the two dark token lists in sync — a media query can't share a rule. */
@media(prefers-color-scheme:dark){:root:not([data-theme=light]){--fg:#e7e7ea;--muted:#9b9ba4;--line:#2d2d33;--bg:#16161a;--accent:#8fa8ff;--soft:#202026;--danger:#ff8a80;--a0bg:#1b2748;--a0fg:#a6bcff;--a1bg:#15301f;--a1fg:#7ad498;--a2bg:#38280f;--a2fg:#e8ab5c;--a3bg:#2a2140;--a3fg:#c39bf0}}
:root[data-theme=dark]{color-scheme:dark;--fg:#e7e7ea;--muted:#9b9ba4;--line:#2d2d33;--bg:#16161a;--accent:#8fa8ff;--soft:#202026;--danger:#ff8a80;--a0bg:#1b2748;--a0fg:#a6bcff;--a1bg:#15301f;--a1fg:#7ad498;--a2bg:#38280f;--a2fg:#e8ab5c;--a3bg:#2a2140;--a3fg:#c39bf0}
:root[data-theme=light]{color-scheme:light}
.themebtn{display:inline-flex;align-items:center;justify-content:center;width:1.85rem;height:1.85rem;margin-left:-.5rem;padding:0;border:1px solid var(--line);border-radius:6px;background:none;color:var(--muted);cursor:pointer}
.themebtn:hover{color:var(--fg);border-color:var(--muted)}
.themebtn svg{display:none;width:1.05rem;height:1.05rem;fill:none;stroke:currentColor;stroke-width:1.7;stroke-linecap:round;stroke-linejoin:round}
:root:not([data-theme]) .themebtn .i-auto{display:block}
:root[data-theme=light] .themebtn .i-light{display:block}
:root[data-theme=dark] .themebtn .i-dark{display:block}
@media(max-width:900px){.layout{grid-template-columns:1fr}nav{display:none}article{grid-template-columns:1fr}aside{position:static}}
";

#[cfg(test)]
mod tests {
    use super::{edge_label, fts_match, percent_decode, render_markdown, rewrite_wikilinks};

    #[test]
    fn edge_labels_read_by_direction() {
        assert_eq!(edge_label("references", true), "references");
        assert_eq!(edge_label("references", false), "referenced by");
        assert_eq!(edge_label("depends_on", false), "required by");
        assert_eq!(edge_label("refines", false), "refined by");
        assert_eq!(edge_label("unknown", true), "unknown"); // graceful fallback
    }

    #[test]
    fn renders_gfm_tables() {
        let html = render_markdown("| A | B |\n|---|---|\n| 1 | 2 |\n");
        assert!(html.contains("<table>") && html.contains("<td>1</td>"), "{html}");
    }

    #[test]
    fn rewrites_prose_wikilinks_only() {
        // Prose links convert; aliases are honored.
        let out = rewrite_wikilinks("see [[token-store]] and [[oauth|OAuth]]");
        assert!(out.contains("[token-store](/doc/token-store)"), "{out}");
        assert!(out.contains("[OAuth](/doc/oauth)"), "{out}");
    }

    #[test]
    fn leaves_wikilinks_in_code_untouched() {
        // Inline code span and fenced block are documentation, not links.
        let out = rewrite_wikilinks("`[[x]]` then [[y]]\n\n```\n[[z]]\n```\n");
        assert!(out.contains("`[[x]]`"), "inline code changed: {out}");
        assert!(out.contains("[[z]]"), "fenced block changed: {out}");
        assert!(out.contains("[y](/doc/y)"), "prose link missing: {out}");
    }

    #[test]
    fn percent_decode_handles_escapes_and_plus() {
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("x%2Fy"), "x/y");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn fts_match_tokenizes_safely() {
        assert_eq!(fts_match("hello, world!").as_deref(), Some("\"hello\" OR \"world\""));
        assert_eq!(fts_match("   "), None);
    }
}
