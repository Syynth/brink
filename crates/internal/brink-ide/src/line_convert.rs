//! Line type conversion logic for the editor.
//!
//! `convert_element()` produces a byte-range text edit that transforms a line
//! from one structural type to another (e.g. narrative → gather → choice).
//! This replaces the regex-based sigil manipulation in TypeScript.

use brink_ir::HirFile;
use brink_syntax::SyntaxNode;
use serde::Serialize;

use crate::line_context::{self, LineContext, LineElement};

// ── Public types ────────────────────────────────────────────────────

/// What to convert a line to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertTarget {
    /// Strip all sigils — plain narrative text.
    Narrative,
    /// Convert to a choice line (`*` or `+` if sticky).
    Choice { sticky: bool },
    /// Convert to a gather line (`-`).
    Gather,
    /// Convert to indented body text (strip sigils, indent to depth).
    ChoiceBody,
}

/// A text edit: replace bytes `from..to` with `insert`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextEdit {
    pub from: u32,
    pub to: u32,
    pub insert: String,
}

// ── Public API ──────────────────────────────────────────────────────

/// Compute a text edit to convert the line at `byte_offset` to `target`.
///
/// Returns `None` if the conversion doesn't make sense (e.g. converting a
/// knot header, or converting a choice to a choice).
pub fn convert_element(
    source: &str,
    hir: &HirFile,
    root: &SyntaxNode,
    byte_offset: u32,
    target: ConvertTarget,
) -> Option<TextEdit> {
    let contexts = line_context::line_contexts(hir, source, root);
    let (line_idx, line_start, line_text) = line_at_offset(source, byte_offset as usize)?;

    let ctx = contexts.get(line_idx)?;

    // Don't convert structural headers or non-weave elements
    if !is_convertible(ctx) {
        return None;
    }

    let prefix_end = sigil_prefix_end(line_text);
    let depth = effective_depth(ctx, target);

    let new_prefix = match target {
        ConvertTarget::Narrative => String::new(),
        ConvertTarget::Choice { sticky } => {
            let sigil = if sticky { "+" } else { "*" };
            build_sigils(sigil, depth)
        }
        ConvertTarget::Gather => build_sigils("-", depth),
        ConvertTarget::ChoiceBody => "  ".repeat(depth as usize),
    };

    #[expect(clippy::cast_possible_truncation)]
    Some(TextEdit {
        from: line_start as u32,
        to: (line_start + prefix_end) as u32,
        insert: new_prefix,
    })
}

// ── Internals ───────────────────────────────────────────────────────

/// Find the line containing `offset`. Returns (index, start byte, text).
fn line_at_offset(source: &str, offset: usize) -> Option<(usize, usize, &str)> {
    let offset = offset.min(source.len());
    let mut line_start = 0;
    let lines: Vec<&str> = source.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        let line_end = line_start + line.len();
        // Offset is within this line, or this is the last line
        if offset <= line_end || i == lines.len() - 1 {
            return Some((i, line_start, line));
        }
        line_start = line_end + 1; // +1 for the '\n'
    }
    None
}

/// Whether a line's element type is convertible.
fn is_convertible(ctx: &LineContext) -> bool {
    matches!(
        ctx.element,
        LineElement::Narrative | LineElement::Choice | LineElement::Gather | LineElement::Blank
    )
}

/// Find the byte length of the sigil prefix on a line (leading whitespace + sigil chars + spaces).
///
/// Detects sigil prefixes from the text itself rather than relying solely on
/// the HIR element type, since the HIR may classify gather-continuation content
/// as narrative even though the line starts with `-`.
fn sigil_prefix_end(text: &str) -> usize {
    let bytes = text.as_bytes();
    let len = bytes.len();

    // Skip leading whitespace
    let mut pos = 0;
    while pos < len && bytes[pos] == b' ' {
        pos += 1;
    }

    let ws_end = pos;

    // Try choice sigils: `*` or `+` separated by spaces
    if pos < len && (bytes[pos] == b'*' || bytes[pos] == b'+') {
        while pos < len && (bytes[pos] == b'*' || bytes[pos] == b'+') {
            pos += 1;
            while pos < len && bytes[pos] == b' ' {
                pos += 1;
            }
        }
        return pos;
    }

    // Try gather sigils: `-` separated by spaces (but not `->`)
    if pos < len && bytes[pos] == b'-' && !(pos + 1 < len && bytes[pos + 1] == b'>') {
        while pos < len && bytes[pos] == b'-' {
            if pos + 1 < len && bytes[pos + 1] == b'>' {
                break;
            }
            pos += 1;
            while pos < len && bytes[pos] == b' ' {
                pos += 1;
            }
        }
        return pos;
    }

    // No sigils — prefix is just whitespace
    ws_end
}

/// Determine the effective depth for the conversion target.
fn effective_depth(ctx: &LineContext, target: ConvertTarget) -> u32 {
    match target {
        ConvertTarget::Narrative => 0,
        ConvertTarget::Choice { .. } | ConvertTarget::Gather => {
            // Preserve depth if we have one, otherwise default to 1
            if ctx.weave.depth > 0 {
                ctx.weave.depth
            } else {
                1
            }
        }
        ConvertTarget::ChoiceBody => {
            // Indent to the current weave depth
            if ctx.weave.depth > 0 {
                ctx.weave.depth
            } else {
                1
            }
        }
    }
}

/// Build a sigil prefix like `"* * "` for depth=2 with sigil `"*"`.
fn build_sigils(sigil: &str, depth: u32) -> String {
    let parts: Vec<&str> = (0..depth).map(|_| sigil).collect();
    let mut result = parts.join(" ");
    if !result.is_empty() {
        result.push(' ');
    }
    result
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::{FileId, hir};

    fn do_convert(source: &str, offset: u32, target: ConvertTarget) -> Option<TextEdit> {
        let parse = brink_syntax::parse(source);
        let file_id = FileId(0);
        let ast = parse.tree();
        let (hir, _, _) = hir::lower(file_id, &ast);
        convert_element(source, &hir, &parse.syntax(), offset, target)
    }

    fn apply_edit(source: &str, edit: &TextEdit) -> String {
        let mut result = String::new();
        result.push_str(&source[..edit.from as usize]);
        result.push_str(&edit.insert);
        result.push_str(&source[edit.to as usize..]);
        result
    }

    #[test]
    fn narrative_to_gather() {
        let source = "=== start ===\nHello world\n";
        let edit = do_convert(source, 15, ConvertTarget::Gather).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\n- Hello world\n");
    }

    #[test]
    fn narrative_to_choice() {
        let source = "=== start ===\nHello world\n";
        let edit = do_convert(source, 15, ConvertTarget::Choice { sticky: false }).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\n* Hello world\n");
    }

    #[test]
    fn narrative_to_sticky_choice() {
        let source = "=== start ===\nHello world\n";
        let edit = do_convert(source, 15, ConvertTarget::Choice { sticky: true }).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\n+ Hello world\n");
    }

    #[test]
    fn choice_to_gather() {
        let source = "=== start ===\n* Choice one\n";
        let edit = do_convert(source, 15, ConvertTarget::Gather).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\n- Choice one\n");
    }

    #[test]
    fn gather_to_choice() {
        let source = "=== start ===\n* Choice one\n- Gather here\n";
        let edit = do_convert(source, 28, ConvertTarget::Choice { sticky: false }).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\n* Choice one\n* Gather here\n");
    }

    #[test]
    fn choice_to_narrative() {
        let source = "=== start ===\n* Choice one\n";
        let edit = do_convert(source, 15, ConvertTarget::Narrative).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\nChoice one\n");
    }

    #[test]
    fn choice_to_body() {
        let source = "=== start ===\n* Choice one\n";
        let edit = do_convert(source, 15, ConvertTarget::ChoiceBody).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\n  Choice one\n");
    }

    #[test]
    fn knot_header_not_convertible() {
        let source = "=== start ===\nHello\n";
        let edit = do_convert(source, 0, ConvertTarget::Gather);
        assert!(edit.is_none());
    }

    #[test]
    fn deep_choice_preserves_depth() {
        let source = "=== start ===\n* Outer\n* * Inner\n";
        // Offset into the "* * Inner" line
        let edit = do_convert(source, 23, ConvertTarget::Gather).unwrap();
        let result = apply_edit(source, &edit);
        assert_eq!(result, "=== start ===\n* Outer\n- - Inner\n");
    }

    #[test]
    fn build_sigils_examples() {
        assert_eq!(build_sigils("*", 1), "* ");
        assert_eq!(build_sigils("*", 2), "* * ");
        assert_eq!(build_sigils("-", 3), "- - - ");
        assert_eq!(build_sigils("*", 0), "");
    }
}
