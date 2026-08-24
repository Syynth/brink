use super::{Segment, SegmentKind, segment_file};
use rowan::TextSize;

/// Every segmentation must tile the file: in-order, gap-free, exactly
/// covering `source`.
fn assert_tiles(source: &str, segments: &[Segment]) {
    let mut pos = TextSize::from(0);
    for seg in segments {
        assert_eq!(
            seg.range.start(),
            pos,
            "segment {seg:?} does not start where the previous one ended"
        );
        pos = seg.range.end();
    }
    assert_eq!(
        pos,
        TextSize::of(source),
        "segments do not cover the whole file"
    );
}

fn kinds(segments: &[Segment]) -> Vec<SegmentKind> {
    segments.iter().map(|s| s.kind).collect()
}

fn seg_text<'s>(source: &'s str, seg: &Segment) -> &'s str {
    &source[seg.range]
}

#[test]
fn empty_file_is_one_empty_header_segment() {
    let segs = segment_file("");
    assert_tiles("", &segs);
    assert_eq!(kinds(&segs), vec![SegmentKind::Header]);
}

#[test]
fn header_plus_two_knots() {
    let src = "VAR x = 1\nSome intro prose.\n== alpha ==\nBody a.\n=== beta ===\nBody b.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(
        kinds(&segs),
        vec![SegmentKind::Header, SegmentKind::Knot, SegmentKind::Knot]
    );
    assert_eq!(seg_text(src, &segs[0]), "VAR x = 1\nSome intro prose.\n");
    assert_eq!(seg_text(src, &segs[1]), "== alpha ==\nBody a.\n");
    assert_eq!(seg_text(src, &segs[2]), "=== beta ===\nBody b.\n");
}

#[test]
fn top_level_stitch_before_first_knot_gets_its_own_segment() {
    let src = "Intro.\n= lobby\nStitch content.\n== alpha ==\nBody.\n= inner\nInner stitch.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    // `= inner` comes after a knot — internal to alpha's segment.
    assert_eq!(
        kinds(&segs),
        vec![
            SegmentKind::Header,
            SegmentKind::TopLevelStitch,
            SegmentKind::Knot
        ]
    );
    assert_eq!(seg_text(src, &segs[1]), "= lobby\nStitch content.\n");
    assert!(seg_text(src, &segs[2]).contains("= inner\n"));
}

#[test]
fn doc_block_travels_with_its_knot() {
    let src = "Intro.\n/// Greets the player.\n/// @kind scene\n== alpha ==\nBody.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(
        seg_text(src, &segs[1]),
        "/// Greets the player.\n/// @kind scene\n== alpha ==\nBody.\n"
    );
    // The header itself is still reported at the `==` token.
    assert_eq!(
        segs[1].header_start,
        Some(TextSize::of(
            "Intro.\n/// Greets the player.\n/// @kind scene\n"
        ))
    );
}

#[test]
fn blank_line_breaks_the_doc_block() {
    let src = "/// Orphaned doc.\n\n== alpha ==\nBody.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    // The blank line (two newlines) severs the block: the doc stays in
    // the header segment, mirroring `collect_doc_lines`.
    assert_eq!(seg_text(src, &segs[0]), "/// Orphaned doc.\n\n");
    assert_eq!(seg_text(src, &segs[1]), "== alpha ==\nBody.\n");
}

#[test]
fn plain_comment_breaks_the_doc_block() {
    let src = "/// Doc line.\n// plain comment\n== alpha ==\nBody.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    // A `//` between the `///` block and the header severs it.
    assert_eq!(seg_text(src, &segs[1]), "== alpha ==\nBody.\n");
}

#[test]
fn doc_block_between_knots_belongs_to_the_second() {
    let src = "== alpha ==\nBody a.\n/// About beta.\n== beta ==\nBody b.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(seg_text(src, &segs[1]), "== alpha ==\nBody a.\n");
    assert_eq!(
        seg_text(src, &segs[2]),
        "/// About beta.\n== beta ==\nBody b.\n"
    );
}

#[test]
fn header_inside_block_comment_does_not_split() {
    let src = "Intro.\n/* commented out\n== ghost ==\nnot real */\n== alpha ==\nBody.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(kinds(&segs), vec![SegmentKind::Header, SegmentKind::Knot]);
    assert_eq!(seg_text(src, &segs[1]), "== alpha ==\nBody.\n");
}

#[test]
fn unterminated_block_comment_swallows_the_rest() {
    let src = "== alpha ==\nBody.\n/* never closed\n== ghost ==\nmore\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    // The lexer scans the unterminated `/*` to EOF as one token, exactly
    // as the whole-file parse sees it — no boundary after it.
    assert_eq!(kinds(&segs), vec![SegmentKind::Header, SegmentKind::Knot]);
}

#[test]
fn eq_eq_inside_a_prose_line_does_not_split() {
    let src = "The score == high today.\n== alpha ==\nBody with x == y in it.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(kinds(&segs), vec![SegmentKind::Header, SegmentKind::Knot]);
}

#[test]
fn multiline_block_lines_are_not_dispatch_points() {
    // The parser consumes a `{ … }` multiline block inside one statement
    // and its inner loops do not break on knot headers — a header-shaped
    // line inside the braces must not split.
    let src = "== alpha ==\n{ x:\n== not_a_knot ==\n- else:\nother\n}\n== beta ==\nBody.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(
        kinds(&segs),
        vec![SegmentKind::Header, SegmentKind::Knot, SegmentKind::Knot]
    );
    assert!(seg_text(src, &segs[1]).contains("not_a_knot"));
    assert_eq!(seg_text(src, &segs[2]), "== beta ==\nBody.\n");
}

#[test]
fn divert_arrow_and_eq_runs_are_not_stitches() {
    // `=>` and `= =` at a line start are not stitch headers (`at_stitch`
    // excludes a following `GT`/`EQ`); a lone `= name` is.
    let src = "=> somewhere\n= = odd\n= lobby\nContent.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(
        kinds(&segs),
        vec![SegmentKind::Header, SegmentKind::TopLevelStitch]
    );
    assert_eq!(seg_text(src, &segs[1]), "= lobby\nContent.\n");
}

#[test]
fn function_knot_is_a_knot_segment() {
    let src = "Intro.\n=== function add(a, b) ===\n~ return a + b\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(kinds(&segs), vec![SegmentKind::Header, SegmentKind::Knot]);
}

/// The tricky fixtures above assert the segmenter's own answer; this pins
/// that answer against the PARSER's — the same parity the corpus sweep
/// (`brink-test-harness/tests/segmenter_corpus_agreement.rs`) checks
/// repo-wide, applied to shapes the corpus may not contain.
#[test]
fn parity_with_parser_on_tricky_fixtures() {
    use crate::SyntaxKind;
    let fixtures = [
        "VAR x = 1\nSome intro prose.\n== alpha ==\nBody a.\n=== beta ===\nBody b.\n",
        "Intro.\n= lobby\nStitch content.\n== alpha ==\nBody.\n= inner\nInner stitch.\n",
        "Intro.\n/// Greets the player.\n/// @kind scene\n== alpha ==\nBody.\n",
        "/// Orphaned doc.\n\n== alpha ==\nBody.\n",
        "Intro.\n/* commented out\n== ghost ==\nnot real */\n== alpha ==\nBody.\n",
        "== alpha ==\nBody.\n/* never closed\n== ghost ==\nmore\n",
        "The score == high today.\n== alpha ==\nBody with x == y in it.\n",
        "== alpha ==\n{ x:\n== not_a_knot ==\n- else:\nother\n}\n== beta ==\nBody.\n",
        "=> somewhere\n= = odd\n= lobby\nContent.\n",
        "Intro.\n=== function add(a, b) ===\n~ return a + b\n",
        "  == indented ==\nBody.\n",
        "{ x:\n== swallowed ==\n", // unterminated multiline block
    ];
    for src in fixtures {
        let mine: Vec<(SegmentKind, u32)> = segment_file(src)
            .iter()
            .filter(|s| s.kind != SegmentKind::Header)
            .filter_map(|s| s.header_start.map(|h| (s.kind, u32::from(h))))
            .collect();
        let parse = crate::parse(src);
        let mut parsers: Vec<(SegmentKind, u32)> = Vec::new();
        for child in parse.syntax().children() {
            let (kind, header_kind) = match child.kind() {
                SyntaxKind::KNOT_DEF => (SegmentKind::Knot, SyntaxKind::KNOT_HEADER),
                SyntaxKind::STITCH_DEF => (SegmentKind::TopLevelStitch, SyntaxKind::STITCH_HEADER),
                _ => continue,
            };
            let header = child
                .children()
                .find(|n| n.kind() == header_kind)
                .unwrap_or_else(|| child.clone());
            let start = header
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .find(|t| !t.kind().is_trivia())
                .map_or_else(|| header.text_range().start(), |t| t.text_range().start());
            parsers.push((kind, start.into()));
        }
        assert_eq!(mine, parsers, "segmenter/parser disagreement on {src:?}");
    }
}

#[test]
fn file_starting_with_a_knot_has_an_empty_header_segment() {
    let src = "== alpha ==\nBody.\n";
    let segs = segment_file(src);
    assert_tiles(src, &segs);
    assert_eq!(kinds(&segs), vec![SegmentKind::Header, SegmentKind::Knot]);
    assert!(segs[0].range.is_empty());
}
