use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── 1. Dash depth variants ─────────────────────────────────────

/// `- text` — single-dash gather.
#[test]
fn depth_one() {
    assert_equivalent(
        parse("- text\n"),
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

/// `-- text` — double-dash gather (adjacent).
#[test]
fn depth_two_adjacent() {
    assert_equivalent(
        parse("-- text\n"),
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

/// `- - text` — double-dash gather (spaced).
#[test]
fn depth_two_spaced() {
    assert_equivalent(
        parse("- - text\n"),
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

/// `--- text` — triple-dash gather.
#[test]
fn depth_three() {
    assert_equivalent(
        parse("--- text\n"),
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

/// `- - - text` — triple-dash gather (spaced).
#[test]
fn depth_three_spaced() {
    assert_equivalent(
        parse("- - - text\n"),
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

/// `-\n` — bare single dash, no content.
#[test]
fn bare_single_dash() {
    assert_equivalent(
        parse("-\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
            }
        }),
    );
}

/// `--\n` — bare double dash, no content.
#[test]
fn bare_double_dash() {
    assert_equivalent(
        parse("--\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
            }
        }),
    );
}

// ── 2. Labels ──────────────────────────────────────────────────

/// `- (myLabel) Hello` — label with content.
#[test]
fn label_with_content() {
    assert_equivalent(
        parse("- (myLabel) Hello\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `- (end)` — label only, no content.
#[test]
fn label_only() {
    assert_equivalent(
        parse("- (end)\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
            }
        }),
    );
}

/// `- (end) -> done` — label with divert.
#[test]
fn label_with_divert() {
    assert_equivalent(
        parse("- (end) -> done\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
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

/// `- (end) Text #tag` — label with content and tags.
#[test]
fn label_with_tags() {
    assert_equivalent(
        parse("- (end) Text #tag\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
                MIXED_CONTENT {
                    TEXT
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `- (lbl) Text -> next` — label with content and divert.
#[test]
fn label_with_content_and_divert() {
    assert_equivalent(
        parse("- (lbl) Text -> next\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
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

/// `-- (inner) Text` — depth-2 with label.
#[test]
fn nested_with_label() {
    assert_equivalent(
        parse("-- (inner) Text\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

// ── 3. Content variations ──────────────────────────────────────

/// `- Hello world` — plain text content.
#[test]
fn plain_text() {
    assert_equivalent(
        parse("- Hello world\n"),
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

/// `- Hello<>world` — glue in content.
#[test]
fn content_with_glue() {
    assert_equivalent(
        parse("- Hello<>world\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                MIXED_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// `- Hello \# not a tag` — escape in content.
#[test]
fn content_with_escape() {
    assert_equivalent(
        parse("- Hello \\# not a tag\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                MIXED_CONTENT {
                    TEXT
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `- Hello {x}` — inline logic expression in content.
#[test]
fn content_with_inline_logic() {
    assert_equivalent(
        parse("- Hello {x}\n"),
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

/// `- {x: yes}` — inline conditional in content.
#[test]
fn content_with_inline_conditional() {
    assert_equivalent(
        parse("- {x: yes}\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
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

// ── 4. Diverts ─────────────────────────────────────────────────

/// `- -> knot` — simple divert.
#[test]
fn simple_divert() {
    assert_equivalent(
        parse("- -> knot\n"),
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

/// `- -> DONE` — divert to DONE keyword (no PATH child).
#[test]
fn divert_to_done() {
    assert_equivalent(
        parse("- -> DONE\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS
                    }
                }
            }
        }),
    );
}

/// `- -> END` — divert to END keyword (no PATH child).
#[test]
fn divert_to_end() {
    assert_equivalent(
        parse("- -> END\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS
                    }
                }
            }
        }),
    );
}

/// `- -> knot.stitch` — divert with dotted path.
#[test]
fn divert_dotted_path() {
    assert_equivalent(
        parse("- -> knot.stitch\n"),
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

/// `- -> func(x)` — divert with arguments.
#[test]
fn divert_with_args() {
    assert_equivalent(
        parse("- -> func(x)\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
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

/// `- -> a -> b` — chained divert.
#[test]
fn divert_chain() {
    assert_equivalent(
        parse("- -> a -> b\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
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

/// `- -> target ->` — tunnel call.
#[test]
fn tunnel_call() {
    assert_equivalent(
        parse("- -> target ->\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
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

/// `- ->->` — tunnel onwards.
#[test]
fn tunnel_onwards() {
    assert_equivalent(
        parse("- ->->\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE
                }
            }
        }),
    );
}

/// `- <- background` — thread start.
#[test]
fn thread_start() {
    assert_equivalent(
        parse("- <- background\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                DIVERT_NODE {
                    THREAD_START {
                        PATH
                    }
                }
            }
        }),
    );
}

/// `- Gathered -> next` — content then divert.
#[test]
fn content_then_divert() {
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

// ── 5. Tags ────────────────────────────────────────────────────

/// `- Text #tag1` — single tag.
#[test]
fn single_tag() {
    assert_equivalent(
        parse("- Text #tag1\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                MIXED_CONTENT {
                    TEXT
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `- Text #tag1 #tag2` — multiple tags.
#[test]
fn multiple_tags() {
    assert_equivalent(
        parse("- Text #tag1 #tag2\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
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

/// `- -> done #tag` — tag after divert.
#[test]
fn tag_after_divert() {
    assert_equivalent(
        parse("- -> done #tag\n"),
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
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `- #tag` — tag only, no content.
#[test]
fn tag_only() {
    assert_equivalent(
        parse("- #tag\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                TAGS {
                    TAG
                }
            }
        }),
    );
}

// ── 6. Complex combinations ───────────────────────────────────

/// `- (lbl) Text -> next #tag` — all components.
#[test]
fn full_gather() {
    assert_equivalent(
        parse("- (lbl) Text -> next #tag\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
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
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `-- (lbl) Text -> next #tag` — depth-2 with all components.
#[test]
fn nested_full() {
    assert_equivalent(
        parse("-- (lbl) Text -> next #tag\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
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
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `- (end) -> done #tag` — label + divert + tags, no content.
#[test]
fn label_divert_tags_no_content() {
    assert_equivalent(
        parse("- (end) -> done #tag\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                LABEL {
                    IDENTIFIER
                }
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `- Hello -> knot #tag1 #tag2` — content + divert + tags, no label.
#[test]
fn content_divert_tags() {
    assert_equivalent(
        parse("- Hello -> knot #tag1 #tag2\n"),
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
                TAGS {
                    TAG
                    TAG
                }
            }
        }),
    );
}

// ── 7. Structural invariants ──────────────────────────────────

/// Assert gather structural invariants:
/// - Exactly one `GATHER_DASHES` child
/// - `GATHER_DASHES` is the first node child
/// - Children appear in order: `LABEL` < `MIXED_CONTENT` < `DIVERT_NODE` < `TAGS`
fn assert_gather_invariants(src: &str) {
    let p = parse(src);
    assert!(
        p.errors().is_empty(),
        "unexpected errors in {src:?}: {:?}",
        p.errors()
    );
    for node in p.syntax().descendants() {
        if node.kind() != SyntaxKind::GATHER {
            continue;
        }

        // Exactly one GATHER_DASHES
        let dash_count = node
            .children()
            .filter(|c| c.kind() == SyntaxKind::GATHER_DASHES)
            .count();
        assert_eq!(
            dash_count, 1,
            "GATHER should have exactly 1 GATHER_DASHES in {src:?}, found {dash_count}"
        );

        // GATHER_DASHES is the first node child
        let first = node
            .children()
            .next()
            .expect("GATHER must have at least one child");
        assert_eq!(
            first.kind(),
            SyntaxKind::GATHER_DASHES,
            "first child of GATHER must be GATHER_DASHES in {src:?}, found {:?}",
            first.kind()
        );

        // Ordering: LABEL < MIXED_CONTENT < DIVERT_NODE < TAGS
        let label_end = node
            .children()
            .find(|c| c.kind() == SyntaxKind::LABEL)
            .map(|c| c.text_range().end());
        let content_range = node
            .children()
            .find(|c| c.kind() == SyntaxKind::MIXED_CONTENT)
            .map(|c| c.text_range());
        let divert_range = node
            .children()
            .find(|c| c.kind() == SyntaxKind::DIVERT_NODE)
            .map(|c| c.text_range());
        let tags_start = node
            .children()
            .find(|c| c.kind() == SyntaxKind::TAGS)
            .map(|c| c.text_range().start());

        if let (Some(le), Some(cr)) = (label_end, content_range) {
            assert!(
                le <= cr.start(),
                "LABEL (ends {le:?}) should precede MIXED_CONTENT (starts {:?}) in {src:?}",
                cr.start()
            );
        }
        if let (Some(cr), Some(dr)) = (content_range, divert_range) {
            assert!(
                cr.end() <= dr.start(),
                "MIXED_CONTENT (ends {:?}) should precede DIVERT_NODE (starts {:?}) in {src:?}",
                cr.end(),
                dr.start()
            );
        }
        if let (Some(dr), Some(ts)) = (divert_range, tags_start) {
            assert!(
                dr.end() <= ts,
                "DIVERT_NODE (ends {:?}) should precede TAGS (starts {ts:?}) in {src:?}",
                dr.end()
            );
        }
    }
}

#[test]
fn invariants_bare() {
    assert_gather_invariants("-\n");
}

#[test]
fn invariants_with_content() {
    assert_gather_invariants("- text\n");
}

#[test]
fn invariants_with_label() {
    assert_gather_invariants("- (lbl) text\n");
}

#[test]
fn invariants_with_divert() {
    assert_gather_invariants("- -> done\n");
}

#[test]
fn invariants_with_tags() {
    assert_gather_invariants("- text #tag\n");
}

#[test]
fn invariants_full_combination() {
    assert_gather_invariants("- (lbl) Text -> next #tag\n");
}

#[test]
fn invariants_nested() {
    assert_gather_invariants("-- (lbl) Text -> next #tag\n");
}

// ── 8. Positive/negative node assertions ──────────────────────

/// Every gather has `GATHER_DASHES`.
#[test]
fn gather_has_gather_dashes() {
    let p = parse("- text\n");
    let has = p
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::GATHER_DASHES);
    assert!(has, "gather must have GATHER_DASHES");
}

/// Bare gather has no `MIXED_CONTENT`.
#[test]
fn bare_gather_no_mixed_content() {
    let p = parse("-\n");
    let gather = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::GATHER)
        .expect("expected GATHER");
    let has = gather
        .children()
        .any(|c| c.kind() == SyntaxKind::MIXED_CONTENT);
    assert!(!has, "bare gather must not have MIXED_CONTENT");
}

/// Bare gather has no LABEL.
#[test]
fn bare_gather_no_label() {
    let p = parse("-\n");
    let gather = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::GATHER)
        .expect("expected GATHER");
    let has = gather.children().any(|c| c.kind() == SyntaxKind::LABEL);
    assert!(!has, "bare gather must not have LABEL");
}

/// Bare gather has no `DIVERT_NODE`.
#[test]
fn bare_gather_no_divert() {
    let p = parse("-\n");
    let gather = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::GATHER)
        .expect("expected GATHER");
    let has = gather
        .children()
        .any(|c| c.kind() == SyntaxKind::DIVERT_NODE);
    assert!(!has, "bare gather must not have DIVERT_NODE");
}

/// Bare gather has no TAGS.
#[test]
fn bare_gather_no_tags() {
    let p = parse("-\n");
    let gather = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::GATHER)
        .expect("expected GATHER");
    let has = gather.children().any(|c| c.kind() == SyntaxKind::TAGS);
    assert!(!has, "bare gather must not have TAGS");
}

/// `- text` produces `GATHER`, not `CONTENT_LINE`.
#[test]
fn gather_not_content_line() {
    let p = parse("- text\n");
    let root = p.syntax();
    let has_gather = root.descendants().any(|n| n.kind() == SyntaxKind::GATHER);
    let has_content_line = root
        .children()
        .any(|c| c.kind() == SyntaxKind::CONTENT_LINE);
    assert!(has_gather, "expected GATHER node");
    assert!(!has_content_line, "gather should not produce CONTENT_LINE");
}

/// Labeled gather has `LABEL` child, not content that looks like `(x)`.
#[test]
fn label_not_content() {
    let p = parse("- (lbl) Hello\n");
    let gather = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::GATHER)
        .expect("expected GATHER");
    let has_label = gather.children().any(|c| c.kind() == SyntaxKind::LABEL);
    assert!(has_label, "labeled gather must have LABEL child");
}

// ── 9. Error recovery / edge cases ────────────────────────────

/// `-` at EOF (no newline) — lossless round-trip, check error status.
#[test]
fn gather_at_eof_no_newline() {
    let src = "-";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// `- () text` — empty parens should NOT be treated as a label.
#[test]
fn empty_parens_not_label() {
    let p = parse("- () text\n");
    let has_label = p
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::LABEL);
    assert!(!has_label, "empty parens should not produce a LABEL node");
}
