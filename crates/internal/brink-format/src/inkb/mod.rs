//! Binary (.inkb) writer and reader for [`StoryData`].
//!
//! The `.inkb` format is a compact, little-endian binary encoding designed for
//! fast loading by the runtime.
//!
//! ## Header layout
//!
//! ```text
//! Offset  Size   Field
//! ------  -----  ------
//! 0       4      Magic: b"INKB"
//! 4       2      Version: u16 LE (= 1)
//! 6       1      Section count: u8 (N entries in offset table)
//! 7       1      Reserved: 0x00
//! 8       4      File size: u32 LE (total bytes)
//! 12      4      Content checksum: u32 LE (CRC-32 of all bytes after header)
//! 16      N*8    Offset table entries
//! ```
//!
//! Each offset table entry (8 bytes):
//! ```text
//! 0       1      SectionKind: u8 tag
//! 1       3      Reserved: 3 bytes of 0x00
//! 4       4      Offset: u32 LE (byte offset from start of file)
//! ```

pub(crate) mod read;
pub(crate) mod write;

pub use read::{
    read_inkb, read_inkb_index, read_section_address_paths, read_section_addresses,
    read_section_alias_table, read_section_containers, read_section_debug_info,
    read_section_effect_rows, read_section_externals, read_section_frame_shapes,
    read_section_line_tables, read_section_list_defs, read_section_list_items,
    read_section_list_literals, read_section_literal_pool, read_section_name_table,
    read_section_struct_shapes, read_section_variables, read_section_visibility,
};
pub use write::{
    assemble_inkb, write_inkb, write_section_address_paths, write_section_addresses,
    write_section_alias_table, write_section_containers, write_section_debug_info,
    write_section_effect_rows, write_section_externals, write_section_frame_shapes,
    write_section_line_tables, write_section_list_defs, write_section_list_items,
    write_section_list_literals, write_section_literal_pool, write_section_name_table,
    write_section_struct_shapes, write_section_variables, write_section_visibility,
};

use core::ops::Range;

use alloc::vec::Vec;

use crate::opcode::DecodeError;

// ── Constants ───────────────────────────────────────────────────────────────

pub(crate) const MAGIC: &[u8; 4] = b"INKB";
/// On-the-wire format version. Bumped on any byte-layout change; the reader
/// hard-rejects an unrecognized version (see `docs/format-spec.md` § Versioning).
/// v2 added `ContainerDef::param_count` to the Containers section.
/// v3 added the `local` scope bit to `GlobalVarDef` (Variables section) and
/// `ContainerDef` (Containers section) — see `docs/directive-annotations-spec.md`.
/// v4 added the collection value tags `VAL_ARRAY`/`VAL_MAP` (tree encoding) and
/// froze the reserved Tier-1 value-tag/section/opcode surface (the §9 one-bump
/// rule of `docs/value-model-spec.md`) — see `docs/format-v4-rfc.md`.
/// (Also on the v4 line: the optional `Visibility` section, M-2b,
/// `docs/modules-spec.md` §4, tag `0x0E` — omitted when empty, so it didn't
/// need a bump of its own.)
/// v5 added the `AliasTable` section (`docs/modules-spec.md` §5, M-3): this
/// section was not part of the v4 RFC's frozen inventory (unlike
/// `StructShapes`/`EffectRows`, which were pre-reserved and only needed
/// their *encoding* materialized without a bump), so a brand-new *mandatory*
/// section is its own one-bump event. The section itself carries a
/// one-byte section-local version so its *row encoding* can still evolve
/// without a further format bump, matching the `EffectRows` precedent.
/// `AliasTable` takes tag `0x0F` (the next free tag after `Visibility`).
/// v6 added the `PART_SPAN` `LinePart` tag (#1716, `docs/prose-dialect-spec.md`
/// §4.4/§4.5): like `AliasTable`, `PART_SPAN` was not part of the v4 RFC's
/// pre-reserved inventory (that inventory covers `VAL_ARRAY`/`VAL_MAP`-style
/// *value* tags and the `StructShapes`/`EffectRows` sections — it never
/// reserved a `LinePart` tag), so introducing it is its own one-bump event,
/// not a free ride on the `VAL_VEC2`/`VAL_WEIGHTED` no-bump precedent (see
/// `PART_SPAN`'s doc comment below). Ruled directly by issue #1716's own
/// ⚠ ("`LinePart::Span` is a v6 payload") and coordinated with #1683 (the
/// v6 bump manifest); this PR lands only the `Span` payload of that
/// manifest, so `VERSION` 6 stays open to absorb #1683's remaining payloads
/// (element kind/data, universal block id, choice captured environment)
/// without a further bump, the same way v4 absorbed its later Tier-1
/// milestones — a single bump event, not a bump per payload.
/// v7 added the first peephole superinstruction, `EmitLineNl` (`0x6C`,
/// `docs/optimizer-peephole.md`): a brand-new opcode discriminant, not one of
/// the reserved ones, so — like `PART_SPAN` — its own one-bump event. The
/// optimizer is the only producer; codegen output is unchanged, and the
/// reader still hard-rejects the previous version rather than translating.
/// v8 added the second superinstruction family — `BinaryImm` (`0x6D`),
/// `BinaryJumpIfFalse` (`0x6E`), `BinaryImmJumpIfFalse` (`0x6F`), each with a
/// `BinaryKind` operator byte — under the same one-bump-per-new-opcode-event
/// rule and the same optimizer-only provenance.
pub(crate) const VERSION: u16 = 8;
/// Fixed-size preamble: magic + version + section count + reserved + file size + checksum.
pub(crate) const HEADER_PREAMBLE: usize = 16;
/// Each offset table entry: kind(1) + reserved(3) + offset(4)
pub(crate) const SECTION_ENTRY_SIZE: usize = 8;
/// Number of *mandatory* sections in the current format (always present,
/// including the possibly-empty `AliasTable` and `EffectRows`). The optional
/// `Visibility` section (M-2b) adds one more entry to the offset table when
/// non-empty.
pub(crate) const SECTION_COUNT: u8 = 14;

// Value type tags
pub(crate) const VAL_INT: u8 = 0x00;
pub(crate) const VAL_FLOAT: u8 = 0x01;
pub(crate) const VAL_BOOL: u8 = 0x02;
pub(crate) const VAL_STRING: u8 = 0x03;
pub(crate) const VAL_LIST: u8 = 0x04;
pub(crate) const VAL_DIVERT_TARGET: u8 = 0x05;
pub(crate) const VAL_NULL: u8 = 0x06;
pub(crate) const VAL_VAR_POINTER: u8 = 0x07;
pub(crate) const VAL_FRAGMENT_REF: u8 = 0x08;
// v4 collection tags (`docs/format-v4-rfc.md` §1). Tree encoding: sharing is
// not preserved on the wire — a snapshot serializes as a plain nested tree.
pub(crate) const VAL_ARRAY: u8 = 0x09;
pub(crate) const VAL_MAP: u8 = 0x0A;
// TM-4 (`docs/typed-mode-spec.md` §6 / `docs/value-model-spec.md` §11c):
// closed-shape records. `docs/format-v4-rfc.md` §1: `ShapeId (u32 into
// StructShapes), then field values in shape order`.
pub(crate) const VAL_RECORD: u8 = 0x0F;
// T1c (`docs/t1c-spec.md` §6, `docs/format-v4-rfc.md` §1): function values.
// `VAL_FN_REF` = the zero-bound case (a `DefinitionId`); `VAL_CLOSURE` =
// `DefinitionId`, u16 env count, then env entries `{NameId, kind u8 (0=val,
// 1=ref), value}`. Numeric assignments were frozen by the one-bump rule; this
// PR (T1c-2) materializes them.
pub(crate) const VAL_FN_REF: u8 = 0x0B;
pub(crate) const VAL_CLOSURE: u8 = 0x0C;
// T1d (`docs/t1d-spec.md` §2, `docs/format-v4-rfc.md` §1): opaque
// host-resource tokens. `kind NameId, u64 id` — no live pointer, no
// dedicated opcode; handles enter the script world only via bindings.
pub(crate) const VAL_HANDLE: u8 = 0x0D;
// T1e (`docs/t1e-spec.md` §3, `docs/format-v4-rfc.md` §1): symbolic path
// projections. `cell reference (= VAL_VAR_POINTER payload shape), u8 segment
// count, then segments (u8 kind: 0=index i32 / 1=key value)`. Segment kind
// `2=range` is RESERVED — never emitted (icebox #829). First emission of
// this reserved tag.
pub(crate) const VAL_PROJECTION: u8 = 0x0E;
// NS-A1 (`docs/stdlib-spec.md` §1.1/§1.4, ruled 2026-07-18): the compiler-
// owned `Option[T]` enum. Wire form: one flag byte (0 = `none`, 1 =
// `some`), then the inner value when `some` — the enum's two variants,
// nothing more. Next free tag after `VAL_RECORD` (0x0F); this PR's own
// reservation, same "assigned here" precedent as the record/handle/
// projection tags above. Recursion counts toward `MAX_DECODE_DEPTH`
// exactly like the collection tags (a crafted chain of nested `some`s is
// the same stack-overflow shape as nested single-element arrays).
pub(crate) const VAL_OPTION: u8 = 0x10;
// NS-A5 (`docs/stdlib-spec.md` §7, F7 ruled 2026-07-19): the integer range
// value kind. Wire form: start i32, end i32, one flag byte (0 = `..`
// exclusive, 1 = `..=` inclusive) — flat, no recursion, so it does NOT
// count toward `MAX_DECODE_DEPTH` (a range holds two ints, never another
// value). Next free tag after `VAL_OPTION` (0x10); same "assigned here"
// reservation precedent as its neighbors. Distinct from the RESERVED
// projection-*segment* kind 0x02 below — that is a different namespace.
pub(crate) const VAL_RANGE: u8 = 0x11;
// NS-A8 (`docs/tower-mini-spec.md` T5, issue #1114): the numeric tower.
// Wire form is hand-serialized **explicit little-endian f32 lanes** — never
// glam's memory layout (it varies with SIMD features and versions) and never
// serde-through-glam. Lane order: vectors and the quat `x, y(, z, w)`;
// matrices column-major, column-by-column. Fixed payload sizes (no counts,
// no recursion — tower values are leaves, so like `VAL_RANGE` they do NOT
// count toward `MAX_DECODE_DEPTH`): vec2 = 8 bytes, vec3 = 12,
// vec4/quat/mat2 = 16, mat3 = 36, mat4 = 64. Next free tags after
// `VAL_RANGE` (0x11); this PR's own reservation, the same "assigned here"
// precedent as the record/handle/projection/option/range tags above. No
// format `VERSION` bump — additive value tags follow the NS-A1 `VAL_OPTION`
// precedent (an old reader rejects the unknown tag; an old file simply
// never contains one).
pub(crate) const VAL_VEC2: u8 = 0x12;
pub(crate) const VAL_VEC3: u8 = 0x13;
pub(crate) const VAL_VEC4: u8 = 0x14;
pub(crate) const VAL_QUAT: u8 = 0x15;
pub(crate) const VAL_MAT2: u8 = 0x16;
pub(crate) const VAL_MAT3: u8 = 0x17;
pub(crate) const VAL_MAT4: u8 = 0x18;
// NS-A7 (`docs/stdlib-spec.md` §8, issue #1113): the weighted table.
// Wire form: u32 entry count, then per entry an i32 weight followed by a
// recursively-encoded value. Values recurse, so decoding counts toward
// `MAX_DECODE_DEPTH` exactly like the collection tags. The reader enforces
// the §8 evidence-by-construction invariant (non-empty, weights ≥ 1) — a
// violating payload is a decode error, so a `Weighted` never enters the
// runtime invalid, even from a crafted file. Next free tag after
// `VAL_MAT4` (0x18); this PR's own reservation, additive per the NS-A1
// `VAL_OPTION` precedent (no `VERSION` bump).
pub(crate) const VAL_WEIGHTED: u8 = 0x19;
/// Wire kind for a [`crate::ProjSegment::Index`] segment.
pub(crate) const PROJ_SEG_INDEX: u8 = 0x00;
/// Wire kind for a [`crate::ProjSegment::Key`] segment.
pub(crate) const PROJ_SEG_KEY: u8 = 0x01;
// Segment kind 0x02 (range: start i32, end i32) is RESERVED — sequence
// slices/ranges (icebox #829). Never emitted; the reader rejects it
// (`InvalidProjSegmentKind`) since no `ProjSegment` variant exists to decode
// into, the same discipline the value-tag reservations above follow.

// EffectRows call-atom slots (T2-3, `docs/effects-spec.md` §11). The
// capability-parameter slot is populated `(any)` in v1; the handle-parameter
// slot is reserved (`docs/t1d-spec.md` §7) and always `None`.
/// Capability-parameter slot value: the whole capability, unrefined. The only
/// value v1 emits; path-granular tags (#826) are reserved (reader rejects).
pub(crate) const CAP_PARAM_ANY: u8 = 0x00;
/// Handle-parameter slot value: no bound handle. The only value v1 emits;
/// a non-zero slot is the reserved handle-parameterized form (reader rejects).
pub(crate) const HANDLE_PARAM_NONE: u8 = 0x00;

/// NS-A2 (issue #1108): bit assignments for the `DirectEffects` extension
/// flags byte (`EffectRows` section version 3). Bits 3–7 are RESERVED —
/// the strict reader rejects a nonzero reserved bit until a section version
/// graduates it (the same discipline as the capability/handle slots).
pub(crate) const EFFECT_DIM_EMITS: u8 = 0b0000_0001;
pub(crate) const EFFECT_DIM_TAGS: u8 = 0b0000_0010;
pub(crate) const EFFECT_DIM_FAULTS: u8 = 0b0000_0100;
pub(crate) const EFFECT_DIM_KNOWN_MASK: u8 = EFFECT_DIM_EMITS | EFFECT_DIM_TAGS | EFFECT_DIM_FAULTS;

// LineContent tags
pub(crate) const LINE_PLAIN: u8 = 0x00;
pub(crate) const LINE_TEMPLATE: u8 = 0x01;

// LinePart tags
pub(crate) const PART_LITERAL: u8 = 0x00;
pub(crate) const PART_SLOT: u8 = 0x01;
pub(crate) const PART_SELECT: u8 = 0x02;
// #1716 (`docs/prose-dialect-spec.md` §4.4/§4.5): the inline markup span.
// Structurally, adding this tag is one match arm on the existing `u8`
// part-tag dispatch — an old reader hard-rejects the unknown tag
// (`decode_line_part`'s `_ => Err(InvalidLinePart)`) and an old file simply
// never contains one. That is *not* the same as the `VAL_VEC2`/
// `VAL_WEIGHTED` no-`VERSION`-bump precedent, though: those value tags sit
// in the v4 RFC's pre-reserved, frozen tag inventory (`docs/format-v4-rfc.md`,
// the §9 one-bump rule), so materializing their encoding was already paid
// for by v4's bump. `PART_SPAN` was never part of that reservation — issue
// #1716 rules it explicitly as **"a v6 payload"**, coordinated with #1683
// (the v6 bump manifest). It IS its own one-bump event: `VERSION` bumped to
// 6 (see `VERSION`'s doc comment). `.inkl` shares this encoder/decoder
// (`inkl::{read,write}` call straight through to
// `encode_line_content`/`decode_line_content`), so both formats gain the
// tag from this one bump.
pub(crate) const PART_SPAN: u8 = 0x03;

// SelectKey tags
pub(crate) const KEY_CARDINAL: u8 = 0x00;
pub(crate) const KEY_ORDINAL: u8 = 0x01;
pub(crate) const KEY_EXACT: u8 = 0x02;
pub(crate) const KEY_KEYWORD: u8 = 0x03;

// PluralCategory tags
pub(crate) const CAT_ZERO: u8 = 0x00;
pub(crate) const CAT_ONE: u8 = 0x01;
pub(crate) const CAT_TWO: u8 = 0x02;
pub(crate) const CAT_FEW: u8 = 0x03;
pub(crate) const CAT_MANY: u8 = 0x04;
pub(crate) const CAT_OTHER: u8 = 0x05;

// ── Section types ───────────────────────────────────────────────────────────

/// Identifies a section within an `.inkb` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SectionKind {
    NameTable = 0x01,
    Variables = 0x02,
    ListDefs = 0x03,
    ListItems = 0x04,
    Externals = 0x05,
    Containers = 0x06,
    LineTables = 0x07,
    Labels = 0x08,
    ListLiterals = 0x09,
    AddressPaths = 0x0A,
    /// T1b `LiteralPool` (`docs/format-v4-rfc.md` §2): content-hash
    /// deduplicated constant values referenced by `PushLiteral(idx)`.
    /// Additive alongside `ListLiterals` — see the T1b-2 PR description for
    /// why the RFC's `ListLiterals` absorption is deferred, not done here.
    LiteralPool = 0x0B,
    /// TM-4 `StructShapes` (`docs/typed-mode-spec.md` §6): one entry per
    /// declared `STRUCT` — shape id, name, ordered field names. Reserved
    /// (count always 0) through 4.0; this PR lands the section's real
    /// encoding at the format layer only — nothing in the compiler emits a
    /// non-empty table yet (see the PR description's scope note).
    StructShapes = 0x0C,
    /// T2-3 `EffectRows` (`docs/effects-spec.md` §11, `docs/format-v4-rfc.md`
    /// §2): the `DefinitionId → row` table of factored effect rows — one per
    /// knot/stitch (the resume-scheduling estimate, §12.1). Section-locally
    /// versioned (one prefix byte) so the row encoding can grow without a
    /// format-wide bump — the reservation this graduates was made for exactly
    /// this, so no `VERSION` bump accompanies it. Always present (possibly
    /// empty). Was reserved (count-0) through v5; this slice lands the real
    /// encoding **and** first emission (rows are inert metadata the runtime
    /// does not yet read).
    EffectRows = 0x0D,
    /// M-2b `Visibility` (`docs/modules-spec.md` §4, `docs/format-spec.md`):
    /// the `DefinitionId`s of every `#@private` definition, sorted ascending.
    /// **Omitted entirely when empty** (the common all-public case), so
    /// public-only stories stay byte-identical for that section — the
    /// section is purely additive and self-framed in the offset table.
    /// `0x0D` is claimed by `EffectRows`, so this takes the next free tag.
    Visibility = 0x0E,
    /// M-3 `AliasTable` (`docs/modules-spec.md` §5): old→new `DefinitionId`
    /// rename records from `#@was(old_name)` directives. Section-locally
    /// versioned (one prefix byte) so the row encoding can grow without a
    /// format-wide bump. Always present (possibly empty) from v5 onward.
    /// `0x0E` was claimed by `Visibility` (M-2b), so this takes the next
    /// free tag.
    AliasTable = 0x0F,
    /// FS-3 `FrameShapes` (`docs/flow-suspension-spec.md` §4/§11): one
    /// name-keyed frame shape per `await` site — the static crossing-locals
    /// description the runtime spills/restores around a park.
    /// Section-locally versioned (one prefix byte) so the shape encoding can
    /// grow without a format-wide bump. **Omitted entirely when empty** (the
    /// common case — and, behind the E052 fence, the *only* case today, since
    /// no `await` compiles yet), so all existing stories stay byte-identical
    /// and no `VERSION` bump is needed. `0x0F` was claimed by `AliasTable`, so
    /// this takes the next free tag.
    FrameShapes = 0x10,
    /// D6 `DebugInfo` (`docs/debugger-spec.md` §2, issue #3184): the
    /// bytecode-offset → source-range map, plus its section-local file
    /// table. Section-locally versioned (one prefix byte) so the entry
    /// encoding can grow (e.g. the reserved `NodeId` column, §1.3) without a
    /// format-wide bump. **Omitted entirely when not requested** — the
    /// ship-policy default (§1.2): a release-exported story never carries
    /// this section, so every existing story stays byte-identical and no
    /// `VERSION` bump is needed. `0x10` was claimed by `FrameShapes`, so
    /// this takes the next free tag — the test this graduates,
    /// `from_u8_rejects_unclaimed_section_tag`, is updated alongside this
    /// variant (not deleted) to pin the *new* next-free tag (`0x12`).
    DebugInfo = 0x11,
    /// Stage 1 of the shared-alternatives track (issue #3273,
    /// `docs/decision-log.md` 2026-08-29): [`crate::LineVariantGroup`]
    /// records tying runs of consecutive line-table entries back to one
    /// authored line whose inline alternatives were enumerated at
    /// recognition time. Section-locally versioned (one prefix byte) so the
    /// record encoding can grow without a format-wide bump. **Omitted
    /// entirely when empty** — nothing emits a non-empty table until the
    /// stage-2 flip (#3274), so every existing story stays byte-identical
    /// and no `VERSION` bump is needed. `0x11` is claimed by `DebugInfo`,
    /// so this takes the next free tag.
    LineVariantGroups = 0x12,
}

// All v4-reserved section kinds have now graduated: `LiteralPool` (0x0B),
// `StructShapes` (0x0C), and `EffectRows` (0x0D, T2-3). `Visibility` (0x0E)
// and `AliasTable` (0x0F) were later one-bump additions past the reserved
// gap; `LineVariantGroups` (0x12, #3273) is the newest.

impl SectionKind {
    pub(crate) fn from_u8(tag: u8) -> Result<Self, DecodeError> {
        match tag {
            0x01 => Ok(Self::NameTable),
            0x02 => Ok(Self::Variables),
            0x03 => Ok(Self::ListDefs),
            0x04 => Ok(Self::ListItems),
            0x05 => Ok(Self::Externals),
            0x06 => Ok(Self::Containers),
            0x07 => Ok(Self::LineTables),
            0x08 => Ok(Self::Labels),
            0x09 => Ok(Self::ListLiterals),
            0x0A => Ok(Self::AddressPaths),
            0x0B => Ok(Self::LiteralPool),
            0x0C => Ok(Self::StructShapes),
            0x0D => Ok(Self::EffectRows),
            0x0E => Ok(Self::Visibility),
            0x0F => Ok(Self::AliasTable),
            0x10 => Ok(Self::FrameShapes),
            0x11 => Ok(Self::DebugInfo),
            0x12 => Ok(Self::LineVariantGroups),
            _ => Err(DecodeError::InvalidSectionKind(tag)),
        }
    }
}

/// An entry in the `.inkb` offset table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectionEntry {
    pub kind: SectionKind,
    pub offset: u32,
}

/// Parsed header + offset table from an `.inkb` file.
///
/// Allows selective reads without parsing section data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InkbIndex {
    pub version: u16,
    pub file_size: u32,
    pub checksum: u32,
    pub sections: Vec<SectionEntry>,
}

impl InkbIndex {
    /// Total header size in bytes (preamble + offset table).
    pub fn header_size(&self) -> usize {
        HEADER_PREAMBLE + self.sections.len() * SECTION_ENTRY_SIZE
    }

    /// Returns `(offset, length)` for a section, computing length from the
    /// next section's offset (or `file_size` for the last section).
    ///
    /// Subtraction is safe because `read_inkb_index` validates that offsets
    /// are monotonically increasing and within `[header_size, file_size]`.
    pub fn section_range(&self, kind: SectionKind) -> Option<Range<usize>> {
        let idx = self.sections.iter().position(|e| e.kind == kind)?;
        let start = self.sections[idx].offset as usize;
        let end = self
            .sections
            .get(idx + 1)
            .map_or(self.file_size, |e| e.offset) as usize;
        Some(start..end)
    }
}

/// Cap `Vec::with_capacity` allocations against remaining buffer bytes to avoid
/// OOM on crafted inputs with huge count fields. Each element occupies at least
/// `min_element_size` bytes, so the count can't exceed `remaining / min`.
pub(crate) fn safe_capacity(
    count: usize,
    buf_len: usize,
    offset: usize,
    min_element_size: usize,
) -> usize {
    let remaining = buf_len.saturating_sub(offset);
    let max_possible = remaining.checked_div(min_element_size).unwrap_or(remaining);
    count.min(max_possible)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every v4-reserved section tag has now graduated to a real
    /// `SectionKind` variant — `LiteralPool` (0x0B, T1b-2 #570),
    /// `StructShapes` (0x0C, TM-4 #620), and `EffectRows` (0x0D, T2-3 #862).
    /// `LineVariantGroups` (0x12, #3273) is the newest tag; the next
    /// unclaimed tag (0x13) is still rejected. This pin previously named
    /// `0x12` before `LineVariantGroups` claimed it, and `0x11` before
    /// D6 claimed it — flipped here, not deleted, per
    /// `docs/debugger-spec.md` §1.1's explicit instruction.
    #[test]
    fn from_u8_rejects_unclaimed_section_tag() {
        let tag = 0x13u8;
        let err = SectionKind::from_u8(tag).unwrap_err();
        assert_eq!(err, DecodeError::InvalidSectionKind(tag));
    }

    #[test]
    fn from_u8_accepts_all_current_sections() {
        // 0x01..=0x0D are contiguous real sections (EffectRows graduated 0x0D).
        for tag in 0x01u8..=0x0D {
            assert!(SectionKind::from_u8(tag).is_ok());
        }
        assert!(SectionKind::from_u8(0x0E).is_ok(), "Visibility (M-2b)");
        assert!(SectionKind::from_u8(0x0F).is_ok(), "AliasTable (M-3)");
        assert!(SectionKind::from_u8(0x10).is_ok(), "FrameShapes (FS-3c)");
        assert!(SectionKind::from_u8(0x11).is_ok(), "DebugInfo (D6 #3184)");
        assert!(
            SectionKind::from_u8(0x12).is_ok(),
            "LineVariantGroups (#3273)"
        );
    }
}
