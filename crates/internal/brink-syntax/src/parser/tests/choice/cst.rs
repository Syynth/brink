use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── 1. Bullet variants ─────────────────────────────────────────

/// `* Hello` — single star bullet.
#[test]
fn bullet_star() {
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

/// `+ Hello` — sticky choice bullet.
#[test]
fn bullet_plus() {
    assert_equivalent(
        parse("+ Hello\n"),
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

/// `** Hello` — nested two-star bullet.
#[test]
fn bullet_double_star() {
    assert_equivalent(
        parse("** Hello\n"),
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

/// `*** Hello` — deeply nested three-star bullet.
#[test]
fn bullet_triple_star() {
    assert_equivalent(
        parse("*** Hello\n"),
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

/// `*+ Hello` — mixed star-plus bullet.
#[test]
fn bullet_mixed() {
    assert_equivalent(
        parse("*+ Hello\n"),
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

// ── 2. Labels ──────────────────────────────────────────────────

/// `* (myLabel)` — label with no content.
#[test]
fn label_only() {
    assert_equivalent(
        parse("* (myLabel)\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
            }
        }),
    );
}

/// `* (myLabel) Hello` — label followed by start content.
#[test]
fn label_with_content() {
    assert_equivalent(
        parse("* (myLabel) Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* (myLabel) [hidden]shown` — label with bracket and inner content.
#[test]
fn label_with_bracket() {
    assert_equivalent(
        parse("* (myLabel) [hidden]shown\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
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

/// `* (myLabel) Start[mid]end` — label with all three content regions.
#[test]
fn label_with_all_regions() {
    assert_equivalent(
        parse("* (myLabel) Start[mid]end\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
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

/// `* (myLabel) {flag} Hello` — label + condition + content.
#[test]
fn label_with_condition() {
    assert_equivalent(
        parse("* (myLabel) {flag} Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* (myLabel) -> knot` — label + divert, no content.
#[test]
fn label_with_divert() {
    assert_equivalent(
        parse("* (myLabel) -> knot\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
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

// ── 3. Conditions ──────────────────────────────────────────────

/// `* {visited} Hello` — simple path condition.
#[test]
fn condition_simple() {
    assert_equivalent(
        parse("* {visited} Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* {x > 5} Hello` — infix expression condition.
#[test]
fn condition_infix() {
    assert_equivalent(
        parse("* {x > 5} Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_CONDITION {
                    INFIX_EXPR {
                        PATH
                        INTEGER_LIT
                    }
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* {not visited} Hello` — prefix expression condition.
#[test]
fn condition_prefix() {
    assert_equivalent(
        parse("* {not visited} Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_CONDITION {
                    PREFIX_EXPR {
                        PATH
                    }
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* {x} {y} Hello` — two sibling conditions.
#[test]
fn condition_multiple() {
    assert_equivalent(
        parse("* {x} {y} Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* {visited}` — condition with no content.
#[test]
fn condition_no_content() {
    assert_equivalent(
        parse("* {visited}\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_CONDITION {
                    PATH
                }
            }
        }),
    );
}

/// `* {flag} Start[mid]end` — condition + all three content regions.
#[test]
fn condition_with_brackets() {
    assert_equivalent(
        parse("* {flag} Start[mid]end\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_CONDITION {
                    PATH
                }
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

// ── 4. Content regions ─────────────────────────────────────────

/// `* Hello` — start content only.
#[test]
fn start_only() {
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

/// `* [hidden]` — bracket content only.
#[test]
fn bracket_only() {
    assert_equivalent(
        parse("* [hidden]\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* [hidden]shown` — bracket + inner content.
#[test]
fn bracket_and_inner() {
    assert_equivalent(
        parse("* [hidden]shown\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
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

/// `* Start[mid]` — start + bracket content (no inner).
#[test]
fn start_and_bracket() {
    assert_equivalent(
        parse("* Start[mid]\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                }
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* Start[mid]end` — all three content regions.
#[test]
fn all_three_regions() {
    assert_equivalent(
        parse("* Start[mid]end\n"),
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

/// `* Start[]end` — empty bracket with start and inner content.
#[test]
fn empty_bracket() {
    assert_equivalent(
        parse("* Start[]end\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                }
                CHOICE_BRACKET_CONTENT
                CHOICE_INNER_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `* []` — empty bracket only.
#[test]
fn empty_bracket_only() {
    assert_equivalent(
        parse("* []\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT
            }
        }),
    );
}

/// `* []shown` — empty bracket + inner content.
#[test]
fn empty_bracket_with_inner() {
    assert_equivalent(
        parse("* []shown\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT
                CHOICE_INNER_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

// ── 5. Diverts in choices ──────────────────────────────────────

/// `* Hello -> knot` — divert after start content.
#[test]
fn divert_after_content() {
    assert_equivalent(
        parse("* Hello -> knot\n"),
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

/// `* [hidden] -> knot` — divert after bracket content.
#[test]
fn divert_after_bracket() {
    assert_equivalent(
        parse("* [hidden] -> knot\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
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

/// `* Start[mid]end -> knot` — divert after all three regions.
#[test]
fn divert_after_all_regions() {
    assert_equivalent(
        parse("* Start[mid]end -> knot\n"),
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

/// `* -> knot` — bare divert with no content.
#[test]
fn divert_only() {
    assert_equivalent(
        parse("* -> knot\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
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

/// `* Hello -> DONE` — divert to DONE keyword (no PATH child).
#[test]
fn divert_to_done() {
    assert_equivalent(
        parse("* Hello -> DONE\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                }
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS
                    }
                }
            }
        }),
    );
}

// ── 6. Tags ────────────────────────────────────────────────────

/// `* Hello #tag1` — single tag.
#[test]
fn tag_single() {
    assert_equivalent(
        parse("* Hello #tag1\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `* Hello #tag1 #tag2` — multiple tags.
#[test]
fn tag_multiple() {
    assert_equivalent(
        parse("* Hello #tag1 #tag2\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
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

/// `* Hello -> knot #tag1` — divert then tags.
#[test]
fn tag_after_divert() {
    assert_equivalent(
        parse("* Hello -> knot #tag1\n"),
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
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// `* #tag1` — tag only, no content.
#[test]
fn tag_only() {
    assert_equivalent(
        parse("* #tag1\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                TAGS {
                    TAG
                }
            }
        }),
    );
}

// ── 7. Inline logic ────────────────────────────────────────────

/// `* Hello {x} world` — inline expression in start content.
#[test]
fn inline_in_start() {
    assert_equivalent(
        parse("* Hello {x} world\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

/// `* [{x} hidden]` — inline expression in bracket content.
#[test]
fn inline_in_bracket() {
    assert_equivalent(
        parse("* [{x} hidden]\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

/// `* [mid]{x} end` — inline expression in inner content.
#[test]
fn inline_in_inner() {
    assert_equivalent(
        parse("* [mid]{x} end\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
                CHOICE_INNER_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

/// `* Hello {x: yes|no}` — inline conditional in start content.
#[test]
fn inline_conditional() {
    assert_equivalent(
        parse("* Hello {x: yes|no}\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
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

/// `* text {&big|small} choice` — inline sequence in start content.
#[test]
fn inline_sequence() {
    assert_equivalent(
        parse("* text {&big|small} choice\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
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
            }
        }),
    );
}

// ── 8. Escapes ─────────────────────────────────────────────────

/// `* Hello \[ world` — escape in start content.
#[test]
fn escape_in_start() {
    assert_equivalent(
        parse("* Hello \\[ world\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `* [hello \] world]` — escape in bracket content.
#[test]
fn escape_in_bracket() {
    assert_equivalent(
        parse("* [hello \\] world]\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
                    TEXT
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `* [mid]hello \# end` — escape in inner content.
#[test]
fn escape_in_inner() {
    assert_equivalent(
        parse("* [mid]hello \\# end\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
                CHOICE_INNER_CONTENT {
                    TEXT
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

// ── 9. Glue ────────────────────────────────────────────────────

/// `* Hello<>world` — glue in start content.
#[test]
fn glue_in_start() {
    assert_equivalent(
        parse("* Hello<>world\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// `* [hello<>world]` — glue in bracket content.
#[test]
fn glue_in_bracket() {
    assert_equivalent(
        parse("* [hello<>world]\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// `* [mid]hello<>world` — glue in inner content.
#[test]
fn glue_in_inner() {
    assert_equivalent(
        parse("* [mid]hello<>world\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
                CHOICE_INNER_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

// ── 10. Complex combinations ───────────────────────────────────

/// Full combination: label, condition, three regions, divert, tags.
#[test]
fn full_combination() {
    assert_equivalent(
        parse("* (lbl) {flag} Start[mid]end -> knot #tag\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
                CHOICE_INNER_CONTENT {
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

/// Nested full combination with double-star bullets.
#[test]
fn nested_full() {
    assert_equivalent(
        parse("** (lbl) {flag} Start[mid]end -> knot #tag\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
                CHOICE_BRACKET_CONTENT {
                    TEXT
                }
                CHOICE_INNER_CONTENT {
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

/// Sticky + label + three content regions.
#[test]
fn sticky_label_bracket() {
    assert_equivalent(
        parse("+ (lbl) Start[hidden]shown\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
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

/// Three sibling conditions + content.
#[test]
fn triple_conditions() {
    assert_equivalent(
        parse("* {x} {y} {z} Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_CONDITION {
                    PATH
                }
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Divert + tags with no content.
#[test]
fn divert_tags_no_content() {
    assert_equivalent(
        parse("* -> knot #tag1 #tag2\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
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

/// Label + condition + bracket + divert.
#[test]
fn label_condition_bracket_divert() {
    assert_equivalent(
        parse("* (lbl) {x > 5} [hidden] -> knot\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                LABEL {
                    IDENTIFIER
                }
                CHOICE_CONDITION {
                    INFIX_EXPR {
                        PATH
                        INTEGER_LIT
                    }
                }
                CHOICE_BRACKET_CONTENT {
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

// ── 11. Structural uniformity ──────────────────────────────────

const CONTENT_REGION_KINDS: [SyntaxKind; 3] = [
    SyntaxKind::CHOICE_START_CONTENT,
    SyntaxKind::CHOICE_BRACKET_CONTENT,
    SyntaxKind::CHOICE_INNER_CONTENT,
];

/// Every `CHOICE` has exactly one `CHOICE_BULLETS` child.
#[test]
fn uniformity_always_has_bullets() {
    for src in [
        "* Hello\n",
        "+ Hello\n",
        "** Hello\n",
        "* (lbl) {flag} Start[mid]end -> knot #tag\n",
    ] {
        let p = parse(src);
        assert!(p.errors().is_empty(), "errors in {src:?}: {:?}", p.errors());
        for node in p.syntax().descendants() {
            if node.kind() == SyntaxKind::CHOICE {
                let bullet_count = node
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::CHOICE_BULLETS)
                    .count();
                assert_eq!(
                    bullet_count, 1,
                    "CHOICE should have exactly 1 CHOICE_BULLETS in {src:?}, found {bullet_count}"
                );
            }
        }
    }
}

/// No `CHOICE` has more than one of any content region kind.
#[test]
fn uniformity_no_duplicate_regions() {
    for src in [
        "* Hello\n",
        "* [hidden]shown\n",
        "* Start[mid]end\n",
        "* (lbl) {flag} Start[mid]end -> knot #tag\n",
    ] {
        let p = parse(src);
        assert!(p.errors().is_empty(), "errors in {src:?}: {:?}", p.errors());
        for node in p.syntax().descendants() {
            if node.kind() == SyntaxKind::CHOICE {
                for kind in &CONTENT_REGION_KINDS {
                    let count = node.children().filter(|c| c.kind() == *kind).count();
                    assert!(
                        count <= 1,
                        "{kind:?} appears {count} times in CHOICE for {src:?}"
                    );
                }
            }
        }
    }
}

/// `LABEL` text range comes before `CHOICE_CONDITION` text range.
#[test]
fn uniformity_label_before_condition() {
    let p = parse("* (lbl) {flag} Hello\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("expected CHOICE");
    let label_end = choice
        .children()
        .find(|c| c.kind() == SyntaxKind::LABEL)
        .expect("expected LABEL")
        .text_range()
        .end();
    let cond_start = choice
        .children()
        .find(|c| c.kind() == SyntaxKind::CHOICE_CONDITION)
        .expect("expected CHOICE_CONDITION")
        .text_range()
        .start();
    assert!(
        label_end <= cond_start,
        "LABEL (ends at {label_end:?}) should come before CHOICE_CONDITION (starts at {cond_start:?})"
    );
}

/// `CHOICE_CONDITION` text range comes before any content region.
#[test]
fn uniformity_condition_before_content() {
    let p = parse("* {flag} Hello\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("expected CHOICE");
    let cond_end = choice
        .children()
        .find(|c| c.kind() == SyntaxKind::CHOICE_CONDITION)
        .expect("expected CHOICE_CONDITION")
        .text_range()
        .end();
    for kind in &CONTENT_REGION_KINDS {
        if let Some(content) = choice.children().find(|c| c.kind() == *kind) {
            let content_start = content.text_range().start();
            assert!(
                cond_end <= content_start,
                "CHOICE_CONDITION (ends at {cond_end:?}) should come before {kind:?} (starts at {content_start:?})"
            );
        }
    }
}

/// All content region offsets come before `DIVERT_NODE` offset.
#[test]
fn uniformity_divert_after_content() {
    let p = parse("* Start[mid]end -> knot\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("expected CHOICE");
    let divert_start = choice
        .children()
        .find(|c| c.kind() == SyntaxKind::DIVERT_NODE)
        .expect("expected DIVERT_NODE")
        .text_range()
        .start();
    for kind in &CONTENT_REGION_KINDS {
        if let Some(content) = choice.children().find(|c| c.kind() == *kind) {
            let content_end = content.text_range().end();
            assert!(
                content_end <= divert_start,
                "{kind:?} (ends at {content_end:?}) should come before DIVERT_NODE (starts at {divert_start:?})"
            );
        }
    }
}

// ── 12. Error / edge cases ─────────────────────────────────────

/// Unclosed bracket — has errors but round-trips losslessly.
#[test]
fn error_unclosed_bracket() {
    let src = "* Hello [world\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unclosed bracket"
    );
}

/// Unclosed condition brace — has errors but round-trips losslessly.
#[test]
fn error_unclosed_condition() {
    let src = "* {flag Hello\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unclosed brace"
    );
}

/// `* () Hello` — empty parens are NOT a label.
#[test]
fn not_label_empty_parens() {
    let p = parse("* () Hello\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let has_label = p
        .syntax()
        .descendants()
        .any(|n| n.kind() == SyntaxKind::LABEL);
    assert!(!has_label, "empty parens should not produce a LABEL node");
}

/// `*\n` — bare bullets with no content, no error.
#[test]
fn bare_bullets_no_content() {
    let p = parse("*\n");
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let choice = p
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::CHOICE)
        .expect("expected CHOICE");
    let has_content = choice.children().any(|c| {
        CONTENT_REGION_KINDS.contains(&c.kind())
            || c.kind() == SyntaxKind::DIVERT_NODE
            || c.kind() == SyntaxKind::TAGS
    });
    assert!(!has_content, "bare bullets should have no content nodes");
}
