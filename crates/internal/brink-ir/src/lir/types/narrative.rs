//! The **narrative half** of the LIR (#397 waist): weave concepts —
//! diverts, tunnels, threads, choice sets, sequences, and content lines.
//! These stay ink-shaped; the logic half must not depend on them.

use brink_format::{DefinitionId, NameId};

use crate::SequenceType;

use super::{CallArg, Conditional, Expr, Stmt};

// ─── Control flow ────────────────────────────────────────────────────

// ─── Control flow ────────────────────────────────────────────────────

/// A divert — goto another container, DONE, or END.
#[derive(Clone)]
pub struct Divert {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A tunnel call — push return point, enter target.
/// Chained tunnels (`->-> a ->-> b`) produce multiple targets.
#[derive(Clone)]
pub struct TunnelCall {
    pub targets: Vec<TunnelTarget>,
}

/// A single target in a tunnel call chain.
#[derive(Clone)]
pub struct TunnelTarget {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A thread fork — `<- target`.
#[derive(Clone)]
pub struct ThreadStart {
    pub target: DivertTarget,
    pub args: Vec<CallArg>,
}

/// A resolved divert destination.
#[derive(Clone)]
pub enum DivertTarget {
    /// A named address.
    Address(DefinitionId),
    /// A global variable holding a divert target value — `-> x` where `x` is a global variable.
    Variable(DefinitionId),
    /// A temp/parameter variable holding a divert target value — `-> x` where `x` is a parameter.
    VariableTemp(u16, NameId),
    /// `-> DONE` — pause execution, can resume.
    Done,
    /// `-> END` — permanently end the story.
    End,
}

// ─── Choice sets ─────────────────────────────────────────────────────

// ─── Choice sets ─────────────────────────────────────────────────────

/// A set of choices presented to the player, with container boundaries
/// already decided.
#[derive(Clone)]
pub struct ChoiceSet {
    pub choices: Vec<Choice>,
    /// The gather container that loose-end choices implicitly divert to.
    /// `None` if all choices have explicit diverts.
    pub gather_target: Option<DefinitionId>,
}

/// A single choice within a choice set.
///
/// Content is stored as the original three-part split from the HIR:
/// - `start_content` = text before `[` — shared between display and output
/// - `choice_only_content` = text inside `[...]` — display only
/// - `inner_content` = text after `]` — output only
///
/// The choice body lives in a separate `Container` referenced by `target`.
#[derive(Clone)]
pub struct Choice {
    /// `+` (sticky) vs `*` (once-only).
    pub is_sticky: bool,
    /// Invisible default choice (fallback).
    pub is_fallback: bool,
    /// Condition expression — choice is only available when true.
    pub condition: Option<Expr>,
    /// Text before `[` — appears in both choice list and output.
    pub start_content: Option<Content>,
    /// Text inside `[...]` — appears only in the choice list.
    pub choice_only_content: Option<Content>,
    /// Text after `]` — appears only after selection.
    pub inner_content: Option<Content>,
    /// Recognized display text (start+bracket) for the line table.
    /// `Some` when pattern recognition succeeds on the composed display content.
    pub display_emission: Option<ContentEmission>,
    /// Recognized output text (start+inner) for the line table.
    /// `Some` when pattern recognition succeeds on the composed output content.
    pub output_emission: Option<ContentEmission>,
    /// The container holding the choice body (content after selection).
    pub target: DefinitionId,
    pub tags: Vec<Vec<ContentPart>>,
}

// ─── Sequences ───────────────────────────────────────────────────────

/// A block-level sequence (stopping, cycle, once, shuffle).
#[derive(Clone)]
pub struct Sequence {
    pub kind: SequenceType,
    pub branches: Vec<Vec<Stmt>>,
}

// ─── Recognized content (pattern recognizer output) ──────────────────

// ─── Recognized content (pattern recognizer output) ──────────────────

/// Metadata computed during recognition while HIR provenance is available.
#[derive(Clone)]
pub struct LineMetadata {
    pub source_hash: u64,
    pub slot_info: Vec<brink_format::SlotInfo>,
    pub source_location: Option<brink_format::SourceLocation>,
}

/// A recognized line pattern from content analysis.
#[derive(Clone)]
pub enum RecognizedLine {
    Plain(String),
    Template {
        parts: Vec<brink_format::LinePart>,
        slot_exprs: Vec<Expr>,
    },
}

/// Result of pattern recognition on a content line.
#[derive(Clone)]
pub struct ContentEmission {
    pub line: RecognizedLine,
    pub metadata: LineMetadata,
    pub tags: Vec<Vec<ContentPart>>,
}

// ─── Content and inline elements ─────────────────────────────────────

// ─── Content and inline elements ─────────────────────────────────────

/// A line of text output with inline elements and tags.
///
/// Each `Content` maps to one line table entry in the bytecode output.
/// Backends decide the entry format: plain text for content with no
/// dynamic parts, or a template with slots for interpolated content.
#[derive(Clone)]
pub struct Content {
    pub parts: Vec<ContentPart>,
    pub tags: Vec<Vec<ContentPart>>,
}

/// A fragment within a content line.
#[derive(Clone)]
pub enum ContentPart {
    /// Literal text.
    Text(String),
    /// `<>` — glue (suppresses line break).
    Glue,
    /// Word-break spring — conditional space resolved by the runtime.
    Spring,
    /// `{expr}` — interpolated expression, resolved.
    Interpolation(Expr),
    /// `{cond: a | b}` — inline conditional with resolved conditions.
    InlineConditional(Conditional),
    /// `{&a|b|c}` — inline sequence.
    InlineSequence(Sequence),
    /// Enter a child sequence container (inline sequence wrapper).
    EnterSequence(brink_format::DefinitionId),
}
