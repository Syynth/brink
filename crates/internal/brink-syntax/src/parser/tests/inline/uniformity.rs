use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── Section M: Variant uniformity ───────────────────────────────────

const INLINE_VARIANTS: [SyntaxKind; 5] = [
    SyntaxKind::SEQUENCE_WITH_ANNOTATION,
    SyntaxKind::MULTILINE_CONDITIONAL,
    SyntaxKind::IMPLICIT_SEQUENCE,
    SyntaxKind::CONDITIONAL_WITH_EXPR,
    SyntaxKind::INNER_EXPRESSION,
];

/// Assert that every `INLINE_LOGIC` in `src` has exactly one variant child.
fn assert_inline_uniformity(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::INLINE_LOGIC {
            let variant_children: Vec<_> = node
                .children()
                .filter(|c| INLINE_VARIANTS.contains(&c.kind()))
                .collect();
            assert_eq!(
                variant_children.len(),
                1,
                "INLINE_LOGIC should have exactly one variant child, found {} in `{src}`:\n  {:?}",
                variant_children.len(),
                variant_children
                    .iter()
                    .map(crate::SyntaxNode::kind)
                    .collect::<Vec<_>>(),
            );
        }
    }
}

#[test]
fn uniformity_inner_expr() {
    assert_inline_uniformity("{x}\n");
}

#[test]
fn uniformity_conditional() {
    assert_inline_uniformity("{x: yes|no}\n");
}

#[test]
fn uniformity_implicit_seq() {
    assert_inline_uniformity("{a|b|c}\n");
}

#[test]
fn uniformity_sym_annotation() {
    assert_inline_uniformity("{&a|b}\n");
}

#[test]
fn uniformity_word_annotation() {
    assert_inline_uniformity("{stopping: a|b}\n");
}

#[test]
fn uniformity_nested() {
    assert_inline_uniformity("{x: {y}|no}\n");
}

#[test]
fn uniformity_content_line() {
    assert_inline_uniformity("Hello {x} world\n");
}

const MULTILINE_VARIANTS: [SyntaxKind; 3] = [
    SyntaxKind::SEQUENCE_WITH_ANNOTATION,
    SyntaxKind::MULTILINE_BRANCHES_COND,
    SyntaxKind::CONDITIONAL_WITH_EXPR,
];

/// Assert that every `MULTILINE_BLOCK` in `src` has exactly one variant child.
fn assert_multiline_uniformity(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::MULTILINE_BLOCK {
            let variant_children: Vec<_> = node
                .children()
                .filter(|c| MULTILINE_VARIANTS.contains(&c.kind()))
                .collect();
            assert_eq!(
                variant_children.len(),
                1,
                "MULTILINE_BLOCK should have exactly one variant child, found {} in `{src}`:\n  {:?}",
                variant_children.len(),
                variant_children
                    .iter()
                    .map(crate::SyntaxNode::kind)
                    .collect::<Vec<_>>(),
            );
        }
    }
}

#[test]
fn uniformity_multiline_cond() {
    assert_multiline_uniformity("{\n- x:\n  Yes.\n}\n");
}

#[test]
fn uniformity_multiline_seq() {
    assert_multiline_uniformity("{\nstopping:\n- a\n- b\n}\n");
}

#[test]
fn uniformity_multiline_bare() {
    assert_multiline_uniformity("{\n- One.\n- Two.\n}\n");
}

// ── Section N: Positive/negative wrapper assertions ─────────────────

/// `{x}` has `INNER_EXPRESSION`, not `CONDITIONAL_WITH_EXPR` or `IMPLICIT_SEQUENCE`.
#[test]
fn has_inner_expr_not_conditional() {
    let p = parse("{x}\n");
    let root = p.syntax();
    let has = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::INNER_EXPRESSION);
    let has_cond = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CONDITIONAL_WITH_EXPR);
    let has_seq = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE);
    assert!(has, "expected INNER_EXPRESSION");
    assert!(!has_cond, "must not have CONDITIONAL_WITH_EXPR");
    assert!(!has_seq, "must not have IMPLICIT_SEQUENCE");
}

/// `{x: yes|no}` has `CONDITIONAL_WITH_EXPR`, not `IMPLICIT_SEQUENCE` or `INNER_EXPRESSION`.
#[test]
fn has_conditional_not_sequence() {
    let p = parse("{x: yes|no}\n");
    let root = p.syntax();
    let has = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CONDITIONAL_WITH_EXPR);
    let has_seq = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE);
    let has_inner = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::INNER_EXPRESSION);
    assert!(has, "expected CONDITIONAL_WITH_EXPR");
    assert!(!has_seq, "must not have IMPLICIT_SEQUENCE");
    assert!(!has_inner, "must not have INNER_EXPRESSION");
}

/// `{a|b|c}` has `IMPLICIT_SEQUENCE`, not `CONDITIONAL_WITH_EXPR` or `INNER_EXPRESSION`.
#[test]
fn has_implicit_seq_not_conditional() {
    let p = parse("{a|b|c}\n");
    let root = p.syntax();
    let has = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::IMPLICIT_SEQUENCE);
    let has_cond = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::CONDITIONAL_WITH_EXPR);
    let has_inner = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::INNER_EXPRESSION);
    assert!(has, "expected IMPLICIT_SEQUENCE");
    assert!(!has_cond, "must not have CONDITIONAL_WITH_EXPR");
    assert!(!has_inner, "must not have INNER_EXPRESSION");
}

/// `{&a|b}` has `SEQUENCE_SYMBOL_ANNOTATION`, not `SEQUENCE_WORD_ANNOTATION`.
#[test]
fn has_sym_annotation_not_word() {
    let p = parse("{&a|b}\n");
    let root = p.syntax();
    let has_sym = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SEQUENCE_SYMBOL_ANNOTATION);
    let has_word = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SEQUENCE_WORD_ANNOTATION);
    assert!(has_sym, "expected SEQUENCE_SYMBOL_ANNOTATION");
    assert!(!has_word, "must not have SEQUENCE_WORD_ANNOTATION");
}

/// `{stopping: a|b}` has `SEQUENCE_WORD_ANNOTATION`, not `SEQUENCE_SYMBOL_ANNOTATION`.
#[test]
fn has_word_annotation_not_sym() {
    let p = parse("{stopping: a|b}\n");
    let root = p.syntax();
    let has_word = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SEQUENCE_WORD_ANNOTATION);
    let has_sym = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SEQUENCE_SYMBOL_ANNOTATION);
    assert!(has_word, "expected SEQUENCE_WORD_ANNOTATION");
    assert!(!has_sym, "must not have SEQUENCE_SYMBOL_ANNOTATION");
}

/// `{x: yes|no}` has `INLINE_LOGIC`, not `MULTILINE_BLOCK`.
#[test]
fn has_inline_logic_not_multiline() {
    let p = parse("{x: yes|no}\n");
    let root = p.syntax();
    let has_inline = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::INLINE_LOGIC);
    let has_multiline = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::MULTILINE_BLOCK);
    assert!(has_inline, "expected INLINE_LOGIC");
    assert!(!has_multiline, "must not have MULTILINE_BLOCK");
}

/// Multiline block has `MULTILINE_BLOCK`, not `INLINE_LOGIC`.
#[test]
fn has_multiline_not_inline() {
    let p = parse("{\n- x:\n  Yes.\n}\n");
    let root = p.syntax();
    let has_multiline = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::MULTILINE_BLOCK);
    let has_inline = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::INLINE_LOGIC);
    assert!(has_multiline, "expected MULTILINE_BLOCK");
    assert!(!has_inline, "must not have INLINE_LOGIC");
}

/// Inline conditional has `INLINE_BRANCHES_COND`, not `MULTILINE_BRANCHES_COND`.
#[test]
fn conditional_has_inline_not_multiline_branches() {
    let p = parse("{x: yes|no}\n");
    let root = p.syntax();
    let has_inline = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::INLINE_BRANCHES_COND);
    let has_multiline = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::MULTILINE_BRANCHES_COND);
    assert!(has_inline, "expected INLINE_BRANCHES_COND");
    assert!(!has_multiline, "must not have MULTILINE_BRANCHES_COND");
}

// ── Section O: Error recovery ───────────────────────────────────────

/// Unclosed brace — lossless round-trip with errors.
#[test]
fn error_unclosed_brace() {
    let src = "{x\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unclosed brace"
    );
}

/// Missing colon in conditional — parsed but may have errors.
#[test]
fn error_missing_colon_in_cond() {
    let src = "{x yes|no}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// Unclosed outer brace with valid inner inline logic.
#[test]
fn error_unclosed_nested() {
    let src = "{x: {y}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unclosed outer brace"
    );
}

/// Empty braces — verify lossless round-trip.
#[test]
fn error_empty_braces() {
    let src = "{}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

// ── Section P: Pipe / colon / double-pipe disambiguation ───────────
//
// These tests form a matrix covering the interaction between `|` (sequence
// separator), `||` (logical OR), and `:` (conditional marker) inside
// `{...}` blocks. The presence of `:` at depth-0 determines whether `||`
// is logical OR (conditional) or two sequence separators (sequence).
//
// Cases marked "RED" currently fail because the brace-pair pre-scan
// misclassifies `||` as a sequence separator even when `:` follows.
//
// | # | Input            | COLON? | Single PIPE? | `||`? | Expected      |
// |---|------------------|--------|-------------|-------|---------------|
// | 7 | {x || y: body}   | yes    | no          | yes   | conditional   |
// | 8 | {x || y}         | no     | no          | yes   | bare expr     |
// | 9 | {a|b:c}          | yes    | yes (before) | no   | sequence      |
// |10 | {x<10||x>20:body}| yes    | no          | yes   | conditional   |
// |11 | {a||b:c}         | yes    | no          | yes   | conditional   |
// |12 | {a|b||c}         | no     | yes         | yes   | sequence      |
// |13 | {x||y||z:w}      | yes    | no          | yes   | conditional   |
//
// Cases 1–6 are covered by existing tests in sections A–C.

/// Case 7: `{x || y: body}` — `||` + COLON → conditional.
#[test]
fn pipe_pipe_with_colon_is_conditional() {
    assert_equivalent(
        parse("{x || y: body}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            INFIX_EXPR [PIPE, PIPE] {
                                PATH
                                PATH
                            }
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Case 8: `{x || y}` — `||` without COLON → sequence.
///
/// No COLON at depth-0. The reference tries `InnerSequence` before
/// `InnerExpression`, and both `|` tokens act as sequence separators,
/// yielding three branches: `x `, empty, ` y`. This matches the
/// existing `{a||c}` behavior (case 6, `implicit_seq_empty_middle`).
#[test]
fn pipe_pipe_no_colon_is_sequence() {
    assert_equivalent(
        parse("{x || y}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        IMPLICIT_SEQUENCE {
                            BRANCH_CONTENT {
                                TEXT
                            }
                            BRANCH_CONTENT
                            BRANCH_CONTENT {
                                TEXT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Case 9: `{a|b:c}` — single PIPE before COLON → sequence.
///
/// The PIPE at depth-0 appears before the COLON, so this is a sequence
/// (branches: `a`, `b:c`). The `:` is literal text inside the second branch.
#[test]
fn single_pipe_before_colon_is_sequence() {
    assert_equivalent(
        parse("{a|b:c}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        IMPLICIT_SEQUENCE {
                            BRANCH_CONTENT {
                                TEXT
                            }
                            BRANCH_CONTENT {
                                TEXT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Case 10: `{x < 10 || x > 20: body}` — compound `||` expr + COLON → conditional.
#[test]
fn compound_logical_or_conditional() {
    assert_equivalent(
        parse("{x < 10 || x > 20: body}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            INFIX_EXPR {
                                INFIX_EXPR {
                                    PATH
                                    INTEGER_LIT
                                }
                                INFIX_EXPR {
                                    PATH
                                    INTEGER_LIT
                                }
                            }
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Case 11: `{a||b:c}` — `||` + COLON → conditional (condition = `a || b`).
///
/// Despite `a||b` looking like it could be a sequence with empty middle,
/// the COLON makes this a conditional. The reference parser tries
/// `ConditionExpression` first: parses `a || b` as expression, finds `:`,
/// commits to conditional.
#[test]
fn pipe_pipe_colon_is_conditional_not_sequence() {
    assert_equivalent(
        parse("{a||b:c}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            INFIX_EXPR [PIPE, PIPE] {
                                PATH
                                PATH
                            }
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Case 12: `{a|b||c}` — single PIPE + `||`, no COLON → sequence.
///
/// No COLON at depth-0, so this can't be a conditional. The single `|`
/// and `||` are all sequence separators: branches `a`, `b`, empty, `c`.
#[test]
fn mixed_pipe_and_pipe_pipe_no_colon_is_sequence() {
    assert_equivalent(
        parse("{a|b||c}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        IMPLICIT_SEQUENCE {
                            BRANCH_CONTENT {
                                TEXT
                            }
                            BRANCH_CONTENT {
                                TEXT
                            }
                            BRANCH_CONTENT
                            BRANCH_CONTENT {
                                TEXT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Case 13: `{x || y || z: w}` — chained `||` + COLON → conditional.
#[test]
fn chained_logical_or_conditional() {
    assert_equivalent(
        parse("{x || y || z: w}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            INFIX_EXPR {
                                INFIX_EXPR [PIPE, PIPE] {
                                    PATH
                                    PATH
                                }
                                PATH
                            }
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Multiline else branch body with `||` in a nested branchless conditional.
///
/// The second `{...}` block uses `||` in its condition. With the current
/// bug, the pre-scan classifies it as a sequence, causing the `- else:`
/// inside to leak as a 5th branch of the outer conditional.
#[test]
fn else_body_with_logical_or_nested_conditional() {
    assert_equivalent(
        parse(
            "\
{
    - x >= 10:
        big
    - else:
        { x < 5 || x > 20:
            out of range
        - else:
            in range
        }
}
",
        ),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        INFIX_EXPR {
                            PATH
                            INTEGER_LIT
                        }
                        MULTILINE_BRANCH_BODY {
                            TEXT
                        }
                    }
                    MULTILINE_BRANCH_COND {
                        MULTILINE_BRANCH_BODY {
                            INLINE_LOGIC {
                                CONDITIONAL_WITH_EXPR {
                                    INFIX_EXPR {
                                        INFIX_EXPR {
                                            PATH
                                            INTEGER_LIT
                                        }
                                        INFIX_EXPR {
                                            PATH
                                            INTEGER_LIT
                                        }
                                    }
                                    BRANCHLESS_COND_BODY {
                                        TEXT
                                        ELSE_BRANCH {
                                            MULTILINE_BRANCH_COND {
                                                MULTILINE_BRANCH_BODY {
                                                    TEXT
                                                }
                                            }
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

// ── Section Q: Choices inside multiline conditional branches ────────

/// Single choice inside a conditional branch.
#[test]
fn multiline_cond_choice_single() {
    assert_equivalent(
        parse("{\n- x:\n  * Go outside\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Multiple choices inside a conditional branch.
#[test]
fn multiline_cond_choice_multiple() {
    assert_equivalent(
        parse("{\n- x:\n  * Option A\n  * Option B\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Choices in both branches (condition + else).
#[test]
fn multiline_cond_choices_both_branches() {
    assert_equivalent(
        parse(
            "{\n- door_open:\n  * Go outside\n- else:\n  * Ask permission\n  * Open the door\n}\n",
        ),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                    MULTILINE_BRANCH_COND {
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Choice with divert inside a conditional branch.
#[test]
fn multiline_cond_choice_with_divert() {
    assert_equivalent(
        parse("{\n- x:\n  * Go outside -> garden\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                                DIVERT_NODE {
                                    SIMPLE_DIVERT {
                                        DIVERT_TARGET_WITH_ARGS {
                                            PATH
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

/// Choice with bracket content inside a conditional branch.
#[test]
fn multiline_cond_choice_with_brackets() {
    assert_equivalent(
        parse("{\n- x:\n  * [hidden]shown\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_BRACKET_CONTENT {
                                    TEXT
                                }
                                CHOICE_INNER_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Choice with label inside a conditional branch.
#[test]
fn multiline_cond_choice_with_label() {
    assert_equivalent(
        parse("{\n- x:\n  * (my_label) Go outside\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                LABEL {
                                    IDENTIFIER
                                }
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Mixed text and choices in a branch body.
#[test]
fn multiline_cond_text_then_choice() {
    assert_equivalent(
        parse("{\n- x:\n  Some text.\n  * A choice\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            TEXT
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Sticky choice (+) inside a conditional branch.
#[test]
fn multiline_cond_sticky_choice() {
    assert_equivalent(
        parse("{\n- x:\n  + Sticky option\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Nested choice depth inside a conditional branch.
#[test]
fn multiline_cond_nested_choice() {
    assert_equivalent(
        parse("{\n- x:\n  * * Nested choice\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Choice with inline condition inside a conditional branch.
#[test]
fn multiline_cond_choice_with_condition() {
    assert_equivalent(
        parse("{\n- x:\n  * {flag} Conditional choice\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            CHOICE {
                                CHOICE_BULLETS
                                CHOICE_CONDITION {
                                    PATH
                                }
                                CHOICE_START_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}
