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
