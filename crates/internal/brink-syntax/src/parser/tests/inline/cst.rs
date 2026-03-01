use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── Section A: Inner expressions ────────────────────────────────────

/// `{x}` → bare variable as inner expression.
#[test]
fn inner_expr_bare_variable() {
    assert_equivalent(
        parse("Hello {x}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `{knot.stitch}` → dotted path as inner expression.
#[test]
fn inner_expr_dotted_path() {
    assert_equivalent(
        parse("{knot.stitch}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `{42}` → integer literal as inner expression.
#[test]
fn inner_expr_integer() {
    assert_equivalent(
        parse("{42}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

/// `{"hello"}` → string literal as inner expression.
#[test]
fn inner_expr_string() {
    assert_equivalent(
        parse("{\"hello\"}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            STRING_LIT
                        }
                    }
                }
            }
        }),
    );
}

/// `{true}` → boolean literal as inner expression.
#[test]
fn inner_expr_boolean() {
    assert_equivalent(
        parse("{true}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            BOOLEAN_LIT
                        }
                    }
                }
            }
        }),
    );
}

/// `{x + 1}` → infix expression as inner expression.
#[test]
fn inner_expr_infix() {
    assert_equivalent(
        parse("{x + 1}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            INFIX_EXPR {
                                PATH
                                INTEGER_LIT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `{not visited}` → prefix expression as inner expression.
#[test]
fn inner_expr_prefix_not() {
    assert_equivalent(
        parse("{not visited}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PREFIX_EXPR {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `{x > 5 and y < 10}` → nested infix expression.
#[test]
fn inner_expr_complex_infix() {
    assert_equivalent(
        parse("{x > 5 and y < 10}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
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
                        }
                    }
                }
            }
        }),
    );
}

/// `{greet(name)}` → function call as inner expression.
#[test]
fn inner_expr_function_call() {
    assert_equivalent(
        parse("{greet(name)}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            FUNCTION_CALL {
                                IDENTIFIER
                                ARG_LIST {
                                    PATH
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `{count++}` → postfix expression as inner expression.
#[test]
fn inner_expr_postfix() {
    assert_equivalent(
        parse("{count++}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            POSTFIX_EXPR {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

// ── Section B: Inline conditionals ──────────────────────────────────

/// `{x: yes}` → conditional with true branch only.
#[test]
fn cond_true_only() {
    assert_equivalent(
        parse("{x: yes}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
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

/// `{x: yes|no}` → conditional with true and false branches.
#[test]
fn cond_true_and_false() {
    assert_equivalent(
        parse("{x: yes|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
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
            }
        }),
    );
}

/// `{x > 5: big|small}` → infix expression condition.
#[test]
fn cond_infix_expr() {
    assert_equivalent(
        parse("{x > 5: big|small}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            INFIX_EXPR {
                                PATH
                                INTEGER_LIT
                            }
                            INLINE_BRANCHES_COND {
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
            }
        }),
    );
}

/// `{x: |no}` → empty true branch.
#[test]
fn cond_empty_true_branch() {
    assert_equivalent(
        parse("{x: |no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT
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

/// `{x: yes|}` → empty false branch.
#[test]
fn cond_empty_false_branch() {
    assert_equivalent(
        parse("{x: yes|}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                }
                                BRANCH_CONTENT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `{x: |}` → both branches empty.
#[test]
fn cond_empty_both() {
    assert_equivalent(
        parse("{x: |}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT
                                BRANCH_CONTENT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `{x:}` → no content after colon, empty `INLINE_BRANCHES_COND`.
#[test]
fn cond_empty_body() {
    assert_equivalent(
        parse("{x:}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `{check(x): passed}` → function call as condition.
#[test]
fn cond_function_call_expr() {
    assert_equivalent(
        parse("{check(x): passed}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            FUNCTION_CALL {
                                IDENTIFIER
                                ARG_LIST {
                                    PATH
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

/// `{not done: continue}` → prefix expression as condition.
#[test]
fn cond_prefix_not_expr() {
    assert_equivalent(
        parse("{not done: continue}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PREFIX_EXPR {
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

/// `Hello {x: world} goodbye` → text around inline logic.
#[test]
fn cond_with_text_around() {
    assert_equivalent(
        parse("Hello {x: world} goodbye\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

// ── Section C: Implicit sequences ───────────────────────────────────

/// `{a|b}` → minimal implicit sequence.
#[test]
fn implicit_seq_two() {
    assert_equivalent(
        parse("{a|b}\n"),
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

/// `{a|b|c}` → three-branch implicit sequence.
#[test]
fn implicit_seq_three() {
    assert_equivalent(
        parse("{a|b|c}\n"),
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

/// `{First.|Second.|Third.|Fourth.}` → four full-sentence branches.
#[test]
fn implicit_seq_four_sentences() {
    assert_equivalent(
        parse("{First.|Second.|Third.|Fourth.}\n"),
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

/// `{|b|c}` → empty first branch.
#[test]
fn implicit_seq_empty_first() {
    assert_equivalent(
        parse("{|b|c}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        IMPLICIT_SEQUENCE {
                            BRANCH_CONTENT
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

/// `{a||c}` → empty middle branch. Two adjacent pipes are sequence separators
/// (not the OR operator) in implicit-sequence context.
#[test]
fn implicit_seq_empty_middle() {
    assert_equivalent(
        parse("{a||c}\n"),
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

/// `{a|b|}` → empty last branch.
#[test]
fn implicit_seq_empty_last() {
    assert_equivalent(
        parse("{a|b|}\n"),
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
                        }
                    }
                }
            }
        }),
    );
}

/// `{The {&big|small} dog.|The cat.}` → nested inline logic in branch.
#[test]
fn implicit_seq_with_nested() {
    assert_equivalent(
        parse("{The {&big|small} dog.|The cat.}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        IMPLICIT_SEQUENCE {
                            BRANCH_CONTENT {
                                TEXT
                                INLINE_LOGIC {
                                    SEQUENCE_WITH_ANNOTATION {
                                        SEQUENCE_SYMBOL_ANNOTATION
                                        INLINE_BRANCHES_SEQ {
                                            BRANCH_CONTENT {
                                                TEXT
                                            }
                                            BRANCH_CONTENT {
                                                TEXT
                                            }
                                        }
                                    }
                                }
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

/// `{->Fish1->|->Fish2->|nothing.}` → diverts in sequence branches.
#[test]
fn implicit_seq_with_diverts() {
    assert_equivalent(
        parse("{->Fish1->|->Fish2->|nothing.}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        IMPLICIT_SEQUENCE {
                            BRANCH_CONTENT {
                                DIVERT_NODE {
                                    TUNNEL_CALL_NODE {
                                        DIVERT_TARGET_WITH_ARGS {
                                            PATH
                                        }
                                    }
                                }
                            }
                            BRANCH_CONTENT {
                                DIVERT_NODE {
                                    TUNNEL_CALL_NODE {
                                        DIVERT_TARGET_WITH_ARGS {
                                            PATH
                                        }
                                    }
                                }
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

// ── Section D: Symbol-annotated sequences ───────────────────────────

/// `{&first|second|third}` → cycle annotation.
#[test]
fn sym_seq_cycle() {
    assert_equivalent(
        parse("{&first|second|third}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_SYMBOL_ANNOTATION
                            INLINE_BRANCHES_SEQ {
                                BRANCH_CONTENT {
                                    TEXT
                                }
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
            }
        }),
    );
}

/// `{!first|second}` → once annotation.
#[test]
fn sym_seq_once() {
    assert_equivalent(
        parse("{!first|second}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_SYMBOL_ANNOTATION
                            INLINE_BRANCHES_SEQ {
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
            }
        }),
    );
}

/// `{~first|second|third}` → shuffle annotation.
#[test]
fn sym_seq_shuffle() {
    assert_equivalent(
        parse("{~first|second|third}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_SYMBOL_ANNOTATION
                            INLINE_BRANCHES_SEQ {
                                BRANCH_CONTENT {
                                    TEXT
                                }
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
            }
        }),
    );
}

/// `{$first|second}` → stopping annotation.
#[test]
fn sym_seq_stopping() {
    assert_equivalent(
        parse("{$first|second}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_SYMBOL_ANNOTATION
                            INLINE_BRANCHES_SEQ {
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
            }
        }),
    );
}

/// `{&!first|second}` → combined symbol annotations.
#[test]
fn sym_seq_combined() {
    assert_equivalent(
        parse("{&!first|second}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_SYMBOL_ANNOTATION
                            INLINE_BRANCHES_SEQ {
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
            }
        }),
    );
}

/// `{&a|b}` → minimal symbol-annotated sequence.
#[test]
fn sym_seq_two_branches() {
    assert_equivalent(
        parse("{&a|b}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_SYMBOL_ANNOTATION
                            INLINE_BRANCHES_SEQ {
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
            }
        }),
    );
}

// ── Section E: Word-annotated sequences ─────────────────────────────

/// `{stopping: first|second|third}` → stopping word annotation.
#[test]
fn word_seq_stopping() {
    assert_equivalent(
        parse("{stopping: first|second|third}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_WORD_ANNOTATION
                            INLINE_BRANCHES_SEQ {
                                BRANCH_CONTENT {
                                    TEXT
                                }
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
            }
        }),
    );
}

/// `{cycle: a|b|c}` → cycle word annotation.
#[test]
fn word_seq_cycle() {
    assert_equivalent(
        parse("{cycle: a|b|c}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_WORD_ANNOTATION
                            INLINE_BRANCHES_SEQ {
                                BRANCH_CONTENT {
                                    TEXT
                                }
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
            }
        }),
    );
}

/// `{shuffle: a|b|c}` → shuffle word annotation.
#[test]
fn word_seq_shuffle() {
    assert_equivalent(
        parse("{shuffle: a|b|c}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_WORD_ANNOTATION
                            INLINE_BRANCHES_SEQ {
                                BRANCH_CONTENT {
                                    TEXT
                                }
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
            }
        }),
    );
}

/// `{once: a|b}` → once word annotation.
#[test]
fn word_seq_once() {
    assert_equivalent(
        parse("{once: a|b}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_WORD_ANNOTATION
                            INLINE_BRANCHES_SEQ {
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
            }
        }),
    );
}

/// `{stopping: a|b}` → minimal word-annotated sequence.
#[test]
fn word_seq_two_branches() {
    assert_equivalent(
        parse("{stopping: a|b}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_WORD_ANNOTATION
                            INLINE_BRANCHES_SEQ {
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
            }
        }),
    );
}

// ── Section F: Multiline blocks — conditional branches ──────────────

/// Two conditional branches with else.
#[test]
fn multiline_two_cond_branches() {
    assert_equivalent(
        parse("{\n- x > 5:\n  Big.\n- else:\n  Small.\n}\n"),
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
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Three conditional branches with else.
#[test]
fn multiline_three_cond_branches() {
    assert_equivalent(
        parse("{\n- x > 10:\n  Very big.\n- x > 5:\n  Big.\n- else:\n  Small.\n}\n"),
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
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Single conditional branch without else.
#[test]
fn multiline_cond_no_else() {
    assert_equivalent(
        parse("{\n- x:\n  Yes.\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Bare branches without conditions.
#[test]
fn multiline_bare_branches() {
    assert_equivalent(
        parse("{\n- Branch one.\n- Branch two.\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        MULTILINE_BRANCH_BODY {
                            TEXT
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
}

/// Branch with infix expression condition.
#[test]
fn multiline_cond_with_expr() {
    assert_equivalent(
        parse("{\n- x > 5:\n  Big.\n}\n"),
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
                }
            }
        }),
    );
}

/// Just an else branch.
#[test]
fn multiline_else_only() {
    assert_equivalent(
        parse("{\n- else:\n  Fallback.\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        MULTILINE_BRANCH_BODY {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

// ── Section G: Multiline blocks — sequence branches ─────────────────

/// Multiline stopping sequence with word annotation.
#[test]
fn multiline_seq_stopping() {
    assert_equivalent(
        parse("{\nstopping:\n- first\n- second\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                SEQUENCE_WITH_ANNOTATION {
                    SEQUENCE_WORD_ANNOTATION
                    MULTILINE_BRANCHES_SEQ {
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Multiline cycle sequence.
#[test]
fn multiline_seq_cycle() {
    assert_equivalent(
        parse("{\ncycle:\n- a\n- b\n- c\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                SEQUENCE_WITH_ANNOTATION {
                    SEQUENCE_WORD_ANNOTATION
                    MULTILINE_BRANCHES_SEQ {
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Multiline sequence with symbol annotation.
#[test]
fn multiline_seq_symbol() {
    assert_equivalent(
        parse("{\n&\n- first\n- second\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                SEQUENCE_WITH_ANNOTATION {
                    SEQUENCE_SYMBOL_ANNOTATION
                    MULTILINE_BRANCHES_SEQ {
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Multiline stopping sequence with three branches.
#[test]
fn multiline_seq_three_branches() {
    assert_equivalent(
        parse("{\nstopping:\n- one\n- two\n- three\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                SEQUENCE_WITH_ANNOTATION {
                    SEQUENCE_WORD_ANNOTATION
                    MULTILINE_BRANCHES_SEQ {
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                        MULTILINE_BRANCH_SEQ {
                            MULTILINE_BRANCH_BODY {
                                TEXT
                            }
                        }
                    }
                }
            }
        }),
    );
}

// ── Section H: Multiline conditional in INLINE_LOGIC ────────────────

/// Multiline conditional inside inline logic context.
#[test]
fn inline_multiline_conditional() {
    assert_equivalent(
        parse("Hello {\n- x:\n  Yes.\n- else:\n  No.\n}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        MULTILINE_CONDITIONAL {
                            MULTILINE_BRANCH_COND {
                                PATH
                                MULTILINE_BRANCH_BODY {
                                    TEXT
                                }
                            }
                            MULTILINE_BRANCH_COND {
                                MULTILINE_BRANCH_BODY {
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

/// Bare multiline branches in inline logic.
#[test]
fn inline_multiline_bare_branches() {
    assert_equivalent(
        parse("Hello {\n- One.\n- Two.\n}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        MULTILINE_CONDITIONAL {
                            MULTILINE_BRANCH_COND {
                                MULTILINE_BRANCH_BODY {
                                    TEXT
                                }
                            }
                            MULTILINE_BRANCH_COND {
                                MULTILINE_BRANCH_BODY {
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

// ── Section I: Branchless conditional body ──────────────────────────

/// Branchless conditional body as multiline block.
#[test]
fn branchless_body_simple() {
    assert_equivalent(
        parse("{\n  x:\n  Content here.\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                CONDITIONAL_WITH_EXPR {
                    PATH
                    BRANCHLESS_COND_BODY {
                        TEXT
                    }
                }
            }
        }),
    );
}

/// Branchless conditional body with else branch.
#[test]
fn branchless_body_with_else() {
    assert_equivalent(
        parse("{\n  x:\n  Content.\n- else:\n  Other.\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                CONDITIONAL_WITH_EXPR {
                    PATH
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
        }),
    );
}

// ── Section J: Nested inline logic ──────────────────────────────────

/// Conditional inside true branch of another conditional.
#[test]
fn nested_cond_in_cond_true() {
    assert_equivalent(
        parse("{x: {y: inner}|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    INLINE_LOGIC {
                                        CONDITIONAL_WITH_EXPR {
                                            PATH
                                            INLINE_BRANCHES_COND {
                                                BRANCH_CONTENT {
                                                    TEXT
                                                }
                                            }
                                        }
                                    }
                                }
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

/// Expression inside sequence branch.
#[test]
fn nested_expr_in_branch() {
    assert_equivalent(
        parse("{a|{x}|c}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        IMPLICIT_SEQUENCE {
                            BRANCH_CONTENT {
                                TEXT
                            }
                            BRANCH_CONTENT {
                                INLINE_LOGIC {
                                    INNER_EXPRESSION {
                                        PATH
                                    }
                                }
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

/// Annotated sequence inside conditional.
#[test]
fn nested_seq_in_cond() {
    assert_equivalent(
        parse("{x: {&a|b}|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    INLINE_LOGIC {
                                        SEQUENCE_WITH_ANNOTATION {
                                            SEQUENCE_SYMBOL_ANNOTATION
                                            INLINE_BRANCHES_SEQ {
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

/// Three levels of nesting.
#[test]
fn deeply_nested() {
    assert_equivalent(
        parse("{x: {y: {z: deep}}}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    INLINE_LOGIC {
                                        CONDITIONAL_WITH_EXPR {
                                            PATH
                                            INLINE_BRANCHES_COND {
                                                BRANCH_CONTENT {
                                                    INLINE_LOGIC {
                                                        CONDITIONAL_WITH_EXPR {
                                                            PATH
                                                            INLINE_BRANCHES_COND {
                                                                BRANCH_CONTENT {
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
                        }
                    }
                }
            }
        }),
    );
}

// ── Section K: Branch content with special elements ─────────────────

/// Divert in conditional branch.
#[test]
fn branch_with_divert() {
    assert_equivalent(
        parse("{x: -> target|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    DIVERT_NODE {
                                        SIMPLE_DIVERT {
                                            DIVERT_TARGET_WITH_ARGS {
                                                PATH
                                            }
                                        }
                                    }
                                }
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

/// Glue in conditional branch.
#[test]
fn branch_with_glue() {
    assert_equivalent(
        parse("{x: <>glued|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    GLUE_NODE
                                    TEXT
                                }
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

/// Escape in conditional branch (escaped pipe).
#[test]
fn branch_with_escape() {
    assert_equivalent(
        parse("{x: hello\\|world|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                    ESCAPE
                                    TEXT
                                }
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

/// Thread start in conditional branch.
#[test]
fn branch_with_thread() {
    assert_equivalent(
        parse("{x: <- thread|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    DIVERT_NODE {
                                        THREAD_START {
                                            PATH
                                        }
                                    }
                                }
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

/// Diverts as sequence alternatives.
#[test]
fn branch_divert_in_seq() {
    assert_equivalent(
        parse("{&-> a|-> b}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        SEQUENCE_WITH_ANNOTATION {
                            SEQUENCE_SYMBOL_ANNOTATION
                            INLINE_BRANCHES_SEQ {
                                BRANCH_CONTENT {
                                    DIVERT_NODE {
                                        SIMPLE_DIVERT {
                                            DIVERT_TARGET_WITH_ARGS {
                                                PATH
                                            }
                                        }
                                    }
                                }
                                BRANCH_CONTENT {
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
            }
        }),
    );
}

// ── Section L: In context ───────────────────────────────────────────

/// Inline logic in content line: `TEXT` + `INLINE_LOGIC` + `TEXT`.
#[test]
fn inline_in_content_line() {
    assert_equivalent(
        parse("Hello {x} world\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

/// Inline logic in choice start content.
#[test]
fn inline_in_choice() {
    assert_equivalent(
        parse("* Choice {x: yes|no}\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
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
            }
        }),
    );
}

/// Inline logic in gather.
#[test]
fn inline_in_gather() {
    assert_equivalent(
        parse("- Gathered {x}\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Two inline logics in one line.
#[test]
fn multiple_inline_in_line() {
    assert_equivalent(
        parse("{x} and {y}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Inline logic then divert on same line.
#[test]
fn inline_before_divert() {
    assert_equivalent(
        parse("{x} -> target\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

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
