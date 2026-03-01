use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── INCLUDE statements ─────────────────────────────────────────

/// `INCLUDE story.ink` — basic include with `FILE_PATH` child.
#[test]
fn include_basic() {
    assert_equivalent(
        parse("INCLUDE story.ink\n"),
        cst!(SOURCE_FILE {
            INCLUDE_STMT {
                FILE_PATH
            }
        }),
    );
}

/// `INCLUDE path/to/story.ink` — path with slashes.
#[test]
fn include_path_with_slashes() {
    assert_equivalent(
        parse("INCLUDE path/to/story.ink\n"),
        cst!(SOURCE_FILE {
            INCLUDE_STMT {
                FILE_PATH
            }
        }),
    );
}

/// `INCLUDE file with spaces.ink` — file path with spaces.
#[test]
fn include_file_with_spaces() {
    assert_equivalent(
        parse("INCLUDE file with spaces.ink\n"),
        cst!(SOURCE_FILE {
            INCLUDE_STMT {
                FILE_PATH
            }
        }),
    );
}

// ── EXTERNAL declarations ──────────────────────────────────────

/// `EXTERNAL func()` — no params, no `FUNCTION_PARAM_LIST`.
#[test]
fn external_no_params() {
    assert_equivalent(
        parse("EXTERNAL func()\n"),
        cst!(SOURCE_FILE {
            EXTERNAL_DECL {
                IDENTIFIER
            }
        }),
    );
}

/// `EXTERNAL func(a)` — single param.
#[test]
fn external_single_param() {
    assert_equivalent(
        parse("EXTERNAL func(a)\n"),
        cst!(SOURCE_FILE {
            EXTERNAL_DECL {
                IDENTIFIER
                FUNCTION_PARAM_LIST {
                    IDENTIFIER
                }
            }
        }),
    );
}

/// `EXTERNAL func(a, b)` — two params.
#[test]
fn external_two_params() {
    assert_equivalent(
        parse("EXTERNAL func(a, b)\n"),
        cst!(SOURCE_FILE {
            EXTERNAL_DECL {
                IDENTIFIER
                FUNCTION_PARAM_LIST {
                    IDENTIFIER
                    IDENTIFIER
                }
            }
        }),
    );
}

/// `EXTERNAL func(a, b, c)` — three params.
#[test]
fn external_three_params() {
    assert_equivalent(
        parse("EXTERNAL func(a, b, c)\n"),
        cst!(SOURCE_FILE {
            EXTERNAL_DECL {
                IDENTIFIER
                FUNCTION_PARAM_LIST {
                    IDENTIFIER
                    IDENTIFIER
                    IDENTIFIER
                }
            }
        }),
    );
}

// ── VAR declarations ───────────────────────────────────────────

/// `VAR x = 5` — integer literal.
#[test]
fn var_integer() {
    assert_equivalent(
        parse("VAR x = 5\n"),
        cst!(SOURCE_FILE {
            VAR_DECL {
                IDENTIFIER
                INTEGER_LIT
            }
        }),
    );
}

/// `VAR name = "hello"` — string literal.
#[test]
fn var_string() {
    assert_equivalent(
        parse("VAR name = \"hello\"\n"),
        cst!(SOURCE_FILE {
            VAR_DECL {
                IDENTIFIER
                STRING_LIT
            }
        }),
    );
}

/// `VAR flag = true` — boolean literal.
#[test]
fn var_boolean() {
    assert_equivalent(
        parse("VAR flag = true\n"),
        cst!(SOURCE_FILE {
            VAR_DECL {
                IDENTIFIER
                BOOLEAN_LIT
            }
        }),
    );
}

/// `VAR x = 3.14` — float literal.
#[test]
fn var_float() {
    assert_equivalent(
        parse("VAR x = 3.14\n"),
        cst!(SOURCE_FILE {
            VAR_DECL {
                IDENTIFIER
                FLOAT_LIT
            }
        }),
    );
}

/// `VAR x = y` — variable reference (PATH child).
#[test]
fn var_reference() {
    assert_equivalent(
        parse("VAR x = y\n"),
        cst!(SOURCE_FILE {
            VAR_DECL {
                IDENTIFIER
                PATH
            }
        }),
    );
}

/// `VAR x = a + b` — expression (`INFIX_EXPR` child).
#[test]
fn var_expression() {
    assert_equivalent(
        parse("VAR x = a + b\n"),
        cst!(SOURCE_FILE {
            VAR_DECL {
                IDENTIFIER
                INFIX_EXPR {
                    PATH
                    PATH
                }
            }
        }),
    );
}

// ── CONST declarations ─────────────────────────────────────────

/// `CONST PI = 3` — integer literal.
#[test]
fn const_integer() {
    assert_equivalent(
        parse("CONST PI = 3\n"),
        cst!(SOURCE_FILE {
            CONST_DECL {
                IDENTIFIER
                INTEGER_LIT
            }
        }),
    );
}

/// `CONST NAME = "ink"` — string literal.
#[test]
fn const_string() {
    assert_equivalent(
        parse("CONST NAME = \"ink\"\n"),
        cst!(SOURCE_FILE {
            CONST_DECL {
                IDENTIFIER
                STRING_LIT
            }
        }),
    );
}

/// `CONST FLAG = false` — boolean literal.
#[test]
fn const_boolean() {
    assert_equivalent(
        parse("CONST FLAG = false\n"),
        cst!(SOURCE_FILE {
            CONST_DECL {
                IDENTIFIER
                BOOLEAN_LIT
            }
        }),
    );
}

// ── LIST declarations ──────────────────────────────────────────

/// `LIST colors = red, green, blue` — simple members (all `LIST_MEMBER_OFF`).
#[test]
fn list_simple_off() {
    assert_equivalent(
        parse("LIST colors = red, green, blue\n"),
        cst!(SOURCE_FILE {
            LIST_DECL {
                IDENTIFIER
                LIST_DEF {
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                }
            }
        }),
    );
}

/// `LIST items = (sword), (shield)` — all on (`LIST_MEMBER_ON`, no values).
#[test]
fn list_all_on() {
    assert_equivalent(
        parse("LIST items = (sword), (shield)\n"),
        cst!(SOURCE_FILE {
            LIST_DECL {
                IDENTIFIER
                LIST_DEF {
                    LIST_MEMBER {
                        LIST_MEMBER_ON
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_ON
                    }
                }
            }
        }),
    );
}

/// `LIST items = (sword = 1), shield, (potion = 3)` — mixed on/off with values.
#[test]
fn list_mixed_on_off_with_values() {
    assert_equivalent(
        parse("LIST items = (sword = 1), shield, (potion = 3)\n"),
        cst!(SOURCE_FILE {
            LIST_DECL {
                IDENTIFIER
                LIST_DEF {
                    LIST_MEMBER {
                        LIST_MEMBER_ON
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_ON
                    }
                }
            }
        }),
    );
}

/// `LIST single = item` — single member.
#[test]
fn list_single_member() {
    assert_equivalent(
        parse("LIST single = item\n"),
        cst!(SOURCE_FILE {
            LIST_DECL {
                IDENTIFIER
                LIST_DEF {
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                }
            }
        }),
    );
}

/// `LIST items = (x = 1)` — single on member with value.
#[test]
fn list_single_on_with_value() {
    assert_equivalent(
        parse("LIST items = (x = 1)\n"),
        cst!(SOURCE_FILE {
            LIST_DECL {
                IDENTIFIER
                LIST_DEF {
                    LIST_MEMBER {
                        LIST_MEMBER_ON
                    }
                }
            }
        }),
    );
}

/// `LIST kw = or, and, not` — keyword members (off).
#[test]
fn list_keyword_members_off() {
    assert_equivalent(
        parse("LIST kw = or, and, not\n"),
        cst!(SOURCE_FILE {
            LIST_DECL {
                IDENTIFIER
                LIST_DEF {
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_OFF
                    }
                }
            }
        }),
    );
}

/// `LIST kw = (or), (and)` — keyword members (on).
#[test]
fn list_keyword_members_on() {
    assert_equivalent(
        parse("LIST kw = (or), (and)\n"),
        cst!(SOURCE_FILE {
            LIST_DECL {
                IDENTIFIER
                LIST_DEF {
                    LIST_MEMBER {
                        LIST_MEMBER_ON
                    }
                    LIST_MEMBER {
                        LIST_MEMBER_ON
                    }
                }
            }
        }),
    );
}

// ── Variant uniformity ─────────────────────────────────────────
//
// Every declaration must wrap in exactly its expected node kind.

const DECL_KINDS: [SyntaxKind; 5] = [
    SyntaxKind::INCLUDE_STMT,
    SyntaxKind::EXTERNAL_DECL,
    SyntaxKind::VAR_DECL,
    SyntaxKind::CONST_DECL,
    SyntaxKind::LIST_DECL,
];

/// Assert that `src` produces exactly one top-level declaration of `expected_kind`,
/// and none of the other declaration kinds.
fn assert_decl_uniformity(src: &str, expected_kind: SyntaxKind) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    let root = p.syntax();

    // The expected kind should appear exactly once as a direct child of SOURCE_FILE.
    let matching: Vec<_> = root
        .children()
        .filter(|c| c.kind() == expected_kind)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one {expected_kind:?} in `{src}`, found {}",
        matching.len(),
    );

    // No other declaration kinds should appear anywhere in the tree.
    for kind in &DECL_KINDS {
        if *kind == expected_kind {
            continue;
        }
        let has_other = root.descendants().any(|n| n.kind() == *kind);
        assert!(
            !has_other,
            "`{src}` should not contain {kind:?}, but it does",
        );
    }
}

#[test]
fn uniformity_include() {
    assert_decl_uniformity("INCLUDE story.ink\n", SyntaxKind::INCLUDE_STMT);
}

#[test]
fn uniformity_external() {
    assert_decl_uniformity("EXTERNAL func(a)\n", SyntaxKind::EXTERNAL_DECL);
}

#[test]
fn uniformity_var() {
    assert_decl_uniformity("VAR x = 5\n", SyntaxKind::VAR_DECL);
}

#[test]
fn uniformity_const() {
    assert_decl_uniformity("CONST PI = 3\n", SyntaxKind::CONST_DECL);
}

#[test]
fn uniformity_list() {
    assert_decl_uniformity("LIST colors = red, green, blue\n", SyntaxKind::LIST_DECL);
}

// ── Positive/negative assertions ───────────────────────────────

/// `INCLUDE_STMT` is not any other declaration kind.
#[test]
fn include_not_other_decls() {
    let p = parse("INCLUDE story.ink\n");
    let root = p.syntax();
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::INCLUDE_STMT)
    );
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::EXTERNAL_DECL)
    );
    assert!(!root.descendants().any(|n| n.kind() == SyntaxKind::VAR_DECL));
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CONST_DECL)
    );
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::LIST_DECL)
    );
}

/// `EXTERNAL_DECL` is not any other declaration kind.
#[test]
fn external_not_other_decls() {
    let p = parse("EXTERNAL func()\n");
    let root = p.syntax();
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::EXTERNAL_DECL)
    );
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::INCLUDE_STMT)
    );
    assert!(!root.descendants().any(|n| n.kind() == SyntaxKind::VAR_DECL));
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CONST_DECL)
    );
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::LIST_DECL)
    );
}

/// `VAR_DECL` is not `CONST_DECL` — structurally identical but different wrapper.
#[test]
fn var_not_const() {
    let p = parse("VAR x = 5\n");
    let root = p.syntax();
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::VAR_DECL));
    assert!(
        !root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::CONST_DECL)
    );
}

/// `CONST_DECL` is not `VAR_DECL`.
#[test]
fn const_not_var() {
    let p = parse("CONST PI = 3\n");
    let root = p.syntax();
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::CONST_DECL)
    );
    assert!(!root.descendants().any(|n| n.kind() == SyntaxKind::VAR_DECL));
}

/// `LIST_DECL` contains `LIST_DEF` and `LIST_MEMBER`; other declarations do not.
#[test]
fn list_has_list_def_and_members() {
    let p = parse("LIST colors = red, green\n");
    let root = p.syntax();
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::LIST_DECL)
    );
    assert!(root.descendants().any(|n| n.kind() == SyntaxKind::LIST_DEF));
    assert!(
        root.descendants()
            .any(|n| n.kind() == SyntaxKind::LIST_MEMBER)
    );
}

/// Non-list declarations do not contain `LIST_DEF` or `LIST_MEMBER`.
#[test]
fn non_list_has_no_list_nodes() {
    for src in &[
        "INCLUDE story.ink\n",
        "EXTERNAL func(a)\n",
        "VAR x = 5\n",
        "CONST PI = 3\n",
    ] {
        let p = parse(src);
        let root = p.syntax();
        assert!(
            !root.descendants().any(|n| n.kind() == SyntaxKind::LIST_DEF),
            "`{src}` should not contain LIST_DEF",
        );
        assert!(
            !root
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LIST_MEMBER),
            "`{src}` should not contain LIST_MEMBER",
        );
    }
}

// ── Error recovery / edge cases ────────────────────────────────

/// Missing `=` in VAR — errors but lossless round-trip.
#[test]
fn error_var_missing_eq() {
    let src = "VAR x 5\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing `=` in VAR",
    );
}

/// Missing `)` in EXTERNAL — errors but lossless round-trip.
#[test]
fn error_external_missing_rparen() {
    let src = "EXTERNAL func(a, b\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing `)` in EXTERNAL",
    );
}

/// Empty LIST definition `LIST x =\n` — errors but lossless round-trip.
#[test]
fn error_list_empty_def() {
    let src = "LIST x =\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for empty LIST definition",
    );
}

/// Missing filename in INCLUDE — lossless round-trip (may or may not error).
#[test]
fn error_include_missing_filename() {
    let src = "INCLUDE \n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}
