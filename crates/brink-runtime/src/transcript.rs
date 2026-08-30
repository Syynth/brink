//! Transcript binary serialization (`.brkt` format).
//!
//! A transcript is a serialized `Vec<OutputPart>` — the append-only log of
//! all output parts produced during story execution. Combined with an `.inkb`
//! program and optional `.inkl` locale data, a transcript can be re-rendered
//! in any language without re-executing the story.
//!
//! ## Binary format
//!
//! ```text
//! Header (16 bytes):
//!   b"BRKT"           magic (4)
//!   u16 LE            version = 1 (2)
//!   u16 LE            reserved (2)
//!   u32 LE            source_checksum (4)
//!   u32 LE            content CRC-32 (4)
//!
//! Body:
//!   u32 LE            top-level part count
//!   [Part]*           encoded top-level parts
//!
//!   u32 LE            fragment count
//!   ( u32 LE          this fragment's part count
//!     [Part]*          encoded fragment parts
//!   )*
//!
//!   ( u32 LE          this fragment's tag count      -- #953, trailing section
//!     [str]*           tags, in fragment order        (see below)
//!   )*
//! ```
//!
//! Both "part count" fields above count only **persisted** parts —
//! `OutputPart::Checkpoint` is a transient capture marker that is filtered
//! out before encoding (see [`is_persisted`]) and contributes zero bytes to
//! `[Part]*`, so the count must exclude it too or a reader following this
//! doc to extend the format would write `parts.len()` and produce a byte
//! stream whose declared count disagrees with what it actually encoded.
//! `OutputPart::ElementAttach`/`ElementAttachEnd` (issue #2108) are the same
//! kind of transient, zero-byte marker for the identical reason —
//! deliberately in-memory-only; see that variant's own doc.
//!
//! The fragment section and the trailing fragment-tags section are both
//! **backward-compat optional**: `read_transcript` treats "no bytes left"
//! at either boundary as "this section is absent", not as truncated input,
//! and falls back to an empty `Vec` (zero fragments, or every fragment's
//! `tags: Vec::new()`) rather than erroring. This lets a `.brkt` written
//! before a section existed keep decoding under a newer reader.
//!
//! The fragment-tags section is written as a distinct trailing section
//! *after* every fragment's parts — one `(tag count, [str]*)` block per
//! fragment, in the same order the fragments themselves were written —
//! rather than inlined into each fragment's own record. An inline layout
//! could not tell "this fragment has a tags section" apart from "the next
//! fragment's part bytes happen to start here" once a `.brkt` written
//! before tags existed was read by tags-aware code; the trailing-section
//! layout sidesteps that ambiguity by using the same "any bytes left?"
//! probe already used for the fragment section itself. Fixes #953:
//! `Fragment::tags` was silently dropped by this codec. See
//! `write_transcript`/`read_transcript` below for the code-level version of
//! this note.
//!
//! A third trailing section (as the `.inkb` v6 bump is expected to add) is
//! **not yet safe** to bolt on the same way: see
//! `docs/brkt-trailing-section-findings.md` for a traced report of exactly
//! what breaks and why, written as input to #1519's design pass.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use brink_format::{DefinitionId, LineFlags, MAX_DECODE_DEPTH, MapKey, NameId, OrderedMap, Value};

use crate::output::{OutputPart, resolve_lines};
use crate::program::Program;

// ── Format constants ──────────────────────────────────────────────────────

const MAGIC: &[u8; 4] = b"BRKT";
const VERSION: u16 = 1;
const HEADER_SIZE: usize = 16;

// Part tags
const TAG_TEXT: u8 = 0x01;
const TAG_LINE_REF: u8 = 0x02;
const TAG_VALUE_REF: u8 = 0x03;
const TAG_NEWLINE: u8 = 0x04;
const TAG_SPRING: u8 = 0x05;
const TAG_GLUE: u8 = 0x06;
const TAG_TAG: u8 = 0x07;

// Value tags (matching inkb encoding)
const VAL_INT: u8 = 0x00;
const VAL_FLOAT: u8 = 0x01;
const VAL_BOOL: u8 = 0x02;
const VAL_STRING: u8 = 0x03;
const VAL_LIST: u8 = 0x04;
const VAL_DIVERT_TARGET: u8 = 0x05;
const VAL_NULL: u8 = 0x06;
// Shared numeric surface with the `.inkb` format (`inkb::VAL_VAR_POINTER`). A
// `VariablePointer` is preserved distinctly (T1c, #700) so a `ref`-bound
// closure env entry round-trips losslessly — the old code collapsed it to
// `VAL_DIVERT_TARGET`, which is fine for a bare pointer (display-identical) but
// would corrupt a captured `ref` cell inside a persisted function value.
const VAL_VAR_POINTER: u8 = 0x07;
const VAL_FRAGMENT_REF: u8 = 0x08;
// v4 collection tags — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1). Reachable today via `Opcode::EmitValue`, which
// pushes any popped stack value (including a binding/external return that since
// #525 can be a collection) into `OutputPart::ValueRef`.
const VAL_ARRAY: u8 = 0x09;
const VAL_MAP: u8 = 0x0A;
// T1c function-value tags — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1). A function value can ride the append-only
// transcript / journal / speculation snapshots as an ordinary value (spec §6:
// "function values save like every other value"), e.g. via `Opcode::EmitValue`
// or a saved binding.
const VAL_FN_REF: u8 = 0x0B;
const VAL_CLOSURE: u8 = 0x0C;
// T1d handle tag — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1: `kind NameId, u64 id`). A handle received
// from a binding rides the append-only transcript / journal / speculation
// snapshots as an ordinary value (`docs/t1d-spec.md` §2: "handles appear in
// saves, journals, and speculation snapshots as ordinary values"), e.g. via
// `Opcode::EmitValue` or a saved binding.
const VAL_HANDLE: u8 = 0x0D;
// TM-4 record tag — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1: `ShapeId`, then field values in shape order).
const VAL_RECORD: u8 = 0x0F;
// T1e projection tag — shared numeric surface with the `.inkb` format
// (`docs/format-v4-rfc.md` §1: cell reference, u8 segment count, segments).
// A projection rides the append-only transcript / journal / speculation
// snapshots as an ordinary value (`docs/t1e-spec.md` §3: "Saves/journal/
// speculation: ordinary values"), e.g. via `Opcode::EmitValue`.
const VAL_PROJECTION: u8 = 0x0E;
/// NS-A1 Option value tag (`docs/stdlib-spec.md` §1.4): flag byte (0 =
/// `none`, 1 = `some`), then the inner value when `some` — mirrors the
/// `.inkb` `VAL_OPTION` wire form exactly, like every tag above.
const VAL_OPTION: u8 = 0x10;
/// NS-A5 range value tag (`docs/stdlib-spec.md` §7, F7): start i32, end
/// i32, one flag byte (0 = `..` exclusive, 1 = `..=` inclusive) — mirrors
/// the `.inkb` `VAL_RANGE` wire form exactly. Flat: never recurses, no
/// depth accounting (a range holds two ints, never another value).
const VAL_RANGE: u8 = 0x11;
// NS-A8 tower value tags (`docs/tower-mini-spec.md` T5): explicit
// little-endian f32 lanes — vec/quat `x, y(, z, w)`, matrices column-major
// column-by-column — mirroring the `.inkb` wire form exactly, like every
// tag above. NEVER glam's memory layout: lanes go through glam's explicit
// `to_array`/`from_array`/`to_cols_array`/`from_cols_array` conversions.
const VAL_VEC2: u8 = 0x12;
const VAL_VEC3: u8 = 0x13;
const VAL_VEC4: u8 = 0x14;
const VAL_QUAT: u8 = 0x15;
const VAL_MAT2: u8 = 0x16;
const VAL_MAT3: u8 = 0x17;
const VAL_MAT4: u8 = 0x18;
/// NS-A7 weighted tables (`docs/stdlib-spec.md` §8): mirrors the `.inkb`
/// `VAL_WEIGHTED` wire form exactly — u32 entry count, then per entry an
/// i32 weight and a recursively-encoded value. Depth-counted like the
/// collection tags; the reader enforces the evidence-by-construction
/// invariant (non-empty, weights ≥ 1).
const VAL_WEIGHTED: u8 = 0x19;
const PROJ_SEG_INDEX: u8 = 0x00;
const PROJ_SEG_KEY: u8 = 0x01;

// ── Error type ────────────────────────────────────────────────────────────

/// Errors from transcript serialization/deserialization.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("invalid magic: expected BRKT")]
    InvalidMagic,
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u16),
    #[error("checksum mismatch: transcript {transcript:#010x} != program {program:#010x}")]
    ChecksumMismatch { transcript: u32, program: u32 },
    #[error("integrity check failed: content CRC-32 mismatch")]
    IntegrityCheckFailed,
    #[error("unexpected end of data")]
    UnexpectedEof,
    #[error("invalid part tag: {0:#04x}")]
    InvalidPartTag(u8),
    #[error("invalid value tag: {0:#04x}")]
    InvalidValueTag(u8),
    #[error("invalid UTF-8")]
    InvalidUtf8,
    #[error("invalid definition ID")]
    InvalidDefinitionId,
    #[error("value nesting exceeded max decode depth ({0})")]
    MaxDepthExceeded(usize),
    /// A `VAL_MAP` entry list carried the same key twice. A legitimate
    /// writer never emits this — `OrderedMap::insert` de-duplicates on the
    /// write side — so a repeated key is a corrupt or crafted `.brkt`; the
    /// content-based `OrderedMap` `Eq` (issue #909) assumes each key appears
    /// once, so this is rejected rather than silently keeping the last
    /// occurrence (issue #985).
    #[error("duplicate key in map value")]
    DuplicateMapKey,
}

// ── Write ─────────────────────────────────────────────────────────────────

/// Serialize a transcript to the `.brkt` binary format.
///
/// Checkpoint parts are filtered out (they are transient capture markers
/// that should never appear in a persisted transcript).
#[expect(clippy::cast_possible_truncation)]
pub fn write_transcript(
    parts: &[OutputPart],
    source_checksum: u32,
    fragments: &[crate::output::Fragment],
) -> Vec<u8> {
    let mut body = Vec::new();

    // Count non-Checkpoint parts
    let count = parts.iter().filter(|p| is_persisted(p)).count() as u32;
    write_u32(&mut body, count);

    for part in parts {
        encode_part(part, &mut body);
    }

    // Serialize fragments
    write_u32(&mut body, fragments.len() as u32);
    for fragment in fragments {
        let filtered_count = fragment.parts.iter().filter(|p| is_persisted(p)).count() as u32;
        write_u32(&mut body, filtered_count);
        for part in &fragment.parts {
            encode_part(part, &mut body);
        }
    }

    // Serialize fragment tags, appended as a trailing section *after* every
    // fragment's parts (rather than inline per-fragment) so that a `.brkt`
    // file written before this section existed remains readable: the reader
    // detects the section's absence via a plain "any bytes left?" check (the
    // same backward-compat idiom already used for the fragment section
    // itself, above) and falls back to empty tags, instead of misreading a
    // later fragment's part bytes as an earlier fragment's tag bytes (which
    // an inline per-fragment layout could not distinguish after the fact).
    // Fixes #953: `Fragment::tags` was silently dropped by this codec.
    for fragment in fragments {
        write_u32(&mut body, fragment.tags.len() as u32);
        for tag in &fragment.tags {
            write_str(&mut body, tag);
        }
    }

    // Build header
    let content_crc = crc32(&body);
    let mut buf = Vec::with_capacity(HEADER_SIZE + body.len());
    buf.extend_from_slice(MAGIC);
    write_u16(&mut buf, VERSION);
    write_u16(&mut buf, 0); // reserved
    write_u32(&mut buf, source_checksum);
    write_u32(&mut buf, content_crc);
    buf.extend(body);
    buf
}

// ── Read ──────────────────────────────────────────────────────────────────

/// A decoded transcript: the output parts, the source program's checksum
/// (to verify compatibility before rendering), and the captured fragments
/// (for re-rendering choice display text and computed substrings).
///
/// The caller should validate `source_checksum` against the program's
/// checksum (via [`Program::source_checksum`](crate::Program::source_checksum))
/// before passing `parts` to [`render_transcript`].
#[derive(Debug, Clone)]
pub struct TranscriptData {
    pub parts: Vec<OutputPart>,
    pub source_checksum: u32,
    pub fragments: Vec<crate::output::Fragment>,
}

/// Deserialize a transcript from the `.brkt` binary format.
pub fn read_transcript(bytes: &[u8]) -> Result<TranscriptData, TranscriptError> {
    if bytes.len() < HEADER_SIZE {
        return Err(TranscriptError::UnexpectedEof);
    }

    // Validate header
    if &bytes[0..4] != MAGIC {
        return Err(TranscriptError::InvalidMagic);
    }
    let mut off = 4;
    let version = read_u16(bytes, &mut off)?;
    if version != VERSION {
        return Err(TranscriptError::UnsupportedVersion(version));
    }
    let _reserved = read_u16(bytes, &mut off)?;
    let source_checksum = read_u32(bytes, &mut off)?;
    let expected_crc = read_u32(bytes, &mut off)?;

    // Validate body integrity
    let body = &bytes[HEADER_SIZE..];
    if crc32(body) != expected_crc {
        return Err(TranscriptError::IntegrityCheckFailed);
    }

    // Decode parts
    let mut off = HEADER_SIZE;
    let count = read_u32(bytes, &mut off)? as usize;
    let mut parts = Vec::with_capacity(count);

    for _ in 0..count {
        parts.push(decode_part(bytes, &mut off)?);
    }

    // Deserialize fragments
    let fragment_count = if off < bytes.len() {
        read_u32(bytes, &mut off)? as usize
    } else {
        0 // backward compat: old transcripts without fragments
    };
    let mut fragments = Vec::with_capacity(fragment_count);
    for _ in 0..fragment_count {
        let frag_part_count = read_u32(bytes, &mut off)? as usize;
        let mut frag_parts = Vec::with_capacity(frag_part_count);
        for _ in 0..frag_part_count {
            frag_parts.push(decode_part(bytes, &mut off)?);
        }
        fragments.push(crate::output::Fragment {
            parts: frag_parts,
            tags: Vec::new(),
        });
    }

    // Fragment tags (fixes #953): a trailing section written after every
    // fragment's parts (see `write_transcript`'s matching comment). Older
    // transcripts written before this section existed end exactly at the
    // fragment section, so `off == bytes.len()` there and every fragment
    // keeps the empty `tags` it was constructed with above — the same
    // observable (if buggy) behavior those files always had, preserved for
    // backward compatibility rather than erroring on legacy saves.
    if off < bytes.len() {
        for fragment in &mut fragments {
            let tag_count = read_u32(bytes, &mut off)? as usize;
            let mut tags = Vec::with_capacity(tag_count.min(bytes.len().saturating_sub(off)));
            for _ in 0..tag_count {
                tags.push(read_str(bytes, &mut off)?);
            }
            fragment.tags = tags;
        }
    }

    Ok(TranscriptData {
        parts,
        source_checksum,
        fragments,
    })
}

// ── Render ────────────────────────────────────────────────────────────────

/// Re-render a transcript against the given line tables.
///
/// Applies glue resolution, Spring spacing, and line trimming — the same
/// pipeline as `flush_lines` — producing `(text, tags)` tuples per line.
pub fn render_transcript(
    parts: &[OutputPart],
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
    fragments: &[crate::output::Fragment],
) -> Vec<(String, Vec<String>)> {
    // Element-attachment data (issue #2108) is dropped here — moot in
    // practice, since `OutputPart::ElementAttach`/`ElementAttachEnd` are not
    // persisted (`is_persisted`, below), so a `.brkt`-sourced `parts` slice
    // never contains any to begin with. This function's public contract
    // (`(text, tags)`) stays unchanged either way.
    resolve_lines(parts, program, line_tables, resolver, fragments)
        .into_iter()
        .map(|(text, tags, _element, _source)| (text, tags))
        .collect()
}

/// Like [`render_transcript`], but keeps each line's provenance — the first
/// `LineRef`'s line-table `source_location` (the same rule the live
/// delivery stream uses, W7/#3300). The studio's re-render road (RULED
/// 2026-08-30, "Studio saves carry the structural transcript") needs the
/// provenance chips to survive a restore, not just the text.
pub fn render_transcript_with_source(
    parts: &[OutputPart],
    program: &Program,
    line_tables: &[Vec<brink_format::LineEntry>],
    resolver: Option<&dyn brink_format::PluralResolver>,
    fragments: &[crate::output::Fragment],
) -> Vec<(String, Vec<String>, Option<brink_format::SourceLocation>)> {
    resolve_lines(parts, program, line_tables, resolver, fragments)
        .into_iter()
        .map(|(text, tags, _element, source)| (text, tags, source))
        .collect()
}

// ── Part codec ────────────────────────────────────────────────────────────
//
// One shared encode/decode pair for `OutputPart`, used by both the
// top-level part list and each fragment's part list in `write_transcript`/
// `read_transcript`. Before this, the two call sites hand-duplicated the
// same match arms; #953 was exactly that duplication silently dropping
// `Fragment::tags` because the two loops drifted. Any new `OutputPart`
// variant that always writes bytes needs exactly one new arm here, reached
// from both loops. A new *transient* (zero-byte) variant — like
// `OutputPart::Checkpoint` — additionally needs its own arm added to
// `is_persisted` below, which both part-count filters share; see that
// function's doc comment.

/// Returns whether `part` is written to the persisted `.brkt` format.
///
/// `OutputPart::Checkpoint` is the one transient capture marker that is
/// filtered out (it writes zero bytes in [`encode_part`]). This predicate is
/// shared by both of `write_transcript`'s part-count computations (the
/// top-level count and each fragment's `filtered_count`) so they cannot
/// drift from each other or from `encode_part`'s zero-byte arm. Any future
/// transient variant must be added here *and* to `encode_part`'s zero-byte
/// arm in lockstep — otherwise the written count disagrees with the emitted
/// bytes and `read_transcript` misreads the part list.
fn is_persisted(part: &OutputPart) -> bool {
    !matches!(
        part,
        OutputPart::Checkpoint | OutputPart::ElementAttach(..) | OutputPart::ElementAttachEnd
    )
}

/// Encode a single [`OutputPart`] (its tag byte plus payload) onto `buf`.
/// `OutputPart::Checkpoint` writes nothing — it is a transient capture
/// marker filtered out of the persisted `.brkt` format by the caller's part
/// count (see `write_transcript` and [`is_persisted`]).
#[expect(clippy::cast_possible_truncation)]
fn encode_part(part: &OutputPart, buf: &mut Vec<u8>) {
    match part {
        OutputPart::Text(s) => {
            write_u8(buf, TAG_TEXT);
            write_str(buf, s);
        }
        OutputPart::LineRef {
            container_idx,
            line_idx,
            slots,
            flags,
        } => {
            write_u8(buf, TAG_LINE_REF);
            write_u32(buf, *container_idx);
            write_u16(buf, *line_idx);
            write_u8(buf, flags.bits());
            write_u16(buf, slots.len() as u16);
            for val in slots {
                encode_value(val, buf);
            }
        }
        OutputPart::ValueRef(val) => {
            write_u8(buf, TAG_VALUE_REF);
            encode_value(val, buf);
        }
        OutputPart::Newline => write_u8(buf, TAG_NEWLINE),
        OutputPart::Spring => write_u8(buf, TAG_SPRING),
        OutputPart::Glue => write_u8(buf, TAG_GLUE),
        OutputPart::Tag(s) => {
            write_u8(buf, TAG_TAG);
            write_str(buf, s);
        }
        // `Checkpoint` is filtered out (transient capture marker).
        // `ElementAttach`/`ElementAttachEnd` (issue #2108) are the same kind
        // of transient, in-memory-only marker — see `is_persisted`'s doc
        // and `OutputPart::ElementAttach`'s own doc for why they never
        // reach the `.brkt` wire format either.
        OutputPart::Checkpoint | OutputPart::ElementAttach(..) | OutputPart::ElementAttachEnd => {}
    }
}

/// Decode a single [`OutputPart`] (its tag byte plus payload) from `bytes`
/// at `*off`, advancing `*off` past it. The counterpart of [`encode_part`].
fn decode_part(bytes: &[u8], off: &mut usize) -> Result<OutputPart, TranscriptError> {
    let tag = read_u8(bytes, off)?;
    let part = match tag {
        TAG_TEXT => OutputPart::Text(read_str(bytes, off)?),
        TAG_LINE_REF => {
            let container_idx = read_u32(bytes, off)?;
            let line_idx = read_u16(bytes, off)?;
            let flags_bits = read_u8(bytes, off)?;
            let flags = LineFlags::from_bits_truncate(flags_bits);
            let slot_count = read_u16(bytes, off)? as usize;
            let mut slots = Vec::with_capacity(slot_count);
            for _ in 0..slot_count {
                slots.push(decode_value(bytes, off, 0)?);
            }
            OutputPart::LineRef {
                container_idx,
                line_idx,
                slots,
                flags,
            }
        }
        TAG_VALUE_REF => OutputPart::ValueRef(decode_value(bytes, off, 0)?),
        TAG_NEWLINE => OutputPart::Newline,
        TAG_SPRING => OutputPart::Spring,
        TAG_GLUE => OutputPart::Glue,
        TAG_TAG => OutputPart::Tag(read_str(bytes, off)?),
        _ => return Err(TranscriptError::InvalidPartTag(tag)),
    };
    Ok(part)
}

// ── Codec helpers (self-contained, no dependency on brink-format internals) ──

fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

fn write_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

#[expect(clippy::cast_possible_truncation)]
fn write_str(buf: &mut Vec<u8>, s: &str) {
    write_u32(buf, s.len() as u32);
    buf.extend_from_slice(s.as_bytes());
}

fn write_def_id(buf: &mut Vec<u8>, id: DefinitionId) {
    write_u64(buf, id.to_raw());
}

fn read_u8(buf: &[u8], off: &mut usize) -> Result<u8, TranscriptError> {
    if *off >= buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = buf[*off];
    *off += 1;
    Ok(v)
}

fn read_u16(buf: &[u8], off: &mut usize) -> Result<u16, TranscriptError> {
    if *off + 2 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = u16::from_le_bytes([buf[*off], buf[*off + 1]]);
    *off += 2;
    Ok(v)
}

fn read_u32(buf: &[u8], off: &mut usize) -> Result<u32, TranscriptError> {
    if *off + 4 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = u32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
    *off += 4;
    Ok(v)
}

fn read_i32(buf: &[u8], off: &mut usize) -> Result<i32, TranscriptError> {
    if *off + 4 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = i32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
    *off += 4;
    Ok(v)
}

fn read_f32(buf: &[u8], off: &mut usize) -> Result<f32, TranscriptError> {
    if *off + 4 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = f32::from_le_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]);
    *off += 4;
    Ok(v)
}

fn read_u64(buf: &[u8], off: &mut usize) -> Result<u64, TranscriptError> {
    if *off + 8 > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let v = u64::from_le_bytes([
        buf[*off],
        buf[*off + 1],
        buf[*off + 2],
        buf[*off + 3],
        buf[*off + 4],
        buf[*off + 5],
        buf[*off + 6],
        buf[*off + 7],
    ]);
    *off += 8;
    Ok(v)
}

fn read_str(buf: &[u8], off: &mut usize) -> Result<String, TranscriptError> {
    let len = read_u32(buf, off)? as usize;
    if *off + len > buf.len() {
        return Err(TranscriptError::UnexpectedEof);
    }
    let bytes = &buf[*off..*off + len];
    *off += len;
    String::from_utf8(bytes.to_vec()).map_err(|_| TranscriptError::InvalidUtf8)
}

fn read_def_id(buf: &[u8], off: &mut usize) -> Result<DefinitionId, TranscriptError> {
    let raw = read_u64(buf, off)?;
    DefinitionId::from_raw(raw).ok_or(TranscriptError::InvalidDefinitionId)
}

// ── Value encoding ────────────────────────────────────────────────────────

#[expect(clippy::cast_possible_truncation)]
#[expect(
    clippy::too_many_lines,
    reason = "one match arm per value tag — T1e's VAL_PROJECTION arm pushed this past 100"
)]
fn encode_value(v: &Value, buf: &mut Vec<u8>) {
    match v {
        Value::Int(n) => {
            write_u8(buf, VAL_INT);
            write_i32(buf, *n);
        }
        Value::Float(n) => {
            write_u8(buf, VAL_FLOAT);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        Value::Bool(b) => {
            write_u8(buf, VAL_BOOL);
            write_u8(buf, u8::from(*b));
        }
        Value::String(s) => {
            write_u8(buf, VAL_STRING);
            write_str(buf, s);
        }
        Value::List(lv) => {
            write_u8(buf, VAL_LIST);
            write_u32(buf, lv.items.len() as u32);
            for item in &lv.items {
                write_def_id(buf, *item);
            }
            write_u32(buf, lv.origins.len() as u32);
            for origin in &lv.origins {
                write_def_id(buf, *origin);
            }
        }
        Value::DivertTarget(id) => {
            write_u8(buf, VAL_DIVERT_TARGET);
            write_def_id(buf, *id);
        }
        Value::VariablePointer(id) => {
            write_u8(buf, VAL_VAR_POINTER);
            write_def_id(buf, *id);
        }
        Value::FragmentRef(idx) => {
            write_u8(buf, VAL_FRAGMENT_REF);
            write_u32(buf, *idx);
        }
        // TempPointer is runtime-only.
        Value::TempPointer { .. } | Value::Null => {
            write_u8(buf, VAL_NULL);
        }
        // Collections encode as trees (v4, `docs/format-v4-rfc.md` §1): a length
        // prefix then the recursively-encoded elements / key-value pairs. Arc
        // sharing is not preserved on the wire (value-model-spec §5).
        Value::Array(items) => {
            write_u8(buf, VAL_ARRAY);
            write_u32(buf, items.len() as u32);
            for item in items.iter() {
                encode_value(item, buf);
            }
        }
        Value::Map(map) => {
            write_u8(buf, VAL_MAP);
            write_u32(buf, map.len() as u32);
            for (key, val) in map.iter() {
                encode_map_key(key, buf);
                encode_value(val, buf);
            }
        }
        Value::Record { shape, fields } => {
            write_u8(buf, VAL_RECORD);
            write_u32(buf, shape.0);
            write_u32(buf, fields.len() as u32);
            for field in fields.iter() {
                encode_value(field, buf);
            }
        }
        // Function values (T1c, spec §6): save like every other value. `FnRef`
        // is the fn token; `Closure` adds a u32-counted env of `{NameId, kind
        // u8, value}` entries — the named/moded env the rehydration check reads.
        Value::FnRef(target) => {
            write_u8(buf, VAL_FN_REF);
            write_def_id(buf, *target);
        }
        Value::Closure(c) => {
            write_u8(buf, VAL_CLOSURE);
            write_def_id(buf, c.target);
            write_u32(buf, c.env.len() as u32);
            for entry in &c.env {
                write_u16(buf, entry.name.0);
                write_u8(buf, u8::from(entry.is_ref));
                encode_value(&entry.payload, buf);
            }
        }
        // Handle values (T1d, spec §5: "the journal records returned tokens";
        // §2: "handles appear in saves, journals, and speculation snapshots
        // as ordinary values"). Token equality holds at this level; rebinding
        // to a live resource happens at the host boundary, not here.
        Value::Handle { kind, id } => {
            write_u8(buf, VAL_HANDLE);
            write_u16(buf, kind.0);
            write_u64(buf, *id);
        }
        // Projection values (T1e, spec §3: "Saves/journal/speculation:
        // ordinary values"). Segment kind `2=range` is RESERVED and never
        // written — `ProjSegment` has no variant to produce it.
        Value::Projection(p) => {
            write_u8(buf, VAL_PROJECTION);
            write_def_id(buf, p.cell);
            write_u8(buf, p.segments.len() as u8);
            for seg in &p.segments {
                match seg {
                    brink_format::ProjSegment::Index(n) => {
                        write_u8(buf, PROJ_SEG_INDEX);
                        write_i32(buf, *n);
                    }
                    brink_format::ProjSegment::Key(v) => {
                        write_u8(buf, PROJ_SEG_KEY);
                        encode_value(v, buf);
                    }
                }
            }
        }
        // Option values (NS-A1, `docs/stdlib-spec.md` §1.4): an Option in a
        // global/frame slot journals as an ordinary value, same as every
        // variant above.
        Value::OptionVal(inner) => {
            write_u8(buf, VAL_OPTION);
            match inner {
                None => write_u8(buf, 0),
                Some(v) => {
                    write_u8(buf, 1);
                    encode_value(v, buf);
                }
            }
        }
        // Range values (NS-A5, F7): a range in a global/frame slot journals
        // as an ordinary value — this is exactly the FlowFrame iterator-
        // spill durability the F7 ruling demanded (`for i in 0..n` across
        // an `await` parks its snapshot range in the frame record). The
        // written form is preserved.
        Value::Range {
            start,
            end,
            inclusive,
        } => {
            write_u8(buf, VAL_RANGE);
            write_i32(buf, *start);
            write_i32(buf, *end);
            write_u8(buf, u8::from(*inclusive));
        }
        // Tower values (NS-A8, `docs/tower-mini-spec.md` T5): explicit
        // little-endian f32 lanes in the pinned order, via glam's explicit
        // array conversions — mirrors the `.inkb` wire form exactly.
        Value::Vec2(v) => {
            write_u8(buf, VAL_VEC2);
            write_f32_lanes(buf, &v.to_array());
        }
        Value::Vec3(v) => {
            write_u8(buf, VAL_VEC3);
            write_f32_lanes(buf, &v.to_array());
        }
        Value::Vec4(v) => {
            write_u8(buf, VAL_VEC4);
            write_f32_lanes(buf, &v.to_array());
        }
        Value::Quat(q) => {
            write_u8(buf, VAL_QUAT);
            write_f32_lanes(buf, &q.to_array());
        }
        Value::Mat2(m) => {
            write_u8(buf, VAL_MAT2);
            write_f32_lanes(buf, &m.to_cols_array());
        }
        Value::Mat3(m) => {
            write_u8(buf, VAL_MAT3);
            write_f32_lanes(buf, &m.to_cols_array());
        }
        Value::Mat4(m) => {
            write_u8(buf, VAL_MAT4);
            write_f32_lanes(buf, &m.to_cols_array());
        }
        Value::Weighted(w) => {
            write_u8(buf, VAL_WEIGHTED);
            write_u32(buf, w.entries.len() as u32);
            for (weight, value) in &w.entries {
                write_i32(buf, *weight);
                encode_value(value, buf);
            }
        }
    }
}

/// NS-A8 (`docs/tower-mini-spec.md` T5): write tower lanes as explicit
/// little-endian f32s, one by one — the hand-serialized tower wire form
/// (same helper shape as the `.inkb` writer's).
fn write_f32_lanes(buf: &mut Vec<u8>, lanes: &[f32]) {
    for lane in lanes {
        buf.extend_from_slice(&lane.to_le_bytes());
    }
}

/// NS-A8 (`docs/tower-mini-spec.md` T5): read `N` explicit little-endian
/// f32 lanes; the caller rebuilds the glam value through its explicit
/// `from_array`/`from_cols_array` constructor.
fn read_f32_lanes<const N: usize>(
    buf: &[u8],
    off: &mut usize,
) -> Result<[f32; N], TranscriptError> {
    let mut lanes = [0.0f32; N];
    for lane in &mut lanes {
        *lane = read_f32(buf, off)?;
    }
    Ok(lanes)
}

/// Encode a [`MapKey`] using the scalar `VAL_*` tag surface (`int`/`string`/
/// `bool` — the v1 key domain). Self-describing so the reader rejects a
/// non-scalar key tag.
fn encode_map_key(key: &MapKey, buf: &mut Vec<u8>) {
    match key {
        MapKey::Int(n) => {
            write_u8(buf, VAL_INT);
            write_i32(buf, *n);
        }
        MapKey::Str(s) => {
            write_u8(buf, VAL_STRING);
            write_str(buf, s);
        }
        MapKey::Bool(b) => {
            write_u8(buf, VAL_BOOL);
            write_u8(buf, u8::from(*b));
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one match arm per value tag — T1e's VAL_PROJECTION arm pushed this past 100"
)]
fn decode_value(buf: &[u8], off: &mut usize, depth: usize) -> Result<Value, TranscriptError> {
    if depth > MAX_DECODE_DEPTH {
        return Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH));
    }
    let tag = read_u8(buf, off)?;
    match tag {
        VAL_INT => Ok(Value::Int(read_i32(buf, off)?)),
        VAL_FLOAT => Ok(Value::Float(read_f32(buf, off)?)),
        VAL_BOOL => {
            let b = read_u8(buf, off)?;
            Ok(Value::Bool(b != 0))
        }
        VAL_STRING => {
            let s = read_str(buf, off)?;
            Ok(Value::String(Arc::from(s.as_str())))
        }
        VAL_LIST => {
            let item_count = read_u32(buf, off)? as usize;
            let mut items = Vec::with_capacity(item_count);
            for _ in 0..item_count {
                items.push(read_def_id(buf, off)?);
            }
            let origin_count = read_u32(buf, off)? as usize;
            let mut origins = Vec::with_capacity(origin_count);
            for _ in 0..origin_count {
                origins.push(read_def_id(buf, off)?);
            }
            Ok(Value::List(Arc::new(brink_format::ListValue {
                items,
                origins,
            })))
        }
        VAL_DIVERT_TARGET => {
            let id = read_def_id(buf, off)?;
            Ok(Value::DivertTarget(id))
        }
        VAL_VAR_POINTER => {
            let id = read_def_id(buf, off)?;
            Ok(Value::VariablePointer(id))
        }
        VAL_FRAGMENT_REF => Ok(Value::FragmentRef(read_u32(buf, off)?)),
        VAL_NULL => Ok(Value::Null),
        VAL_ARRAY => {
            let len = read_u32(buf, off)? as usize;
            let mut items = Vec::with_capacity(len.min(buf.len().saturating_sub(*off)));
            for _ in 0..len {
                items.push(decode_value(buf, off, depth + 1)?);
            }
            Ok(Value::array(items))
        }
        VAL_MAP => {
            let len = read_u32(buf, off)? as usize;
            let mut map = OrderedMap::with_capacity(len.min(buf.len().saturating_sub(*off)));
            for _ in 0..len {
                let key = decode_map_key(buf, off)?;
                let val = decode_value(buf, off, depth + 1)?;
                // A repeated key would violate the content-based `OrderedMap`
                // `Eq` (#909); reject rather than silently keeping the last
                // occurrence (#985).
                if map.contains_key(&key) {
                    return Err(TranscriptError::DuplicateMapKey);
                }
                map.insert(key, val);
            }
            Ok(Value::map(map))
        }
        VAL_RECORD => {
            let shape = brink_format::ShapeId(read_u32(buf, off)?);
            let len = read_u32(buf, off)? as usize;
            let mut fields = Vec::with_capacity(len.min(buf.len().saturating_sub(*off)));
            for _ in 0..len {
                fields.push(decode_value(buf, off, depth + 1)?);
            }
            Ok(Value::record(shape, fields))
        }
        VAL_FN_REF => Ok(Value::FnRef(read_def_id(buf, off)?)),
        VAL_CLOSURE => {
            let target = read_def_id(buf, off)?;
            let count = read_u32(buf, off)? as usize;
            let mut env = Vec::with_capacity(count.min(buf.len().saturating_sub(*off)));
            for _ in 0..count {
                let name = brink_format::NameId(read_u16(buf, off)?);
                let is_ref = read_u8(buf, off)? != 0;
                let payload = decode_value(buf, off, depth + 1)?;
                env.push(brink_format::ClosureEnvEntry {
                    name,
                    is_ref,
                    payload,
                });
            }
            Ok(Value::closure(target, env))
        }
        // Handle values (T1d, `docs/format-v4-rfc.md` §1).
        VAL_HANDLE => {
            let kind = NameId(read_u16(buf, off)?);
            let id = read_u64(buf, off)?;
            Ok(Value::handle(kind, id))
        }
        // Projection values (T1e, `docs/format-v4-rfc.md` §1).
        VAL_PROJECTION => {
            let cell = read_def_id(buf, off)?;
            let count = read_u8(buf, off)? as usize;
            let mut segments = Vec::with_capacity(count.min(buf.len().saturating_sub(*off)));
            for _ in 0..count {
                let kind = read_u8(buf, off)?;
                let seg = match kind {
                    PROJ_SEG_INDEX => brink_format::ProjSegment::Index(read_i32(buf, off)?),
                    PROJ_SEG_KEY => {
                        brink_format::ProjSegment::Key(decode_value(buf, off, depth + 1)?)
                    }
                    other => return Err(TranscriptError::InvalidValueTag(other)),
                };
                segments.push(seg);
            }
            Ok(Value::projection(cell, segments))
        }
        // Option values (NS-A1): flag byte then inner-when-some; any other
        // flag byte is corrupt input. Depth-counted like the collections.
        VAL_OPTION => match read_u8(buf, off)? {
            0 => Ok(Value::none()),
            1 => Ok(Value::some(decode_value(buf, off, depth + 1)?)),
            other => Err(TranscriptError::InvalidValueTag(other)),
        },
        // Range values (NS-A5, F7): flat — start, end, incl/excl flag; any
        // other flag byte is corrupt input. No recursion, no depth.
        VAL_RANGE => {
            let start = read_i32(buf, off)?;
            let end = read_i32(buf, off)?;
            let inclusive = match read_u8(buf, off)? {
                0 => false,
                1 => true,
                other => return Err(TranscriptError::InvalidValueTag(other)),
            };
            Ok(Value::range(start, end, inclusive))
        }
        // Tower values (NS-A8): fixed-size little-endian f32 lanes in the
        // pinned order, rebuilt through glam's explicit array constructors.
        // Leaves — no counts, no recursion, no depth concerns.
        VAL_VEC2 => Ok(Value::Vec2(glam::Vec2::from_array(read_f32_lanes::<2>(
            buf, off,
        )?))),
        VAL_VEC3 => Ok(Value::Vec3(glam::Vec3::from_array(read_f32_lanes::<3>(
            buf, off,
        )?))),
        VAL_VEC4 => Ok(Value::Vec4(glam::Vec4::from_array(read_f32_lanes::<4>(
            buf, off,
        )?))),
        VAL_QUAT => Ok(Value::Quat(glam::Quat::from_array(read_f32_lanes::<4>(
            buf, off,
        )?))),
        VAL_MAT2 => Ok(Value::Mat2(glam::Mat2::from_cols_array(&read_f32_lanes::<
            4,
        >(
            buf, off
        )?))),
        VAL_MAT3 => Ok(Value::Mat3(glam::Mat3::from_cols_array(&read_f32_lanes::<
            9,
        >(
            buf, off
        )?))),
        VAL_MAT4 => Ok(Value::Mat4(glam::Mat4::from_cols_array(&read_f32_lanes::<
            16,
        >(
            buf, off
        )?))),
        // Weighted tables (NS-A7): mirror of the `.inkb` reader, invariant
        // checks included — a violating payload is corrupt input.
        VAL_WEIGHTED => {
            let count = read_u32(buf, off)? as usize;
            if count == 0 {
                return Err(TranscriptError::InvalidValueTag(VAL_WEIGHTED));
            }
            let mut entries = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                let weight = read_i32(buf, off)?;
                if weight < 1 {
                    return Err(TranscriptError::InvalidValueTag(VAL_WEIGHTED));
                }
                let value = decode_value(buf, off, depth + 1)?;
                entries.push((weight, value));
            }
            Ok(Value::weighted(entries))
        }
        _ => Err(TranscriptError::InvalidValueTag(tag)),
    }
}

/// Decode a [`MapKey`] written by `encode_map_key`: a scalar `VAL_*` tag then
/// its payload. Any other tag is rejected — only `int`/`string`/`bool` keys are
/// permitted (`docs/value-model-spec.md` §4).
fn decode_map_key(buf: &[u8], off: &mut usize) -> Result<MapKey, TranscriptError> {
    let tag = read_u8(buf, off)?;
    match tag {
        VAL_INT => Ok(MapKey::Int(read_i32(buf, off)?)),
        VAL_STRING => Ok(MapKey::Str(Arc::from(read_str(buf, off)?.as_str()))),
        VAL_BOOL => Ok(MapKey::Bool(read_u8(buf, off)? != 0)),
        _ => Err(TranscriptError::InvalidValueTag(tag)),
    }
}

// ── CRC-32 ────────────────────────────────────────────────────────────────

fn crc32(data: &[u8]) -> u32 {
    static TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0u32;
        while i < 256 {
            let mut crc = i;
            let mut j = 0;
            while j < 8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
                j += 1;
            }
            table[i as usize] = crc;
            i += 1;
        }
        table
    };

    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        let idx = ((crc ^ u32::from(byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ TABLE[idx];
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::LineFlags;

    #[test]
    fn round_trip_simple_parts() {
        let parts = vec![
            OutputPart::Text("Hello".to_string()),
            OutputPart::Spring,
            OutputPart::Newline,
            OutputPart::Tag("tag1".to_string()),
            OutputPart::Glue,
        ];
        let bytes = write_transcript(&parts, 0xDEAD_BEEF, &[]);
        let data = read_transcript(&bytes).unwrap();
        assert_eq!(data.source_checksum, 0xDEAD_BEEF);
        assert_eq!(data.parts.len(), 5);
        assert!(matches!(&data.parts[0], OutputPart::Text(s) if s == "Hello"));
        assert!(matches!(&data.parts[1], OutputPart::Spring));
        assert!(matches!(&data.parts[2], OutputPart::Newline));
        assert!(matches!(&data.parts[3], OutputPart::Tag(s) if s == "tag1"));
        assert!(matches!(&data.parts[4], OutputPart::Glue));
    }

    // #1443 review finding: `write_transcript`'s top-level `count` and each
    // fragment's `filtered_count` used to hand-duplicate the same
    // `!matches!(p, OutputPart::Checkpoint)` predicate. They now both call
    // the single shared `is_persisted` helper, which must stay in lockstep
    // with `encode_part`'s zero-byte arms: `Checkpoint` and — since issue
    // #2108 — `ElementAttach`/`ElementAttachEnd` are transient (zero
    // bytes); every other variant persists.
    #[test]
    fn is_persisted_filters_transient_markers_only() {
        assert!(!is_persisted(&OutputPart::Checkpoint));
        assert!(!is_persisted(&OutputPart::ElementAttach(
            "speaker".to_string(),
            "VENDOR".to_string()
        )));
        assert!(!is_persisted(&OutputPart::ElementAttachEnd));
        assert!(is_persisted(&OutputPart::Text("hi".to_string())));
        assert!(is_persisted(&OutputPart::LineRef {
            container_idx: 0,
            line_idx: 0,
            slots: Vec::new(),
            flags: LineFlags::empty(),
        }));
        assert!(is_persisted(&OutputPart::ValueRef(Value::Bool(true))));
        assert!(is_persisted(&OutputPart::Newline));
        assert!(is_persisted(&OutputPart::Spring));
        assert!(is_persisted(&OutputPart::Glue));
        assert!(is_persisted(&OutputPart::Tag("t".to_string())));
    }

    // #1443: `write_transcript`/`read_transcript` used to hand-duplicate one
    // match arm per `OutputPart` tag for the top-level part list and again
    // for each fragment's part list (plus the #953 fix for `Fragment::tags`
    // landing in only one of the two copies, which is exactly how that tags
    // regression happened). Both loops now call the single shared
    // `encode_part`/`decode_part` pair. This pins that: encoding the same
    // `OutputPart` sequence through the top-level path and through a
    // fragment's path produces byte-identical part payloads, and both paths
    // decode back to equal parts — proving one shared codec, not two copies
    // that happen to still agree.
    #[test]
    fn top_level_and_fragment_part_codec_are_byte_identical() {
        let parts = vec![
            OutputPart::Text("Hello".to_string()),
            OutputPart::LineRef {
                container_idx: 3,
                line_idx: 9,
                slots: vec![Value::Int(1), Value::String(Arc::from("hi"))],
                flags: LineFlags::ALL_WS,
            },
            OutputPart::ValueRef(Value::Bool(true)),
            OutputPart::Spring,
            OutputPart::Newline,
            OutputPart::Glue,
            OutputPart::Tag("tag1".to_string()),
            OutputPart::Checkpoint, // filtered identically by both paths
        ];

        // Independently reconstruct the expected encoded bytes by calling
        // the shared `encode_part` codec directly, one call per non-Checkpoint
        // part — this is what both `write_transcript` loops should be
        // producing under the hood.
        let mut expected = Vec::new();
        for part in &parts {
            if !matches!(part, OutputPart::Checkpoint) {
                encode_part(part, &mut expected);
            }
        }

        // Top-level loop: header, then a u32 part count, then the parts.
        let top_level_bytes = write_transcript(&parts, 0, &[]);
        let top_level_part_bytes =
            &top_level_bytes[HEADER_SIZE + 4..HEADER_SIZE + 4 + expected.len()];
        assert_eq!(
            top_level_part_bytes,
            expected.as_slice(),
            "top-level part encoding must match the shared codec exactly"
        );

        // Fragment loop: header, u32 top-level count (0), u32 fragment count
        // (1), u32 this-fragment's part count, then the same parts.
        let fragment = crate::output::Fragment {
            parts: parts.clone(),
            tags: Vec::new(),
        };
        let fragment_bytes = write_transcript(&[], 0, &[fragment]);
        let frag_start = HEADER_SIZE + 4 + 4 + 4;
        let fragment_part_bytes = &fragment_bytes[frag_start..frag_start + expected.len()];
        assert_eq!(
            fragment_part_bytes,
            expected.as_slice(),
            "fragment part encoding must match the shared codec exactly"
        );

        // And decoding both paths yields the same, correct parts.
        let top_level_data = read_transcript(&top_level_bytes).unwrap();
        let fragment_data = read_transcript(&fragment_bytes).unwrap();
        assert_eq!(top_level_data.parts.len(), 7); // Checkpoint filtered
        assert_eq!(fragment_data.fragments.len(), 1);
        assert_eq!(fragment_data.fragments[0].parts.len(), 7);
        assert_eq!(top_level_data.parts, fragment_data.fragments[0].parts);
    }

    /// NS-A8 (`docs/tower-mini-spec.md` T5): a tower value in an
    /// `OutputPart::ValueRef` crosses the `.brkt` round-trip as explicit
    /// little-endian lanes — including a NaN lane, compared here by lane
    /// bits (a NaN-bearing vector correctly never compares equal, T4).
    #[test]
    fn round_trip_value_ref_tower() {
        let parts = vec![
            OutputPart::ValueRef(Value::Vec3(glam::Vec3::new(1.5, -0.0, 3.0))),
            OutputPart::ValueRef(Value::Quat(glam::Quat::from_xyzw(0.5, -0.5, 0.5, 0.5))),
            OutputPart::ValueRef(Value::Mat2(glam::Mat2::from_cols_array(&[
                1.0, 2.0, 3.0, 4.0,
            ]))),
            OutputPart::ValueRef(Value::Vec2(glam::Vec2::new(f32::NAN, 7.0))),
        ];
        let bytes = write_transcript(&parts, 0, &[]);
        let data = read_transcript(&bytes).unwrap();
        assert_eq!(data.parts.len(), 4);
        assert!(
            matches!(&data.parts[0], OutputPart::ValueRef(v) if *v == Value::Vec3(glam::Vec3::new(1.5, -0.0, 3.0)))
        );
        assert!(
            matches!(&data.parts[1], OutputPart::ValueRef(v) if *v == Value::Quat(glam::Quat::from_xyzw(0.5, -0.5, 0.5, 0.5)))
        );
        assert!(
            matches!(&data.parts[2], OutputPart::ValueRef(v) if *v == Value::Mat2(glam::Mat2::from_cols_array(&[1.0, 2.0, 3.0, 4.0])))
        );
        let OutputPart::ValueRef(Value::Vec2(v)) = &data.parts[3] else {
            unreachable!("expected vec2 part, got {:?}", data.parts[3]);
        };
        assert_eq!(v.x.to_bits(), f32::NAN.to_bits(), "NaN lane bits drifted");
        assert_eq!(v.y.to_bits(), 7.0f32.to_bits());
    }

    // A collection reaches the transcript through `Opcode::EmitValue`, which
    // pops any stack value — including a binding/external return that since #525
    // can be an `Array`/`Map` — into `OutputPart::ValueRef`. This locks the v4
    // tree encoding of that part: structural equality, insertion order, scalar
    // key types, and nesting all survive the `.brkt` round-trip (#526).
    #[test]
    fn round_trip_value_ref_collections() {
        use brink_format::{MapKey, OrderedMap};

        let map: OrderedMap = [
            (MapKey::from("name"), Value::String(Arc::from("goblin"))),
            (
                MapKey::from(1),
                Value::array(vec![Value::Int(10), Value::Int(20)]),
            ),
            (MapKey::from(true), Value::Bool(false)),
        ]
        .into_iter()
        .collect();
        let array = Value::array(vec![
            Value::Int(1),
            Value::String(Arc::from("two")),
            Value::map(map.clone()),
            Value::Null,
        ]);

        let parts = vec![
            OutputPart::ValueRef(array.clone()),
            OutputPart::ValueRef(Value::map(map.clone())),
        ];
        let bytes = write_transcript(&parts, 42, &[]);
        let data = read_transcript(&bytes).unwrap();

        assert_eq!(data.parts.len(), 2);
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, array),
            other => unreachable!("expected ValueRef(array), got {other:?}"),
        }
        match &data.parts[1] {
            OutputPart::ValueRef(v) => assert_eq!(*v, Value::map(map)),
            other => unreachable!("expected ValueRef(map), got {other:?}"),
        }
    }

    // Function values (T1c, #700) persist through the transcript/journal as
    // ordinary values (spec §6). This locks the VAL_FN_REF / VAL_CLOSURE
    // encoding — the fn token, the bound-env names/modes, and both payload
    // shapes (`val` snapshot, `ref` VariablePointer) — across the round-trip.
    #[test]
    fn round_trip_value_ref_function_values() {
        use brink_format::{ClosureEnvEntry, DefinitionId, DefinitionTag, NameId};

        let target = DefinitionId::new(DefinitionTag::Address, 7);
        let cell = DefinitionId::new(DefinitionTag::Address, 3);
        let fn_ref = Value::FnRef(target);
        let closure = Value::closure(
            target,
            vec![
                ClosureEnvEntry {
                    name: NameId(2),
                    is_ref: true,
                    payload: Value::VariablePointer(cell),
                },
                ClosureEnvEntry {
                    name: NameId(5),
                    is_ref: false,
                    payload: Value::Int(41),
                },
            ],
        );

        let parts = vec![
            OutputPart::ValueRef(fn_ref.clone()),
            OutputPart::ValueRef(closure.clone()),
        ];
        let bytes = write_transcript(&parts, 7, &[]);
        let data = read_transcript(&bytes).unwrap();

        assert_eq!(data.parts.len(), 2);
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, fn_ref),
            other => unreachable!("expected ValueRef(fn_ref), got {other:?}"),
        }
        match &data.parts[1] {
            OutputPart::ValueRef(v) => assert_eq!(*v, closure),
            other => unreachable!("expected ValueRef(closure), got {other:?}"),
        }
    }

    // Handle values (T1d, `docs/t1d-spec.md` §2/§5) persist through the
    // transcript/journal codec as ordinary values — "handles appear in
    // saves, journals, and speculation snapshots" per the spec. This locks
    // the VAL_HANDLE (0x0D) encode/decode arms: a bare handle, one nested
    // inside a collection, and the `u64::MAX` id to exercise the full
    // write_u64/read_u64 leg (not just small ids that might coincidentally
    // round-trip through a truncated path).
    #[test]
    fn round_trip_value_ref_handle() {
        let handle = Value::handle(NameId(9), u64::MAX);
        let nested = Value::array(vec![
            Value::handle(NameId(3), 0),
            Value::String(Arc::from("goblin")),
        ]);

        let parts = vec![
            OutputPart::ValueRef(handle.clone()),
            OutputPart::ValueRef(nested.clone()),
        ];
        let bytes = write_transcript(&parts, 13, &[]);
        let data = read_transcript(&bytes).unwrap();

        assert_eq!(data.parts.len(), 2);
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, handle),
            other => unreachable!("expected ValueRef(handle), got {other:?}"),
        }
        match &data.parts[1] {
            OutputPart::ValueRef(v) => assert_eq!(*v, nested),
            other => unreachable!("expected ValueRef(nested handle), got {other:?}"),
        }
    }

    /// T1e (`docs/t1e-spec.md` §3: "Saves/journal/speculation: ordinary
    /// values") — the transcript leg of the per-codec round-trip discipline
    /// (inkb/inkt/transcript, the wave-11 lesson): the `VAL_PROJECTION`
    /// (0x0E) encode/decode arms, a bare projection and one nested inside a
    /// collection, with a mixed index+key segment chain.
    #[test]
    fn round_trip_value_ref_projection() {
        use brink_format::ProjSegment;

        let cell = DefinitionId::new(brink_format::DefinitionTag::GlobalVar, 42);
        let proj = Value::projection(
            cell,
            vec![
                ProjSegment::Key(Value::String("hp".into())),
                ProjSegment::Index(3),
            ],
        );
        let nested = Value::array(vec![Value::projection(cell, vec![]), Value::Bool(true)]);

        let parts = vec![
            OutputPart::ValueRef(proj.clone()),
            OutputPart::ValueRef(nested.clone()),
        ];
        let bytes = write_transcript(&parts, 13, &[]);
        let data = read_transcript(&bytes).unwrap();

        assert_eq!(data.parts.len(), 2);
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, proj),
            other => unreachable!("expected ValueRef(projection), got {other:?}"),
        }
        match &data.parts[1] {
            OutputPart::ValueRef(v) => assert_eq!(*v, nested),
            other => unreachable!("expected ValueRef(nested projection), got {other:?}"),
        }
    }

    #[test]
    fn round_trip_line_ref_with_slots() {
        let parts = vec![OutputPart::LineRef {
            container_idx: 42,
            line_idx: 7,
            slots: vec![Value::Int(123), Value::String(Arc::from("hello"))],
            flags: LineFlags::ALL_WS | LineFlags::EMPTY,
        }];
        let bytes = write_transcript(&parts, 1234, &[]);
        let data = read_transcript(&bytes).unwrap();
        assert_eq!(data.parts.len(), 1);
        match &data.parts[0] {
            OutputPart::LineRef {
                container_idx,
                line_idx,
                slots,
                flags,
            } => {
                assert_eq!(*container_idx, 42);
                assert_eq!(*line_idx, 7);
                assert_eq!(slots.len(), 2);
                assert!(matches!(&slots[0], Value::Int(123)));
                assert!(flags.contains(LineFlags::ALL_WS));
                assert!(flags.contains(LineFlags::EMPTY));
            }
            other => unreachable!("expected LineRef, got {other:?}"),
        }
    }

    #[test]
    fn checkpoint_filtered_on_write() {
        let parts = vec![
            OutputPart::Text("hello".to_string()),
            OutputPart::Checkpoint,
            OutputPart::Newline,
        ];
        let bytes = write_transcript(&parts, 0, &[]);
        let data = read_transcript(&bytes).unwrap();
        assert_eq!(data.parts.len(), 2); // Checkpoint filtered
        assert!(matches!(&data.parts[0], OutputPart::Text(_)));
        assert!(matches!(&data.parts[1], OutputPart::Newline));
    }

    // ── #953: Fragment::tags round-trip ─────────────────────────────────────
    //
    // `write_transcript` never serialized `Fragment::tags` and
    // `read_transcript` always reconstructed an empty `Vec` — a transcript
    // with tagged fragments (live, populated data — see
    // `OutputBuffer::push_fragment_tag`) round-tripped to untagged. This
    // pins the fix: tags now travel through the `.brkt` codec.
    #[test]
    fn round_trip_fragment_tags() {
        let fragments = vec![
            crate::output::Fragment {
                parts: vec![OutputPart::Text("hp: 10".to_string())],
                tags: vec!["a_tag".to_string(), "b_tag".to_string()],
            },
            crate::output::Fragment {
                parts: vec![OutputPart::Newline],
                tags: Vec::new(),
            },
        ];
        let bytes = write_transcript(&[], 0, &fragments);
        let data = read_transcript(&bytes).unwrap();

        assert_eq!(data.fragments.len(), 2);
        assert_eq!(
            data.fragments[0].tags,
            vec!["a_tag".to_string(), "b_tag".to_string()]
        );
        assert_eq!(data.fragments[0].parts, fragments[0].parts);
        assert!(data.fragments[1].tags.is_empty());
    }

    // Every `.brkt` file written before this fix has the fragment section
    // (fragment_count + per-fragment parts) with NO trailing tag section —
    // the reader must keep decoding those files (not error), falling back
    // to empty tags per fragment, exactly as it did before this fix. This
    // hand-builds that exact pre-fix byte shape rather than relying on the
    // current writer (which now always appends the tag section) so the
    // legacy shape is pinned even after the writer changes further.
    #[test]
    fn legacy_transcript_without_tag_section_reads_as_empty_tags() {
        let mut body = Vec::new();
        write_u32(&mut body, 0); // part count
        write_u32(&mut body, 1); // fragment count
        write_u32(&mut body, 1); // fragment 0's part count
        write_u8(&mut body, TAG_TEXT);
        write_str(&mut body, "legacy");
        // (no tag section appended — matches the pre-#953 writer)

        let content_crc = crc32(&body);
        let mut bytes = Vec::with_capacity(HEADER_SIZE + body.len());
        bytes.extend_from_slice(MAGIC);
        write_u16(&mut bytes, VERSION);
        write_u16(&mut bytes, 0);
        write_u32(&mut bytes, 0xCAFE_BABE);
        write_u32(&mut bytes, content_crc);
        bytes.extend(body);

        let data = read_transcript(&bytes).expect("legacy transcript must still decode");
        assert_eq!(data.fragments.len(), 1);
        assert!(matches!(&data.fragments[0].parts[0], OutputPart::Text(s) if s == "legacy"));
        assert!(data.fragments[0].tags.is_empty());
    }

    // The *other* backward-compat boundary this module's doc claims but only
    // `legacy_transcript_without_tag_section_reads_as_empty_tags` above pins:
    // a `.brkt` written before the fragment section existed at all (pre-
    // fragments feature), where the body ends right after the top-level part
    // list — no `fragment_count` `u32`, not even a zero one. `write_transcript`
    // has *always* written `fragments.len()` unconditionally (even `0` for an
    // empty slice — see the call site right after the part loop), so no call
    // through the real writer can ever produce this exact shape; it has to be
    // hand-built, same rationale as the tag-section test above. The read-side
    // `if off < bytes.len()` probe at the fragment-count read (this module's
    // `read_transcript`) is what is actually under test here: with zero bytes
    // left after the parts, it must fall back to "no fragments" rather than
    // erroring as truncated input.
    #[test]
    fn legacy_transcript_without_fragment_section_reads_as_no_fragments() {
        let mut body = Vec::new();
        write_u32(&mut body, 1); // part count
        write_u8(&mut body, TAG_TEXT);
        write_str(&mut body, "legacy");
        // (body ends here — no fragment section, no tag section, matches a
        // `.brkt` written before fragments existed at all)

        let content_crc = crc32(&body);
        let mut bytes = Vec::with_capacity(HEADER_SIZE + body.len());
        bytes.extend_from_slice(MAGIC);
        write_u16(&mut bytes, VERSION);
        write_u16(&mut bytes, 0);
        write_u32(&mut bytes, 0xCAFE_BABE);
        write_u32(&mut bytes, content_crc);
        bytes.extend(body);

        let data = read_transcript(&bytes).expect("legacy transcript must still decode");
        assert_eq!(data.parts.len(), 1);
        assert!(matches!(&data.parts[0], OutputPart::Text(s) if s == "legacy"));
        assert!(
            data.fragments.is_empty(),
            "a pre-fragments `.brkt` must decode with zero fragments, not error: {:?}",
            data.fragments
        );
    }

    #[test]
    fn invalid_magic_errors() {
        let mut bytes = write_transcript(&[], 0, &[]);
        bytes[0] = b'X';
        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::InvalidMagic)
        ));
    }

    #[test]
    fn integrity_check_errors() {
        let mut bytes = write_transcript(&[OutputPart::Newline], 0, &[]);
        // Corrupt a body byte
        if let Some(last) = bytes.last_mut() {
            *last ^= 0xFF;
        }
        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::IntegrityCheckFailed)
        ));
    }

    // ── Recursion-depth cap on VAL_ARRAY/VAL_MAP decode (#553, #561, #562) ──
    //
    // `decode_value` recurses into itself for VAL_ARRAY/VAL_MAP children with
    // no depth limit. A crafted transcript of nested single-element arrays
    // (~5 bytes/level) can stack-overflow the reader. These tests hand-build
    // a `Value` nested exactly at, and one past,
    // `brink_format::MAX_DECODE_DEPTH` (the single canonical definition
    // shared by every `decode_value` implementation, #561) and prove the
    // reader accepts the former and rejects the latter with a proper decode
    // error instead of overflowing the stack. Both the `VAL_ARRAY` recursion
    // branch and the parallel `VAL_MAP` branch are exercised at the boundary
    // (#562).

    /// A `Value` wrapped in `depth` single-element arrays around a scalar
    /// leaf, matching the issue's "nested single-element arrays" shape.
    fn nested_array(depth: usize) -> Value {
        let mut v = Value::Int(42);
        for _ in 0..depth {
            v = Value::array(vec![v]);
        }
        v
    }

    /// A `Value` wrapped in `depth` single-entry maps around a scalar leaf —
    /// the `VAL_MAP` analogue of [`nested_array`], exercising the parallel
    /// map recursion branch in `decode_value` (#562).
    fn nested_map(depth: usize) -> Value {
        use brink_format::{MapKey, OrderedMap};

        let mut v = Value::Int(42);
        for _ in 0..depth {
            let mut map = OrderedMap::with_capacity(1);
            map.insert(MapKey::Int(0), v);
            v = Value::map(map);
        }
        v
    }

    #[test]
    fn decode_value_accepts_max_depth_nesting() {
        // Exactly MAX_DECODE_DEPTH levels of nesting must still decode
        // cleanly — the cap must not clip legitimate (if unusual) data.
        let value = nested_array(MAX_DECODE_DEPTH);
        let parts = vec![OutputPart::ValueRef(value.clone())];
        let bytes = write_transcript(&parts, 0, &[]);

        let data = read_transcript(&bytes).expect("depth exactly at cap must decode");
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, value),
            other => unreachable!("expected ValueRef, got {other:?}"),
        }
    }

    #[test]
    fn decode_value_rejects_beyond_max_depth() {
        // One level past the cap must be rejected with a proper decode
        // error, not a stack overflow.
        let value = nested_array(MAX_DECODE_DEPTH + 1);
        let parts = vec![OutputPart::ValueRef(value)];
        let bytes = write_transcript(&parts, 0, &[]);

        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH))
        ));
    }

    #[test]
    fn decode_value_rejects_deeply_crafted_nesting() {
        // The actual attack scenario the issue describes: a much deeper
        // chain than any legitimate story would produce (well beyond the
        // cap, but shallow enough that constructing/encoding the fixture
        // itself — which has no depth cap by design; only the
        // untrusted-input decode path is guarded — doesn't hit unrelated
        // recursion limits). The reader must reject it promptly rather than
        // recursing hundreds of frames deep.
        let value = nested_array(8 * MAX_DECODE_DEPTH);
        let parts = vec![OutputPart::ValueRef(value)];
        let bytes = write_transcript(&parts, 0, &[]);

        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH))
        ));
    }

    // ── #562: parallel VAL_MAP recursion branch at the boundary ────────────

    #[test]
    fn decode_value_accepts_max_depth_map_nesting() {
        // Exactly MAX_DECODE_DEPTH levels of map nesting must still decode
        // cleanly — the cap must not clip legitimate (if unusual) data.
        let value = nested_map(MAX_DECODE_DEPTH);
        let parts = vec![OutputPart::ValueRef(value.clone())];
        let bytes = write_transcript(&parts, 0, &[]);

        let data = read_transcript(&bytes).expect("map depth exactly at cap must decode");
        match &data.parts[0] {
            OutputPart::ValueRef(v) => assert_eq!(*v, value),
            other => unreachable!("expected ValueRef, got {other:?}"),
        }
    }

    #[test]
    fn decode_value_rejects_beyond_max_depth_map_nesting() {
        // One level past the cap must be rejected with a proper decode
        // error, not a stack overflow.
        let value = nested_map(MAX_DECODE_DEPTH + 1);
        let parts = vec![OutputPart::ValueRef(value)];
        let bytes = write_transcript(&parts, 0, &[]);

        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::MaxDepthExceeded(MAX_DECODE_DEPTH))
        ));
    }

    // Issue #985 (follow-up to #909): `OrderedMap`'s `Eq` is content-based
    // and assumes each key appears at most once. A legitimate `write_transcript`
    // never emits a duplicate `VAL_MAP` key — `OrderedMap::insert`
    // de-duplicates on the write side, so `encode_value` can't be driven
    // into producing one from an in-memory `Value`. This hand-builds the raw
    // `VAL_MAP` bytes (the crafted/corrupt-payload scenario the issue
    // describes) with the same `int` key twice, proving the reader rejects
    // it with a decode error rather than silently keeping the last
    // occurrence and handing back an invariant-violating `OrderedMap`.
    fn duplicate_int_key_map_body() -> Vec<u8> {
        let mut body = Vec::new();
        write_u32(&mut body, 1); // part count
        write_u8(&mut body, TAG_VALUE_REF);
        write_u8(&mut body, VAL_MAP);
        write_u32(&mut body, 2); // entry count
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 0);
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 1);
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 0);
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 2);
        write_u32(&mut body, 0); // fragment count
        body
    }

    fn wrap_body_as_transcript(body: &[u8]) -> Vec<u8> {
        let content_crc = crc32(body);
        let mut bytes = Vec::with_capacity(HEADER_SIZE + body.len());
        bytes.extend_from_slice(MAGIC);
        write_u16(&mut bytes, VERSION);
        write_u16(&mut bytes, 0);
        write_u32(&mut bytes, 0);
        write_u32(&mut bytes, content_crc);
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn decode_value_rejects_duplicate_map_key() {
        let bytes = wrap_body_as_transcript(&duplicate_int_key_map_body());
        assert!(matches!(
            read_transcript(&bytes),
            Err(TranscriptError::DuplicateMapKey)
        ));
    }

    #[test]
    fn decode_value_accepts_distinct_map_keys() {
        let mut body = Vec::new();
        write_u32(&mut body, 1); // part count
        write_u8(&mut body, TAG_VALUE_REF);
        write_u8(&mut body, VAL_MAP);
        write_u32(&mut body, 2); // entry count
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 0);
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 1);
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 5);
        write_u8(&mut body, VAL_INT);
        write_i32(&mut body, 2);
        write_u32(&mut body, 0); // fragment count

        let bytes = wrap_body_as_transcript(&body);
        let data = read_transcript(&bytes).expect("distinct keys must decode cleanly");
        match &data.parts[0] {
            OutputPart::ValueRef(Value::Map(map)) => assert_eq!(map.len(), 2),
            other => unreachable!("expected ValueRef(map), got {other:?}"),
        }
    }
}
