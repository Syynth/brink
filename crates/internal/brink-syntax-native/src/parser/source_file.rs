use crate::SyntaxKind::SOURCE_FILE;

use super::Parser;

/// Parse the entire source file.
///
/// ```text
/// source_file = { item* }
/// ```
///
/// `item` is the same dispatcher `BLOCK` bodies use (`super::block::item`)
/// — the charter's "no one-flow-per-file constraint" (§4) means a file is
/// just a top-level, brace-less body: many declarations plus (for the
/// code-bodied-flow-at-top-of-file edge case, and for robustness under
/// error recovery) the same body-line grammar a block accepts.
pub(crate) fn source_file(p: &mut Parser<'_, '_>) {
    p.start_node(SOURCE_FILE);

    // `loop { skip_ws(); if at_eof() { break } ... }` — not
    // `while !p.at_eof() { skip_ws(); ... }`. The distinction matters:
    // `at_eof()` trivia-skips to decide "any real token left", so a
    // `while` guard checked *before* the loop body's own `skip_ws()` can
    // see EOF and exit without ever having called it — dropping trailing
    // trivia (a final `//` comment, trailing whitespace) that never gets a
    // chance to become a child of `SOURCE_FILE` at all. Caught by
    // `proptest_native`'s `arbitrary_garbage_never_panics` (`"#//"` lost
    // its trailing `//`) and `truncated_input_never_panics_and_roundtrips`
    // (a truncated `flow a_a_() ` lost its trailing space). Mirrors
    // `block::braced_item_list`'s loop shape, which was already safe.
    loop {
        p.skip_ws();
        if p.at_eof() {
            break;
        }

        let before = p.pos();
        super::block::item(p);
        if p.pos() == before {
            // No progress — skip the stuck token to avoid an infinite loop.
            p.error_recover("unexpected token at top level");
        }
    }

    p.finish_node();
}
