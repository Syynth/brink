//! Serde-derived JSON types for `lines.json` export.

use serde::{Deserialize, Serialize};

/// Top-level `lines.json` structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinesJson {
    pub version: u32,
    pub source_checksum: String,
    pub scopes: Vec<ScopeJson>,
}

/// A single scope (root, knot, or stitch) in the line table export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub id: String,
    pub lines: Vec<LineJson>,
}

/// Slot metadata in the JSON export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotJson {
    pub index: u8,
    pub name: String,
}

/// Source location metadata in the JSON export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceJson {
    pub file: String,
    pub range_start: u32,
    pub range_end: u32,
}

/// A single line entry within a scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineJson {
    pub index: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentJson>,
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub slots: Vec<SlotJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceJson>,
}

/// Line content — either a plain string or a template with parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentJson {
    Template { template: Vec<PartJson> },
    Plain(String),
}

/// A single part of a template line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PartJson {
    Slot { slot: u8 },
    Select { select: SelectJson },
    Literal(String),
}

/// A plural/keyword select over a slot value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectJson {
    pub slot: u8,
    pub variants: Vec<serde_json::Map<String, serde_json::Value>>,
    pub default: String,
}
