use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

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

/// Branchless conditional body with a choice before `- else:`.
/// Regression: `choice(p)` consumes its trailing NEWLINE, so after it
/// returns we are at the start of the next line. `at_line_start` must
/// be set to `true` so that the MINUS in `- else:` is recognized as a
/// branch separator rather than literal text.
#[test]
fn branchless_body_with_choice_then_else() {
    assert_equivalent(
        parse("{\n  x:\n  * Choice A\n- else:\n  * Choice B\n}\n"),
        cst!(SOURCE_FILE {
            MULTILINE_BLOCK {
                CONDITIONAL_WITH_EXPR {
                    PATH
                    BRANCHLESS_COND_BODY {
                        CHOICE {
                            CHOICE_BULLETS
                            CHOICE_START_CONTENT {
                                TEXT
                            }
                        }
                        ELSE_BRANCH {
                            MULTILINE_BRANCH_COND {
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
