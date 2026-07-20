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

    while !p.at_eof() {
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
