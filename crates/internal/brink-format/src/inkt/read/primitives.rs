//! Primitive token parsers shared across the `.inkt` reader's grammar-rule
//! clusters: hex/decimal scalars, string unescaping, the `def_id` scalar, the
//! `next_rule` skip-to-rule cursor helper, and the `err` error-builder.
//!
//! Pure `mod` extraction (issue #685) from the former monolithic `read.rs` —
//! no logic changes, only the module boundary is new.

use super::{InktParseError, P, Rule};
use crate::id::DefinitionId;

pub(super) fn err(pair: &P<'_>, msg: impl Into<String>) -> InktParseError {
    let (line, col) = pair.line_col();
    InktParseError {
        message: msg.into(),
        line,
        col,
    }
}

#[expect(clippy::needless_pass_by_value)]
pub(super) fn parse_def_id(pair: P<'_>) -> Result<DefinitionId, InktParseError> {
    let s = pair.as_str();
    // Format: $TT_HHHHHHHHHHHHHH
    if !s.starts_with('$') || s.len() < 4 {
        return Err(err(&pair, format!("invalid def_id: {s}")));
    }
    let tag_str = &s[1..3];
    let hash_str = &s[4..]; // skip $TT_

    let tag_byte = u8::from_str_radix(tag_str, 16)
        .map_err(|_| err(&pair, format!("invalid tag: {tag_str}")))?;
    let hash = u64::from_str_radix(hash_str, 16)
        .map_err(|_| err(&pair, format!("invalid hash: {hash_str}")))?;

    let tag = crate::id::DefinitionTag::from_u8(tag_byte)
        .ok_or_else(|| err(&pair, format!("unknown tag byte: {tag_byte:#04x}")))?;

    Ok(DefinitionId::new(tag, hash))
}

pub(super) fn parse_hex_u32(s: &str) -> u32 {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(hex, 16).unwrap_or(0)
}

pub(super) fn parse_hex_u64(s: &str) -> Result<u64, InktParseError> {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(hex, 16).map_err(|_| InktParseError {
        message: format!("invalid hex: {s}"),
        line: 0,
        col: 0,
    })
}

pub(super) fn parse_u16(pair: &P<'_>) -> Result<u16, InktParseError> {
    pair.as_str().parse().map_err(|_| err(pair, "invalid u16"))
}

/// D6 (`docs/debugger-spec.md` §2.2): the `DebugInfo` entry table's fields
/// (`bytecode_offset`, `file_idx`, `range_start`, `range_len`, `slot`
/// declaring ranges) are all `u32`-domain, wider than the existing
/// [`parse_u16`].
pub(super) fn parse_u32(pair: &P<'_>) -> Result<u32, InktParseError> {
    pair.as_str().parse().map_err(|_| err(pair, "invalid u32"))
}

pub(super) fn parse_u8(pair: &P<'_>) -> Result<u8, InktParseError> {
    pair.as_str().parse().map_err(|_| err(pair, "invalid u8"))
}

pub(super) fn unescape_string(s: &str) -> String {
    // Strip surrounding quotes
    let inner = &s[1..s.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') | None => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub(super) fn next_rule<'a>(
    iter: &mut impl Iterator<Item = P<'a>>,
    expected: Rule,
    context: &str,
) -> Result<P<'a>, InktParseError> {
    for pair in iter.by_ref() {
        if pair.as_rule() == expected {
            return Ok(pair);
        }
    }
    Err(InktParseError {
        message: format!("expected {expected:?} in {context}"),
        line: 0,
        col: 0,
    })
}
