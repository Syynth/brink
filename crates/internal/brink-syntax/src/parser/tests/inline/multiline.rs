use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

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
