use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

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
