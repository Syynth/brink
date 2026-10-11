//! Issue #3480: `E197` — inklecate's "Blank choice" warning, on a choice
//! with an explicit but empty `[]` and no text before it. Every case here
//! was measured against inklecate's compiler (inkjs 2.4.0's, via
//! `tools/inkjs-oracle`'s `node_modules`): the cases that fire are the ones
//! that print "Blank choice - if you intended a default fallback choice,
//! use the `* ->` syntax", and the clean ones print nothing of the kind.

// `expect` calls below sit in plain helper functions, not directly inside a
// `#[test]` fn, so clippy's `allow-expect-in-tests` (`clippy.toml`) does not
// cover them.
#![allow(clippy::expect_used)]

use brink_compiler::DiagnosticCode;

/// The empty-choice-family codes (`E195`, `E197`) a story compiles with.
fn empty_choice_codes(source: &str) -> Vec<DiagnosticCode> {
    let out = brink_compiler::compile("main.ink", |_| Ok(source.to_owned()))
        .expect("E195 and E197 are warnings; the story still compiles");
    out.warnings
        .iter()
        .map(|d| d.code)
        .filter(|c| matches!(c, DiagnosticCode::E195 | DiagnosticCode::E197))
        .collect()
}

#[test]
fn a_blank_bracket_with_text_after_it_warns() {
    for source in [
        "* [] some text\n-> END\n",
        "+ [] some text\n-> END\n",
        "* []text\n  -> END\n",
    ] {
        assert_eq!(
            empty_choice_codes(source),
            [DiagnosticCode::E197],
            "{source}"
        );
    }
}

/// None of a divert, a tag, a label or a condition exempts it.
#[test]
fn nothing_on_the_line_exempts_it() {
    for source in [
        "* [] -> END\n",
        "* [] #t\n-> END\n",
        "* [] text #t\n-> END\n",
        "* (lab) [] some text\n -> END\n",
        "* {true} [] some text\n -> END\n",
    ] {
        assert_eq!(
            empty_choice_codes(source),
            [DiagnosticCode::E197],
            "{source}"
        );
    }
}

/// A comment inside the brackets is elided, so they are still empty.
#[test]
fn a_comment_does_not_fill_the_brackets() {
    assert_eq!(
        empty_choice_codes("* [/* c */] text\n-> END\n"),
        [DiagnosticCode::E197]
    );
}

/// A wholly empty choice with no divert is E195's, and only E195's.
#[test]
fn a_completely_empty_choice_is_e195_alone() {
    assert_eq!(empty_choice_codes("* []\n-> END\n"), [DiagnosticCode::E195]);
}

#[test]
fn clean_shapes_do_not_warn() {
    for source in [
        "* hi [] there\n-> END\n",
        "* [x] y\n-> END\n",
        // A space is content, as in ink.
        "* [ ] text\n-> END\n",
        "* ->\n-> END\n",
    ] {
        assert!(empty_choice_codes(source).is_empty(), "{source}");
    }
}
