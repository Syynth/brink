//! Regression tests for issue #3353: a `TODO` line inside a multiline
//! conditional block (`{ cond: … - else: … }`, including nested blocks) used
//! to be parsed as ordinary branch prose — landing as a `TEXT` node (and
//! later a story-facing line) instead of an `AUTHOR_WARNING` node. Covers
//! the then-arm, the `- else:` arm, and a nested block, in the `TODO:`,
//! `TODO(TAG) —`, and indented spellings — all already recognized at
//! weave level (`story::author_warning`), now also recognized wherever
//! branch content lines are parsed (`branchless_cond_body` and
//! `multiline_branch_body` in `inline.rs`).

use crate::ast::{self, AstNode};
use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, SyntaxNode, parse};

/// Recursively collect the text of every `TEXT` node under `node`.
fn collect_text_nodes(node: &SyntaxNode) -> Vec<String> {
    let mut out = Vec::new();
    if node.kind() == SyntaxKind::TEXT {
        out.push(node.text().to_string());
    }
    for child in node.children() {
        out.extend(collect_text_nodes(&child));
    }
    out
}

/// Recursively collect every `AUTHOR_WARNING` node's stripped text
/// (`AuthorWarning::text()`) under `node`.
fn collect_author_warnings(node: &SyntaxNode) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(warning) = ast::AuthorWarning::cast(node.clone()) {
        out.push(warning.text());
    }
    for child in node.children() {
        out.extend(collect_author_warnings(&child));
    }
    out
}

/// Find a `KNOT_DEF` named `name` anywhere under `node`.
fn find_knot_named(node: &SyntaxNode, name: &str) -> Option<ast::KnotDef> {
    if let Some(knot) = ast::KnotDef::cast(node.clone())
        && knot.header().and_then(|h| h.name()).as_deref() == Some(name)
    {
        return Some(knot);
    }
    node.children()
        .find_map(|child| find_knot_named(&child, name))
}

/// `TODO:` in the then-arm of an explicit `- cond:` branch (`MULTILINE_BRANCH_BODY`).
#[test]
fn todo_in_then_arm() {
    let src = "{\n- x:\n  Then branch.\n  TODO: inside then branch\n- else:\n  Else branch.\n}\n";
    let parsed = parse(src);
    assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());

    assert_equivalent(
        parse(src),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            TEXT
                            AUTHOR_WARNING
                        }
                    }
                    MULTILINE_BRANCH_COND {
                        MULTILINE_BRANCH_BODY {
                            TEXT
                        }
                    }
                }
            }
        }),
    );

    let root = parsed.syntax();
    assert_eq!(
        collect_author_warnings(&root),
        vec!["inside then branch".to_owned()]
    );
    for text in collect_text_nodes(&root) {
        assert!(!text.contains("TODO"), "TODO leaked into TEXT: {text:?}");
    }
}

/// `TODO:` in an `- else:` arm (the exact issue #3353 repro shape, minus the
/// leading `VAR` decl which is irrelevant at the parser level).
#[test]
fn todo_in_else_arm() {
    let src = "{ x:\n    Then branch.\n    TODO: inside then branch\n- else:\n    TODO: inside else branch\n}\n";
    let parsed = parse(src);
    assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());

    let root = parsed.syntax();
    assert_eq!(
        collect_author_warnings(&root),
        vec![
            "inside then branch".to_owned(),
            "inside else branch".to_owned()
        ]
    );
    for text in collect_text_nodes(&root) {
        assert!(!text.contains("TODO"), "TODO leaked into TEXT: {text:?}");
    }

    // `{ x:` (colon on the same line as `{`, with a space) fails the
    // parser's `is_multiline_block` newline lookahead, so this routes
    // through `inline_logic`/`CONDITIONAL_WITH_EXPR` rather than a
    // top-level `MULTILINE_BLOCK` — see `branchless_body_with_else` for
    // the `{\n` shape that *does* hit `MULTILINE_BLOCK` directly.
    assert_equivalent(
        parse(src),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            BRANCHLESS_COND_BODY {
                                TEXT
                                AUTHOR_WARNING
                                ELSE_BRANCH {
                                    MULTILINE_BRANCH_COND {
                                        MULTILINE_BRANCH_BODY {
                                            AUTHOR_WARNING
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `TODO:` inside a block nested within another block's then-arm.
#[test]
fn todo_in_nested_block() {
    let src = "{ x:\n  { y:\n    Nested then.\n    TODO: inside nested then\n  - else:\n    TODO: inside nested else\n  }\n- else:\n  Outer else.\n}\n";
    let parsed = parse(src);
    assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());

    let root = parsed.syntax();
    assert_eq!(
        collect_author_warnings(&root),
        vec![
            "inside nested then".to_owned(),
            "inside nested else".to_owned()
        ]
    );
    for text in collect_text_nodes(&root) {
        assert!(!text.contains("TODO"), "TODO leaked into TEXT: {text:?}");
    }
}

/// `TODO(TAG) —` spelling in a then-arm.
#[test]
fn todo_tag_spelling_in_branch() {
    let src = "{ x:\n  TODO(TAG) — tagged note\n}\n";
    let parsed = parse(src);
    assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());

    let root = parsed.syntax();
    let warnings = collect_author_warnings(&root);
    assert_eq!(warnings.len(), 1, "warnings: {warnings:?}");
    assert!(
        warnings[0].contains("tagged note"),
        "warning text: {:?}",
        warnings[0]
    );
    for text in collect_text_nodes(&root) {
        assert!(!text.contains("TODO"), "TODO leaked into TEXT: {text:?}");
    }
}

/// Indented `TODO:` (extra leading whitespace beyond the branch baseline) in
/// an `- else:` arm.
#[test]
fn todo_indented_spelling_in_else_arm() {
    let src = "{ x:\n  Then.\n- else:\n        TODO: deeply indented\n}\n";
    let parsed = parse(src);
    assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());

    let root = parsed.syntax();
    assert_eq!(
        collect_author_warnings(&root),
        vec!["deeply indented".to_owned()]
    );
    for text in collect_text_nodes(&root) {
        assert!(!text.contains("TODO"), "TODO leaked into TEXT: {text:?}");
    }
}

/// `TODO:` with no other content in a branchless then-arm (no `- else:`).
#[test]
fn todo_only_branchless_body() {
    let src = "{ x:\n  TODO: only line\n}\n";
    let parsed = parse(src);
    assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());

    assert_equivalent(
        parse(src),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            BRANCHLESS_COND_BODY {
                                AUTHOR_WARNING
                            }
                        }
                    }
                }
            }
        }),
    );
}

// ── PR #3367 review: blocking findings ──────────────────────────────
//
// Two follow-up bugs surfaced in review of the fix above, both already
// covered by the tests in this module for a `TODO` recognized cleanly on
// its own line, but not for a `}` sharing the note's line, or a `TODO`
// word landing mid-sentence.

/// A branch's closing `}` sharing the same line as a `TODO` note (via
/// `branchless_cond_body`, the `{ x: …` form) must not be swallowed into
/// the note — the block must still close there, and content after it must
/// still parse as ordinary story structure (here, a real `KNOT_DEF`).
///
/// Before the fix, `story::author_warning`'s plain `NEWLINE`-only scan
/// consumed the `}` as note text, so the block never closed and the rest
/// of the file — including `=== later ===` — was absorbed as conditional
/// prose.
#[test]
fn todo_with_brace_on_same_line_closes_block_branchless() {
    let src =
        "VAR x = true\n{ x:\n  TODO: fix }\n}\nPlain line.\n=== later ===\nKnot body.\n-> DONE\n";
    let parsed = parse(src);
    let root = parsed.syntax();

    assert_eq!(
        collect_author_warnings(&root),
        vec!["fix".to_owned()],
        "the closing brace must not be absorbed into the note text"
    );

    let later = find_knot_named(&root, "later");
    assert!(
        later.is_some(),
        "`=== later ===` must lower to a real KNOT_DEF, not conditional prose; tree: {root:#?}"
    );
}

/// Same as above, through `multiline_branch_body` (the `{\n- x: …` form).
#[test]
fn todo_with_brace_on_same_line_closes_block_multiline() {
    let src = "VAR x = true\n{\n- x:\n  TODO: fix }\n}\nPlain line.\n=== later ===\nKnot body.\n-> DONE\n";
    let parsed = parse(src);
    let root = parsed.syntax();

    assert_eq!(
        collect_author_warnings(&root),
        vec!["fix".to_owned()],
        "the closing brace must not be absorbed into the note text"
    );

    let later = find_knot_named(&root, "later");
    assert!(
        later.is_some(),
        "`=== later ===` must lower to a real KNOT_DEF, not conditional prose; tree: {root:#?}"
    );
}

/// A `TODO` word landing mid-line after an `INLINE_LOGIC` interpolation, a
/// `GLUE_NODE`, or an `ESCAPE`, inside an explicit `- cond:` arm
/// (`multiline_branch_body`), must stay ordinary prose — not misfire as an
/// `AUTHOR_WARNING`. Before the fix, `multiline_branch_body`'s `KW_TODO`
/// arm was unconditional (unlike its `branchless_cond_body` sibling, which
/// gates the same arm on `at_line_start`), so it fired anywhere the
/// position happened to land on `KW_TODO` — including mid-sentence.
#[test]
fn todo_mid_line_after_inline_logic_is_not_misfired() {
    let src = "{\n- x:\n  Value is {y} TODO fix this later\n}\n";
    let parsed = parse(src);
    let root = parsed.syntax();

    assert!(
        collect_author_warnings(&root).is_empty(),
        "a mid-line TODO must not become an AUTHOR_WARNING; tree: {root:#?}"
    );
    let joined = collect_text_nodes(&root).concat();
    assert!(
        joined.contains("TODO fix this later"),
        "prose must survive intact: {joined:?}"
    );
}

/// Same as above, with the `TODO` landing right after a `GLUE_NODE` (`<>`).
#[test]
fn todo_mid_line_after_glue_is_not_misfired() {
    let src = "{\n- x:\n  Hello <> TODO not a note\n}\n";
    let parsed = parse(src);
    let root = parsed.syntax();

    assert!(
        collect_author_warnings(&root).is_empty(),
        "a mid-line TODO must not become an AUTHOR_WARNING; tree: {root:#?}"
    );
    let joined = collect_text_nodes(&root).concat();
    assert!(
        joined.contains("TODO not a note"),
        "prose must survive intact: {joined:?}"
    );
}

/// Same as above, with the `TODO` landing right after an `ESCAPE` (`\*`).
#[test]
fn todo_mid_line_after_escape_is_not_misfired() {
    let src = "{\n- x:\n  Hello \\* TODO not a note\n}\n";
    let parsed = parse(src);
    let root = parsed.syntax();

    assert!(
        collect_author_warnings(&root).is_empty(),
        "a mid-line TODO must not become an AUTHOR_WARNING; tree: {root:#?}"
    );
    let joined = collect_text_nodes(&root).concat();
    assert!(
        joined.contains("TODO not a note"),
        "prose must survive intact: {joined:?}"
    );
}
