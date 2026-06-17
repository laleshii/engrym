//! `engrym lint` — validate the KB against the frontmatter contract.
//!
//! Default surfaces warnings but exits 0; `--strict` (run in CI) turns warnings
//! into failures. Hard structural problems (missing required fields, bad
//! altitude, duplicate ids, unknown relation types) are always errors.

use crate::config::Config;
use crate::model::{Frontmatter, EDGE_TYPES, MAX_ALTITUDE};
use crate::parse::{self, ParsedDoc};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use walkdir::WalkDir;

#[derive(Clone, Copy, PartialEq)]
enum Severity {
    Error,
    Warn,
}

struct Issue {
    severity: Severity,
    file: String,
    message: String,
}

/// Returns `true` if the KB passes (given the strictness setting).
pub fn run(config: &Config, strict: bool, json: bool) -> Result<bool> {
    let docs_root = config.docs_root();
    if !docs_root.is_dir() {
        bail!(
            "docs root {} does not exist (configured as docs.root = \"{}\")",
            docs_root.display(),
            config.docs.root
        );
    }

    // Pass 1: parse everything, gather ids + the topic prefix universe.
    let mut parsed_docs: Vec<ParsedDoc> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for entry in WalkDir::new(&docs_root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = rel_path_str(&docs_root, path);
        match parse::parse_file(path, &rel)? {
            Some(p) => parsed_docs.push(p),
            None => skipped.push(rel),
        }
    }

    // Collect declared ids (for dangling-target detection) and the map from
    // every topic prefix to the docs that use it (for typo detection).
    let mut ids: HashSet<String> = HashSet::new();
    let mut id_files: HashMap<String, Vec<String>> = HashMap::new();
    let mut prefix_docs: HashMap<String, HashSet<String>> = HashMap::new();

    for doc in &parsed_docs {
        if let Some(id) = &doc.frontmatter.id {
            ids.insert(id.clone());
            id_files.entry(id.clone()).or_default().push(doc.rel_path.clone());
            for topic in &doc.frontmatter.topics {
                for prefix in ancestor_prefixes(topic) {
                    prefix_docs.entry(prefix).or_default().insert(id.clone());
                }
            }
        }
    }

    // Pass 2: validate each doc.
    let mut issues: Vec<Issue> = Vec::new();
    for doc in &parsed_docs {
        validate_doc(doc, &ids, &prefix_docs, &mut issues);
    }

    // Duplicate ids (reported once per offending id).
    for (id, files) in &id_files {
        if files.len() > 1 {
            issues.push(Issue {
                severity: Severity::Error,
                file: files.join(", "),
                message: format!("duplicate id `{}`", id),
            });
        }
    }

    let errors = issues.iter().filter(|i| i.severity == Severity::Error).count();
    let warnings = issues.iter().filter(|i| i.severity == Severity::Warn).count();
    // Under --strict, warnings also fail the run.
    let passed = errors == 0 && (!strict || warnings == 0);

    report(&issues, &skipped, parsed_docs.len(), strict, passed, json);
    Ok(passed)
}

fn validate_doc(
    doc: &ParsedDoc,
    ids: &HashSet<String>,
    prefix_docs: &HashMap<String, HashSet<String>>,
    issues: &mut Vec<Issue>,
) {
    let fm: &Frontmatter = &doc.frontmatter;

    // Collect (severity, message) locally, then attach the file once at the end.
    let mut found: Vec<(Severity, String)> = Vec::new();
    let mut err = |m: String| found.push((Severity::Error, m));

    // Required fields.
    if fm.id.is_none() {
        err("missing required field `id`".into());
    }
    if fm.title.is_none() {
        err("missing required field `title`".into());
    }
    match fm.altitude {
        None => err("missing required field `altitude`".into()),
        Some(a) if !(0..=MAX_ALTITUDE).contains(&a) => {
            err(format!("altitude {} out of range 0..={}", a, MAX_ALTITUDE))
        }
        _ => {}
    }
    if fm.topics.is_empty() {
        err("missing required field `topics` (need at least one)".into());
    }

    // Relation types + dangling targets.
    for rel in &fm.relations {
        if !EDGE_TYPES.contains(&rel.edge_type.as_str()) {
            found.push((
                Severity::Error,
                format!(
                    "unknown relation type `{}` (expected one of: {})",
                    rel.edge_type,
                    EDGE_TYPES.join(", ")
                ),
            ));
        } else if !ids.contains(&rel.target) {
            found.push((
                Severity::Warn,
                format!("relation `{}` → `{}` targets a non-existent doc id", rel.edge_type, rel.target),
            ));
        }
    }

    // Wikilink targets that don't resolve to a doc id.
    for target in &doc.wikilinks {
        if !ids.contains(target) {
            found.push((
                Severity::Warn,
                format!("wikilink [[{}]] targets a non-existent doc id", target),
            ));
        }
    }

    // Topic typo heuristic: a topic whose entire prefix chain is used by no
    // other document is likely a typo.
    if let Some(id) = &fm.id {
        for topic in &fm.topics {
            let lonely = ancestor_prefixes(topic).iter().all(|prefix| {
                prefix_docs
                    .get(prefix)
                    .map(|docs| docs.iter().all(|d| d == id))
                    .unwrap_or(true)
            });
            if lonely {
                found.push((
                    Severity::Warn,
                    format!("topic `{}` is not shared by any other document (possible typo)", topic),
                ));
            }
        }
    }

    // Empty body.
    if doc.body.trim().is_empty() {
        found.push((Severity::Warn, "body is empty (frontmatter only)".into()));
    }

    for (severity, message) in found {
        issues.push(Issue {
            severity,
            file: doc.rel_path.clone(),
            message,
        });
    }
}

/// `a/b/c` → [`a`, `a/b`, `a/b/c`].
fn ancestor_prefixes(topic: &str) -> Vec<String> {
    let segments: Vec<&str> = topic.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::with_capacity(segments.len());
    let mut acc = String::new();
    for seg in segments {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(seg);
        out.push(acc.clone());
    }
    out
}

fn rel_path_str(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn report(
    issues: &[Issue],
    skipped: &[String],
    doc_count: usize,
    strict: bool,
    passed: bool,
    json: bool,
) {
    let errors = issues.iter().filter(|i| i.severity == Severity::Error).count();
    let warnings = issues.iter().filter(|i| i.severity == Severity::Warn).count();

    if json {
        let arr: Vec<_> = issues
            .iter()
            .map(|i| {
                serde_json::json!({
                    "severity": match i.severity { Severity::Error => "error", Severity::Warn => "warn" },
                    "file": i.file,
                    "message": i.message,
                })
            })
            .collect();
        let out = serde_json::json!({
            "documents": doc_count,
            "errors": errors,
            "warnings": warnings,
            "strict": strict,
            "passed": passed,
            "issues": arr,
            "skipped": skipped,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return;
    }

    for i in issues {
        let tag = match i.severity {
            Severity::Error => "\x1b[31merror\x1b[0m",
            Severity::Warn => "\x1b[33mwarn \x1b[0m",
        };
        println!("{} {}: {}", tag, i.file, i.message);
    }
    if !issues.is_empty() {
        println!();
    }
    println!(
        "Checked {} document(s): {} error(s), {} warning(s){}.",
        doc_count,
        errors,
        warnings,
        if strict { " [strict]" } else { "" }
    );
    if passed {
        println!("\x1b[32mOK\x1b[0m");
    } else {
        println!("\x1b[31mFAILED\x1b[0m");
    }
}
