use super::extensions::Extensions;

/// An inline content element within source/target content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineElement {
    Text(String),
    CData(String),
    /// Code point (`<cp hex="XXXX"/>`). Stores the hex string (e.g. `"0001"`).
    Cp(String),
    Ph(Ph),
    Pc(Pc),
    Sc(Sc),
    Ec(Ec),
    Mrk(Mrk),
    Sm(Sm),
    Em(Em),
}

/// Standalone code placeholder (`<ph>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ph {
    pub id: String,
    pub data_ref: Option<String>,
    pub equiv: Option<String>,
    pub disp: Option<String>,
    pub sub_type: Option<String>,
    pub extensions: Extensions,
}

/// Paired code container (`<pc>`). Contains inline content between open/close tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pc {
    pub id: String,
    pub data_ref_start: Option<String>,
    pub data_ref_end: Option<String>,
    pub sub_type: Option<String>,
    pub content: Vec<InlineElement>,
    pub extensions: Extensions,
}

/// Start of a spanning code (`<sc>`). Paired with a corresponding `<ec>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sc {
    pub id: String,
    pub data_ref: Option<String>,
    pub sub_type: Option<String>,
    pub can_copy: Option<bool>,
    pub can_delete: Option<bool>,
    pub can_overlap: Option<bool>,
    pub can_reorder: Option<CanReorder>,
    pub extensions: Extensions,
}

/// End of a spanning code (`<ec>`). References its `<sc>` via `start_ref`,
/// or stands alone with `isolated="yes"` and an `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ec {
    pub start_ref: Option<String>,
    pub id: Option<String>,
    pub isolated: Option<bool>,
    pub data_ref: Option<String>,
    pub sub_type: Option<String>,
    pub can_copy: Option<bool>,
    pub can_delete: Option<bool>,
    pub can_overlap: Option<bool>,
    pub can_reorder: Option<CanReorder>,
    pub extensions: Extensions,
}

/// Annotation marker (`<mrk>`). Wraps inline content with metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mrk {
    pub id: String,
    pub translate: Option<bool>,
    pub mrk_type: Option<String>,
    pub ref_: Option<String>,
    pub value: Option<String>,
    pub content: Vec<InlineElement>,
    pub extensions: Extensions,
}

/// Start of an annotation span (`<sm>`). Paired with `<em>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sm {
    pub id: String,
    pub translate: Option<bool>,
    pub sm_type: Option<String>,
    pub ref_: Option<String>,
    pub value: Option<String>,
    pub extensions: Extensions,
}

/// End of an annotation span (`<em>`). References its `<sm>` via `start_ref`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Em {
    pub start_ref: String,
}

/// Values for the `canReorder` attribute on spanning codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanReorder {
    Yes,
    No,
    FirstNo,
}
