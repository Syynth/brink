use rowan::{TextRange, TextSize};

/// Extract the identifier word surrounding `offset` in `source`.
pub fn word_at_offset(source: &str, offset: TextSize) -> Option<&str> {
    let pos: usize = offset.into();
    if pos >= source.len() {
        return None;
    }
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    // The cursor must be on a word character
    if !is_word(bytes[pos]) {
        return None;
    }
    let mut start = pos;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = pos + 1;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    Some(&source[start..end])
}

/// Like `word_at_offset` but returns the `TextRange` of the word.
pub fn word_range_at_offset(source: &str, offset: TextSize) -> Option<TextRange> {
    let pos: usize = offset.into();
    if pos >= source.len() {
        return None;
    }
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    if !is_word(bytes[pos]) {
        return None;
    }
    let mut start = pos;
    while start > 0 && is_word(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = pos + 1;
    while end < bytes.len() && is_word(bytes[end]) {
        end += 1;
    }
    let start = u32::try_from(start).ok()?;
    let end = u32::try_from(end).ok()?;
    Some(TextRange::new(TextSize::from(start), TextSize::from(end)))
}

/// Return hover markdown for an ink built-in function, or `None` if not a builtin.
pub fn builtin_hover_text(name: &str) -> Option<String> {
    let (signature, description) = match name {
        "CHOICE_COUNT" => ("CHOICE_COUNT()", "Number of currently available choices"),
        "TURNS_SINCE" => (
            "TURNS_SINCE(-> knot)",
            "Turns since a knot was last visited (-1 if never)",
        ),
        "READ_COUNT" => (
            "READ_COUNT(-> knot)",
            "Number of times a knot has been visited",
        ),
        "RANDOM" => (
            "RANDOM(min, max)",
            "Random integer between min and max (inclusive)",
        ),
        "SEED_RANDOM" => ("SEED_RANDOM(seed)", "Seed the random number generator"),
        "INT" => ("INT(value)", "Cast to integer"),
        "FLOAT" => ("FLOAT(value)", "Cast to float"),
        "FLOOR" => ("FLOOR(value)", "Round down to nearest integer"),
        "CEILING" => ("CEILING(value)", "Round up to nearest integer"),
        "POW" => ("POW(base, exp)", "Raise base to the power of exp"),
        "MIN" => ("MIN(a, b)", "Minimum of two values"),
        "MAX" => ("MAX(a, b)", "Maximum of two values"),
        "LIST_COUNT" => ("LIST_COUNT(list)", "Number of items in a list value"),
        "LIST_MIN" => ("LIST_MIN(list)", "Lowest-valued item in a list"),
        "LIST_MAX" => ("LIST_MAX(list)", "Highest-valued item in a list"),
        "LIST_ALL" => ("LIST_ALL(list)", "All possible items for a list's type"),
        "LIST_INVERT" => ("LIST_INVERT(list)", "Items not in the list (from its type)"),
        "LIST_RANGE" => (
            "LIST_RANGE(list, min, max)",
            "Items in list between min and max",
        ),
        "LIST_RANDOM" => ("LIST_RANDOM(list)", "Random item from a list"),
        "LIST_VALUE" => ("LIST_VALUE(item)", "Numeric value of a list item"),
        "LIST_FROM_INT" => (
            "LIST_FROM_INT(list, n)",
            "Item at numeric position n in a list type",
        ),
        _ => return None,
    };
    Some(format!("**built-in** `{signature}`\n\n{description}"))
}

/// Return hover markdown for a T1b stdlib slice 1 function
/// (docs/t1b-surface-spec.md §5 — `len`/`keys`/`values`/`contains`/`push`/
/// `insert`/`remove`), or `None` if `name` isn't one.
///
/// Dialect-agnostic by design, like [`builtin_hover_text`]: hovering a name
/// is informational even in a strict-ink file, where a resolved use of the
/// name would be flagged `E051` ("brink extension") — the hover explains
/// what the name means and that it needs the brink dialect, rather than
/// staying silent. Callers that want dialect-gated *offering* of these names
/// (completion, signature help) consult `brink_analyzer::Dialect` themselves;
/// this function never does.
#[must_use]
pub fn stdlib_hover_text(name: &str) -> Option<String> {
    let f = crate::stdlib::stdlib_fn(name)?;
    let note = if f.is_mutator() {
        "brink dialect only — mutates its first argument in place; statement position only"
    } else {
        "brink dialect only"
    };
    Some(format!(
        "**brink stdlib** `{}`\n\n{}\n\n*({note})*",
        f.signature_label(),
        f.doc,
    ))
}

/// Find the function call context at the given byte offset.
///
/// Returns `(function_name, active_parameter_index)` if the cursor is inside
/// a function call's parentheses, e.g. `myFunc(a, |)` → `("myFunc", 1)`.
pub fn find_call_context(source: &str, byte_offset: usize) -> Option<(String, usize)> {
    let before = source.get(..byte_offset)?;

    // Scan backwards to find the matching open paren, tracking nesting.
    let mut depth = 0i32;
    let mut commas = 0usize;
    let mut paren_pos = None;

    for (i, ch) in before.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    paren_pos = Some(i);
                    break;
                }
                depth -= 1;
            }
            ',' if depth == 0 => commas += 1,
            '\n' if depth == 0 => return None, // don't cross line boundaries at depth 0
            _ => {}
        }
    }

    let paren_pos = paren_pos?;

    // Extract the identifier immediately before the open paren.
    let before_paren = before[..paren_pos].trim_end();
    if before_paren.is_empty() {
        return None;
    }

    // Walk backwards over identifier characters.
    let name_end = before_paren.len();
    let name_start = before_paren
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
        .last()
        .map_or(name_end, |(i, _)| i);

    let name = &before_paren[name_start..name_end];
    if name.is_empty() {
        return None;
    }

    Some((name.to_owned(), commas))
}

/// Compute a whole-document replacement edit.
///
/// Returns a `Vec` containing a single `(range, new_text)` pair that replaces
/// the entire old document with the new text. The LSP adapter converts to
/// `tower_lsp::lsp_types::TextEdit` using `LineIndex`.
pub fn diff_to_edits(old: &str, new: &str) -> Vec<(TextRange, String)> {
    let len = u32::try_from(old.len()).unwrap_or(u32::MAX);
    let range = TextRange::new(TextSize::from(0), TextSize::from(len));
    vec![(range, new.to_owned())]
}

/// Byte offset where a declaration's *ownership* starts: the start of the
/// contiguous `///` doc-comment block immediately above the declaration at
/// `decl_start`, or `decl_start` itself if there is none.
///
/// Mirrors the attachment rule of `parse_doc_comment` in brink-ir: contiguous
/// `///` lines directly above the declaration, broken by a blank line or a
/// plain `//` comment. Per the decision log, a doc block is structurally part
/// of its declaration — folding, structural moves, and view slices use this
/// to keep them together.
#[must_use]
pub fn doc_extended_start(source: &str, decl_start: usize) -> usize {
    let decl_start = decl_start.min(source.len());
    // Start of the line containing the declaration.
    let mut line_start = source[..decl_start].rfind('\n').map_or(0, |i| i + 1);
    let mut extended = None;
    while line_start > 0 {
        let prev_newline = line_start - 1;
        let prev_start = source[..prev_newline].rfind('\n').map_or(0, |i| i + 1);
        if source[prev_start..prev_newline]
            .trim_start()
            .starts_with("///")
        {
            extended = Some(prev_start);
            line_start = prev_start;
        } else {
            break;
        }
    }
    extended.unwrap_or(decl_start)
}

#[cfg(test)]
mod doc_extended_start_tests {
    use super::doc_extended_start;

    #[test]
    fn extends_over_contiguous_doc_block() {
        let src = "text\n/// one\n/// two\n=== k ===\n";
        let decl = src.find("=== k ===").expect("decl");
        assert_eq!(
            doc_extended_start(src, decl),
            src.find("/// one").expect("doc")
        );
    }

    #[test]
    fn no_docs_returns_decl_start() {
        let src = "text\n=== k ===\n";
        let decl = src.find("=== k ===").expect("decl");
        assert_eq!(doc_extended_start(src, decl), decl);
    }

    #[test]
    fn blank_line_breaks_attachment() {
        let src = "/// orphan\n\n=== k ===\n";
        let decl = src.find("=== k ===").expect("decl");
        assert_eq!(doc_extended_start(src, decl), decl);
    }

    #[test]
    fn plain_comment_breaks_attachment() {
        let src = "/// kept\n// plain\n=== k ===\n";
        let decl = src.find("=== k ===").expect("decl");
        assert_eq!(doc_extended_start(src, decl), decl);
    }

    #[test]
    fn doc_block_at_file_start() {
        let src = "/// top\n=== k ===\n";
        let decl = src.find("=== k ===").expect("decl");
        assert_eq!(doc_extended_start(src, decl), 0);
    }

    #[test]
    fn indented_doc_lines_attach() {
        let src = "=== k ===\n  /// indented\n= s\n";
        let decl = src.find("= s").expect("decl");
        assert_eq!(
            doc_extended_start(src, decl),
            src.find("  ///").expect("doc")
        );
    }
}
