pub mod extensions;
pub mod inline;

pub use extensions::{ExtensionAttribute, ExtensionElement, ExtensionNode, Extensions};
pub use inline::{CanReorder, Ec, Em, InlineElement, Mrk, Pc, Ph, Sc, Sm};

/// Root element of an XLIFF 2.0 document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Must be `"2.0"` or `"2.1"`.
    pub version: String,
    /// BCP 47 source language tag.
    pub src_lang: String,
    /// BCP 47 target language tag.
    pub trg_lang: Option<String>,
    pub files: Vec<File>,
    pub extensions: Extensions,
}

/// A `<file>` element containing translation units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub id: String,
    pub original: Option<String>,
    pub notes: Vec<Note>,
    pub skeleton: Option<Skeleton>,
    pub groups: Vec<Group>,
    pub units: Vec<Unit>,
    pub extensions: Extensions,
}

/// A `<group>` element for organizing units and nested groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub name: Option<String>,
    pub notes: Vec<Note>,
    pub groups: Vec<Group>,
    pub units: Vec<Unit>,
    pub extensions: Extensions,
}

/// A `<unit>` element — the fundamental container for translatable content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub id: String,
    pub name: Option<String>,
    pub notes: Vec<Note>,
    pub sub_units: Vec<SubUnit>,
    pub original_data: Option<OriginalData>,
    pub extensions: Extensions,
}

/// Either a `<segment>` or `<ignorable>` within a unit, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubUnit {
    Segment(Segment),
    Ignorable(Ignorable),
}

/// A `<segment>` element containing source (and optionally target) content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub id: Option<String>,
    pub state: Option<State>,
    pub sub_state: Option<String>,
    pub source: Content,
    pub target: Option<Content>,
}

/// A `<ignorable>` element — non-translatable content preserved in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ignorable {
    pub id: Option<String>,
    pub source: Content,
    pub target: Option<Content>,
}

/// Translation state of a segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Initial,
    Translated,
    Reviewed,
    Final,
}

/// Inline content of a `<source>` or `<target>` element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub lang: Option<String>,
    pub elements: Vec<InlineElement>,
}

/// A `<note>` element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: Option<String>,
    pub category: Option<String>,
    pub priority: Option<u8>,
    pub applies_to: Option<AppliesTo>,
    pub content: String,
}

/// What a note applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppliesTo {
    Source,
    Target,
}

/// A `<skeleton>` element — either an external reference or inline content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    pub href: Option<String>,
    pub content: Option<String>,
}

/// The `<originalData>` element containing data entries for inline codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalData {
    pub entries: Vec<DataEntry>,
}

/// A `<data>` entry within `<originalData>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataEntry {
    pub id: String,
    pub content: String,
}
