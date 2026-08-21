//! Issue #2976: a mid-line comment inside an inline-alternative branch
//! (`{a|b}`, `{cond: a|b}`, sequences) has the same zero-progress
//! fragmentation bug #2366/#2958 fixed for plain content lines
//! (`content::mixed_content`) and #2960/#2974 fixed for choice text
//! (`choice::choice_content_elements`/`choice_content_element`), third
//! sibling and one function away — `inline::branch_content` (the shared
//! parser for `IMPLICIT_SEQUENCE`/`INLINE_BRANCHES_SEQ`/
//! `INLINE_BRANCHES_COND` branches) never got the `skip_comment_tokens()`
//! retry.
//!
//! Traced against `inline.rs` pre-fix (the issue's own probe transcript):
//! `{ a /* c */ x | b }` produced 5 parse errors — the comment was hoisted
//! out of `IMPLICIT_SEQUENCE`, the `|` became an `ERROR` node, and the
//! closing `}` became `STRAY_CLOSING_BRACE`.
//!
//! This file proves the bug through the REAL pipeline (compile + runtime
//! transcript, not just CST): before the fix, the compiled program
//! diagnoses parse errors and/or the alternative's displayed text is
//! destroyed. After the fix, there must be zero diagnostics and the
//! alternative's chosen branch prints intact, comment elided.
//!
//! Byte-exact whitespace preservation around the elided comment (the
//! double space #2958's `content.rs` precedent established) is pinned
//! separately at the CST/`TEXT`-node level in
//! `crates/internal/brink-syntax/src/parser/tests/inline/midline_comment.rs`,
//! the same layer #2958/#2974's own precedent tests use — NOT at this
//! runtime layer. `OutputBuffer::push_text` (`brink-runtime/src/output/mod.rs`)
//! collapses adjacent whitespace at text-part boundaries by design (see
//! `adjacent_whitespace_collapsed`), and an elided comment leaves nothing
//! between the two surviving whitespace-bearing `TEXT` parts, so the two
//! spaces this fix preserves in the CST collapse to one single space by
//! the time the output reaches the transcript. That collapse is
//! pre-existing brink output-buffer behavior, unrelated to and unchanged
//! by this fix, and is tracked separately as #2975 (not fixed or
//! overclaimed here). The runtime assertions below expect brink's current
//! single collapsed space accordingly.

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

/// Run to completion, collecting all output text.
fn collect_output(source: &str) -> String {
    let mut story = link_and_start(source);
    let mut out = String::new();
    loop {
        match story.continue_single().expect("no runtime fault") {
            Step::Line(line) => out.push_str(&line.text),
            Step::Choices(_) => panic!("expected no choices in this fixture"),
            Step::Done | Step::End | Step::Suspended => break,
        }
    }
    out
}

/// The issue's own probe case, driven through the real pipeline: a
/// mid-line block comment inside an `IMPLICIT_SEQUENCE` branch must not
/// fragment it. `{ a /* c */ x | b }` on a story's first (and only, in
/// this fixture) content line runs the implicit sequence exactly once,
/// which selects its first branch ("a x") only -- the second branch ("b")
/// is never visited.
#[test]
fn block_comment_mid_branch_prints_intact_implicit_sequence() {
    let src = "{ a /* c */ x | b }\n";
    let out = collect_output(src);
    assert_eq!(
        out, "a x\n",
        "expected the comment elided with no fragmentation (the CST's two \
         surviving spaces around the elided comment collapse to one at \
         the runtime output-buffer layer -- see this file's module doc, \
         tracked separately as #2975)"
    );
}

/// Same bug, comment in the LAST branch instead of the first.
#[test]
fn block_comment_in_last_branch_prints_intact() {
    let src = "{ a | b /* c */ x }\n";
    let out = collect_output(src);
    assert_eq!(
        out, "a\n",
        "first branch runs first, unaffected by the comment in the second"
    );
}

/// A comment sitting between two interpolations in the same branch must
/// also not fragment the alternative. Unlike `content.rs`'s/`choice.rs`'s
/// `L_BRACE` arms (and unlike this file's other tests, which do exercise
/// this fix's `skip_comment_tokens` retry), `inline::branch_content`'s own
/// `L_BRACE` arm was left as a bare, unconditional `p.skip_ws()` — see
/// `parser/tests/inline/midline_comment.rs`'s
/// `block_comment_between_interpolations_in_branch` for why. That call
/// already consumed any trivia between two adjacent `{...}` elements
/// without stalling, comment or not, so this path never hit the
/// zero-progress bug this issue fixes, and the pre-existing whitespace
/// output for it is unaffected here (the whitespace is swallowed as bare
/// trivia with no `TEXT` node at all, so no space survives between the
/// interpolations' output at all -- a different, pre-existing, unrelated
/// quirk from #2975's comment-elision double-space collapse). This test
/// pins that it keeps producing zero diagnostics and its existing output,
/// not that comment elision here goes through this fix's retry.
#[test]
fn block_comment_between_interpolations_in_branch_prints_intact() {
    let src = "VAR a = \"A\"\nVAR b = \"B\"\n{ {a} /* c */ {b} | fallback }\n";
    let out = collect_output(src);
    assert_eq!(
        out, "AB\n",
        "expected the comment elided with no fragmentation between the two \
         interpolations (pre-existing bare-skip_ws whitespace handling for \
         this path, unaffected by this fix)"
    );
}

/// Inline conditional branches (`{cond: yes|no}`, not just bare
/// `IMPLICIT_SEQUENCE`) share `branch_content` and must also survive a
/// mid-line comment.
#[test]
fn block_comment_in_inline_conditional_branch_prints_intact() {
    let src = "VAR flag = true\n{flag: yes /* c */ indeed|no}\n";
    let out = collect_output(src);
    assert_eq!(out, "yes indeed\n");
}

/// A comment inside a multiline conditional branch body (the other two
/// fixed zero-progress sites, `branchless_cond_body` /
/// `multiline_branch_body`'s `multiline_branch_text` call sites) must
/// also survive intact through the real pipeline.
#[test]
fn block_comment_in_multiline_branch_body_prints_intact() {
    let src = "VAR x = 10\n{\n- x > 5:\n  Big /* c */ number.\n- else:\n  Small.\n}\n";
    let out = collect_output(src);
    assert_eq!(out, "Big number.\n");
}
