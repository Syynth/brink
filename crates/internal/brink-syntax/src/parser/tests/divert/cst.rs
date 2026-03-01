use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── Simple targets ──────────────────────────────────────────────

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

// ── With arguments ──────────────────────────────────────────────

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

// ── Chains (no trailing arrow) ──────────────────────────────────

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

// ── Tunnel calls (trailing `->`) ────────────────────────────────

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

// ── Tunnel call before tunnel onwards ───────────────────────────

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

// ── Tunnel onwards ──────────────────────────────────────────────

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

// ── Thread starts ───────────────────────────────────────────────

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

// ── In context ──────────────────────────────────────────────────

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

// ── Variant uniformity ──────────────────────────────────────────
//
// Every `DIVERT_NODE` must have exactly one inner wrapper child:
// `SIMPLE_DIVERT` | `TUNNEL_CALL_NODE` | `TUNNEL_ONWARDS_NODE` | `THREAD_START`.

const DIVERT_VARIANTS: [SyntaxKind; 4] = [
    SyntaxKind::SIMPLE_DIVERT,
    SyntaxKind::TUNNEL_CALL_NODE,
    SyntaxKind::TUNNEL_ONWARDS_NODE,
    SyntaxKind::THREAD_START,
];

/// Assert that every `DIVERT_NODE` in `src` has exactly one variant child.
fn assert_divert_uniformity(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());
    for node in p.syntax().descendants() {
        if node.kind() == SyntaxKind::DIVERT_NODE {
            let variant_children: Vec<_> = node
                .children()
                .filter(|c| DIVERT_VARIANTS.contains(&c.kind()))
                .collect();
            assert_eq!(
                variant_children.len(),
                1,
                "DIVERT_NODE should have exactly one variant child, found {} in `{src}`:\n  {:?}",
                variant_children.len(),
                variant_children
                    .iter()
                    .map(crate::SyntaxNode::kind)
                    .collect::<Vec<_>>(),
            );
        }
    }
}

/// Uniformity holds for simple diverts.
#[test]
fn uniformity_simple_divert() {
    assert_divert_uniformity("-> target\n");
}

/// Uniformity holds for chained diverts.
#[test]
fn uniformity_chain() {
    assert_divert_uniformity("-> a -> b\n");
}

/// Uniformity holds for tunnel calls.
#[test]
fn uniformity_tunnel_call() {
    assert_divert_uniformity("-> target ->\n");
}

/// Uniformity holds for tunnel onwards.
#[test]
fn uniformity_tunnel_onwards() {
    assert_divert_uniformity("->->\n");
}

/// Uniformity holds for tunnel onwards with divert chain.
#[test]
fn uniformity_tunnel_onwards_with_chain() {
    assert_divert_uniformity("->-> -> target\n");
}

/// Uniformity holds for thread starts.
#[test]
fn uniformity_thread() {
    assert_divert_uniformity("<- background\n");
}

/// Uniformity holds for tunnel call + tunnel onwards combo (two `DIVERT_NODE`s).
#[test]
fn uniformity_tunnel_call_then_onwards() {
    assert_divert_uniformity("-> tunnel ->->\n");
}

/// Uniformity holds for content line with divert.
#[test]
fn uniformity_content_then_divert() {
    assert_divert_uniformity("Hello -> knot\n");
}

// ── Positive/negative wrapper assertions ────────────────────────

/// Simple divert has `SIMPLE_DIVERT`, not `TUNNEL_CALL_NODE`.
#[test]
fn has_simple_divert_not_tunnel_call() {
    let p = parse("-> target\n");
    let root = p.syntax();
    let has_simple = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SIMPLE_DIVERT);
    let has_tunnel = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(has_simple, "simple divert must have SIMPLE_DIVERT");
    assert!(!has_tunnel, "simple divert must not have TUNNEL_CALL_NODE");
}

/// Chained divert has `SIMPLE_DIVERT`, not `TUNNEL_CALL_NODE`.
#[test]
fn chain_has_simple_divert_not_tunnel_call() {
    let p = parse("-> a -> b\n");
    let root = p.syntax();
    let has_simple = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SIMPLE_DIVERT);
    let has_tunnel = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(has_simple, "chain must have SIMPLE_DIVERT");
    assert!(!has_tunnel, "chain must not have TUNNEL_CALL_NODE");
}

/// Long chain has `SIMPLE_DIVERT`, not `TUNNEL_CALL_NODE`.
#[test]
fn long_chain_has_simple_divert_not_tunnel_call() {
    let p = parse("-> a -> b -> c\n");
    let root = p.syntax();
    let has_simple = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SIMPLE_DIVERT);
    let has_tunnel = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(has_simple, "long chain must have SIMPLE_DIVERT");
    assert!(!has_tunnel, "long chain must not have TUNNEL_CALL_NODE");
}

/// Tunnel call has `TUNNEL_CALL_NODE`, not `SIMPLE_DIVERT`.
#[test]
fn tunnel_call_has_tunnel_call_not_simple_divert() {
    let p = parse("-> target ->\n");
    let root = p.syntax();
    let has_tunnel = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    let has_simple = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SIMPLE_DIVERT);
    assert!(has_tunnel, "tunnel call must have TUNNEL_CALL_NODE");
    assert!(!has_simple, "tunnel call must not have SIMPLE_DIVERT");
}

/// Thread start has `THREAD_START`, not `SIMPLE_DIVERT` or `TUNNEL_CALL_NODE`.
#[test]
fn thread_has_thread_start_only() {
    let p = parse("<- background\n");
    let root = p.syntax();
    let has_thread = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::THREAD_START);
    let has_simple = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SIMPLE_DIVERT);
    let has_tunnel = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(has_thread, "thread must have THREAD_START");
    assert!(!has_simple, "thread must not have SIMPLE_DIVERT");
    assert!(!has_tunnel, "thread must not have TUNNEL_CALL_NODE");
}

/// Bare tunnel onwards has `TUNNEL_ONWARDS_NODE`, no `SIMPLE_DIVERT` or `TUNNEL_CALL_NODE`.
#[test]
fn tunnel_onwards_has_onwards_only() {
    let p = parse("->->\n");
    let root = p.syntax();
    let has_onwards = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_ONWARDS_NODE);
    let has_simple = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::SIMPLE_DIVERT);
    let has_tunnel = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::TUNNEL_CALL_NODE);
    assert!(has_onwards, "tunnel onwards must have TUNNEL_ONWARDS_NODE");
    assert!(
        !has_simple,
        "bare tunnel onwards must not have SIMPLE_DIVERT"
    );
    assert!(
        !has_tunnel,
        "bare tunnel onwards must not have TUNNEL_CALL_NODE"
    );
}

/// `TUNNEL_ONWARDS_NODE` with a trailing divert chain contains `SIMPLE_DIVERT` inside.
#[test]
fn tunnel_onwards_inner_chain_is_simple_divert() {
    let p = parse("->-> -> target\n");
    let root = p.syntax();
    // The TUNNEL_ONWARDS_NODE should contain a SIMPLE_DIVERT child.
    let onwards = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::TUNNEL_ONWARDS_NODE)
        .expect("expected TUNNEL_ONWARDS_NODE");
    let has_inner_simple = onwards
        .children()
        .any(|c| c.kind() == SyntaxKind::SIMPLE_DIVERT);
    assert!(
        has_inner_simple,
        "TUNNEL_ONWARDS_NODE with divert chain should contain SIMPLE_DIVERT"
    );
}

// ── Tunnel onwards with tunnel call chain ───────────────────────

/// `->-> -> a ->` — tunnel onwards followed by a tunnel call chain.
#[test]
fn tunnel_onwards_with_tunnel_call() {
    assert_equivalent(
        parse("->-> -> a ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE {
                        TUNNEL_CALL_NODE {
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

/// `->-> -> a -> b ->` — tunnel onwards followed by chained tunnel call.
#[test]
fn tunnel_onwards_with_tunnel_call_chain() {
    assert_equivalent(
        parse("->-> -> a -> b ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE {
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
            }
        }),
    );
}

// ── Error recovery / edge cases ─────────────────────────────────

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
