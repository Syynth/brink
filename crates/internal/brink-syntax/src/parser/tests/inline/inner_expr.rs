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
