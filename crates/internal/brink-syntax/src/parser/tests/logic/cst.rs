use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── 1. Logic lines — Return statements ──────────────────────────────

/// `~ return` — bare return, no expression.
#[test]
fn return_bare() {
    assert_equivalent(
        parse("~ return\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT
            }
        }),
    );
}

/// `~ return 5` — return with integer literal.
#[test]
fn return_integer() {
    assert_equivalent(
        parse("~ return 5\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT {
                    INTEGER_LIT
                }
            }
        }),
    );
}

/// `~ return x + 1` — return with infix expression.
#[test]
fn return_infix_expr() {
    assert_equivalent(
        parse("~ return x + 1\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT {
                    INFIX_EXPR {
                        PATH
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

/// `~ return "hello"` — return with string literal.
#[test]
fn return_string() {
    assert_equivalent(
        parse("~ return \"hello\"\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT {
                    STRING_LIT
                }
            }
        }),
    );
}

/// `~ return true` — return with boolean literal.
#[test]
fn return_boolean() {
    assert_equivalent(
        parse("~ return true\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT {
                    BOOLEAN_LIT
                }
            }
        }),
    );
}

/// `~ return foo()` — return with function call (no args).
#[test]
fn return_function_call() {
    assert_equivalent(
        parse("~ return foo()\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT {
                    FUNCTION_CALL {
                        IDENTIFIER
                    }
                }
            }
        }),
    );
}

/// `~ return foo(1)` — return with function call with args.
#[test]
fn return_function_call_with_arg() {
    assert_equivalent(
        parse("~ return foo(1)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT {
                    FUNCTION_CALL {
                        IDENTIFIER
                        ARG_LIST {
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

/// `~ return (1 + 2)` — return with parenthesized expression.
#[test]
fn return_paren_expr() {
    assert_equivalent(
        parse("~ return (1 + 2)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                RETURN_STMT {
                    PAREN_EXPR {
                        INFIX_EXPR {
                            INTEGER_LIT
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

// ── 2. Logic lines — Temp declarations ──────────────────────────────

/// `~ temp x = 5` — integer value.
#[test]
fn temp_integer() {
    assert_equivalent(
        parse("~ temp x = 5\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                TEMP_DECL {
                    IDENTIFIER
                    INTEGER_LIT
                }
            }
        }),
    );
}

/// `~ temp x = 3.14` — float value.
#[test]
fn temp_float() {
    assert_equivalent(
        parse("~ temp x = 3.14\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                TEMP_DECL {
                    IDENTIFIER
                    FLOAT_LIT
                }
            }
        }),
    );
}

/// `~ temp x = "hello"` — string value.
#[test]
fn temp_string() {
    assert_equivalent(
        parse("~ temp x = \"hello\"\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                TEMP_DECL {
                    IDENTIFIER
                    STRING_LIT
                }
            }
        }),
    );
}

/// `~ temp x = true` — boolean value.
#[test]
fn temp_boolean() {
    assert_equivalent(
        parse("~ temp x = true\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                TEMP_DECL {
                    IDENTIFIER
                    BOOLEAN_LIT
                }
            }
        }),
    );
}

/// `~ temp x = a + b` — infix expression value.
#[test]
fn temp_infix_expr() {
    assert_equivalent(
        parse("~ temp x = a + b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                TEMP_DECL {
                    IDENTIFIER
                    INFIX_EXPR {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

/// `~ temp x = foo(1)` — function call value.
#[test]
fn temp_function_call() {
    assert_equivalent(
        parse("~ temp x = foo(1)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                TEMP_DECL {
                    IDENTIFIER
                    FUNCTION_CALL {
                        IDENTIFIER
                        ARG_LIST {
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

// ── 3. Logic lines — Assignments ────────────────────────────────────

/// `~ x = 10` — simple assignment with integer.
#[test]
fn assign_integer() {
    assert_equivalent(
        parse("~ x = 10\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    INTEGER_LIT
                }
            }
        }),
    );
}

/// `~ x += 1` — compound add assignment.
#[test]
fn assign_plus_eq() {
    assert_equivalent(
        parse("~ x += 1\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    INTEGER_LIT
                }
            }
        }),
    );
}

/// `~ x -= 1` — compound subtract assignment.
#[test]
fn assign_minus_eq() {
    assert_equivalent(
        parse("~ x -= 1\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    INTEGER_LIT
                }
            }
        }),
    );
}

/// `~ x = a + b` — assignment with infix expression.
#[test]
fn assign_infix_expr() {
    assert_equivalent(
        parse("~ x = a + b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    INFIX_EXPR {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

/// `~ x = foo()` — assignment with function call.
#[test]
fn assign_function_call() {
    assert_equivalent(
        parse("~ x = foo()\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    FUNCTION_CALL {
                        IDENTIFIER
                    }
                }
            }
        }),
    );
}

/// `~ x = "hello"` — assignment with string literal.
#[test]
fn assign_string() {
    assert_equivalent(
        parse("~ x = \"hello\"\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    STRING_LIT
                }
            }
        }),
    );
}

/// `~ x = (a, b, c)` — assignment with list expression.
#[test]
fn assign_list_expr() {
    assert_equivalent(
        parse("~ x = (a, b, c)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    LIST_EXPR {
                        PATH
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

/// `~ x = true` — assignment with boolean.
#[test]
fn assign_boolean() {
    assert_equivalent(
        parse("~ x = true\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT {
                    PATH
                    BOOLEAN_LIT
                }
            }
        }),
    );
}

// ── 4. Logic lines — Bare expressions ───────────────────────────────

/// `~ foo()` — bare function call (no args).
#[test]
fn bare_function_call_no_args() {
    assert_equivalent(
        parse("~ foo()\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                FUNCTION_CALL {
                    IDENTIFIER
                }
            }
        }),
    );
}

/// `~ x++` — bare postfix increment.
#[test]
fn bare_postfix_increment() {
    assert_equivalent(
        parse("~ x++\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                POSTFIX_EXPR {
                    PATH
                }
            }
        }),
    );
}

/// `~ x--` — bare postfix decrement.
#[test]
fn bare_postfix_decrement() {
    assert_equivalent(
        parse("~ x--\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                POSTFIX_EXPR {
                    PATH
                }
            }
        }),
    );
}

/// `~ foo(1, 2)` — bare function call with multiple args.
#[test]
fn bare_function_call_with_args() {
    assert_equivalent(
        parse("~ foo(1, 2)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                FUNCTION_CALL {
                    IDENTIFIER
                    ARG_LIST {
                        INTEGER_LIT
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

/// `~ foo(x, y)` — bare function call with path args.
#[test]
fn bare_function_call_path_args() {
    assert_equivalent(
        parse("~ foo(x, y)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                FUNCTION_CALL {
                    IDENTIFIER
                    ARG_LIST {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

// ── 5. Inline logic — Inner expressions ─────────────────────────────

/// `{x}` in content — inner expression with path.
#[test]
fn inline_inner_expr_path() {
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

/// `{5}` — inner expression with integer literal.
#[test]
fn inline_inner_expr_integer() {
    assert_equivalent(
        parse("Hello {5}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
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

/// `{x + 1}` — inner expression with infix expression.
#[test]
fn inline_inner_expr_infix() {
    assert_equivalent(
        parse("Hello {x + 1}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
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

/// `{greet(name)}` — inner expression with function call.
#[test]
fn inline_inner_expr_function_call() {
    assert_equivalent(
        parse("Hello {greet(name)}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
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

// ── 6. Inline logic — Conditionals with inline branches ─────────────

/// `{x: yes}` — conditional with single true branch.
#[test]
fn conditional_true_only() {
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

/// `{x: yes|no}` — conditional with true and false branches.
#[test]
fn conditional_true_and_false() {
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

/// `{x > 5: big|small}` — conditional with infix expression condition.
#[test]
fn conditional_infix_condition() {
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

/// `{x:}` — empty conditional (empty `BRANCH_CONTENT`).
#[test]
fn conditional_empty() {
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

/// `{x: yes|}` — true has content, false is empty.
#[test]
fn conditional_true_content_false_empty() {
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

/// `{x: |no}` — true is empty, false has content.
#[test]
fn conditional_true_empty_false_content() {
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

/// `{x: content with spaces}` — branch content with spaces.
#[test]
fn conditional_content_with_spaces() {
    assert_equivalent(
        parse("{x: content with spaces}\n"),
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

/// `{x: {y}|no}` — nested inline logic in true branch.
#[test]
fn conditional_nested_inline_logic() {
    assert_equivalent(
        parse("{x: {y}|no}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
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
            }
        }),
    );
}

// ── 7. Inline logic — Sequences with symbol annotations ─────────────

/// `{&a|b|c}` — cycle annotation with three branches.
#[test]
fn sequence_symbol_amp_three() {
    assert_equivalent(
        parse("{&a|b|c}\n"),
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

/// `{!a|b}` — once-only annotation.
#[test]
fn sequence_symbol_bang() {
    assert_equivalent(
        parse("{!a|b}\n"),
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

/// `{~a|b}` — shuffle annotation.
#[test]
fn sequence_symbol_tilde() {
    assert_equivalent(
        parse("{~a|b}\n"),
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

/// `{$a|b}` — stopping annotation.
#[test]
fn sequence_symbol_dollar() {
    assert_equivalent(
        parse("{$a|b}\n"),
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

/// `{&a|b}` — cycle annotation with two branches.
#[test]
fn sequence_symbol_amp_two() {
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

/// `{&a|b|c|d}` — cycle annotation with four branches.
#[test]
fn sequence_symbol_amp_four() {
    assert_equivalent(
        parse("{&a|b|c|d}\n"),
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

// ── 8. Inline logic — Sequences with word annotations ───────────────

/// `{stopping: a|b}` — stopping word annotation.
#[test]
fn sequence_word_stopping() {
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

/// `{cycle: a|b|c}` — cycle word annotation with three branches.
#[test]
fn sequence_word_cycle() {
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

/// `{shuffle: a|b}` — shuffle word annotation.
#[test]
fn sequence_word_shuffle() {
    assert_equivalent(
        parse("{shuffle: a|b}\n"),
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

/// `{once: a|b}` — once word annotation.
#[test]
fn sequence_word_once() {
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

/// `{stopping: a|b|c|d}` — four branches.
#[test]
fn sequence_word_stopping_four() {
    assert_equivalent(
        parse("{stopping: a|b|c|d}\n"),
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

// ── 9. Inline logic — Implicit sequences ────────────────────────────

/// `{a|b|c}` — three-branch implicit sequence.
#[test]
fn implicit_sequence_three() {
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

/// `{a|b}` — two-branch implicit sequence.
#[test]
fn implicit_sequence_two() {
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

/// `{Hello world.|Goodbye.}` — sentence-style content.
#[test]
fn implicit_sequence_sentences() {
    assert_equivalent(
        parse("{Hello world.|Goodbye.}\n"),
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

/// `{a|b|c|d|e}` — five branches.
#[test]
fn implicit_sequence_five() {
    assert_equivalent(
        parse("{a|b|c|d|e}\n"),
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

/// `{text with spaces|more text}` — branches with spaces.
#[test]
fn implicit_sequence_with_spaces() {
    assert_equivalent(
        parse("{text with spaces|more text}\n"),
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

// ── 10. Multiline conditionals ──────────────────────────────────────
//
// Standalone `{\n...\n}` at the start of a line is parsed as a
// `MULTILINE_BLOCK` directly under `SOURCE_FILE` (not wrapped in
// `CONTENT_LINE > MIXED_CONTENT > INLINE_LOGIC`).

/// Multiline conditional with bare branches (no leading expression).
#[test]
fn multiline_conditional_bare_branches() {
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

/// Multiline block conditional with single branch.
#[test]
fn multiline_block_conditional() {
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

/// Multiline conditional with multiple body lines.
#[test]
fn multiline_conditional_multi_body() {
    assert_equivalent(
        parse("{\n- x:\n  Line one.\n  Line two.\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            TEXT
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Multiline conditional with three branches.
#[test]
fn multiline_conditional_three_branches() {
    assert_equivalent(
        parse("{\n- x:\n  A.\n- y:\n  B.\n- else:\n  C.\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                MULTILINE_BRANCHES_COND {
                    MULTILINE_BRANCH_COND {
                        PATH
                        MULTILINE_BRANCH_BODY {
                            TEXT
                        }
                    }
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
        }),
    );
}

// ── 11. Multiline sequences ─────────────────────────────────────────

/// `{&\n- first\n- second\n}` — multiline sequence with symbol annotation.
#[test]
fn multiline_sequence_symbol() {
    assert_equivalent(
        parse("{&\n- first\n- second\n}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
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
                }
            }
        }),
    );
}

/// `{stopping:\n- first\n- second\n}` — multiline sequence with word annotation.
#[test]
fn multiline_sequence_word() {
    assert_equivalent(
        parse("{stopping:\n- first\n- second\n}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
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
                }
            }
        }),
    );
}

/// Three multiline sequence branches.
#[test]
fn multiline_sequence_three_branches() {
    assert_equivalent(
        parse("{&\n- a\n- b\n- c\n}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
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
                                MULTILINE_BRANCH_SEQ {
                                    MULTILINE_BRANCH_BODY {
                                        TEXT
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

// ── 12. Variant uniformity ──────────────────────────────────────────
//
// Every `LOGIC_LINE` must have exactly one statement child from:
// [RETURN_STMT, TEMP_DECL, ASSIGNMENT] or a bare expression node.
//
// Every `INLINE_LOGIC` must have exactly one dispatch child from:
// [INNER_EXPRESSION, CONDITIONAL_WITH_EXPR, SEQUENCE_WITH_ANNOTATION,
//  IMPLICIT_SEQUENCE, MULTILINE_CONDITIONAL].

const LOGIC_LINE_STATEMENT_KINDS: [SyntaxKind; 3] = [
    SyntaxKind::RETURN_STMT,
    SyntaxKind::TEMP_DECL,
    SyntaxKind::ASSIGNMENT,
];

const INLINE_LOGIC_DISPATCH_KINDS: [SyntaxKind; 5] = [
    SyntaxKind::INNER_EXPRESSION,
    SyntaxKind::CONDITIONAL_WITH_EXPR,
    SyntaxKind::SEQUENCE_WITH_ANNOTATION,
    SyntaxKind::IMPLICIT_SEQUENCE,
    SyntaxKind::MULTILINE_CONDITIONAL,
];

/// Assert that every `LOGIC_LINE` in `src` has at most one statement wrapper child.
fn assert_logic_line_uniformity(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::LOGIC_LINE {
            let stmt_children: Vec<_> = node
                .children()
                .filter(|c| LOGIC_LINE_STATEMENT_KINDS.contains(&c.kind()))
                .collect();
            assert!(
                stmt_children.len() <= 1,
                "LOGIC_LINE should have at most one statement child, found {} in `{src}`:\n  {:?}",
                stmt_children.len(),
                stmt_children
                    .iter()
                    .map(crate::SyntaxNode::kind)
                    .collect::<Vec<_>>(),
            );
        }
    }
}

/// Assert that every `INLINE_LOGIC` in `src` has exactly one dispatch child.
fn assert_inline_logic_uniformity(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::INLINE_LOGIC {
            let dispatch_children: Vec<_> = node
                .children()
                .filter(|c| INLINE_LOGIC_DISPATCH_KINDS.contains(&c.kind()))
                .collect();
            assert_eq!(
                dispatch_children.len(),
                1,
                "INLINE_LOGIC should have exactly one dispatch child, found {} in `{src}`:\n  {:?}",
                dispatch_children.len(),
                dispatch_children
                    .iter()
                    .map(crate::SyntaxNode::kind)
                    .collect::<Vec<_>>(),
            );
        }
    }
}

#[test]
fn uniformity_logic_line_return() {
    assert_logic_line_uniformity("~ return 5\n");
}

#[test]
fn uniformity_logic_line_temp() {
    assert_logic_line_uniformity("~ temp x = 5\n");
}

#[test]
fn uniformity_logic_line_assignment() {
    assert_logic_line_uniformity("~ x = 10\n");
}

#[test]
fn uniformity_logic_line_bare_expr() {
    assert_logic_line_uniformity("~ foo()\n");
}

#[test]
fn uniformity_logic_line_postfix() {
    assert_logic_line_uniformity("~ x++\n");
}

#[test]
fn uniformity_logic_line_bare_return() {
    assert_logic_line_uniformity("~ return\n");
}

#[test]
fn uniformity_inline_logic_inner_expr() {
    assert_inline_logic_uniformity("Hello {x}\n");
}

#[test]
fn uniformity_inline_logic_conditional() {
    assert_inline_logic_uniformity("{x: yes|no}\n");
}

#[test]
fn uniformity_inline_logic_sequence_symbol() {
    assert_inline_logic_uniformity("{&a|b}\n");
}

#[test]
fn uniformity_inline_logic_sequence_word() {
    assert_inline_logic_uniformity("{stopping: a|b}\n");
}

#[test]
fn uniformity_inline_logic_implicit_seq() {
    assert_inline_logic_uniformity("{a|b|c}\n");
}

/// Inline multiline conditional (preceded by content, so it's inline).
#[test]
fn uniformity_inline_logic_multiline_cond() {
    assert_inline_logic_uniformity("Hello {\n- x:\n  Yes.\n}\n");
}

// ── 13. Positive/negative exclusivity ───────────────────────────────

/// Helper: returns `true` if any descendant has the given kind.
fn has_kind(src: &str, kind: SyntaxKind) -> bool {
    let p = parse(src);
    p.syntax().descendants().any(|n| n.kind() == kind)
}

/// `~ return 5` has `RETURN_STMT`, not `TEMP_DECL` or `ASSIGNMENT`.
#[test]
fn exclusivity_return_has_return_stmt() {
    let src = "~ return 5\n";
    assert!(has_kind(src, SyntaxKind::RETURN_STMT));
    assert!(!has_kind(src, SyntaxKind::TEMP_DECL));
    assert!(!has_kind(src, SyntaxKind::ASSIGNMENT));
}

/// `~ temp x = 5` has `TEMP_DECL`, not `RETURN_STMT` or `ASSIGNMENT`.
#[test]
fn exclusivity_temp_has_temp_decl() {
    let src = "~ temp x = 5\n";
    assert!(has_kind(src, SyntaxKind::TEMP_DECL));
    assert!(!has_kind(src, SyntaxKind::RETURN_STMT));
    assert!(!has_kind(src, SyntaxKind::ASSIGNMENT));
}

/// `~ x = 5` has `ASSIGNMENT`, not `RETURN_STMT` or `TEMP_DECL`.
#[test]
fn exclusivity_assign_has_assignment() {
    let src = "~ x = 5\n";
    assert!(has_kind(src, SyntaxKind::ASSIGNMENT));
    assert!(!has_kind(src, SyntaxKind::RETURN_STMT));
    assert!(!has_kind(src, SyntaxKind::TEMP_DECL));
}

/// `~ x++` has none of `RETURN_STMT`, `TEMP_DECL`, `ASSIGNMENT` (bare expression).
#[test]
fn exclusivity_bare_expr_has_no_statement() {
    let src = "~ x++\n";
    assert!(!has_kind(src, SyntaxKind::RETURN_STMT));
    assert!(!has_kind(src, SyntaxKind::TEMP_DECL));
    assert!(!has_kind(src, SyntaxKind::ASSIGNMENT));
    assert!(has_kind(src, SyntaxKind::POSTFIX_EXPR));
}

/// `{x}` has `INNER_EXPRESSION`, not other dispatch types.
#[test]
fn exclusivity_inner_expression() {
    let src = "Hello {x}\n";
    assert!(has_kind(src, SyntaxKind::INNER_EXPRESSION));
    assert!(!has_kind(src, SyntaxKind::CONDITIONAL_WITH_EXPR));
    assert!(!has_kind(src, SyntaxKind::SEQUENCE_WITH_ANNOTATION));
    assert!(!has_kind(src, SyntaxKind::IMPLICIT_SEQUENCE));
}

/// `{x: y}` has `CONDITIONAL_WITH_EXPR`, not other dispatch types.
#[test]
fn exclusivity_conditional() {
    let src = "{x: y}\n";
    assert!(has_kind(src, SyntaxKind::CONDITIONAL_WITH_EXPR));
    assert!(!has_kind(src, SyntaxKind::INNER_EXPRESSION));
    assert!(!has_kind(src, SyntaxKind::SEQUENCE_WITH_ANNOTATION));
    assert!(!has_kind(src, SyntaxKind::IMPLICIT_SEQUENCE));
}

/// `{&a|b}` has `SEQUENCE_WITH_ANNOTATION`, not other dispatch types.
#[test]
fn exclusivity_sequence_symbol() {
    let src = "{&a|b}\n";
    assert!(has_kind(src, SyntaxKind::SEQUENCE_WITH_ANNOTATION));
    assert!(!has_kind(src, SyntaxKind::CONDITIONAL_WITH_EXPR));
    assert!(!has_kind(src, SyntaxKind::INNER_EXPRESSION));
    assert!(!has_kind(src, SyntaxKind::IMPLICIT_SEQUENCE));
}

/// `{a|b}` has `IMPLICIT_SEQUENCE`, not other dispatch types.
#[test]
fn exclusivity_implicit_sequence() {
    let src = "{a|b}\n";
    assert!(has_kind(src, SyntaxKind::IMPLICIT_SEQUENCE));
    assert!(!has_kind(src, SyntaxKind::CONDITIONAL_WITH_EXPR));
    assert!(!has_kind(src, SyntaxKind::SEQUENCE_WITH_ANNOTATION));
    assert!(!has_kind(src, SyntaxKind::INNER_EXPRESSION));
}

/// `{stopping: a|b}` has `SEQUENCE_WITH_ANNOTATION`, not `IMPLICIT_SEQUENCE`.
#[test]
fn exclusivity_sequence_word_not_implicit() {
    let src = "{stopping: a|b}\n";
    assert!(has_kind(src, SyntaxKind::SEQUENCE_WITH_ANNOTATION));
    assert!(!has_kind(src, SyntaxKind::IMPLICIT_SEQUENCE));
}

/// Standalone multiline block has `MULTILINE_BLOCK` with `MULTILINE_BRANCHES_COND`,
/// not `CONDITIONAL_WITH_EXPR` or inline dispatch types.
#[test]
fn exclusivity_multiline_conditional() {
    let src = "{\n- x:\n  Yes.\n}\n";
    assert!(has_kind(src, SyntaxKind::MULTILINE_BLOCK));
    assert!(has_kind(src, SyntaxKind::MULTILINE_BRANCHES_COND));
    assert!(!has_kind(src, SyntaxKind::CONDITIONAL_WITH_EXPR));
    assert!(!has_kind(src, SyntaxKind::IMPLICIT_SEQUENCE));
    assert!(!has_kind(src, SyntaxKind::INNER_EXPRESSION));
}

// ── 14. Error recovery / edge cases ─────────────────────────────────

/// `~ return\n` with no expr — valid, no error.
#[test]
fn error_return_bare_is_valid() {
    let src = "~ return\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        p.errors().is_empty(),
        "bare return should not produce errors"
    );
}

/// `~ temp = 5\n` — missing identifier (error but lossless).
#[test]
fn error_temp_missing_ident() {
    let src = "~ temp = 5\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected error for missing identifier in temp decl"
    );
}

/// `{x:\n` — unclosed brace (error but lossless).
#[test]
fn error_unclosed_brace() {
    let src = "{x:\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// `{x: yes|` — unclosed brace with pipe (error but lossless).
#[test]
fn error_unclosed_brace_with_pipe() {
    let src = "{x: yes|";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// `{&}` — sequence annotation with no content (error but lossless).
#[test]
fn error_sequence_empty_branches() {
    let src = "{&}\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}
