//! Structural-transcript JSON — the studio-side mirror of the runtime's
//! `.brkt` content model (RULED 2026-08-30, `docs/decision-log.md`
//! "Studio saves carry the structural transcript and re-render it" +
//! "Studio-side saves and transcripts serialize as JSON, not binary").
//!
//! A save (or a hot reload) keeps the transcript as `OutputPart`s —
//! `LineRef`s + slots, never resolved text — and the studio re-renders it
//! against whatever program is CURRENT at read time. Editing a line and
//! reloading therefore re-renders the story-so-far with the updated text,
//! the exact property the binary `.brkt` format was built for; this module
//! is the same content model in human-readable JSON, because authoring-side
//! artifacts should be inspectable ("binary formats are for shipping
//! games").
//!
//! Transient in-memory markers (`Checkpoint`, `ElementAttach`/`-End`) are
//! not part of the persisted model — same rule as `.brkt`'s `is_persisted`
//! — and are skipped on export / absent from the schema on import.

use brink_format::{LineFlags, Value};
use brink_runtime::{Fragment, OutputPart};
use serde::{Deserialize, Serialize};

/// The JSON envelope a save slot (or a reload hand-off) stores.
#[derive(Serialize, Deserialize)]
pub(crate) struct TranscriptJson {
    /// Schema version — 1.
    pub version: u32,
    /// The exporting program's CRC-32 source checksum. Purely advisory on
    /// render: re-rendering against a DIFFERENT compile is the point of
    /// the format (live edit → reload), so a mismatch never refuses — the
    /// studio's own `OLD` chip is where drift surfaces to the author.
    pub checksum: u32,
    pub parts: Vec<PartJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragments: Vec<FragmentJson>,
}

/// One persisted output part. Tagged (`"part": "line"` etc.) so the stored
/// JSON reads as what it is.
#[derive(Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub(crate) enum PartJson {
    Text {
        text: String,
    },
    Line {
        container: u32,
        line: u16,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        slots: Vec<Value>,
        #[serde(default)]
        flags: u8,
    },
    Value {
        value: Value,
    },
    Newline,
    Spring,
    Glue,
    Tag {
        tag: String,
    },
}

#[derive(Serialize, Deserialize)]
pub(crate) struct FragmentJson {
    pub parts: Vec<PartJson>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Mirror a runtime part into the JSON model; `None` for the transient
/// variants that are never persisted (same set `.brkt` filters).
fn part_to_json(part: &OutputPart) -> Option<PartJson> {
    match part {
        OutputPart::Text(s) => Some(PartJson::Text { text: s.clone() }),
        OutputPart::LineRef {
            container_idx,
            line_idx,
            slots,
            flags,
        } => Some(PartJson::Line {
            container: *container_idx,
            line: *line_idx,
            slots: slots.clone(),
            flags: flags.bits(),
        }),
        OutputPart::ValueRef(v) => Some(PartJson::Value { value: v.clone() }),
        OutputPart::Newline => Some(PartJson::Newline),
        OutputPart::Spring => Some(PartJson::Spring),
        OutputPart::Glue => Some(PartJson::Glue),
        OutputPart::Tag(t) => Some(PartJson::Tag { tag: t.clone() }),
        OutputPart::Checkpoint | OutputPart::ElementAttach(..) | OutputPart::ElementAttachEnd => {
            None
        }
    }
}

fn part_from_json(part: PartJson) -> OutputPart {
    match part {
        PartJson::Text { text } => OutputPart::Text(text),
        PartJson::Line {
            container,
            line,
            slots,
            flags,
        } => OutputPart::LineRef {
            container_idx: container,
            line_idx: line,
            slots,
            flags: LineFlags::from_bits_truncate(flags),
        },
        PartJson::Value { value } => OutputPart::ValueRef(value),
        PartJson::Newline => OutputPart::Newline,
        PartJson::Spring => OutputPart::Spring,
        PartJson::Glue => OutputPart::Glue,
        PartJson::Tag { tag } => OutputPart::Tag(tag),
    }
}

/// Build the JSON envelope from a live story's transcript + fragments.
pub(crate) fn export_transcript_json(
    parts: &[OutputPart],
    fragments: &[Fragment],
    checksum: u32,
) -> TranscriptJson {
    TranscriptJson {
        version: 1,
        checksum,
        parts: parts.iter().filter_map(part_to_json).collect(),
        fragments: fragments
            .iter()
            .map(|f| FragmentJson {
                parts: f.parts.iter().filter_map(part_to_json).collect(),
                tags: f.tags.clone(),
            })
            .collect(),
    }
}

/// Decode the envelope back into runtime parts + fragments, dropping any
/// `LineRef` whose container index no longer exists in `program` — the
/// structural drift an edited-then-reloaded save can carry. The runtime's
/// resolvers already treat an out-of-range LINE index as empty text
/// (`resolve_line_ref`'s `.get`), but an out-of-range CONTAINER index
/// would panic in `scope_table_idx`, so it is filtered here. (A stale
/// in-range index re-renders best-effort — the same index-keyed contract
/// locale hot-swap lives with.)
pub(crate) fn decode_transcript_json(
    t: TranscriptJson,
    container_count: u32,
) -> (Vec<OutputPart>, Vec<Fragment>) {
    let keep = |p: &OutputPart| match p {
        OutputPart::LineRef { container_idx, .. } => *container_idx < container_count,
        _ => true,
    };
    let parts: Vec<OutputPart> = t
        .parts
        .into_iter()
        .map(part_from_json)
        .filter(keep)
        .collect();
    let fragments: Vec<Fragment> = t
        .fragments
        .into_iter()
        .map(|f| Fragment {
            parts: f
                .parts
                .into_iter()
                .map(part_from_json)
                .filter(keep)
                .collect(),
            tags: f.tags,
        })
        .collect();
    (parts, fragments)
}
