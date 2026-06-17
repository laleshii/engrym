//! The document data model: frontmatter contract + typed edges.
//!
//! Mirrors `spec/document-schema.md`. Frontmatter fields are parsed as `Option`
//! so that `lint` can report *which* required fields are missing rather than
//! failing to parse the whole file; `index` enforces presence explicitly.
//!
//! The same struct is serialized by the authoring commands (`new`/`set`), so the
//! field order here is the on-disk field order, and empty collections / absent
//! optionals are omitted to keep generated frontmatter clean and reviewable.

use serde::{Deserialize, Serialize};

/// The five typed relation kinds. Inline `[[wikilinks]]` synthesize `references`.
pub const EDGE_TYPES: &[&str] = &["refines", "part_of", "depends_on", "references", "supersedes"];

pub const MAX_ALTITUDE: i64 = 3;

/// Raw, possibly-incomplete frontmatter as authored. Validation happens later.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Frontmatter {
    pub id: Option<String>,
    pub title: Option<String>,
    pub altitude: Option<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Relation {
    #[serde(rename = "type")]
    pub edge_type: String,
    pub target: String,
}

impl Relation {
    pub fn is_valid_type(&self) -> bool {
        EDGE_TYPES.contains(&self.edge_type.as_str())
    }
}
