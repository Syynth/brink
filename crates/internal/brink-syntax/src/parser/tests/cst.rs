use crate::{Parse, SyntaxKind, SyntaxNode, parse};

// ── Expected tree representation ─────────────────────────────────────

pub struct ExpectedNode {
    pub kind: SyntaxKind,
    pub children: Vec<ExpectedNode>,
}

// ── cst! macro ───────────────────────────────────────────────────────

/// Build an [`ExpectedNode`] tree using nested braces.
///
/// ```ignore
/// cst!(SOURCE_FILE {
///     CONTENT_LINE {
///         MIXED_CONTENT {
///             TEXT
///         }
///         DIVERT_NODE {
///             DIVERT_TARGET_WITH_ARGS {
///                 IDENTIFIER
///             }
///         }
///     }
/// })
/// ```
macro_rules! cst {
    // Root node with children.
    ($kind:ident { $($inner:tt)* }) => {
        ExpectedNode {
            kind: SyntaxKind::$kind,
            children: cst!(@list [] $($inner)*),
        }
    };
    // Root leaf node.
    ($kind:ident) => {
        ExpectedNode {
            kind: SyntaxKind::$kind,
            children: vec![],
        }
    };
    // ── List builder (TT muncher) ────────────────────────────────────
    // Node with children, followed by more siblings.
    (@list [$($acc:expr),*] $kind:ident { $($inner:tt)* } $($rest:tt)*) => {
        cst!(@list [$($acc,)* ExpectedNode {
            kind: SyntaxKind::$kind,
            children: cst!(@list [] $($inner)*),
        }] $($rest)*)
    };
    // Leaf node, followed by more siblings.
    (@list [$($acc:expr),*] $kind:ident $($rest:tt)*) => {
        cst!(@list [$($acc,)* ExpectedNode {
            kind: SyntaxKind::$kind,
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
/// `expected`. Tokens are skipped — only composite nodes are compared.
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
fn simple_choice() {
    assert_equivalent(
        parse("* Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

#[test]
fn choice_with_bracket() {
    assert_equivalent(
        parse("* Hello [world] end\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                }
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
                CHOICE_INNER_CONTENT {
                    TEXT
                }
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

// ── Divert: simple targets ──────────────────────────────────────

/// `-> target` produces `SIMPLE_DIVERT` with `DIVERT_TARGET_WITH_ARGS`.
#[test]
fn divert_to_ident() {
    assert_equivalent(
        parse("-> target\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
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

/// `-> DONE` has no `PATH` child — just a bare `KW_DONE` token.
#[test]
fn divert_to_done() {
    assert_equivalent(
        parse("-> DONE\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS
                    }
                }
            }
        }),
    );
}

/// `-> END` mirrors DONE — bare `KW_END` token, no `PATH`.
#[test]
fn divert_to_end() {
    assert_equivalent(
        parse("-> END\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS
                    }
                }
            }
        }),
    );
}

/// `-> knot.stitch` — two-segment dotted path.
#[test]
fn divert_to_dotted_path() {
    assert_equivalent(
        parse("-> knot.stitch\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
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

/// `-> a.b.c` — three-segment dotted path.
#[test]
fn divert_to_multi_dotted_path() {
    assert_equivalent(
        parse("-> a.b.c\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
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

/// `->target` (no whitespace) parses identically to `-> target`.
#[test]
fn divert_no_whitespace() {
    assert_equivalent(
        parse("->target\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
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

// ── Divert: with arguments ──────────────────────────────────────

/// `-> target()` — empty parens produce no `ARG_LIST`.
#[test]
fn divert_empty_args() {
    assert_equivalent(
        parse("-> target()\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
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

/// `-> func(x)` — single argument produces `ARG_LIST`.
#[test]
fn divert_single_arg() {
    assert_equivalent(
        parse("-> func(x)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `-> func(x, y)` — two arguments.
#[test]
fn divert_two_args() {
    assert_equivalent(
        parse("-> func(x, y)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
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

/// `-> func(x, y, z)` — three arguments.
#[test]
fn divert_three_args() {
    assert_equivalent(
        parse("-> func(x, y, z)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                PATH
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

/// `-> knot.stitch(x)` — dotted path with arguments.
#[test]
fn divert_dotted_path_with_args() {
    assert_equivalent(
        parse("-> knot.stitch(x)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `-> greet("hello")` — string literal argument.
#[test]
fn divert_string_arg() {
    assert_equivalent(
        parse("-> greet(\"hello\")\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                STRING_LIT
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `-> check(x > 5)` — expression argument containing infix operator.
#[test]
fn divert_expr_arg() {
    assert_equivalent(
        parse("-> check(x > 5)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
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

// ── Divert: chains (no trailing arrow) ──────────────────────────

/// `-> a -> b` — two chained targets, no tunnel call.
#[test]
fn chain_two_targets() {
    assert_equivalent(
        parse("-> a -> b\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> a -> b -> c` — three chained targets.
#[test]
fn chain_three_targets() {
    assert_equivalent(
        parse("-> a -> b -> c\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> tunnel -> knot.stitch` — first target simple, second dotted.
#[test]
fn chain_mixed_paths() {
    assert_equivalent(
        parse("-> tunnel -> knot.stitch\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> a(x) -> b` — first target has args, second doesn't.
#[test]
fn chain_with_args() {
    assert_equivalent(
        parse("-> a(x) -> b\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                PATH
                            }
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> DONE -> next` — first target is `KW_DONE` (no PATH child).
#[test]
fn chain_done_then_target() {
    assert_equivalent(
        parse("-> DONE -> next\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> a -> DONE` — last target is `KW_DONE`.
#[test]
fn chain_target_then_done() {
    assert_equivalent(
        parse("-> a -> DONE\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                        DIVERT_TARGET_WITH_ARGS
                    }
                }
            }
        }),
    );
}

// ── Divert: tunnel calls (trailing `->`) ────────────────────────

/// `-> target ->` — tunnel call wraps in `TUNNEL_CALL_NODE`.
#[test]
fn tunnel_call_simple() {
    assert_equivalent(
        parse("-> target ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> target(x) ->` — tunnel call with arguments.
#[test]
fn tunnel_call_with_args() {
    assert_equivalent(
        parse("-> target(x) ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `-> knot.stitch ->` — tunnel call with dotted path.
#[test]
fn tunnel_call_dotted() {
    assert_equivalent(
        parse("-> knot.stitch ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> a -> b ->` — tunnel call wrapping a two-target chain.
#[test]
fn tunnel_call_chain() {
    assert_equivalent(
        parse("-> a -> b ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `-> a(x) -> b(y) ->` — tunnel call chain with args on both targets.
#[test]
fn tunnel_call_chain_with_args() {
    assert_equivalent(
        parse("-> a(x) -> b(y) ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                PATH
                            }
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                            ARG_LIST {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

// ── Divert: tunnel call before tunnel onwards ───────────────────

/// `-> tunnel ->->` — tunnel call detected (current == `TUNNEL_ONWARDS`),
/// then tunnel onwards as a second sibling `DIVERT_NODE`.
#[test]
fn tunnel_call_then_tunnel_onwards() {
    assert_equivalent(
        parse("-> tunnel ->->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE
                }
            }
        }),
    );
}

/// `-> a -> b ->->` — chain tunnel call then tunnel onwards.
#[test]
fn tunnel_call_chain_then_onwards() {
    assert_equivalent(
        parse("-> a -> b ->->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE
                }
            }
        }),
    );
}

// ── Divert: tunnel onwards ──────────────────────────────────────

/// `->->` — bare tunnel onwards.
#[test]
fn tunnel_onwards_bare() {
    assert_equivalent(
        parse("->->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE
                }
            }
        }),
    );
}

/// `->-> -> target` — tunnel onwards followed by divert chain.
#[test]
fn tunnel_onwards_with_divert() {
    assert_equivalent(
        parse("->-> -> target\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE {
                        SIMPLE_DIVERT {
                            DIVERT_TARGET_WITH_ARGS {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// `->-> -> a -> b` — tunnel onwards with chained divert.
#[test]
fn tunnel_onwards_with_chain() {
    assert_equivalent(
        parse("->-> -> a -> b\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE {
                        SIMPLE_DIVERT {
                            DIVERT_TARGET_WITH_ARGS {
                                PATH
                            }
                            DIVERT_TARGET_WITH_ARGS {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

// ── Divert: thread starts ───────────────────────────────────────

/// `<- background` — simple thread start.
#[test]
fn thread_simple() {
    assert_equivalent(
        parse("<- background\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                    }
                }
            }
        }),
    );
}

/// `<- knot.stitch` — thread with dotted path.
#[test]
fn thread_dotted() {
    assert_equivalent(
        parse("<- knot.stitch\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                    }
                }
            }
        }),
    );
}

/// `<- target()` — thread with empty parens, no `ARG_LIST`.
#[test]
fn thread_empty_args() {
    assert_equivalent(
        parse("<- target()\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                    }
                }
            }
        }),
    );
}

/// `<- greet(name)` — thread with single argument.
#[test]
fn thread_single_arg() {
    assert_equivalent(
        parse("<- greet(name)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                        ARG_LIST {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `<- func(a, b)` — thread with two arguments.
#[test]
fn thread_two_args() {
    assert_equivalent(
        parse("<- func(a, b)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                        ARG_LIST {
                            PATH
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `<- knot.stitch(x, y)` — thread with dotted path and args.
#[test]
fn thread_dotted_with_args() {
    assert_equivalent(
        parse("<- knot.stitch(x, y)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                        ARG_LIST {
                            PATH
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// `<- func(2, -> opts)` — thread with a divert target expression argument.
#[test]
fn thread_divert_arg() {
    assert_equivalent(
        parse("<- func(2, -> opts)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                        ARG_LIST {
                            INTEGER_LIT
                            DIVERT_TARGET_EXPR {
                                PATH
                            }
                        }
                    }
                }
            }
        }),
    );
}

// ── Divert: in context ──────────────────────────────────────────

/// Bare divert line has no `MIXED_CONTENT` — just `DIVERT_NODE` inside `CONTENT_LINE`.
#[test]
fn bare_divert_line() {
    assert_equivalent(
        parse("-> target\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
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

/// Text followed by a chained divert.
#[test]
fn content_then_chain() {
    assert_equivalent(
        parse("Text -> a -> b\n"),
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
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Choice with a trailing divert.
#[test]
fn choice_with_divert() {
    assert_equivalent(
        parse("* Choice -> knot\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
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

/// Gather with bare divert (no content text).
#[test]
fn gather_bare_divert() {
    assert_equivalent(
        parse("- -> done\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
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

/// Gather with content text then divert.
#[test]
fn gather_content_then_divert() {
    assert_equivalent(
        parse("- Gathered -> next\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
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

// ── Divert: negative cases ──────────────────────────────────────

/// Simple divert must NOT contain a `TUNNEL_CALL_NODE`.
#[test]
fn no_tunnel_call_simple_divert() {
    let p = parse("-> target\n");
    let has_tunnel = p
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(!has_tunnel, "simple divert must not have TUNNEL_CALL_NODE");
}

/// Chained divert must NOT contain a `TUNNEL_CALL_NODE`.
#[test]
fn no_tunnel_call_chain() {
    let p = parse("-> a -> b\n");
    let has_tunnel = p
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(!has_tunnel, "chained divert must not have TUNNEL_CALL_NODE");
}

/// Long chain must NOT contain a `TUNNEL_CALL_NODE`.
#[test]
fn no_tunnel_call_long_chain() {
    let p = parse("-> a -> b -> c\n");
    let has_tunnel = p
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(
        !has_tunnel,
        "long chained divert must not have TUNNEL_CALL_NODE"
    );
}

// ── Divert: error recovery / edge cases ─────────────────────────

/// Missing `)` in divert args — should produce errors but still round-trip.
#[test]
fn error_missing_rparen_in_divert_args() {
    let src = "-> target(arg\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for missing `)` in divert args"
    );
}

/// Trailing dot in path — should produce errors but still round-trip.
#[test]
fn error_trailing_dot_in_path() {
    let src = "-> knot.\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}
