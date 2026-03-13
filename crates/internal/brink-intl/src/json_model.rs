//! Serde-derived JSON types for `lines.json` export.

use serde::Serialize;

/// Top-level `lines.json` structure.
#[derive(Debug, Serialize)]
pub struct LinesJson {
    pub version: u32,
    pub source_checksum: String,
    pub scopes: Vec<ScopeJson>,
}

/// A single scope (root, knot, or stitch) in the line table export.
#[derive(Debug, Serialize)]
pub struct ScopeJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub id: String,
    pub lines: Vec<LineJson>,
}

/// A single line entry within a scope.
#[derive(Debug, Serialize)]
pub struct LineJson {
    pub index: u16,
    pub content: ContentJson,
    pub hash: String,
}

/// Line content — either a plain string or a template with parts.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ContentJson {
    Plain(String),
    Template { template: Vec<PartJson> },
}

/// A single part of a template line.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PartJson {
    Literal(String),
    Slot { slot: u8 },
    Select { select: SelectJson },
}

/// A plural/keyword select over a slot value.
#[derive(Debug, Serialize)]
pub struct SelectJson {
    pub slot: u8,
    pub variants: Vec<serde_json::Map<String, serde_json::Value>>,
    pub default: String,
}
