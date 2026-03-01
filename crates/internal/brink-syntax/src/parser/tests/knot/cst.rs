use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── A. Knot header variants ────────────────────────────────────────

/// `== myKnot ==` — basic knot with trailing equals.
#[test]
fn knot_double_eq_trailing() {
    assert_equivalent(
        parse("== myKnot ==\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `=== myKnot ===` — triple equals.
#[test]
fn knot_triple_eq_trailing() {
    assert_equivalent(
        parse("=== myKnot ===\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `==== myKnot ====` — quad equals.
#[test]
fn knot_quad_eq_trailing() {
    assert_equivalent(
        parse("==== myKnot ====\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== myKnot` — no trailing equals.
#[test]
fn knot_no_trailing_eq() {
    assert_equivalent(
        parse("== myKnot\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `=== myKnot` — triple equals, no trailing.
#[test]
fn knot_triple_eq_no_trailing() {
    assert_equivalent(
        parse("=== myKnot\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== myKnot ===` — asymmetric equals.
#[test]
fn knot_asymmetric_eq() {
    assert_equivalent(
        parse("== myKnot ===\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== function greet ==` — function knot.
#[test]
fn function_knot_basic() {
    assert_equivalent(
        parse("== function greet ==\nHi!\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER [KW_FUNCTION] {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== function greet` — function knot, no trailing equals.
#[test]
fn function_knot_no_trailing_eq() {
    assert_equivalent(
        parse("== function greet\nHi!\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER [KW_FUNCTION] {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `=== function greet ===` — function knot, triple equals.
#[test]
fn function_knot_triple_eq() {
    assert_equivalent(
        parse("=== function greet ===\nHi!\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER [KW_FUNCTION] {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

// ── B. Knot parameters ─────────────────────────────────────────────

/// `== greet(name) ==` — single param.
#[test]
fn knot_single_param() {
    assert_equivalent(
        parse("== greet(name) ==\nHi.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== greet(a, b) ==` — two params.
#[test]
fn knot_two_params() {
    assert_equivalent(
        parse("== greet(a, b) ==\nHi.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== greet(a, b, c) ==` — three params.
#[test]
fn knot_three_params() {
    assert_equivalent(
        parse("== greet(a, b, c) ==\nHi.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== greet() ==` — empty parens, no `KNOT_PARAM_DECL`.
#[test]
fn knot_empty_parens() {
    assert_equivalent(
        parse("== greet() ==\nHi.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== modify(ref x) ==` — ref param.
#[test]
fn knot_ref_param() {
    assert_equivalent(
        parse("== modify(ref x) ==\nDone.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL [KW_REF] {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== modify(ref x, y) ==` — mixed ref and non-ref.
#[test]
fn knot_mixed_ref_params() {
    assert_equivalent(
        parse("== modify(ref x, y) ==\nDone.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL [KW_REF] {
                            IDENTIFIER
                        }
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== modify(ref x, ref y) ==` — all ref params.
#[test]
fn knot_all_ref_params() {
    assert_equivalent(
        parse("== modify(ref x, ref y) ==\nDone.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL [KW_REF] {
                            IDENTIFIER
                        }
                        KNOT_PARAM_DECL [KW_REF] {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== f(-> target) ==` — divert-type param.
#[test]
fn knot_divert_param() {
    assert_equivalent(
        parse("== f(-> target) ==\nDone.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL [DIVERT] {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== f(-> ref x) ==` — divert + ref param.
#[test]
fn knot_divert_ref_param() {
    assert_equivalent(
        parse("== f(-> ref x) ==\nDone.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL [DIVERT, KW_REF] {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== function greet(name) ==` — function with params.
#[test]
fn function_knot_with_param() {
    assert_equivalent(
        parse("== function greet(name) ==\nHi!\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER [KW_FUNCTION] {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== function greet(ref name) ==` — function with ref param.
#[test]
fn function_knot_with_ref_param() {
    assert_equivalent(
        parse("== function greet(ref name) ==\nHi!\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER [KW_FUNCTION] {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL [KW_REF] {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// `== function f(a, ref b, -> c) ==` — function with mixed params.
#[test]
fn function_knot_mixed_params() {
    assert_equivalent(
        parse("== function f(a, ref b, -> c) ==\nDone.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER [KW_FUNCTION] {
                    IDENTIFIER
                    KNOT_PARAMS {
                        KNOT_PARAM_DECL {
                            IDENTIFIER
                        }
                        KNOT_PARAM_DECL [KW_REF] {
                            IDENTIFIER
                        }
                        KNOT_PARAM_DECL [DIVERT] {
                            IDENTIFIER
                        }
                    }
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

// ── C. Stitch structure ────────────────────────────────────────────

/// Basic stitch inside a knot.
#[test]
fn stitch_basic() {
    assert_equivalent(
        parse("== k ==\n= s\nContent.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Stitch with parameters.
#[test]
fn stitch_with_params() {
    assert_equivalent(
        parse("== k ==\n= s(x)\nContent.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                            KNOT_PARAMS {
                                KNOT_PARAM_DECL {
                                    IDENTIFIER
                                }
                            }
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Stitch with ref parameter.
#[test]
fn stitch_with_ref_param() {
    assert_equivalent(
        parse("== k ==\n= s(ref x)\nContent.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                            KNOT_PARAMS {
                                KNOT_PARAM_DECL [KW_REF] {
                                    IDENTIFIER
                                }
                            }
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Multiple stitches in a single knot.
#[test]
fn stitch_multiple() {
    assert_equivalent(
        parse("== k ==\n= s1\nA.\n= s2\nB.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Multiple stitches with params.
#[test]
fn stitch_multiple_with_params() {
    assert_equivalent(
        parse("== k ==\n= s1(x)\nA.\n= s2(y)\nB.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                            KNOT_PARAMS {
                                KNOT_PARAM_DECL {
                                    IDENTIFIER
                                }
                            }
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                            KNOT_PARAMS {
                                KNOT_PARAM_DECL {
                                    IDENTIFIER
                                }
                            }
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Content before first stitch in knot body.
#[test]
fn stitch_content_before_first() {
    assert_equivalent(
        parse("== k ==\nBefore.\n= s\nIn stitch.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Three stitches in a single knot.
#[test]
fn stitch_three() {
    assert_equivalent(
        parse("== k ==\n= s1\nA.\n= s2\nB.\n= s3\nC.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Stitch with multiple content lines in body.
#[test]
fn stitch_multi_line_body() {
    assert_equivalent(
        parse("== k ==\n= s\nLine one.\nLine two.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

// ── D. Knot body content ──────────────────────────────────────────

/// Plain content line in knot body.
#[test]
fn body_plain_content() {
    assert_equivalent(
        parse("== k ==\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Multiple content lines in knot body.
#[test]
fn body_multiple_content_lines() {
    assert_equivalent(
        parse("== k ==\nLine one.\nLine two.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Divert in knot body.
#[test]
fn body_divert() {
    assert_equivalent(
        parse("== k ==\n-> target\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
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
        }),
    );
}

/// VAR declaration absorbed in knot body.
#[test]
fn body_var_decl() {
    assert_equivalent(
        parse("== k ==\nVAR x = 5\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    VAR_DECL {
                        IDENTIFIER
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

/// CONST declaration absorbed in knot body.
#[test]
fn body_const_decl() {
    assert_equivalent(
        parse("== k ==\nCONST x = 5\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONST_DECL {
                        IDENTIFIER
                        INTEGER_LIT
                    }
                }
            }
        }),
    );
}

/// Logic line (return) in knot body.
#[test]
fn body_logic_return() {
    assert_equivalent(
        parse("== function f ==\n~ return 5\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER [KW_FUNCTION] {
                    IDENTIFIER
                }
                KNOT_BODY {
                    LOGIC_LINE {
                        RETURN_STMT {
                            INTEGER_LIT
                        }
                    }
                }
            }
        }),
    );
}

// ── E. Knot boundary / termination ─────────────────────────────────

/// Two consecutive knots — knot body ends at next `==`.
#[test]
fn boundary_two_consecutive_knots() {
    assert_equivalent(
        parse("== k1 ==\nA.\n== k2 ==\nB.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Knot terminates at EXTERNAL declaration.
#[test]
fn boundary_knot_terminates_at_external() {
    assert_equivalent(
        parse("== k ==\nA.\nEXTERNAL greet()\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
            EXTERNAL_DECL {
                IDENTIFIER
            }
        }),
    );
}

/// Knot terminates at INCLUDE statement.
#[test]
fn boundary_knot_terminates_at_include() {
    assert_equivalent(
        parse("== k ==\nA.\nINCLUDE other.ink\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
            INCLUDE_STMT {
                FILE_PATH
            }
        }),
    );
}

/// Stitch body ends at next stitch.
#[test]
fn boundary_stitch_ends_at_next_stitch() {
    assert_equivalent(
        parse("== k ==\n= s1\nA.\n= s2\nB.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
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

/// Stitch body ends at next knot.
#[test]
fn boundary_stitch_ends_at_next_knot() {
    assert_equivalent(
        parse("== k1 ==\n= s\nA.\n== k2 ==\nB.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Stitch terminates at EXTERNAL declaration.
#[test]
fn boundary_stitch_terminates_at_external() {
    assert_equivalent(
        parse("== k ==\n= s\nA.\nEXTERNAL greet()\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    STITCH_DEF {
                        STITCH_HEADER {
                            IDENTIFIER
                        }
                        STITCH_BODY {
                            CONTENT_LINE {
                                MIXED_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
            EXTERNAL_DECL {
                IDENTIFIER
            }
        }),
    );
}

// ── F. Structural uniformity assertions ────────────────────────────
//
// Every KNOT_DEF must have exactly one KNOT_HEADER and one KNOT_BODY.
// Every STITCH_DEF must have exactly one STITCH_HEADER and one STITCH_BODY.
// KNOT_HEADER always comes before KNOT_BODY.
// STITCH_HEADER always comes before STITCH_BODY.

fn assert_knot_uniformity(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());

    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::KNOT_DEF {
            let headers: Vec<_> = node
                .children()
                .filter(|c| c.kind() == SyntaxKind::KNOT_HEADER)
                .collect();
            let bodies: Vec<_> = node
                .children()
                .filter(|c| c.kind() == SyntaxKind::KNOT_BODY)
                .collect();
            assert_eq!(
                headers.len(),
                1,
                "KNOT_DEF should have exactly 1 KNOT_HEADER, found {} in `{src}`",
                headers.len(),
            );
            assert_eq!(
                bodies.len(),
                1,
                "KNOT_DEF should have exactly 1 KNOT_BODY, found {} in `{src}`",
                bodies.len(),
            );
            // Header comes before body (by text offset).
            let header_start = headers[0].text_range().start();
            let body_start = bodies[0].text_range().start();
            assert!(
                header_start < body_start,
                "KNOT_HEADER ({header_start:?}) should come before KNOT_BODY ({body_start:?}) in `{src}`",
            );
        }

        if node.kind() == SyntaxKind::STITCH_DEF {
            let headers: Vec<_> = node
                .children()
                .filter(|c| c.kind() == SyntaxKind::STITCH_HEADER)
                .collect();
            let bodies: Vec<_> = node
                .children()
                .filter(|c| c.kind() == SyntaxKind::STITCH_BODY)
                .collect();
            assert_eq!(
                headers.len(),
                1,
                "STITCH_DEF should have exactly 1 STITCH_HEADER, found {} in `{src}`",
                headers.len(),
            );
            assert_eq!(
                bodies.len(),
                1,
                "STITCH_DEF should have exactly 1 STITCH_BODY, found {} in `{src}`",
                bodies.len(),
            );
            let header_start = headers[0].text_range().start();
            let body_start = bodies[0].text_range().start();
            assert!(
                header_start < body_start,
                "STITCH_HEADER ({header_start:?}) should come before STITCH_BODY ({body_start:?}) in `{src}`",
            );
        }
    }
}

/// Uniformity for basic knot.
#[test]
fn uniformity_basic_knot() {
    assert_knot_uniformity("== myKnot ==\nHello.\n");
}

/// Uniformity for function knot.
#[test]
fn uniformity_function_knot() {
    assert_knot_uniformity("== function greet ==\nHi!\n");
}

/// Uniformity for knot with params.
#[test]
fn uniformity_knot_with_params() {
    assert_knot_uniformity("== greet(name, ref title) ==\nHi.\n");
}

/// Uniformity for knot with stitch.
#[test]
fn uniformity_knot_with_stitch() {
    assert_knot_uniformity("== k ==\n= s\nContent.\n");
}

/// Uniformity for knot with multiple stitches.
#[test]
fn uniformity_knot_with_multiple_stitches() {
    assert_knot_uniformity("== k ==\n= s1\nA.\n= s2\nB.\n= s3\nC.\n");
}

/// Uniformity for consecutive knots.
#[test]
fn uniformity_consecutive_knots() {
    assert_knot_uniformity("== k1 ==\nA.\n== k2 ==\nB.\n");
}

/// Uniformity for knot with inline declarations.
#[test]
fn uniformity_knot_with_inline_decls() {
    assert_knot_uniformity("== k ==\nVAR x = 5\nCONST y = 10\nHello.\n");
}

/// Uniformity for stitch with params.
#[test]
fn uniformity_stitch_with_params() {
    assert_knot_uniformity("== k ==\n= s(x, ref y)\nContent.\n");
}

// ── G. Positive/negative wrapper assertions ────────────────────────

/// Basic knot has `KNOT_DEF`, not `STITCH_DEF` at top level.
#[test]
fn has_knot_def_not_stitch_def_at_top() {
    let p = parse("== myKnot ==\nHello.\n");
    let root = p.syntax();
    let has_knot = root.children().any(|n| n.kind() == SyntaxKind::KNOT_DEF);
    let has_stitch_at_top = root.children().any(|n| n.kind() == SyntaxKind::STITCH_DEF);
    assert!(has_knot, "top level must have KNOT_DEF");
    assert!(!has_stitch_at_top, "top level must not have STITCH_DEF");
}

/// Stitch is inside `KNOT_BODY`, not at top level.
#[test]
fn stitch_inside_knot_body_not_top_level() {
    let p = parse("== k ==\n= s\nContent.\n");
    let root = p.syntax();

    // No STITCH_DEF at SOURCE_FILE level
    let has_stitch_at_top = root.children().any(|n| n.kind() == SyntaxKind::STITCH_DEF);
    assert!(
        !has_stitch_at_top,
        "STITCH_DEF must not be a SOURCE_FILE child"
    );

    // STITCH_DEF parent should be KNOT_BODY
    let stitch = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::STITCH_DEF)
        .expect("expected STITCH_DEF");
    let parent = stitch.parent().expect("STITCH_DEF has no parent");
    assert_eq!(
        parent.kind(),
        SyntaxKind::KNOT_BODY,
        "STITCH_DEF should be inside KNOT_BODY, found in {:?}",
        parent.kind(),
    );
}

/// Function knot header has no `KNOT_PARAMS` when no parens.
#[test]
fn function_no_parens_has_no_knot_params() {
    let p = parse("== function greet ==\nHi!\n");
    let root = p.syntax();
    let header = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KNOT_HEADER)
        .expect("expected KNOT_HEADER");
    let has_params = header
        .children()
        .any(|c| c.kind() == SyntaxKind::KNOT_PARAMS);
    assert!(
        !has_params,
        "KNOT_HEADER without parens must not have KNOT_PARAMS"
    );
}

/// Function knot header has `KNOT_PARAMS` when parens present.
#[test]
fn function_with_parens_has_knot_params() {
    let p = parse("== function greet(name) ==\nHi!\n");
    let root = p.syntax();
    let header = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KNOT_HEADER)
        .expect("expected KNOT_HEADER");
    let has_params = header
        .children()
        .any(|c| c.kind() == SyntaxKind::KNOT_PARAMS);
    assert!(has_params, "KNOT_HEADER with parens must have KNOT_PARAMS");
}

/// `KNOT_PARAM_DECL` is inside `KNOT_PARAMS`, not directly in header.
#[test]
fn param_decl_inside_knot_params() {
    let p = parse("== greet(name) ==\nHi.\n");
    let root = p.syntax();
    let param_decl = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KNOT_PARAM_DECL)
        .expect("expected KNOT_PARAM_DECL");
    let parent = param_decl.parent().expect("KNOT_PARAM_DECL has no parent");
    assert_eq!(
        parent.kind(),
        SyntaxKind::KNOT_PARAMS,
        "KNOT_PARAM_DECL should be inside KNOT_PARAMS, found in {:?}",
        parent.kind(),
    );
}

/// Inline VAR is in `KNOT_BODY`, not `SOURCE_FILE` direct child.
#[test]
fn inline_var_in_knot_body_not_source_file() {
    let p = parse("== k ==\nVAR x = 5\n");
    let root = p.syntax();
    let var_decl = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::VAR_DECL)
        .expect("expected VAR_DECL");
    let parent = var_decl.parent().expect("VAR_DECL has no parent");
    assert_eq!(
        parent.kind(),
        SyntaxKind::KNOT_BODY,
        "VAR_DECL should be inside KNOT_BODY, found in {:?}",
        parent.kind(),
    );

    // Not a direct child of SOURCE_FILE
    let has_var_at_top = root.children().any(|n| n.kind() == SyntaxKind::VAR_DECL);
    assert!(
        !has_var_at_top,
        "VAR_DECL inside knot must not be a SOURCE_FILE child"
    );
}

/// EXTERNAL is NOT in `KNOT_BODY` — it's a sibling at `SOURCE_FILE` level.
#[test]
fn external_not_in_knot_body() {
    let p = parse("== k ==\nA.\nEXTERNAL greet()\n");
    let root = p.syntax();

    // EXTERNAL_DECL should be a direct child of SOURCE_FILE
    let has_external_at_top = root
        .children()
        .any(|n| n.kind() == SyntaxKind::EXTERNAL_DECL);
    assert!(
        has_external_at_top,
        "EXTERNAL_DECL must be a SOURCE_FILE child"
    );

    // EXTERNAL_DECL should NOT be inside KNOT_BODY
    let knot_body = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::KNOT_BODY)
        .expect("expected KNOT_BODY");
    let has_external_in_body = knot_body
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EXTERNAL_DECL);
    assert!(
        !has_external_in_body,
        "EXTERNAL_DECL must not be inside KNOT_BODY",
    );
}

/// Stitch params: `KNOT_PARAMS` appears in `STITCH_HEADER`.
#[test]
fn stitch_params_in_stitch_header() {
    let p = parse("== k ==\n= s(x)\nContent.\n");
    let root = p.syntax();
    let stitch_header = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::STITCH_HEADER)
        .expect("expected STITCH_HEADER");
    let has_params = stitch_header
        .children()
        .any(|c| c.kind() == SyntaxKind::KNOT_PARAMS);
    assert!(
        has_params,
        "STITCH_HEADER with parens must have KNOT_PARAMS"
    );
}

// ── H. Error recovery / edge cases ─────────────────────────────────

/// `== ==` — missing identifier (error, lossless).
#[test]
fn error_missing_identifier() {
    let src = "== ==\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing identifier in knot header",
    );
}

/// `== myKnot(\n` — unclosed params (error, lossless).
#[test]
fn error_unclosed_params() {
    let src = "== myKnot(\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unclosed params in knot header",
    );
}

/// `== myKnot(ref ) ==\n` — ref with no identifier (error, lossless).
#[test]
fn error_ref_no_identifier() {
    let src = "== myKnot(ref ) ==\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for ref with no identifier",
    );
}

/// `== myKnot ==` — no trailing newline (lossless).
#[test]
fn no_trailing_newline_lossless() {
    let src = "== myKnot ==";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// Empty knot body (knot header followed immediately by next knot).
#[test]
fn empty_knot_body() {
    let src = "== k1 ==\n== k2 ==\nContent.\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    // First knot body should have no content children
    let knot_bodies: Vec<_> = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::KNOT_BODY)
        .collect();
    assert_eq!(knot_bodies.len(), 2, "should have 2 knot bodies");

    let first_body_children: Vec<_> = knot_bodies[0].children().collect();
    assert!(
        first_body_children.is_empty(),
        "first knot body should be empty, found: {:?}",
        first_body_children
            .iter()
            .map(crate::SyntaxNode::kind)
            .collect::<Vec<_>>(),
    );
}
