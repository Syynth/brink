//! Lexer-driven file segmentation for per-knot incremental lowering
//! (issue #3084, `docs/per-knot-incremental-lowering-spec.md` §3 step 1).
//!
//! [`segment_file`] splits an ink source file into a leading header
//! segment plus one segment per top-level knot (and per top-level stitch
//! before the first knot — the ones lowering promotes to knots). Segments
//! **tile** the file: every byte belongs to exactly one segment, in
//! source order, so downstream assembly can rebase per-segment output by
//! each segment's current offset.
//!
//! The boundary decision mirrors the parser's dispatch rule exactly — it
//! must, because per-segment parse output has to match the corresponding
//! subtree of the whole-file parse byte-for-byte:
//!
//! - A header starts at a **dispatch point**: the file start, or after a
//!   `NEWLINE` at interpolation-brace depth zero, with trivia
//!   (whitespace, `//`, `/* … */`) skipped. This is `source_file`'s /
//!   `knot_body`'s loop shape: a `==` inside a prose line, a string, or a
//!   `/* … */` block comment (including a multi-line or unterminated one
//!   — the lexer scans those to `*/` or EOF as one token) never splits.
//! - Brace depth tracks `{`/`}` tokens because a multiline block's inner
//!   lines are consumed inside one statement — the parser reaches no
//!   dispatch point there, and its inner loops do not break on knot
//!   headers, so an unterminated `{` swallowing the rest of the file is
//!   mirrored here by depth never returning to zero.
//! - `EQ_EQ` at a dispatch point opens a knot (`at_knot`); a single `EQ`
//!   whose next non-trivia token is neither `EQ` nor `GT` opens a
//!   top-level stitch (`at_stitch`) — only until the first knot, after
//!   which stitch headers are internal to their knot's segment.
//! - A segment's range extends **backward over the contiguous `///`
//!   doc-comment block** preceding its header, mirroring
//!   `collect_doc_lines` in `brink-ir`'s lowering (walk back over
//!   whitespace and newlines, attach `///` line comments, break on a
//!   blank-line gap of two newlines, a plain `//` comment, or any other
//!   token): a doc block is structurally part of the declaration it
//!   precedes, so it must travel with the knot's segment.

use rowan::{TextRange, TextSize};

use crate::SyntaxKind;
use crate::lexer::lex;

/// What a [`Segment`] covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentKind {
    /// Everything before the first knot/stitch header: declarations,
    /// includes, root weave content. Always present (possibly empty).
    Header,
    /// One top-level knot (`== name …`), doc block included.
    Knot,
    /// One top-level stitch (`= name`, before any knot), doc block
    /// included — lowering promotes these to knots.
    TopLevelStitch,
}

/// One contiguous slice of the file. Produced by [`segment_file`];
/// segments tile the file in source order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Segment {
    pub kind: SegmentKind,
    /// The byte range this segment covers, doc-block extension included.
    /// Ranges TILE the file (offset bookkeeping); the text a consumer
    /// should PARSE is [`lowered_range`](Self::lowered_range).
    pub range: TextRange,
    /// The range to parse for this segment: `range` extended through the
    /// trailing trivia up to the NEXT segment's header token (or EOF).
    /// The whole-file parse absorbs the trivia before a knot header —
    /// blank lines, comments, and the next knot's `///` doc block — into
    /// the PRECEDING knot's node, so a fragment must end at the next
    /// header for its node ranges to match the whole-file tree
    /// byte-for-byte. Where the next boundary carries a doc block, this
    /// makes adjacent `lowered_range`s OVERLAP on the doc bytes: the doc
    /// block is trailing trivia to this segment and doc attachment to the
    /// next — both readings are what the whole-file parse does.
    pub lowered_range: TextRange,
    /// The byte offset of the header's first `=` token — the position the
    /// whole-file parse gives the corresponding `KNOT_HEADER`/
    /// `STITCH_HEADER` node. `None` for the header segment.
    pub header_start: Option<TextSize>,
}

/// Split `source` into a header segment plus one segment per top-level
/// knot / top-level stitch. See the module doc for the boundary rules.
#[must_use]
pub fn segment_file(source: &str) -> Vec<Segment> {
    let tokens = lex(source);

    // Token start offsets (the lexer is lossless: slices tile the file).
    let mut starts: Vec<TextSize> = Vec::with_capacity(tokens.len());
    let mut pos = TextSize::from(0);
    for (_, text) in &tokens {
        starts.push(pos);
        pos += TextSize::of(*text);
    }
    let total = pos;

    // (cut, header_start, kind) per boundary, in source order.
    let mut boundaries: Vec<(TextSize, TextSize, SegmentKind)> = Vec::new();
    let mut brace_depth: u32 = 0;
    let mut at_dispatch = true;
    let mut seen_knot = false;

    let mut i = 0;
    while i < tokens.len() {
        let kind = tokens[i].0;
        if at_dispatch {
            match kind {
                _ if kind.is_trivia() => {
                    i += 1;
                    continue;
                }
                // An empty line: consume and stay at the dispatch point.
                SyntaxKind::NEWLINE => {
                    i += 1;
                    continue;
                }
                SyntaxKind::EQ_EQ => {
                    boundaries.push((
                        doc_extended_start(&tokens, &starts, i),
                        starts[i],
                        SegmentKind::Knot,
                    ));
                    seen_knot = true;
                    at_dispatch = false;
                    i += 1;
                    continue;
                }
                SyntaxKind::EQ if !seen_knot && at_stitch_lookahead(&tokens, i) => {
                    boundaries.push((
                        doc_extended_start(&tokens, &starts, i),
                        starts[i],
                        SegmentKind::TopLevelStitch,
                    ));
                    at_dispatch = false;
                    i += 1;
                    continue;
                }
                // Any other token starts an ordinary statement/line —
                // fall through to the normal (non-dispatch) handling of
                // this same token.
                _ => at_dispatch = false,
            }
        }
        match kind {
            SyntaxKind::L_BRACE => brace_depth += 1,
            SyntaxKind::R_BRACE => brace_depth = brace_depth.saturating_sub(1),
            SyntaxKind::NEWLINE if brace_depth == 0 => at_dispatch = true,
            _ => {}
        }
        i += 1;
    }

    // Assemble tiling segments.
    let first_cut = boundaries.first().map_or(total, |b| b.0);
    let first_header = boundaries.first().map_or(total, |b| b.1);
    let mut segments = Vec::with_capacity(boundaries.len() + 1);
    segments.push(Segment {
        kind: SegmentKind::Header,
        range: TextRange::new(TextSize::from(0), first_cut),
        lowered_range: TextRange::new(TextSize::from(0), first_header),
        header_start: None,
    });
    for (idx, &(cut, header_start, kind)) in boundaries.iter().enumerate() {
        let end = boundaries.get(idx + 1).map_or(total, |b| b.0);
        let lowered_end = boundaries.get(idx + 1).map_or(total, |b| b.1);
        segments.push(Segment {
            kind,
            range: TextRange::new(cut, end),
            lowered_range: TextRange::new(cut, lowered_end),
            header_start: Some(header_start),
        });
    }
    segments
}

/// The `at_stitch` mirror: the next non-trivia token after the `EQ` is
/// neither `EQ` (a `= =` run is not a stitch) nor `GT` (`=>`).
fn at_stitch_lookahead(tokens: &[(SyntaxKind, &str)], eq_idx: usize) -> bool {
    let mut j = eq_idx + 1;
    while j < tokens.len() && tokens[j].0.is_trivia() {
        j += 1;
    }
    !matches!(
        tokens.get(j).map(|t| t.0),
        Some(SyntaxKind::EQ | SyntaxKind::GT)
    )
}

/// Extend a header boundary backward over the contiguous `///` doc block
/// preceding it — the exact walk `collect_doc_lines` performs during
/// lowering (skip whitespace; count newlines, two in a row is a blank
/// line and ends the block; a `///` line comment attaches and resets the
/// newline count; a plain `//` comment or any other token ends the walk).
/// Returns the cut position: the start of the earliest attached `///`
/// token, or the header token's own start when no doc block precedes it.
fn doc_extended_start(
    tokens: &[(SyntaxKind, &str)],
    starts: &[TextSize],
    header_idx: usize,
) -> TextSize {
    let mut cut = starts[header_idx];
    let mut newlines = 0u32;
    let mut j = header_idx;
    while j > 0 {
        j -= 1;
        match tokens[j].0 {
            SyntaxKind::WHITESPACE => {}
            SyntaxKind::NEWLINE => {
                newlines += 1;
                if newlines >= 2 {
                    break;
                }
            }
            SyntaxKind::LINE_COMMENT if tokens[j].1.starts_with("///") => {
                newlines = 0;
                cut = starts[j];
            }
            _ => break,
        }
    }
    cut
}

#[cfg(test)]
mod tests;
