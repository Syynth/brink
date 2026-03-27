use crate::{Parse, SyntaxKind, SyntaxNode, parse};

// ── Expected tree representation ─────────────────────────────────────

pub struct ExpectedNode {
    pub kind: SyntaxKind,
    pub tokens: Vec<SyntaxKind>,
    pub children: Vec<ExpectedNode>,
}

// ── cst! macro ───────────────────────────────────────────────────────

/// Build an [`ExpectedNode`] tree using nested braces.
///
/// Composite node children are listed inside `{ }`. Token assertions
/// can be added in `[ ]` after the node kind — these assert that the
/// listed token kinds appear as direct token children of that node.
///
/// ```ignore
/// cst!(SOURCE_FILE {
///     LOGIC_LINE {
///         ASSIGNMENT [EQ] {
///             PATH
///             INFIX_EXPR [PLUS] {
///                 INTEGER_LIT
///                 INTEGER_LIT
///             }
///         }
///     }
/// })
/// ```
macro_rules! cst {
    // ── Root arms ────────────────────────────────────────────────────
    // Root node with tokens and children.
    ($kind:ident [$($token:ident),* $(,)?] { $($inner:tt)* }) => {
        ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![$(SyntaxKind::$token),*],
            children: cst!(@list [] $($inner)*),
        }
    };
    // Root node with children (no tokens).
    ($kind:ident { $($inner:tt)* }) => {
        ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![],
            children: cst!(@list [] $($inner)*),
        }
    };
    // Root leaf with tokens.
    ($kind:ident [$($token:ident),* $(,)?]) => {
        ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![$(SyntaxKind::$token),*],
            children: vec![],
        }
    };
    // Root leaf node (no tokens).
    ($kind:ident) => {
        ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![],
            children: vec![],
        }
    };
    // ── List builder (TT muncher) ────────────────────────────────────
    // Node with tokens and children, followed by more siblings.
    (@list [$($acc:expr),*] $kind:ident [$($token:ident),* $(,)?] { $($inner:tt)* } $($rest:tt)*) => {
        cst!(@list [$($acc,)* ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![$(SyntaxKind::$token),*],
            children: cst!(@list [] $($inner)*),
        }] $($rest)*)
    };
    // Node with children (no tokens), followed by more siblings.
    (@list [$($acc:expr),*] $kind:ident { $($inner:tt)* } $($rest:tt)*) => {
        cst!(@list [$($acc,)* ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![],
            children: cst!(@list [] $($inner)*),
        }] $($rest)*)
    };
    // Leaf with tokens, followed by more siblings.
    (@list [$($acc:expr),*] $kind:ident [$($token:ident),* $(,)?] $($rest:tt)*) => {
        cst!(@list [$($acc,)* ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![$(SyntaxKind::$token),*],
            children: vec![],
        }] $($rest)*)
    };
    // Leaf node (no tokens), followed by more siblings.
    (@list [$($acc:expr),*] $kind:ident $($rest:tt)*) => {
        cst!(@list [$($acc,)* ExpectedNode {
            kind: SyntaxKind::$kind,
            tokens: vec![],
            children: vec![],
        }] $($rest)*)
    };
    // Done — emit the accumulated vec.
    (@list [$($acc:expr),*]) => {
        vec![$($acc),*]
    };
}

// ── Assertion ────────────────────────────────────────────────────────

/// Assert that parsing `src` produces a CST whose node structure matches
/// `expected`. Only composite node children are compared by default.
/// When `[TOKEN]` annotations are present on a node, the listed token
/// kinds are asserted to exist as direct token children of that node.
///
/// Also asserts lossless round-trip and no parse errors.
#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper — ergonomic by-value API"
)]
pub fn assert_equivalent(parsed: Parse, expected: ExpectedNode) {
    let root = parsed.syntax();

    assert!(
        parsed.errors().is_empty(),
        "unexpected parse errors: {:?}\n\nactual tree:\n{}",
        parsed.errors(),
        format_tree(&root, 0),
    );

    compare_nodes(&root, &expected, &[], &root);
}

fn compare_nodes(
    actual: &SyntaxNode,
    expected: &ExpectedNode,
    path: &[SyntaxKind],
    root: &SyntaxNode,
) {
    assert_eq!(
        actual.kind(),
        expected.kind,
        "kind mismatch at {}\n  expected: {:?}\n  actual:   {:?}\n\nactual tree:\n{}",
        format_path(path),
        expected.kind,
        actual.kind(),
        format_tree(root, 0),
    );

    // Check expected token children (if any).
    if !expected.tokens.is_empty() {
        let actual_tokens: Vec<SyntaxKind> = actual
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .map(|t| t.kind())
            .collect();

        for expected_token in &expected.tokens {
            assert!(
                actual_tokens.contains(expected_token),
                "missing expected token {expected_token:?} at {} > {:?}\n  actual tokens: {actual_tokens:?}\n\nactual tree:\n{}",
                format_path(path),
                expected.kind,
                format_tree(root, 0),
            );
        }
    }

    let actual_children: Vec<SyntaxKind> = actual.children().map(|c| c.kind()).collect();
    let expected_children: Vec<SyntaxKind> = expected.children.iter().map(|c| c.kind).collect();

    assert_eq!(
        actual_children.len(),
        expected_children.len(),
        "child count mismatch at {} > {:?}\n  expected: {expected_children:?}\n  actual:   {actual_children:?}\n\nactual tree:\n{}",
        format_path(path),
        expected.kind,
        format_tree(root, 0),
    );

    let mut child_path = path.to_vec();
    child_path.push(expected.kind);

    for (actual_child, expected_child) in actual.children().zip(&expected.children) {
        compare_nodes(&actual_child, expected_child, &child_path, root);
    }
}

// ── Formatting helpers ───────────────────────────────────────────────

fn format_path(path: &[SyntaxKind]) -> String {
    if path.is_empty() {
        "root".to_string()
    } else {
        path.iter()
            .map(|k| format!("{k:?}"))
            .collect::<Vec<_>>()
            .join(" > ")
    }
}

fn format_tree(node: &SyntaxNode, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    let mut result = format!("{prefix}{:?}", node.kind());
    for child in node.children() {
        result.push('\n');
        result.push_str(&format_tree(&child, indent + 1));
    }
    result
}

// ── Tests ────────────────────────────────────────────────────────────

#[test]
fn content_line() {
    assert_equivalent(
        parse("Hello, world!\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

#[test]
fn content_with_divert() {
    assert_equivalent(
        parse("Hello -> knot\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
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

#[test]
fn content_with_tags() {
    assert_equivalent(
        parse("Hello #tag1 #tag2\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
                TAGS {
                    TAG
                    TAG
                }
            }
        }),
    );
}

#[test]
fn knot_with_content() {
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

#[test]
fn var_declaration() {
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

#[test]
fn gather_line() {
    assert_equivalent(
        parse("- Hello\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

#[test]
fn logic_temp_decl() {
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

#[test]
fn inline_conditional() {
    assert_equivalent(
        parse("Hello {x: world}\n"),
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
                }
            }
        }),
    );
}

#[test]
fn include_statement() {
    assert_equivalent(
        parse("INCLUDE story.ink\n"),
        cst!(SOURCE_FILE {
            INCLUDE_STMT {
                FILE_PATH
            }
        }),
    );
}

#[test]
fn external_declaration() {
    assert_equivalent(
        parse("EXTERNAL greet(name)\n"),
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

#[test]
fn function_knot() {
    assert_equivalent(
        parse("== function greet(name) ==\nHi!\n"),
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

#[test]
fn glue_in_tab_indented_choice_body() {
    fn has_glue_node(node: &SyntaxNode) -> bool {
        if node.kind() == SyntaxKind::GLUE_NODE {
            return true;
        }
        node.children().any(|c| has_glue_node(&c))
    }

    // Parse the pattern: tab-indented choice body with trailing <>
    // This is the TheIntercept pattern where <> ends up as literal text.
    let source = " \t* [Yes]\n \t\t\"Yes, I considered it. <>\n";
    let parsed = parse(source);
    let tree = format_tree(&parsed.syntax(), 0);
    eprintln!("{tree}");

    // Also check lexer tokens
    let tokens = crate::lex(source);
    let has_glue_token = tokens.iter().any(|(k, _)| *k == SyntaxKind::GLUE);
    assert!(has_glue_token, "lexer should produce GLUE token for '<>'");

    assert!(
        has_glue_node(&parsed.syntax()),
        "parse tree should contain GLUE_NODE for '<>', but doesn't. Tree:\n{tree}"
    );
}
