use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── A. Literals ─────────────────────────────────────────────────────

#[test]
fn literal_integer() {
    assert_equivalent(
        parse("~ x = 42\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INTEGER_LIT
                }
            }
        }),
    );
}

#[test]
fn literal_float() {
    assert_equivalent(
        parse("~ x = 3.14\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    FLOAT_LIT
                }
            }
        }),
    );
}

#[test]
fn literal_true() {
    assert_equivalent(
        parse("~ x = true\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    BOOLEAN_LIT [KW_TRUE]
                }
            }
        }),
    );
}

#[test]
fn literal_false() {
    assert_equivalent(
        parse("~ x = false\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    BOOLEAN_LIT [KW_FALSE]
                }
            }
        }),
    );
}

#[test]
fn literal_string() {
    assert_equivalent(
        parse("~ x = \"hello\"\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    STRING_LIT
                }
            }
        }),
    );
}

#[test]
fn literal_string_empty() {
    assert_equivalent(
        parse("~ x = \"\"\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    STRING_LIT
                }
            }
        }),
    );
}

// ── B. Identifiers and Paths ────────────────────────────────────────

#[test]
fn ident_simple() {
    assert_equivalent(
        parse("~ x = y\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PATH
                }
            }
        }),
    );
}

#[test]
fn path_two_segments() {
    assert_equivalent(
        parse("~ x = a.b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PATH
                }
            }
        }),
    );
}

#[test]
fn path_three_segments() {
    assert_equivalent(
        parse("~ x = a.b.c\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PATH
                }
            }
        }),
    );
}

// ── C. Prefix Expressions ───────────────────────────────────────────

#[test]
fn prefix_negate() {
    assert_equivalent(
        parse("~ x = -1\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PREFIX_EXPR [MINUS] {
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

#[test]
fn prefix_bang() {
    assert_equivalent(
        parse("~ x = !flag\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PREFIX_EXPR [BANG] {
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn prefix_not_keyword() {
    assert_equivalent(
        parse("~ x = not flag\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PREFIX_EXPR [KW_NOT] {
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn prefix_negate_paren() {
    assert_equivalent(
        parse("~ x = -(a + b)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PREFIX_EXPR [MINUS] {
                        PAREN_EXPR {
                            INFIX_EXPR [PLUS] {
                                PATH
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

#[test]
fn prefix_not_boolean() {
    assert_equivalent(
        parse("~ x = not true\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PREFIX_EXPR [KW_NOT] {
                        BOOLEAN_LIT [KW_TRUE]
                    }
                }
            }
        }),
    );
}

// ── D. Postfix Expressions ──────────────────────────────────────────

#[test]
fn postfix_increment() {
    assert_equivalent(
        parse("~ x++\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                POSTFIX_EXPR [PLUS] {
                    PATH
                }
            }
        }),
    );
}

#[test]
fn postfix_decrement() {
    assert_equivalent(
        parse("~ x--\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                POSTFIX_EXPR [MINUS] {
                    PATH
                }
            }
        }),
    );
}

// ── E. Infix — One Per Operator ─────────────────────────────────────

#[test]
fn infix_plus() {
    assert_equivalent(
        parse("~ x = a + b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PLUS] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_minus() {
    assert_equivalent(
        parse("~ x = a - b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [MINUS] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_star() {
    assert_equivalent(
        parse("~ x = a * b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [STAR] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_slash() {
    assert_equivalent(
        parse("~ x = a / b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [SLASH] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_percent() {
    assert_equivalent(
        parse("~ x = a % b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PERCENT] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_mod() {
    assert_equivalent(
        parse("~ x = a mod b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [KW_MOD] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_caret() {
    assert_equivalent(
        parse("~ x = a ^ b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [CARET] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_lt() {
    assert_equivalent(
        parse("~ x = a < b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [LT] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_gt() {
    assert_equivalent(
        parse("~ x = a > b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [GT] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_lte() {
    assert_equivalent(
        parse("~ x = a <= b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [LT_EQ] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_gte() {
    assert_equivalent(
        parse("~ x = a >= b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [GT_EQ] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_eq_eq() {
    assert_equivalent(
        parse("~ x = a == b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [EQ_EQ] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_bang_eq() {
    assert_equivalent(
        parse("~ x = a != b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [BANG_EQ] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_amp_amp() {
    assert_equivalent(
        parse("~ x = a && b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [AMP_AMP] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_pipe_pipe() {
    assert_equivalent(
        parse("~ x = a || b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PIPE, PIPE] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_and() {
    assert_equivalent(
        parse("~ x = a and b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [KW_AND] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_or() {
    assert_equivalent(
        parse("~ x = a or b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [KW_OR] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_has() {
    assert_equivalent(
        parse("~ x = a has b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [KW_HAS] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_hasnt() {
    assert_equivalent(
        parse("~ x = a hasnt b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [KW_HASNT] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_question() {
    assert_equivalent(
        parse("~ x = a ? b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [QUESTION] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_bang_question() {
    assert_equivalent(
        parse("~ x = a !? b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [BANG_QUESTION] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_plus_eq() {
    assert_equivalent(
        parse("~ x = a += b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PLUS_EQ] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn infix_minus_eq() {
    assert_equivalent(
        parse("~ x = a -= b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [MINUS_EQ] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

// ── F. Precedence and Associativity ─────────────────────────────────

/// `1 + 2 * 3` → `+` is outer, `*` is inner right.
#[test]
fn prec_mul_over_add() {
    assert_equivalent(
        parse("~ x = 1 + 2 * 3\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PLUS] {
                        INTEGER_LIT
                        INFIX_EXPR [STAR] {
                            INTEGER_LIT
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

/// `1 * 2 + 3` → `+` is outer, `*` is inner left.
#[test]
fn prec_mul_then_add() {
    assert_equivalent(
        parse("~ x = 1 * 2 + 3\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PLUS] {
                        INFIX_EXPR [STAR] {
                            INTEGER_LIT
                            INTEGER_LIT
                        }
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

/// `a && b || c` → `||` is outer, `&&` is inner left.
#[test]
fn prec_and_over_or() {
    assert_equivalent(
        parse("~ x = a && b || c\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PIPE, PIPE] {
                        INFIX_EXPR [AMP_AMP] {
                            PATH
                            PATH
                        }
                        PATH
                    }
                }
            }
        }),
    );
}

/// `a == b && c` → `&&` is outer, `==` is inner left.
#[test]
fn prec_eq_over_and() {
    assert_equivalent(
        parse("~ x = a == b && c\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [AMP_AMP] {
                        INFIX_EXPR [EQ_EQ] {
                            PATH
                            PATH
                        }
                        PATH
                    }
                }
            }
        }),
    );
}

/// `a < b == c` → `==` is outer, `<` is inner left.
#[test]
fn prec_cmp_over_eq() {
    assert_equivalent(
        parse("~ x = a < b == c\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [EQ_EQ] {
                        INFIX_EXPR [LT] {
                            PATH
                            PATH
                        }
                        PATH
                    }
                }
            }
        }),
    );
}

/// `a + b < c` → `<` is outer, `+` is inner left.
#[test]
fn prec_add_over_cmp() {
    assert_equivalent(
        parse("~ x = a + b < c\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [LT] {
                        INFIX_EXPR [PLUS] {
                            PATH
                            PATH
                        }
                        PATH
                    }
                }
            }
        }),
    );
}

/// `a ^ b ^ c` → right-associative: `a ^ (b ^ c)`.
#[test]
fn assoc_intersect_right() {
    assert_equivalent(
        parse("~ x = a ^ b ^ c\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [CARET] {
                        PATH
                        INFIX_EXPR [CARET] {
                            PATH
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `a += b += c` → right-associative: `a += (b += c)`.
#[test]
fn assoc_compound_right() {
    assert_equivalent(
        parse("~ x = a += b += c\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PLUS_EQ] {
                        PATH
                        INFIX_EXPR [PLUS_EQ] {
                            PATH
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

// ── G. Parenthesized Expressions ────────────────────────────────────

#[test]
fn paren_simple() {
    assert_equivalent(
        parse("~ x = (1 + 2)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PAREN_EXPR {
                        INFIX_EXPR [PLUS] {
                            INTEGER_LIT
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

/// `((a))` — double-nested parens. The inner `(a)` is a single IDENT
/// which `looks_like_list_expr` classifies as a list. So `((a))` is
/// `PAREN_EXPR { LIST_EXPR { PATH } }`.
#[test]
fn paren_nested() {
    assert_equivalent(
        parse("~ x = ((a))\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PAREN_EXPR {
                        LIST_EXPR {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

#[test]
fn paren_override_prec() {
    assert_equivalent(
        parse("~ x = (1 + 2) * 3\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [STAR] {
                        PAREN_EXPR {
                            INFIX_EXPR [PLUS] {
                                INTEGER_LIT
                                INTEGER_LIT
                            }
                        }
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

// ── H. Function Calls ───────────────────────────────────────────────

#[test]
fn function_no_args() {
    assert_equivalent(
        parse("~ x = foo()\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    FUNCTION_CALL {
                        IDENTIFIER
                    }
                }
            }
        }),
    );
}

#[test]
fn function_one_arg() {
    assert_equivalent(
        parse("~ x = foo(1)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
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

#[test]
fn function_multi_args() {
    assert_equivalent(
        parse("~ x = foo(1, 2, 3)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    FUNCTION_CALL {
                        IDENTIFIER
                        ARG_LIST {
                            INTEGER_LIT
                            INTEGER_LIT
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

#[test]
fn function_expr_arg() {
    assert_equivalent(
        parse("~ x = foo(1 + 2)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    FUNCTION_CALL {
                        IDENTIFIER
                        ARG_LIST {
                            INFIX_EXPR [PLUS] {
                                INTEGER_LIT
                                INTEGER_LIT
                            }
                        }
                    }
                }
            }
        }),
    );
}

#[test]
fn function_nested() {
    assert_equivalent(
        parse("~ x = foo(bar(y))\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    FUNCTION_CALL {
                        IDENTIFIER
                        ARG_LIST {
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

/// `a.b` in expression context is a `PATH`, not a `FUNCTION_CALL`.
#[test]
fn dotted_is_path_not_call() {
    assert_equivalent(
        parse("~ x = a.b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    PATH
                }
            }
        }),
    );
}

// ── I. List Expressions ─────────────────────────────────────────────

#[test]
fn list_empty() {
    assert_equivalent(
        parse("~ x = ()\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    LIST_EXPR
                }
            }
        }),
    );
}

/// `(a)` — single ident in parens is classified as `LIST_EXPR` by the
/// `looks_like_list_expr` heuristic (IDENT followed by `R_PAREN`).
#[test]
fn list_single_ident() {
    assert_equivalent(
        parse("~ x = (a)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    LIST_EXPR {
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn list_multiple() {
    assert_equivalent(
        parse("~ x = (a, b, c)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
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

#[test]
fn list_dotted_items() {
    assert_equivalent(
        parse("~ x = (a.b, c.d)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    LIST_EXPR {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

// ── J. Divert Target Expressions ────────────────────────────────────

#[test]
fn divert_target_simple() {
    assert_equivalent(
        parse("~ x = -> knot\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    DIVERT_TARGET_EXPR [DIVERT] {
                        PATH
                    }
                }
            }
        }),
    );
}

#[test]
fn divert_target_dotted() {
    assert_equivalent(
        parse("~ x = -> k.s\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    DIVERT_TARGET_EXPR [DIVERT] {
                        PATH
                    }
                }
            }
        }),
    );
}

// ── K. Combined/Complex ─────────────────────────────────────────────

/// `1 + 2 * 3 > 4` → three-level nesting: `>` outer, `+` middle, `*` inner.
#[test]
fn mixed_prec_chain() {
    assert_equivalent(
        parse("~ x = 1 + 2 * 3 > 4\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [GT] {
                        INFIX_EXPR [PLUS] {
                            INTEGER_LIT
                            INFIX_EXPR [STAR] {
                                INTEGER_LIT
                                INTEGER_LIT
                            }
                        }
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

/// `foo(1) + bar(2)` → `INFIX_EXPR { FUNCTION_CALL, FUNCTION_CALL }`.
#[test]
fn function_as_operand() {
    assert_equivalent(
        parse("~ x = foo(1) + bar(2)\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PLUS] {
                        FUNCTION_CALL {
                            IDENTIFIER
                            ARG_LIST {
                                INTEGER_LIT
                            }
                        }
                        FUNCTION_CALL {
                            IDENTIFIER
                            ARG_LIST {
                                INTEGER_LIT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `-a + b` → `INFIX_EXPR { PREFIX_EXPR { PATH }, PATH }`.
#[test]
fn prefix_with_infix() {
    assert_equivalent(
        parse("~ x = -a + b\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [PLUS] {
                        PREFIX_EXPR [MINUS] {
                            PATH
                        }
                        PATH
                    }
                }
            }
        }),
    );
}

/// `~ x++` — postfix as bare expression in logic line (no assignment).
#[test]
fn postfix_bare() {
    assert_equivalent(
        parse("~ x++\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                POSTFIX_EXPR [PLUS] {
                    PATH
                }
            }
        }),
    );
}

/// `items has sword` → `INFIX_EXPR { PATH, PATH }`.
#[test]
fn has_with_list() {
    assert_equivalent(
        parse("~ x = items has sword\n"),
        cst!(SOURCE_FILE {
            LOGIC_LINE {
                ASSIGNMENT [EQ] {
                    PATH
                    INFIX_EXPR [KW_HAS] {
                        PATH
                        PATH
                    }
                }
            }
        }),
    );
}

// ── L. Structural Invariants ────────────────────────────────────────

/// Assert that every `INFIX_EXPR` in `src` has exactly 2 node children.
fn assert_infix_has_two_children(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::INFIX_EXPR {
            let child_count = node.children().count();
            assert_eq!(
                child_count, 2,
                "INFIX_EXPR should have exactly 2 node children, found {child_count} in `{src}`"
            );
        }
    }
}

/// Assert that every `PREFIX_EXPR` in `src` has exactly 1 node child.
fn assert_prefix_has_one_child(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::PREFIX_EXPR {
            let child_count = node.children().count();
            assert_eq!(
                child_count, 1,
                "PREFIX_EXPR should have exactly 1 node child, found {child_count} in `{src}`"
            );
        }
    }
}

/// Assert that every `FUNCTION_CALL` in `src` has `IDENTIFIER` as first child.
fn assert_function_call_starts_with_identifier(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::FUNCTION_CALL {
            let first_child = node
                .children()
                .next()
                .expect("FUNCTION_CALL should have at least one child");
            assert_eq!(
                first_child.kind(),
                SyntaxKind::IDENTIFIER,
                "FUNCTION_CALL first child should be IDENTIFIER in `{src}`"
            );
        }
    }
}

/// Assert that every `POSTFIX_EXPR` in `src` has exactly 1 node child.
fn assert_postfix_has_one_child(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::POSTFIX_EXPR {
            let child_count = node.children().count();
            assert_eq!(
                child_count, 1,
                "POSTFIX_EXPR should have exactly 1 node child, found {child_count} in `{src}`"
            );
        }
    }
}

#[test]
fn invariant_infix_simple() {
    assert_infix_has_two_children("~ x = a + b\n");
}

#[test]
fn invariant_infix_chained() {
    assert_infix_has_two_children("~ x = 1 + 2 * 3\n");
}

#[test]
fn invariant_infix_comparison() {
    assert_infix_has_two_children("~ x = a > 5\n");
}

#[test]
fn invariant_prefix_negate() {
    assert_prefix_has_one_child("~ x = -1\n");
}

#[test]
fn invariant_prefix_not() {
    assert_prefix_has_one_child("~ x = not true\n");
}

#[test]
fn invariant_prefix_bang() {
    assert_prefix_has_one_child("~ x = !flag\n");
}

#[test]
fn invariant_function_call_no_args() {
    assert_function_call_starts_with_identifier("~ x = foo()\n");
}

#[test]
fn invariant_function_call_with_args() {
    assert_function_call_starts_with_identifier("~ x = foo(1, 2)\n");
}

#[test]
fn invariant_function_call_nested() {
    assert_function_call_starts_with_identifier("~ x = foo(bar(y))\n");
}

#[test]
fn invariant_postfix_increment() {
    assert_postfix_has_one_child("~ x++\n");
}

#[test]
fn invariant_postfix_decrement() {
    assert_postfix_has_one_child("~ x--\n");
}

// ── M. Positive/Negative Assertions ─────────────────────────────────

fn has_kind(src: &str, kind: SyntaxKind) -> bool {
    let p = parse(src);
    p.syntax().descendants().any(|n| n.kind() == kind)
}

/// Integer literal contains `INTEGER_LIT`, not `FLOAT_LIT`.
#[test]
fn integer_not_float() {
    let src = "~ x = 5\n";
    assert!(has_kind(src, SyntaxKind::INTEGER_LIT));
    assert!(!has_kind(src, SyntaxKind::FLOAT_LIT));
}

/// `foo(y)` has `FUNCTION_CALL`, not `PAREN_EXPR`.
#[test]
fn call_not_paren() {
    let src = "~ x = foo(y)\n";
    assert!(has_kind(src, SyntaxKind::FUNCTION_CALL));
    assert!(!has_kind(src, SyntaxKind::PAREN_EXPR));
}

/// `(1 + 2)` has `PAREN_EXPR`, not `FUNCTION_CALL`.
#[test]
fn paren_not_call() {
    let src = "~ x = (1 + 2)\n";
    assert!(has_kind(src, SyntaxKind::PAREN_EXPR));
    assert!(!has_kind(src, SyntaxKind::FUNCTION_CALL));
}

/// `(a, b)` has `LIST_EXPR`, not `PAREN_EXPR`.
#[test]
fn list_not_paren() {
    let src = "~ x = (a, b)\n";
    assert!(has_kind(src, SyntaxKind::LIST_EXPR));
    assert!(!has_kind(src, SyntaxKind::PAREN_EXPR));
}

/// `(a)` — single ident in parens is `LIST_EXPR`, not `PAREN_EXPR`.
#[test]
fn single_ident_paren_is_list() {
    let src = "~ x = (a)\n";
    assert!(has_kind(src, SyntaxKind::LIST_EXPR));
    assert!(!has_kind(src, SyntaxKind::PAREN_EXPR));
}

// ── N. Error Recovery ───────────────────────────────────────────────

#[test]
fn error_unterminated_string() {
    let src = "~ x = \"hello\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unterminated string"
    );
}

#[test]
fn error_missing_rparen_function() {
    let src = "~ x = foo(1, 2\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing `)` in function call"
    );
}

#[test]
fn error_missing_rparen_paren_expr() {
    let src = "~ x = (1 + 2\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing `)` in paren expression"
    );
}

#[test]
fn error_missing_operand() {
    let src = "~ x = 1 +\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}
