use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

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
