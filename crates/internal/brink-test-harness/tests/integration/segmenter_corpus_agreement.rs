//! Segmenter–parser agreement across the whole in-tree ink corpus
//! (issue #3084, `docs/per-knot-incremental-lowering-spec.md` §3 step 1).
//!
//! `brink_syntax::segment_file` re-derives the parser's dispatch rule from
//! the raw token stream — line-start detection, trivia skipping, brace
//! depth, `at_knot`/`at_stitch` lookahead. Any divergence between the two
//! is a future byte-identity break for per-segment lowering, so this
//! sweep pins them against each other over every `.ink` file in
//! `tests/tier{1,2,3}`, `tests/tests_github`, and `tests/tests_patched`:
//!
//! - **Tiling**: segments cover the file exactly, in order, gap-free.
//! - **Boundary parity**: the segments' `(kind, header_start)` sequence
//!   equals the whole-file parse's top-level `KNOT_DEF`/`STITCH_DEF`
//!   sequence, with each header located at its header node's first
//!   non-trivia token.

use std::path::{Path, PathBuf};

use brink_syntax::{SegmentKind, SyntaxKind, segment_file};

fn collect_ink_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ink_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ink") {
            out.push(path);
        }
    }
}

/// The parser's own answer: for each top-level `KNOT_DEF` / `STITCH_DEF`,
/// the start of its header node's first non-trivia token.
fn parser_boundaries(source: &str) -> Vec<(SegmentKind, u32)> {
    let parse = brink_syntax::parse(source);
    let mut out = Vec::new();
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
            .filter_map(brink_syntax::SyntaxElement::into_token)
            .find(|t| !t.kind().is_trivia())
            .map_or_else(|| header.text_range().start(), |t| t.text_range().start());
        out.push((kind, start.into()));
    }
    out
}

#[test]
fn segmenter_agrees_with_the_parser_across_the_corpus() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tests");

    let mut files = Vec::new();
    for tier in [
        "tier1",
        "tier2",
        "tier3",
        "tier1-native",
        "tests_github",
        "tests_patched",
    ] {
        collect_ink_files(&root.join(tier), &mut files);
    }
    files.sort();
    assert!(
        files.len() >= 300,
        "corpus discovery looks broken: only {} .ink files found under {}",
        files.len(),
        root.display()
    );

    let mut disagreements: Vec<String> = Vec::new();
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue; // non-UTF-8 fixture — the compiler pipeline skips it too
        };
        let segments = segment_file(&source);

        // Tiling.
        let mut pos = 0u32;
        let mut tiled = true;
        for seg in &segments {
            if u32::from(seg.range.start()) != pos {
                tiled = false;
                break;
            }
            pos = seg.range.end().into();
        }
        if !tiled || pos != u32::try_from(source.len()).unwrap_or(u32::MAX) {
            disagreements.push(format!("{}: segments do not tile the file", path.display()));
            continue;
        }

        // Boundary parity with the whole-file parse.
        let mine: Vec<(SegmentKind, u32)> = segments
            .iter()
            .filter(|s| s.kind != SegmentKind::Header)
            .filter_map(|s| s.header_start.map(|h| (s.kind, u32::from(h))))
            .collect();
        let parsers = parser_boundaries(&source);
        if mine != parsers {
            disagreements.push(format!(
                "{}: segmenter {:?} != parser {:?}",
                path.display(),
                mine.iter().take(8).collect::<Vec<_>>(),
                parsers.iter().take(8).collect::<Vec<_>>(),
            ));
        }
    }

    assert!(
        disagreements.is_empty(),
        "segmenter/parser disagreement on {} of {} files:\n{}",
        disagreements.len(),
        files.len(),
        disagreements
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
