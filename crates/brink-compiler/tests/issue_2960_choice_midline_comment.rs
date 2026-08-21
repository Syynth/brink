//! Issue #2960: a mid-line comment inside choice text has the same
//! zero-progress fragmentation bug #2366/#2958 fixed for plain content
//! lines (`content::mixed_content`), one function away —
//! `choice::choice_content_elements` (analogous to `mixed_content`'s
//! catch-all arm) and `choice::choice_text` (analogous to `text_content`)
//! never got the `skip_comment_tokens()` retry.
//!
//! Traced against `choice.rs` pre-fix: `* Hello /* c */ world\n` —
//! `choice_start_content` (`choice_content_elements`) breaks at the
//! comment, `choice_inner_content` immediately breaks too (empty `TEXT`),
//! `p.skip_ws()` at `choice.rs`'s trailing-divert check eats the comment
//! AND its trailing space, `expected newline after choice` fires, and
//! `world` spills into a separate `CONTENT_LINE` after the choice instead
//! of staying part of the choice text.
//!
//! This file proves the bug through the REAL pipeline (compile + runtime
//! transcript, not just CST): before the fix, the compiled program either
//! diagnoses `expected newline after choice` or the choice's displayed
//! text is missing `world` / a bogus follow-on line appears. After the
//! fix, there must be zero diagnostics and exactly one choice.
//!
//! Byte-exact whitespace preservation around the elided comment (the
//! double space #2958's `content.rs` precedent established, matching
//! inklecate's own `astrochili__narrator` `comments.ink` corpus output) is
//! pinned separately at the CST/`TEXT`-node level in
//! `crates/internal/brink-syntax/src/parser/tests/choice/mod.rs`, the same
//! layer #2958's own precedent tests use — NOT at this runtime layer.
//! `OutputBuffer::push_text` (`brink-runtime/src/output/mod.rs`) collapses
//! adjacent whitespace at text-part boundaries by design (see
//! `adjacent_whitespace_collapsed`), and an elided comment leaves nothing
//! between the two surviving whitespace-bearing `TEXT` parts, so the two
//! spaces this fix preserves in the CST collapse to one single space by
//! the time a choice's displayed text (or a chosen choice's follow-on
//! output) reaches the transcript. That collapse is pre-existing brink
//! output-buffer behavior, unrelated to and unchanged by this fix — but
//! note it DIVERGES from the C# reference for this exact shape: inklecate
//! compiles the elided comment's surrounding whitespace verbatim (a double
//! space — see `tests/tests_github/astrochili__narrator/test/units/`
//! `comments.ink.json`) and the C# runtime prints it as-is. No oracle
//! episode covers the shape today, so the ratchet doesn't see it; the
//! divergence is tracked as its own issue rather than resolved (or
//! declared correct) here. The runtime assertions below expect brink's
//! current single collapsed space accordingly.

#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Severity};
use brink_runtime::{DotNetRng, Step, Story};

fn compile_ink(
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    // Default dialect is `StrictInk` (ink-compat surface) — exactly the
    // surface this bug (`crates/internal/brink-syntax`) lives on.
    let options = AnalysisOptions::default();
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
}

fn link_and_start(source: &str) -> Story<DotNetRng> {
    let compiled = compile_ink(source).expect("compile should succeed with no error diagnostics");
    let errors: Vec<_> = compiled
        .warnings
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics, got {errors:?}"
    );
    let (program, line_tables) = brink_runtime::link(&compiled.data).expect("link");
    Story::<DotNetRng>::new(Arc::new(program), line_tables)
}

/// Compile and run to the first `Step::Choices`, returning the choice texts.
fn first_choice_texts(source: &str) -> Vec<String> {
    let mut story = link_and_start(source);
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(_) => {}
            Step::Choices(choices) => {
                return choices.into_iter().map(|c| c.text).collect();
            }
            Step::Done | Step::End | Step::Suspended => {
                panic!("story ended before offering any choices")
            }
        }
    }
}

/// Compile, run to the first `Step::Choices`, select choice 0, and return
/// the concatenated output text that follows (this is where choice INNER
/// content — the region after `]` — actually surfaces: it is story output
/// emitted once the choice is taken, not part of the choice's own
/// displayed text; see `LowerChoice::lower_choice` / ink's own
/// text1[bracket]text2 semantics).
fn choose_first_and_collect_output(source: &str) -> String {
    let mut story = link_and_start(source);
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(_) => {}
            Step::Choices(_) => break,
            Step::Done | Step::End | Step::Suspended => {
                panic!("story ended before offering any choices")
            }
        }
    }
    story.choose(0).expect("choose(0) should succeed");
    let mut out = String::new();
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(line) => out.push_str(&line.text),
            Step::Choices(_) => panic!("expected no further choices in this fixture"),
            Step::Done | Step::End | Step::Suspended => break,
        }
    }
    out
}

/// A mid-line block comment in choice START content (before any `[`) must
/// not fragment the choice: exactly one choice, its text intact with both
/// surrounding spaces of the elided comment preserved.
#[test]
fn block_comment_in_choice_start_content_stays_one_choice() {
    let src = "* Hello /* c */ world\n    -> END\n";
    let texts = first_choice_texts(src);
    assert_eq!(
        texts,
        vec!["Hello world".to_string()],
        "expected exactly one intact choice, comment elided, no fragmentation \
         (the CST's two surviving spaces collapse to one at the runtime \
         output-buffer layer -- see this file's module doc)"
    );
}

/// Same bug, choice INNER content (after the `]` bracket region) — the
/// third of the issue's three choice regions, alongside bracket content
/// (already working via bump-not-break) and start content (previous test).
/// Inner content is story output emitted after the choice is taken, not
/// part of the choice's own displayed text, so this drives the choice and
/// inspects the follow-on transcript rather than `Choice::text`.
#[test]
fn block_comment_in_choice_inner_content_stays_one_choice() {
    let src = "* [opt] Hello /* c */ world\n    -> END\n";
    let out = choose_first_and_collect_output(src);
    assert_eq!(
        out, "Hello world\n",
        "expected the comment elided with no fragmentation in the inner \
         (post-bracket) content region, once collapsed at the output layer"
    );
}

/// A `LINE_COMMENT` (`//`) in choice text runs to end of line, so it hits
/// `choice_text`'s ordinary `NEWLINE`-adjacent stop, never the zero-progress
/// path at all (mirrors `content.rs`'s
/// `line_comment_mid_line_unaffected_by_fix` — this must keep working
/// unchanged by the fix).
#[test]
fn line_comment_in_choice_text_unaffected_by_fix() {
    let src = "* Hello // note\n    -> END\n";
    let texts = first_choice_texts(src);
    assert_eq!(texts, vec!["Hello".to_string()]);
}

/// A comment sitting between two interpolations in choice text must also
/// not fragment the choice, and whitespace on both sides must survive.
/// Leading text ("Hi ") before the first `{a}` is deliberate: a choice
/// whose text starts directly with `{` is parsed as a leading
/// `choice_condition`, not inline-logic content (see `choice()`'s
/// `while p.current() == L_BRACE` loop) — that's a different, unrelated
/// grammar rule, not the bug under test here.
#[test]
fn block_comment_between_interpolations_in_choice_text() {
    let src = "VAR a = \"A\"\nVAR b = \"B\"\n* Hi {a} /* c */ {b}\n    -> END\n";
    let texts = first_choice_texts(src);
    assert_eq!(
        texts,
        vec!["Hi A B".to_string()],
        "expected the comment elided with no fragmentation between the two \
         interpolations, once collapsed at the output layer"
    );
}

/// `choice_bracket_content` already survives a mid-line comment (stuck-token
/// handler is `p.bump()`, not `break`) — pinned here so a future change
/// can't silently regress the one region that was never broken. Unlike
/// `skip_comment_tokens`, `choice_bracket_content`'s `p.bump()` recovery
/// consumes exactly the stuck comment token and nothing more, so it does
/// not carry the same double-space whitespace-preservation guarantee as
/// the fixed start/inner regions below — this test pins its actual
/// existing (untouched by this fix) output, not a normative claim about
/// what it should produce.
#[test]
fn block_comment_in_choice_bracket_content_already_works() {
    let src = "* Hello[hidden /* c */ bracket]world\n    -> END\n";
    let texts = first_choice_texts(src);
    assert_eq!(texts, vec!["Hellohidden bracket".to_string()]);
}
