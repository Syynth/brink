//! The conventions-projection wire shape (issue #2111, continuation
//! finding 2): a flat, span-free mirror of `brink_ir::ConventionsProjection`
//! that can round-trip through bytes, in the same hand-rolled
//! `.inkb`-section-codec idiom `StructShapeDef`/`FrameShapeDef` already use
//! — never `serde`. Nothing in this crate's `.inkb` format is serde-based
//! (`definition.rs`'s types derive no `Serialize`/`Deserialize` at all);
//! `serde` only appears elsewhere in this crate for the unrelated
//! JSON-based save-game format (`save.rs`) and `Value`'s `serde_json` law
//! tests. Following the established `.inkb` idiom here — rather than
//! reaching for `serde` because the review finding's prose said "no
//! serde" — is the "do not fork a second format" instruction read
//! literally: a serde-derived type would BE a second, inconsistent
//! encoding convention living next to every other section in this format.
//!
//! # What this module does NOT yet do
//!
//! This defines the wire SHAPE and its codec only — it is not yet wired
//! into [`crate::StoryData`]/[`crate::inkb::SectionKind`]. Doing that
//! requires threading the project-layer conventions projection —
//! `brink_ir::ConventionsProjection::from_decls` (built from
//! `ClaimHandlerDecl`s), surfaced editor-side via `brink_db::queries::
//! analysis::conventions_projection_query` (#2111/#2212) — through LIR
//! lowering, which `brink-compiler`'s production pipeline does not do today
//! (it never runs through `brink-db`'s salsa layer — only the editor does).
//! The join this used to name, `brink_analyzer::conventions_registry`, was
//! deleted with the rest of the dissolved `fn conventions()`/`register`
//! machinery (issue #2165); it is not what this section still needs to
//! wire up. Allocating a `.inkb` section tag and `StoryData` field ahead of
//! that consumer (the #2108 host binding join) settling its exact needs
//! risks locking in the wrong shape; wiring is left to a tracked follow-up.
//! What exists here —
//! the types, [`write_conventions_projection`]/[`read_conventions_projection`],
//! and `brink_ir::ConventionsProjection::to_wire`'s conversion (every field
//! this wire shape carries survives the round trip — see
//! [`ConventionEntryDef`]'s own doc for the one field it deliberately does
//! not carry, and why that is not a loss today) — is the reusable, tested
//! groundwork that follow-up builds on, not a stand-in for it.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::codec::{
    read_str, read_u8, read_u32, read_u64, write_str, write_u8, write_u32, write_u64,
};
use crate::inkb::safe_capacity;
use crate::opcode::DecodeError;
use crate::value::MAX_DECODE_DEPTH;

/// Section-local encoding version — independent of the whole-`.inkb`
/// `VERSION`, matching every other section-local-versioned table
/// (`EffectRows`, `AliasTable`, `FrameShapes`) so this section's own
/// encoding can grow without a format-wide bump once it is wired in.
pub const CONVENTIONS_PROJECTION_WIRE_VERSION: u8 = 1;

/// The conventions projection, as it would ride the wire: every
/// `@[convention]` handler declared in the project's one configured
/// conventions module, ascending by `order` — the same shape
/// `brink_ir::ConventionsProjection` carries in-process, with source spans
/// stripped (a `.inkb`-loaded host has no source text to point a range at)
/// and `disposition` intentionally not carried — see
/// [`ConventionEntryDef`]'s own doc for why.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConventionsProjectionDef {
    pub entries: Vec<ConventionEntryDef>,
}

/// One `@[convention]` handler's projected shape on the wire. Mirrors
/// `brink_ir::ConventionProjectionEntry`, with one deliberate exception:
/// there is no `disposition` field here, because every wire entry is,
/// today, the single existing `ElementDisposition::Call` case — this format
/// has no other disposition to distinguish yet, unlike the in-process type,
/// which carries the field explicitly (`ConventionProjectionEntry::disposition`'s
/// own "read what happened, don't infer it from absence" reasoning) so a
/// future second disposition doesn't need a wire bump to become visible
/// in-process. Adding a second disposition to this wire shape is exactly the
/// kind of section-local-version bump
/// [`CONVENTIONS_PROJECTION_WIRE_VERSION`] exists for — until then, this
/// one field's omission is a considered simplification of a
/// currently-single-variant enum, not evidence of a lossy conversion
/// elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionEntryDef {
    pub name: String,
    pub pattern: String,
    pub order: i64,
    pub mode: ConventionModeDef,
    pub attach: Option<ConventionAttachDef>,
}

/// Wire mirror of `brink_ir::ConventionMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConventionModeDef {
    Attach,
    Wrap,
}

/// Wire mirror of `brink_ir::ConventionAttachSchema` — issue #2111 finding
/// 1's resolved field list, carried all the way to the wire shape rather
/// than collapsing back to a bare name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConventionAttachDef {
    Resolved {
        name: String,
        fields: Vec<ConventionAttachFieldDef>,
    },
    Unresolved(String),
}

/// Wire mirror of `brink_ir::ConventionAttachField`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionAttachFieldDef {
    pub name: String,
    pub ty: SchemaTypeDef,
}

/// Wire mirror of `brink_ir::SchemaTypeShape`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaTypeDef {
    Named(String),
    Generic {
        name: String,
        args: Vec<SchemaTypeDef>,
    },
    Fn {
        params: Vec<SchemaTypeDef>,
        ret: Box<SchemaTypeDef>,
    },
}

const TAG_ATTACH: u8 = 0;
const TAG_WRAP: u8 = 1;

const TAG_RESOLVED: u8 = 0;
const TAG_UNRESOLVED: u8 = 1;

const TAG_TYPE_NAMED: u8 = 0;
const TAG_TYPE_GENERIC: u8 = 1;
const TAG_TYPE_FN: u8 = 2;

/// Write the conventions-projection section (no header framing beyond its
/// own section-local version byte, matching [`CONVENTIONS_PROJECTION_WIRE_VERSION`]):
/// entry count, then each entry in the order given. Callers sort/dedupe
/// before calling — this function trusts its input's order, the same
/// posture every writer in this crate takes toward its own table.
#[expect(clippy::cast_possible_truncation)]
pub fn write_conventions_projection(projection: &ConventionsProjectionDef, buf: &mut Vec<u8>) {
    write_u8(buf, CONVENTIONS_PROJECTION_WIRE_VERSION);
    write_u32(buf, projection.entries.len() as u32);
    for entry in &projection.entries {
        write_str(buf, &entry.name);
        write_str(buf, &entry.pattern);
        write_u64(buf, entry.order.cast_unsigned());
        write_u8(
            buf,
            match entry.mode {
                ConventionModeDef::Attach => TAG_ATTACH,
                ConventionModeDef::Wrap => TAG_WRAP,
            },
        );
        match &entry.attach {
            None => write_u8(buf, 0),
            Some(attach) => {
                write_u8(buf, 1);
                write_attach(attach, buf);
            }
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn write_attach(attach: &ConventionAttachDef, buf: &mut Vec<u8>) {
    match attach {
        ConventionAttachDef::Resolved { name, fields } => {
            write_u8(buf, TAG_RESOLVED);
            write_str(buf, name);
            write_u32(buf, fields.len() as u32);
            for field in fields {
                write_str(buf, &field.name);
                write_schema_type(&field.ty, buf);
            }
        }
        ConventionAttachDef::Unresolved(name) => {
            write_u8(buf, TAG_UNRESOLVED);
            write_str(buf, name);
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn write_schema_type(ty: &SchemaTypeDef, buf: &mut Vec<u8>) {
    match ty {
        SchemaTypeDef::Named(name) => {
            write_u8(buf, TAG_TYPE_NAMED);
            write_str(buf, name);
        }
        SchemaTypeDef::Generic { name, args } => {
            write_u8(buf, TAG_TYPE_GENERIC);
            write_str(buf, name);
            write_u32(buf, args.len() as u32);
            for arg in args {
                write_schema_type(arg, buf);
            }
        }
        SchemaTypeDef::Fn { params, ret } => {
            write_u8(buf, TAG_TYPE_FN);
            write_u32(buf, params.len() as u32);
            for param in params {
                write_schema_type(param, buf);
            }
            write_schema_type(ret, buf);
        }
    }
}

/// Read a conventions-projection section written by
/// [`write_conventions_projection`]. `buf`/`offset` are a raw cursor, not an
/// `.inkb`-indexed section range — this codec is not wired into
/// [`crate::inkb::InkbIndex`] yet (see this module's own doc).
pub fn read_conventions_projection(
    buf: &[u8],
    offset: &mut usize,
) -> Result<ConventionsProjectionDef, DecodeError> {
    let section_version = read_u8(buf, offset)?;
    if section_version != CONVENTIONS_PROJECTION_WIRE_VERSION {
        return Err(DecodeError::UnsupportedSectionVersion {
            // No `SectionKind` tag exists for this section yet (see this
            // module's own doc) — `0` stands in as "not yet a real section".
            section: 0,
            version: section_version,
        });
    }
    let count = read_u32(buf, offset)? as usize;
    // Minimum per-entry footprint: two empty strings (4+4) + order (8) +
    // mode (1) + no-attach (1) = 18 bytes.
    let mut entries = Vec::with_capacity(safe_capacity(count, buf.len(), *offset, 18));
    for _ in 0..count {
        let name = read_str(buf, offset)?;
        let pattern = read_str(buf, offset)?;
        let order = read_u64(buf, offset)?.cast_signed();
        let mode = match read_u8(buf, offset)? {
            TAG_ATTACH => ConventionModeDef::Attach,
            TAG_WRAP => ConventionModeDef::Wrap,
            other => return Err(DecodeError::InvalidConventionsProjectionTag(other)),
        };
        let attach = match read_u8(buf, offset)? {
            0 => None,
            1 => Some(read_attach(buf, offset, 0)?),
            other => return Err(DecodeError::InvalidConventionsProjectionTag(other)),
        };
        entries.push(ConventionEntryDef {
            name,
            pattern,
            order,
            mode,
            attach,
        });
    }
    Ok(ConventionsProjectionDef { entries })
}

fn read_attach(
    buf: &[u8],
    offset: &mut usize,
    depth: usize,
) -> Result<ConventionAttachDef, DecodeError> {
    match read_u8(buf, offset)? {
        TAG_RESOLVED => {
            let name = read_str(buf, offset)?;
            let count = read_u32(buf, offset)? as usize;
            // Minimum per-field footprint: empty name (4) + type tag (1) = 5.
            let mut fields = Vec::with_capacity(safe_capacity(count, buf.len(), *offset, 5));
            for _ in 0..count {
                let field_name = read_str(buf, offset)?;
                let ty = read_schema_type(buf, offset, depth)?;
                fields.push(ConventionAttachFieldDef {
                    name: field_name,
                    ty,
                });
            }
            Ok(ConventionAttachDef::Resolved { name, fields })
        }
        TAG_UNRESOLVED => Ok(ConventionAttachDef::Unresolved(read_str(buf, offset)?)),
        other => Err(DecodeError::InvalidConventionsProjectionTag(other)),
    }
}

fn read_schema_type(
    buf: &[u8],
    offset: &mut usize,
    depth: usize,
) -> Result<SchemaTypeDef, DecodeError> {
    if depth >= MAX_DECODE_DEPTH {
        return Err(DecodeError::MaxDepthExceeded(MAX_DECODE_DEPTH));
    }
    match read_u8(buf, offset)? {
        TAG_TYPE_NAMED => Ok(SchemaTypeDef::Named(read_str(buf, offset)?)),
        TAG_TYPE_GENERIC => {
            let name = read_str(buf, offset)?;
            let count = read_u32(buf, offset)? as usize;
            let mut args = Vec::with_capacity(safe_capacity(count, buf.len(), *offset, 1));
            for _ in 0..count {
                args.push(read_schema_type(buf, offset, depth + 1)?);
            }
            Ok(SchemaTypeDef::Generic { name, args })
        }
        TAG_TYPE_FN => {
            let count = read_u32(buf, offset)? as usize;
            let mut params = Vec::with_capacity(safe_capacity(count, buf.len(), *offset, 1));
            for _ in 0..count {
                params.push(read_schema_type(buf, offset, depth + 1)?);
            }
            let ret = Box::new(read_schema_type(buf, offset, depth + 1)?);
            Ok(SchemaTypeDef::Fn { params, ret })
        }
        other => Err(DecodeError::InvalidConventionsProjectionTag(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn sample() -> ConventionsProjectionDef {
        ConventionsProjectionDef {
            entries: vec![
                ConventionEntryDef {
                    name: "cue".to_string(),
                    pattern: "^(?<name>[A-Z]+)$".to_string(),
                    order: 5,
                    mode: ConventionModeDef::Wrap,
                    attach: Some(ConventionAttachDef::Resolved {
                        name: "Cue".to_string(),
                        fields: vec![
                            ConventionAttachFieldDef {
                                name: "speaker".to_string(),
                                ty: SchemaTypeDef::Named("string".to_string()),
                            },
                            ConventionAttachFieldDef {
                                name: "voiceover".to_string(),
                                ty: SchemaTypeDef::Named("bool".to_string()),
                            },
                        ],
                    }),
                },
                ConventionEntryDef {
                    name: "interior".to_string(),
                    pattern: "^INT\\. (?<place>.+)$".to_string(),
                    order: 10,
                    mode: ConventionModeDef::Attach,
                    attach: None,
                },
                ConventionEntryDef {
                    name: "broken".to_string(),
                    pattern: "^X$".to_string(),
                    order: 99,
                    mode: ConventionModeDef::Attach,
                    attach: Some(ConventionAttachDef::Unresolved("Ghost".to_string())),
                },
            ],
        }
    }

    #[test]
    fn round_trips_a_mixed_projection() {
        let projection = sample();
        let mut buf = Vec::new();
        write_conventions_projection(&projection, &mut buf);
        let mut offset = 0;
        let decoded = read_conventions_projection(&buf, &mut offset).expect("decode");
        assert_eq!(decoded, projection);
        assert_eq!(
            offset,
            buf.len(),
            "reader must consume exactly what the writer wrote"
        );
    }

    #[test]
    fn round_trips_an_empty_projection() {
        let projection = ConventionsProjectionDef::default();
        let mut buf = Vec::new();
        write_conventions_projection(&projection, &mut buf);
        let mut offset = 0;
        let decoded = read_conventions_projection(&buf, &mut offset).expect("decode");
        assert_eq!(decoded, projection);
    }

    #[test]
    fn round_trips_a_generic_and_fn_typed_field() {
        let projection = ConventionsProjectionDef {
            entries: vec![ConventionEntryDef {
                name: "handler".to_string(),
                pattern: "^Z$".to_string(),
                order: 1,
                mode: ConventionModeDef::Attach,
                attach: Some(ConventionAttachDef::Resolved {
                    name: "Fancy".to_string(),
                    fields: vec![
                        ConventionAttachFieldDef {
                            name: "items".to_string(),
                            ty: SchemaTypeDef::Generic {
                                name: "List".to_string(),
                                args: vec![SchemaTypeDef::Named("L".to_string())],
                            },
                        },
                        ConventionAttachFieldDef {
                            name: "callback".to_string(),
                            ty: SchemaTypeDef::Fn {
                                params: vec![SchemaTypeDef::Named("int".to_string())],
                                ret: Box::new(SchemaTypeDef::Named("bool".to_string())),
                            },
                        },
                    ],
                }),
            }],
        };
        let mut buf = Vec::new();
        write_conventions_projection(&projection, &mut buf);
        let mut offset = 0;
        let decoded = read_conventions_projection(&buf, &mut offset).expect("decode");
        assert_eq!(decoded, projection);
    }

    /// Rule 20a/mutation-style: an unknown mode tag must be a decode error,
    /// not silently coerced to a valid variant.
    #[test]
    fn unknown_mode_tag_is_rejected() {
        let projection = sample();
        let mut buf = Vec::new();
        write_conventions_projection(&projection, &mut buf);
        // The mode byte for the first entry sits right after: version(1) +
        // count(4) + name(4+len) + pattern(4+len) + order(8). Computed from
        // the actual fixture strings' lengths (not hardcoded) so this stays
        // correct if `sample()` ever changes.
        let first = &projection.entries[0];
        let mode_byte_offset = 1 + 4 + (4 + first.name.len()) + (4 + first.pattern.len()) + 8;
        assert_eq!(buf[mode_byte_offset], TAG_WRAP, "test fixture assumption");
        buf[mode_byte_offset] = 0xFF;
        let mut offset = 0;
        let err = read_conventions_projection(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::InvalidConventionsProjectionTag(0xFF));
    }

    /// Companion to `unknown_mode_tag_is_rejected`: corrupts the byte right
    /// after the mode tag — the attach-presence flag — so the attach-tag
    /// match's `other` arm is exercised independently. Before this pair, a
    /// wrong offset made the "mode" test actually corrupt this byte instead,
    /// leaving the mode match's `other` arm with zero coverage.
    #[test]
    fn unknown_attach_presence_tag_is_rejected() {
        let projection = sample();
        let mut buf = Vec::new();
        write_conventions_projection(&projection, &mut buf);
        let first = &projection.entries[0];
        let mode_byte_offset = 1 + 4 + (4 + first.name.len()) + (4 + first.pattern.len()) + 8;
        let attach_presence_offset = mode_byte_offset + 1;
        assert_eq!(
            buf[attach_presence_offset], 1,
            "test fixture assumption: attach present"
        );
        buf[attach_presence_offset] = 0xFF;
        let mut offset = 0;
        let err = read_conventions_projection(&buf, &mut offset).unwrap_err();
        assert_eq!(err, DecodeError::InvalidConventionsProjectionTag(0xFF));
    }

    /// The section-local version byte is checked independently of decoding
    /// the rest of the buffer — a future version bump must not be silently
    /// misread as today's shape.
    #[test]
    fn unsupported_section_version_is_rejected() {
        let mut buf = Vec::new();
        write_conventions_projection(&ConventionsProjectionDef::default(), &mut buf);
        buf[0] = CONVENTIONS_PROJECTION_WIRE_VERSION + 1;
        let mut offset = 0;
        let err = read_conventions_projection(&buf, &mut offset).unwrap_err();
        assert_eq!(
            err,
            DecodeError::UnsupportedSectionVersion {
                section: 0,
                version: CONVENTIONS_PROJECTION_WIRE_VERSION + 1,
            }
        );
    }
}
